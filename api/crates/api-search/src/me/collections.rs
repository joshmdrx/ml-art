//! `/v1/me/collections/*` — the authenticated user's saved-artwork collections.
//!
//! Every handler authenticates first, then enforces row-level ownership in
//! SQL (`WHERE user_id = $auth_user_id`). A collection that doesn't exist
//! OR belongs to someone else returns the same 404 so we don't leak the
//! existence of others' rows.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use ml_art_core::{
    error::ApiError,
    images::url_for_s3_key,
    models::{ArtworkSummary, CollectionDetail, CollectionSummary, Paginated},
};
use rand::distributions::{Alphanumeric, DistString};
use serde::Deserialize;
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AuthedUser;
use crate::AppState;

const COVER_THUMB_COUNT: usize = 4;
const COLLECTION_PAGE_LIMIT: i64 = 60;

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/me/collections
// ─────────────────────────────────────────────────────────────────────────────

/// Query params for the collections list. `artwork_id` is the Save modal's
/// opt-in: when present, each row's `contains_artwork` reflects whether
/// that artwork is currently in the collection. Lets the modal render
/// check-state without N membership queries.
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub artwork_id: Option<Uuid>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListQuery>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<Paginated<CollectionSummary>>, ApiError> {
    // Membership join is conditional: when no `artwork_id` is supplied we
    // pass NULL; the LEFT JOIN's `ca.artwork_id = $2` filter then never
    // matches and `contains_artwork` is uniformly false. One query path
    // for both cases keeps the code path narrow.
    let rows: Vec<CollectionRow> = sqlx::query_as(
        r#"
        SELECT
            uc.id,
            uc.name,
            uc.description,
            uc.is_public,
            uc.share_id,
            uc.updated_at,
            COALESCE(
                (SELECT count(*)::int FROM collection_artworks ca WHERE ca.collection_id = uc.id),
                0
            ) AS artwork_count,
            EXISTS (
                SELECT 1 FROM collection_artworks ca
                 WHERE ca.collection_id = uc.id
                   AND ca.artwork_id    = $2
            ) AS contains_artwork
        FROM user_collections uc
        WHERE uc.user_id = $1
          AND uc.deleted_at IS NULL
        ORDER BY uc.updated_at DESC
        "#,
    )
    .bind(user.id)
    .bind(params.artwork_id)
    .fetch_all(&state.pool)
    .await?;

    // Batch-fetch cover thumbs across all collections in one query — keeps
    // the response O(1) extra queries no matter how many collections.
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let covers = fetch_collection_covers(&state.pool, &ids).await?;

    let items: Vec<CollectionSummary> = rows
        .into_iter()
        .map(|r| {
            let cover_image_urls = covers.get(&r.id).cloned().unwrap_or_default();
            r.into_summary(cover_image_urls)
        })
        .collect();

    Ok(Json(Paginated {
        items,
        next_cursor: None,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/me/collections
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateCollection {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub is_public: Option<bool>,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Json(body): Json<CreateCollection>,
) -> Result<(StatusCode, Json<CollectionSummary>), ApiError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("name must not be empty".into()));
    }
    if name.len() > 80 {
        return Err(ApiError::BadRequest("name is too long (max 80)".into()));
    }

    let is_public = body.is_public.unwrap_or(false);
    let share_id = if is_public {
        Some(new_share_id())
    } else {
        None
    };

    let row: CollectionRow = sqlx::query_as(
        r#"
        INSERT INTO user_collections (user_id, name, description, is_public, share_id)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING
            id, name, description, is_public, share_id, updated_at,
            0::int AS artwork_count
        "#,
    )
    .bind(user.id)
    .bind(name)
    .bind(body.description)
    .bind(is_public)
    .bind(&share_id)
    .fetch_one(&state.pool)
    .await?;

    Ok((StatusCode::CREATED, Json(row.into_summary(Vec::new()))))
}

// ─────────────────────────────────────────────────────────────────────────────
// PATCH /v1/me/collections/:id
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PatchCollection {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub is_public: Option<bool>,
}

pub async fn patch(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    AuthedUser(user): AuthedUser,
    Json(body): Json<PatchCollection>,
) -> Result<Json<CollectionSummary>, ApiError> {
    if let Some(name) = &body.name {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(ApiError::BadRequest("name must not be empty".into()));
        }
        if trimmed.len() > 80 {
            return Err(ApiError::BadRequest("name is too long (max 80)".into()));
        }
    }

    // Use a single SQL statement with COALESCE so unspecified fields keep
    // their existing values. share_id transitions: when is_public toggles
    // from false→true, mint a new id; from true→false, clear it.
    let new_share_id = body
        .is_public
        .map(|p| if p { Some(new_share_id()) } else { None });

    let row: Option<CollectionRow> = sqlx::query_as(
        r#"
        UPDATE user_collections
           SET name        = COALESCE($3, name),
               description = CASE WHEN $4::boolean THEN $5 ELSE description END,
               is_public   = COALESCE($6, is_public),
               share_id    = CASE
                                WHEN $7::boolean THEN $8
                                ELSE share_id
                             END,
               updated_at  = now()
         WHERE id = $1
           AND user_id = $2
           AND deleted_at IS NULL
        RETURNING
            id, name, description, is_public, share_id, updated_at,
            (SELECT count(*)::int FROM collection_artworks ca WHERE ca.collection_id = user_collections.id) AS artwork_count
        "#,
    )
    .bind(id)
    .bind(user.id)
    .bind(body.name.as_deref())
    .bind(body.description.is_some())
    .bind(body.description.flatten())
    .bind(body.is_public)
    .bind(new_share_id.is_some())
    .bind(new_share_id.flatten())
    .fetch_optional(&state.pool)
    .await?;

    let row = row.ok_or(ApiError::NotFound)?;
    let covers = fetch_collection_covers(&state.pool, &[row.id]).await?;
    let cover_image_urls = covers.get(&row.id).cloned().unwrap_or_default();
    Ok(Json(row.into_summary(cover_image_urls)))
}

