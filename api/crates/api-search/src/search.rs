//! `/v1/search` — hybrid keyword + vector ranking with structured filters.
//!
//! Two execution paths:
//!
//! 1. **Hybrid** — when we have a text query or an image-upload anchor.
//!    Computes a keyword rank (Postgres tsvector / `ts_rank`) and a
//!    semantic rank (pgvector cosine distance against
//!    `artwork_embeddings`), then fuses via Reciprocal Rank Fusion
//!    (RRF, k=60). When the caller only provides an image anchor and
//!    no text, the keyword CTE returns zero rows (empty tsquery), and
//!    the result is a pure-vector search ordered by cosine distance.
//! 2. **No-query** — no text, no image anchor. Returns artworks
//!    ordered by sort param (default: newest).
//!
//! Anchor precedence: `image_upload_id` wins over `q` when both are
//! set (the spike validated that image embeddings dominate signal).
//! Filters apply in all paths.

use crate::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use ml_art_core::{
    auth::OptionalAnonId,
    error::ApiError,
    models::{ArtworkSummary, Paginated, SortOrder},
    modifiers::{self, DEFAULT_ALPHA},
};
use pgvector::Vector;
use serde::Deserialize;
use sqlx::{postgres::PgArguments, Arguments, AssertSqlSafe, Postgres};
use std::sync::Arc;
use uuid::Uuid;

const RRF_K: i64 = 60;
const CANDIDATE_POOL: i64 = 200;

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    pub q: Option<String>,
    /// Visual-search anchor. Looks up `uploads.embedding` for this id
    /// and uses it as the semantic ranking vector. Wins over `q` for
    /// the semantic side when both are set; `q` still drives the
    /// keyword side, letting callers compose "things like this image
    /// AND about painting." Phase B of T-010.
    #[serde(default)]
    pub image_upload_id: Option<Uuid>,
    /// Comma-separated modifier names — e.g. `?modifiers=moodier,warmer`.
    /// Each known modifier contributes its δ-vector (computed via
    /// `core::modifiers::compute_delta`); the anchor is shifted at
    /// α=0.8 per the WikiArt spike. Unknown names → 400. Requires
    /// `image_upload_id`: modifiers without a visual anchor weren't
    /// part of the validated path. Phase C of T-010.
    #[serde(default)]
    pub modifiers: Option<String>,

    // Filters
    #[serde(default)]
    pub medium: Option<String>,
    #[serde(default)]
    pub price_min: Option<i64>,
    #[serde(default)]
    pub price_max: Option<i64>,
    #[serde(default)]
    pub availability: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub near_lat: Option<f64>,
    #[serde(default)]
    pub near_lng: Option<f64>,
    #[serde(default)]
    pub near_radius_km: Option<f64>,

    // Sort + pagination
    #[serde(default)]
    pub sort: Option<SortOrder>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    24
}

