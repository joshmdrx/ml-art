//! `/v1/neighborhoods` and `/v1/neighborhoods/:slug`.
//!
//! V0 returns the 6 manually-curated neighborhoods we seed. Algorithmic
//! clustering (semantic / geographic) lands later (see 99-deferred.md).

use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use ml_art_core::{
    error::ApiError,
    images::url_for_s3_key,
    models::{ArtworkSummary, Neighborhood, NeighborhoodDetail, Paginated},
};
use serde::Deserialize;
use sqlx::{postgres::PgArguments, Arguments, AssertSqlSafe, FromRow, Postgres};
use std::sync::Arc;
use uuid::Uuid;

const DETAIL_PAGE_LIMIT: i64 = 24;

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/neighborhoods
// ─────────────────────────────────────────────────────────────────────────────

pub async fn index(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Paginated<Neighborhood>>, ApiError> {
    let rows: Vec<NeighborhoodRow> = sqlx::query_as(
        r#"
        SELECT
            id, slug, name, description, kind,
            representative_artwork_ids, artwork_count, is_featured
        FROM neighborhoods
        ORDER BY is_featured DESC, display_order ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    // Look up representative image URLs in one batched query so we don't N+1.
    let all_rep_ids: Vec<Uuid> = rows
        .iter()
        .flat_map(|r| r.representative_artwork_ids.iter().copied())
        .collect();

    let rep_urls = fetch_primary_image_urls(&state, &all_rep_ids).await?;

    let items: Vec<Neighborhood> = rows
        .into_iter()
        .map(|r| r.into_neighborhood(&rep_urls))
        .collect();

    Ok(Json(Paginated {
        items,
        next_cursor: None,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/neighborhoods/:slug
// ─────────────────────────────────────────────────────────────────────────────

/// Filter params for the neighborhood detail's artwork list. Mirror the
/// shape `/v1/search` accepts so the same FilterBar component can drive
/// both surfaces. `location` is intentionally absent — the neighborhood
/// slug already pins place.
#[derive(Debug, Deserialize, Default)]
pub struct DetailQuery {
    #[serde(default)]
    pub medium: Option<String>,
    #[serde(default)]
    pub price_min: Option<i64>,
    #[serde(default)]
    pub price_max: Option<i64>,
    #[serde(default)]
    pub availability: Option<String>,
}

pub async fn detail(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    Query(filters): Query<DetailQuery>,
) -> Result<Json<NeighborhoodDetail>, ApiError> {
    let row: Option<NeighborhoodRow> = sqlx::query_as(
        r#"
        SELECT
            id, slug, name, description, kind,
            representative_artwork_ids, artwork_count, is_featured
        FROM neighborhoods
        WHERE slug = $1
        "#,
    )
    .bind(&slug)
    .fetch_optional(&state.pool)
    .await?;

    let Some(row) = row else {
        return Err(ApiError::NotFound);
    };

    // First page of artworks. Order by distance_to_centroid if populated,
    // else by published_at — both work; the seed leaves distance_to_centroid
    // null because clusters are manually curated, so NULLS LAST keeps things
    // deterministic.
    //
    // Filter params extend the WHERE with the same shape `/v1/search` accepts.
    // Built dynamically + `AssertSqlSafe` so the parameter indices align with
    // the bind order. Pattern matches `search.rs`.
    let mut args = PgArguments::default();
    args.add(row.id)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("bind: {e}")))?;
    args.add(DETAIL_PAGE_LIMIT)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("bind: {e}")))?;
    let mut clauses: Vec<String> = Vec::new();
    let mut next_idx: usize = 3; // $1 = neighborhood_id, $2 = limit
    if let Some(m) = filters.medium.as_deref().filter(|s| !s.is_empty()) {
        clauses.push(format!("AND a.medium = ${next_idx}"));
        args.add(m.to_string())
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("bind: {e}")))?;
        next_idx += 1;
    }
    if let Some(pm) = filters.price_min {
        clauses.push(format!("AND a.price_cents >= ${next_idx}"));
        args.add(pm)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("bind: {e}")))?;
        next_idx += 1;
    }
    if let Some(pm) = filters.price_max {
        clauses.push(format!("AND a.price_cents <= ${next_idx}"));
        args.add(pm)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("bind: {e}")))?;
        next_idx += 1;
    }
    if let Some(av) = filters.availability.as_deref().filter(|s| !s.is_empty()) {
        clauses.push(format!("AND a.availability = ${next_idx}"));
        args.add(av.to_string())
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("bind: {e}")))?;
        // next_idx += 1; // last filter — bump only if more are added later
    }

    let sql = format!(
        r#"
        SELECT
            a.id,
            a.title,
            ar.display_name AS artist_name,
            ar.slug         AS artist_slug,
            ai.s3_key       AS primary_s3_key,
            a.price_cents,
            a.currency,
            a.availability
        FROM neighborhood_artworks na
        JOIN artworks a   ON a.id = na.artwork_id
        JOIN artists ar   ON ar.id = a.artist_id
        LEFT JOIN artwork_images ai
               ON ai.artwork_id = a.id AND ai.is_primary
        WHERE na.neighborhood_id = $1
          AND a.deleted_at IS NULL
          AND a.status = 'published'
          AND ar.deleted_at IS NULL
          AND ar.status = 'active'
          {extra}
        ORDER BY na.distance_to_centroid ASC NULLS LAST, a.published_at DESC
        LIMIT $2
        "#,
        extra = clauses.join("\n          ")
    );

    let artwork_rows: Vec<ArtworkRow> =
        sqlx::query_as_with::<Postgres, ArtworkRow, _>(AssertSqlSafe(sql), args)
            .fetch_all(&state.pool)
            .await?;

    let artworks: Vec<ArtworkSummary> = artwork_rows
        .into_iter()
        .map(ArtworkRow::into_summary)
        .collect();

    // Build representative URLs from the curated rep ids.
    let rep_urls = fetch_primary_image_urls(&state, &row.representative_artwork_ids).await?;

    Ok(Json(NeighborhoodDetail {
        neighborhood: row.into_neighborhood(&rep_urls),
        artworks: Paginated {
            items: artworks,
            next_cursor: None,
        },
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

async fn fetch_primary_image_urls(
    state: &AppState,
    ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, String>, ApiError> {
    if ids.is_empty() {
        return Ok(Default::default());
    }
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        r#"
        SELECT artwork_id, s3_key
        FROM artwork_images
        WHERE is_primary = true
          AND artwork_id = ANY($1)
        "#,
    )
    .bind(ids)
    .fetch_all(&state.pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, key)| (id, url_for_s3_key(&key)))
        .collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// Row types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(FromRow)]
struct NeighborhoodRow {
    id: Uuid,
    slug: String,
    name: String,
    description: Option<String>,
    kind: String,
    representative_artwork_ids: Vec<Uuid>,
    artwork_count: i32,
    is_featured: bool,
}

impl NeighborhoodRow {
    fn into_neighborhood(self, rep_urls: &std::collections::HashMap<Uuid, String>) -> Neighborhood {
        let representative_image_urls = self
            .representative_artwork_ids
            .iter()
            .filter_map(|id| rep_urls.get(id).cloned())
            .collect();
        Neighborhood {
            id: self.id,
            slug: self.slug,
            name: self.name,
            description: self.description,
            kind: self.kind,
            representative_image_urls,
            artwork_count: self.artwork_count,
            is_featured: self.is_featured,
        }
    }
}

#[derive(FromRow)]
struct ArtworkRow {
    id: Uuid,
    title: Option<String>,
    artist_name: String,
    artist_slug: String,
    primary_s3_key: Option<String>,
    price_cents: Option<i64>,
    currency: String,
    availability: String,
}

impl ArtworkRow {
    fn into_summary(self) -> ArtworkSummary {
        ArtworkSummary {
            id: self.id,
            title: self.title,
            artist_name: self.artist_name,
            artist_slug: self.artist_slug,
            primary_image_url: self.primary_s3_key.map(|k| url_for_s3_key(&k)),
            price_cents: self.price_cents,
            currency: self.currency,
            availability: self.availability,
        }
    }
}