// ─────────────────────────────────────────────────────────────────────────────
// DELETE /v1/me/collections/:id   (soft-delete)
// ─────────────────────────────────────────────────────────────────────────────

pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    AuthedUser(user): AuthedUser,
) -> Result<StatusCode, ApiError> {
    let res = sqlx::query(
        r#"
        UPDATE user_collections
           SET deleted_at = now()
         WHERE id = $1
           AND user_id = $2
           AND deleted_at IS NULL
        "#,
    )
    .bind(id)
    .bind(user.id)
    .execute(&state.pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/me/collections/:id   (single collection + artworks)
// ─────────────────────────────────────────────────────────────────────────────

pub async fn detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<CollectionDetail>, ApiError> {
    let row: Option<CollectionRow> = sqlx::query_as(
        r#"
        SELECT
            uc.id, uc.name, uc.description, uc.is_public, uc.share_id, uc.updated_at,
            (SELECT count(*)::int FROM collection_artworks ca WHERE ca.collection_id = uc.id) AS artwork_count
        FROM user_collections uc
        WHERE uc.id = $1
          AND uc.user_id = $2
          AND uc.deleted_at IS NULL
        "#,
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await?;
    let row = row.ok_or(ApiError::NotFound)?;

    let artworks: Vec<ArtworkRow> = sqlx::query_as(
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
        FROM collection_artworks ca
        JOIN artworks a   ON a.id = ca.artwork_id
        JOIN artists ar   ON ar.id = a.artist_id
        LEFT JOIN artwork_images ai
               ON ai.artwork_id = a.id AND ai.is_primary
        WHERE ca.collection_id = $1
          AND a.deleted_at IS NULL
          AND a.status = 'published'
          AND ar.deleted_at IS NULL
        ORDER BY ca.display_order ASC NULLS LAST, ca.added_at DESC
        LIMIT $2
        "#,
    )
    .bind(row.id)
    .bind(COLLECTION_PAGE_LIMIT)
    .fetch_all(&state.pool)
    .await?;

    let summaries: Vec<ArtworkSummary> =
        artworks.into_iter().map(ArtworkRow::into_summary).collect();
    let cover_image_urls = summaries
        .iter()
        .filter_map(|a| a.primary_image_url.clone())
        .take(COVER_THUMB_COUNT)
        .collect();

    Ok(Json(CollectionDetail {
        collection: row.into_summary(cover_image_urls),
        artworks: Paginated {
            items: summaries,
            next_cursor: None,
        },
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/me/collections/:id/artworks
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AddArtwork {
    pub artwork_id: Uuid,
}

pub async fn add_artwork(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    AuthedUser(user): AuthedUser,
    Json(body): Json<AddArtwork>,
) -> Result<StatusCode, ApiError> {
    // Confirm the collection exists and belongs to this user. Done in a
    // single round-trip via the INSERT's WHERE check.
    let res = sqlx::query(
        r#"
        INSERT INTO collection_artworks (collection_id, artwork_id)
        SELECT uc.id, $3
        FROM user_collections uc
        WHERE uc.id = $1
          AND uc.user_id = $2
          AND uc.deleted_at IS NULL
        ON CONFLICT (collection_id, artwork_id) DO NOTHING
        "#,
    )
    .bind(id)
    .bind(user.id)
    .bind(body.artwork_id)
    .execute(&state.pool)
    .await?;

    // If no row was inserted, either the collection wasn't found (or wasn't
    // owned by this user) OR the artwork was already in it. Disambiguate
    // by checking ownership separately.
    if res.rows_affected() == 0 {
        let ok: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM user_collections WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(user.id)
        .fetch_optional(&state.pool)
        .await?;
        if ok.is_none() {
            return Err(ApiError::NotFound);
        }
        // Already-present is a no-op success.
    }

    // Bump updated_at on the collection so the list view re-sorts.
    sqlx::query("UPDATE user_collections SET updated_at = now() WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user.id)
        .execute(&state.pool)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────────
// DELETE /v1/me/collections/:id/artworks/:artwork_id
// ─────────────────────────────────────────────────────────────────────────────

pub async fn remove_artwork(
    State(state): State<Arc<AppState>>,
    Path((id, artwork_id)): Path<(Uuid, Uuid)>,
    AuthedUser(user): AuthedUser,
) -> Result<StatusCode, ApiError> {
    let res = sqlx::query(
        r#"
        DELETE FROM collection_artworks ca
        USING user_collections uc
        WHERE ca.collection_id = $1
          AND ca.artwork_id    = $3
          AND uc.id            = ca.collection_id
          AND uc.user_id       = $2
          AND uc.deleted_at IS NULL
        "#,
    )
    .bind(id)
    .bind(user.id)
    .bind(artwork_id)
    .execute(&state.pool)
    .await?;

    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }

    sqlx::query("UPDATE user_collections SET updated_at = now() WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user.id)
        .execute(&state.pool)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn new_share_id() -> String {
    // URL-safe, 12 chars, ~71 bits of entropy. Collisions effectively never.
    Alphanumeric.sample_string(&mut rand::thread_rng(), 12)
}

async fn fetch_collection_covers(
    pool: &sqlx::PgPool,
    collection_ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, Vec<String>>, ApiError> {
    if collection_ids.is_empty() {
        return Ok(Default::default());
    }
    // Distinct, ranked window: for each collection, pick the N most-recent
    // primary images.
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        r#"
        SELECT collection_id, s3_key FROM (
            SELECT
                ca.collection_id,
                ai.s3_key,
                ROW_NUMBER() OVER (
                    PARTITION BY ca.collection_id
                    ORDER BY ca.added_at DESC
                ) AS rk
            FROM collection_artworks ca
            JOIN artwork_images ai
              ON ai.artwork_id = ca.artwork_id AND ai.is_primary
            WHERE ca.collection_id = ANY($1)
        ) ranked
        WHERE rk <= $2
        ORDER BY collection_id, rk
        "#,
    )
    .bind(collection_ids)
    .bind(COVER_THUMB_COUNT as i64)
    .fetch_all(pool)
    .await?;

    let mut out: std::collections::HashMap<Uuid, Vec<String>> = Default::default();
    for (id, s3_key) in rows {
        out.entry(id).or_default().push(url_for_s3_key(&s3_key));
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// Row types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(FromRow)]
struct CollectionRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    is_public: bool,
    share_id: Option<String>,
    updated_at: DateTime<Utc>,
    artwork_count: i32,
    /// Set only by the list handler when `?artwork_id=` is supplied.
    /// Other handlers (create / patch / detail) don't select this
    /// column; `#[sqlx(default)]` fills in `false` so we don't need
    /// to duplicate the row struct.
    #[sqlx(default)]
    contains_artwork: bool,
}

impl CollectionRow {
    fn into_summary(self, cover_image_urls: Vec<String>) -> CollectionSummary {
        CollectionSummary {
            id: self.id,
            name: self.name,
            description: self.description,
            is_public: self.is_public,
            share_id: self.share_id,
            cover_image_urls,
            artwork_count: self.artwork_count,
            updated_at: self.updated_at,
            contains_artwork: self.contains_artwork,
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