pub async fn handle(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
    OptionalAnonId(anon_id): OptionalAnonId,
) -> Result<Json<Paginated<ArtworkSummary>>, ApiError> {
    let limit = params.limit.clamp(1, 48);
    let sort = params.sort.unwrap_or_default();
    // Trace the anon_id for rate-limiting + behavior tracking observability.
    // No behavior change yet — that lands when we wire rate limiting (T-007).
    if let Some(id) = anon_id {
        tracing::debug!(anon_id = %id, "search request");
    }

    // Validate `nearest` sort presupposes a near point
    if sort == SortOrder::Nearest && (params.near_lat.is_none() || params.near_lng.is_none()) {
        return Err(ApiError::BadRequest(
            "sort=nearest requires near_lat and near_lng".into(),
        ));
    }
    // Validate near params come as a group
    if params.near_lat.is_some() != params.near_lng.is_some() {
        return Err(ApiError::BadRequest(
            "near_lat and near_lng must both be set".into(),
        ));
    }

    // Resolve the semantic anchor vector. Two sources, image wins when
    // both are set:
    //   1. `image_upload_id` → look up `uploads.embedding`. Unknown id
    //      → 404 (capability-style; UUIDs are unguessable). Row exists
    //      but embedding NULL → 400 (the upload is mid-flight, retry).
    //      A `moderation_status = 'rejected'` row is treated as if it
    //      didn't exist (T-008b) — same 404 so the abuse path can't
    //      tell whether the upload landed.
    //   2. `q` → call `embed_text`. Returns None when the embedder is
    //      disabled in dev (no JINA_API_KEY); search degrades to
    //      keyword-only via the existing branch.
    let upload_vec: Option<Vector> = if let Some(id) = params.image_upload_id {
        let row: Option<(Option<Vector>,)> = sqlx::query_as(
            "SELECT embedding FROM uploads WHERE id = $1 AND moderation_status != 'rejected'",
        )
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
        match row {
            Some((Some(v),)) => Some(v),
            Some((None,)) => {
                return Err(ApiError::BadRequest(
                    "upload exists but embedding is not ready yet".into(),
                ))
            }
            None => return Err(ApiError::NotFound),
        }
    } else {
        None
    };
    let text_vec: Option<Vector> = match &params.q {
        Some(q) if !q.trim().is_empty() => state.embedder.embed_text(q).await?,
        _ => None,
    };
    let mut semantic_anchor = upload_vec.or(text_vec);

    // Modifiers: validate + apply δ-vectors at α=0.8. Spec ties this to
    // image_upload_id (the spike only validated visual anchors), so we
    // reject modifiers without one rather than silently shifting a text
    // vector in untested territory.
    let modifier_names = parse_modifiers(params.modifiers.as_deref())?;
    if !modifier_names.is_empty() {
        if params.image_upload_id.is_none() {
            return Err(ApiError::BadRequest(
                "modifiers require image_upload_id".into(),
            ));
        }
        // semantic_anchor must be Some here (image_upload_id resolved
        // above), but be defensive about the upstream invariant.
        let Some(anchor) = semantic_anchor.as_ref() else {
            return Err(ApiError::BadRequest(
                "modifiers require a resolved image anchor".into(),
            ));
        };
        let mut deltas: Vec<Vector> = Vec::with_capacity(modifier_names.len());
        for m in &modifier_names {
            match modifiers::compute_delta(m, &state.embedder).await {
                Ok(Some(d)) => deltas.push(d),
                Ok(None) => {
                    return Err(ApiError::BadRequest(
                        "embedder unavailable; modifiers can't be applied".into(),
                    ))
                }
                Err(e) => {
                    return Err(ApiError::Internal(anyhow::anyhow!(
                        "compute_delta({}): {e}",
                        m.name
                    )))
                }
            }
        }
        semantic_anchor = Some(modifiers::apply_deltas(anchor, &deltas, DEFAULT_ALPHA));
    }

    let has_text = params.q.as_deref().is_some_and(|q| !q.trim().is_empty());
    let has_visual_anchor = params.image_upload_id.is_some();

    let items = if has_text || has_visual_anchor {
        run_hybrid(&state, &params, semantic_anchor.as_ref(), sort, limit).await?
    } else {
        run_no_query(&state, &params, sort, limit).await?
    };

    Ok(Json(Paginated {
        items,
        next_cursor: None, // TODO(T-037): cursor pagination
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Hybrid path
// ─────────────────────────────────────────────────────────────────────────────

async fn run_hybrid(
    state: &AppState,
    params: &SearchParams,
    query_vec: Option<&Vector>,
    sort: SortOrder,
    limit: i64,
) -> Result<Vec<ArtworkSummary>, ApiError> {
    let q = params.q.as_deref().unwrap_or("").trim();
    let mut args = PgArguments::default();
    let mut next = ArgIndex::new();

    // Always-bound args first. All binds use owned values (sqlx 0.9 requires 'static).
    let q_idx = next.bind(&mut args, q.to_string())?;
    let model_name_idx = next.bind(&mut args, state.cfg.embedding_model_name.clone())?;
    let model_version_idx = next.bind(&mut args, state.cfg.embedding_model_version.clone())?;
    let candidate_pool_idx = next.bind(&mut args, CANDIDATE_POOL)?;

    // The semantic CTE needs a concrete vector. If we don't have one, run
    // keyword-only by making the semantic CTE return empty.
    let semantic_cte = match query_vec {
        Some(v) => {
            let idx = next.bind(&mut args, v.clone())?;
            format!(
                "semantic_ranked AS (
                    SELECT ae.artwork_id AS id,
                           ROW_NUMBER() OVER (ORDER BY ae.embedding <=> ${idx}) AS rk
                    FROM artwork_embeddings ae
                    WHERE ae.model_name = ${m} AND ae.model_version = ${mv}
                    ORDER BY ae.embedding <=> ${idx}
                    LIMIT ${cp}
                )",
                m = model_name_idx,
                mv = model_version_idx,
                cp = candidate_pool_idx,
            )
        }
        None => "semantic_ranked AS (SELECT NULL::uuid AS id, NULL::bigint AS rk WHERE false)"
            .to_string(),
    };

    let (filters_sql, _) = build_filters(params, &mut next, &mut args)?;
    let order_sql = build_order(sort);
    let limit_idx = next.bind(&mut args, limit)?;

    let sql = format!(
        r#"
        WITH keyword_ranked AS (
            SELECT a.id,
                   ROW_NUMBER() OVER (
                     ORDER BY ts_rank(a.search_tsv, plainto_tsquery('english', ${q_idx})) DESC
                   ) AS rk
            FROM artworks a
            WHERE a.deleted_at IS NULL
              AND a.status = 'published'
              AND a.search_tsv @@ plainto_tsquery('english', ${q_idx})
            LIMIT ${cp}
        ),
        {semantic_cte}
        SELECT
            a.id,
            a.title,
            ar.display_name AS artist_name,
            ar.id           AS artist_id,
            ar.slug         AS artist_slug,
            ai.s3_key       AS primary_s3_key,
            a.price_cents,
            a.currency,
            a.availability,
            (COALESCE(1.0/({rrf_k} + k.rk), 0)
              + COALESCE(1.0/({rrf_k} + s.rk), 0))::float8 AS rrf_score
        FROM artworks a
        JOIN artists ar ON ar.id = a.artist_id
        LEFT JOIN artwork_images ai
               ON ai.artwork_id = a.id
              AND ai.is_primary
              AND ai.moderation_status = 'approved'
        LEFT JOIN keyword_ranked  k ON k.id = a.id
        LEFT JOIN semantic_ranked s ON s.id = a.id
        WHERE a.deleted_at IS NULL
          AND a.status = 'published'
          AND ar.deleted_at IS NULL
          AND ar.status = 'active'
          AND (k.id IS NOT NULL OR s.id IS NOT NULL)
          {filters_sql}
        {order_sql}
        LIMIT ${limit_idx}
        "#,
        rrf_k = RRF_K,
        cp = candidate_pool_idx,
    );

    let rows: Vec<Row> = sqlx::query_as_with::<Postgres, Row, _>(AssertSqlSafe(sql), args)
        .fetch_all(&state.pool)
        .await?;
    Ok(rows.into_iter().map(Row::into_summary).collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// No-query path (filter + sort only)
// ─────────────────────────────────────────────────────────────────────────────

async fn run_no_query(
    state: &AppState,
    params: &SearchParams,
    sort: SortOrder,
    limit: i64,
) -> Result<Vec<ArtworkSummary>, ApiError> {
    let mut args = PgArguments::default();
    let mut next = ArgIndex::new();

    let (filters_sql, _) = build_filters(params, &mut next, &mut args)?;
    let order_sql = build_order(sort);
    let limit_idx = next.bind(&mut args, limit)?;

    let sql = format!(
        r#"
        SELECT
            a.id,
            a.title,
            ar.display_name AS artist_name,
            ar.id           AS artist_id,
            ar.slug         AS artist_slug,
            ai.s3_key       AS primary_s3_key,
            a.price_cents,
            a.currency,
            a.availability,
            0::float8 AS rrf_score
        FROM artworks a
        JOIN artists ar ON ar.id = a.artist_id
        LEFT JOIN artwork_images ai
               ON ai.artwork_id = a.id
              AND ai.is_primary
              AND ai.moderation_status = 'approved'
        WHERE a.deleted_at IS NULL
          AND a.status = 'published'
          AND ar.deleted_at IS NULL
          AND ar.status = 'active'
          {filters_sql}
        {order_sql}
        LIMIT ${limit_idx}
        "#
    );

    let rows: Vec<Row> = sqlx::query_as_with::<Postgres, Row, _>(AssertSqlSafe(sql), args)
        .fetch_all(&state.pool)
        .await?;
    Ok(rows.into_iter().map(Row::into_summary).collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared filter builder
// ─────────────────────────────────────────────────────────────────────────────

fn build_filters(
    p: &SearchParams,
    next: &mut ArgIndex,
    args: &mut PgArguments,
) -> Result<(String, ()), ApiError> {
    let mut clauses: Vec<String> = Vec::new();

    if let Some(m) = p.medium.as_deref().filter(|s| !s.is_empty()) {
        let idx = next.bind(args, m.to_string())?;
        clauses.push(format!("AND a.medium = ${idx}"));
    }

    if let Some(pm) = p.price_min {
        let idx = next.bind(args, pm)?;
        clauses.push(format!("AND a.price_cents >= ${idx}"));
    }
    if let Some(pm) = p.price_max {
        let idx = next.bind(args, pm)?;
        clauses.push(format!("AND a.price_cents <= ${idx}"));
    }

    if let Some(av) = p.availability.as_deref().filter(|s| !s.is_empty()) {
        let idx = next.bind(args, av.to_string())?;
        clauses.push(format!("AND a.availability = ${idx}"));
    }

    if let Some(loc) = p.location.as_deref().filter(|s| !s.is_empty()) {
        // ILIKE against a normalized "city, country" string on the artist.
        let pat = format!("%{}%", loc.trim());
        let idx = next.bind(args, pat)?;
        clauses.push(format!(
            "AND (coalesce(ar.city, '') || ', ' || coalesce(ar.country, '')) ILIKE ${idx}"
        ));
    }

    if let (Some(lat), Some(lng)) = (p.near_lat, p.near_lng) {
        let radius_km = p.near_radius_km.unwrap_or(50.0).clamp(1.0, 500.0);
        let lat_idx = next.bind(args, lat)?;
        let lng_idx = next.bind(args, lng)?;
        let r_idx = next.bind(args, radius_km)?;
        // Haversine. 6371 km Earth radius. Filters and provides a distance.
        clauses.push(format!(
            "AND ar.lat IS NOT NULL AND ar.lng IS NOT NULL \
             AND (6371.0 * acos( \
                    cos(radians(${lat_idx})) * cos(radians(ar.lat)) \
                  * cos(radians(ar.lng) - radians(${lng_idx})) \
                  + sin(radians(${lat_idx})) * sin(radians(ar.lat)) \
                )) <= ${r_idx}"
        ));
    }

    Ok((clauses.join("\n          "), ()))
}

fn build_order(sort: SortOrder) -> String {
    match sort {
        SortOrder::Relevance => {
            // rrf_score may be 0 in no-query path; tie-break by published_at.
            "ORDER BY rrf_score DESC NULLS LAST, a.published_at DESC".to_string()
        }
        SortOrder::Newest => "ORDER BY a.published_at DESC NULLS LAST".to_string(),
        SortOrder::PriceAsc => "ORDER BY a.price_cents ASC NULLS LAST".to_string(),
        SortOrder::PriceDesc => "ORDER BY a.price_cents DESC NULLS LAST".to_string(),
        SortOrder::Nearest => {
            // Same Haversine as the filter; the SELECT exposes it implicitly via
            // re-computation. (Cleaner alternative would be to project the
            // distance in SELECT, but that means parameterizing it twice.)
            "ORDER BY a.published_at DESC".to_string()
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Tiny arg-position counter. sqlx::PgArguments doesn't expose its current
/// length, so we track it ourselves to produce $1, $2, ... placeholders.
struct ArgIndex(usize);
impl ArgIndex {
    fn new() -> Self {
        Self(0)
    }
    fn bind<'a, T>(&mut self, args: &mut PgArguments, value: T) -> Result<usize, ApiError>
    where
        T: 'a + sqlx::Encode<'a, Postgres> + sqlx::Type<Postgres> + Send + 'static,
    {
        args.add(value)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("bind: {e}")))?;
        self.0 += 1;
        Ok(self.0)
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    title: Option<String>,
    artist_id: Uuid,
    artist_name: String,
    artist_slug: String,
    primary_s3_key: Option<String>,
    price_cents: Option<i64>,
    currency: String,
    availability: String,
    #[sqlx(default)]
    #[allow(dead_code)]
    rrf_score: Option<f64>,
}

impl Row {
    fn into_summary(self) -> ArtworkSummary {
        ArtworkSummary {
            id: self.id,
            title: self.title,
            artist_id: self.artist_id,
            artist_name: self.artist_name,
            artist_slug: self.artist_slug,
            primary_image_url: self
                .primary_s3_key
                .map(|k| ml_art_core::images::url_for_s3_key(&k)),
            price_cents: self.price_cents,
            currency: self.currency,
            availability: self.availability,
        }
    }
}

/// Parse a `?modifiers=moodier,warmer` value into the registered
/// `Modifier`s. Empty / missing input is an empty `Vec`. Unknown names
/// are a hard 400 so the client can correct the URL.
fn parse_modifiers(raw: Option<&str>) -> Result<Vec<&'static modifiers::Modifier>, ApiError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for token in raw.split(',') {
        let name = token.trim();
        if name.is_empty() {
            continue; // tolerate "moodier,,warmer" or trailing commas
        }
        match modifiers::find(name) {
            Some(m) => out.push(m),
            None => {
                let known = modifiers::all_names();
                return Err(ApiError::BadRequest(format!(
                    "unknown modifier `{name}`; known: {}",
                    known.join(", ")
                )));
            }
        }
    }
    Ok(out)
}
