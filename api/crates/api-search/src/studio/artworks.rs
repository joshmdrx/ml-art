//! `/v1/studio/artworks/*` — the authenticated artist's own portfolio.
//!
//! Endpoints:
//!   - `GET    /v1/studio/artworks?status=draft|published|all`
//!   - `POST   /v1/studio/artworks`
//!   - `GET    /v1/studio/artworks/:id`
//!   - `PATCH  /v1/studio/artworks/:id`
//!   - `DELETE /v1/studio/artworks/:id`             (soft-delete)
//!   - `POST   /v1/studio/artworks/:id/images`
//!   - `DELETE /v1/studio/artworks/:id/images/:image_id`
//!
//! Ownership: every handler resolves the caller's artist via
//! `studio::current_artist_id`, then filters all SQL on
//! `artworks.artist_id = $artist`. Cross-artist access returns 404.
//!
//! Image add invokes `artwork_embeddings::process_image` for the
//! primary image (T-036). For now the handler accepts a raw `s3_key`
//! from the caller — `T-010` (upload endpoint) lands the validated
//! upload flow that mints those keys server-side.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use ml_art_core::{
    artwork_embeddings,
    error::ApiError,
    images::url_for_s3_key,
    jobs::{EnqueueOpts, JobEvent},
    models::Paginated,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AuthedUser;
use crate::studio::current_artist_id;
use crate::AppState;

const MAX_TITLE_LEN: usize = 200;
const MAX_DESC_LEN: usize = 8_000;
const VALID_AVAILABILITY: &[&str] = &["available", "sold", "not_for_sale", "inquire"];
const VALID_STATUS: &[&str] = &["draft", "published", "archived"];

// ─────────────────────────────────────────────────────────────────────────────
// Wire shapes
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, FromRow)]
pub struct StudioArtworkSummary {
    pub id: Uuid,
    pub title: Option<String>,
    pub status: String,
    pub medium: Option<String>,
    pub price_cents: Option<i64>,
    pub currency: String,
    pub availability: String,
    /// Primary image URL if one exists, otherwise `None`. Drafts can be
    /// imageless (artist is still writing copy).
    pub primary_image_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct StudioArtworkDetail {
    #[serde(flatten)]
    pub summary: StudioArtworkSummary,
    pub description: Option<String>,
    pub year_created: Option<i32>,
    pub dimensions: Option<serde_json::Value>,
    pub external_url: Option<String>,
    pub images: Vec<StudioImage>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct StudioImage {
    pub id: Uuid,
    pub s3_key: String,
    pub url: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub is_primary: bool,
    pub display_order: i32,
    pub moderation_status: String,
    /// Comma-joined Rekognition labels written by the moderation
    /// handler when the row is rejected. `None` for pending /
    /// approved rows. T-008c.
    pub moderation_reason: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/studio/artworks
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    /// `draft`, `published`, `archived`, or `all` (default). Anything
    /// else is treated as `all` to keep the surface tolerant.
    #[serde(default)]
    pub status: Option<String>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListQuery>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<Paginated<StudioArtworkSummary>>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    let status_filter = params.status.as_deref().and_then(|s| match s {
        "draft" | "published" | "archived" => Some(s.to_string()),
        _ => None, // includes "all" and any unknown value
    });

    let rows: Vec<ArtworkRow> = sqlx::query_as(
        r#"
        SELECT
            a.id, a.title, a.status, a.medium,
            a.price_cents, a.currency, a.availability,
            ai.s3_key AS primary_s3_key,
            a.created_at, a.updated_at, a.published_at
        FROM artworks a
        LEFT JOIN artwork_images ai
               ON ai.artwork_id = a.id AND ai.is_primary
        WHERE a.artist_id = $1
          AND a.deleted_at IS NULL
          AND ($2::text IS NULL OR a.status = $2)
        ORDER BY a.updated_at DESC
        "#,
    )
    .bind(artist_id)
    .bind(status_filter)
    .fetch_all(&state.pool)
    .await?;

    let items: Vec<StudioArtworkSummary> = rows.into_iter().map(ArtworkRow::into_summary).collect();
    Ok(Json(Paginated {
        items,
        next_cursor: None,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/studio/artworks
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateArtwork {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub year_created: Option<i32>,
    #[serde(default)]
    pub medium: Option<String>,
    #[serde(default)]
    pub dimensions: Option<serde_json::Value>,
    #[serde(default)]
    pub price_cents: Option<i64>,
    #[serde(default)]
    pub currency: Option<String>,
    /// Defaults to `available`.
    #[serde(default)]
    pub availability: Option<String>,
    #[serde(default)]
    pub external_url: Option<String>,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Json(body): Json<CreateArtwork>,
) -> Result<(StatusCode, Json<StudioArtworkSummary>), ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    let title = body.title.as_deref().map(str::trim);
    if let Some(t) = title {
        if t.len() > MAX_TITLE_LEN {
            return Err(ApiError::BadRequest(format!(
                "title exceeds {MAX_TITLE_LEN}-char limit"
            )));
        }
    }
    let description = body.description.as_deref().map(str::trim);
    if let Some(d) = description {
        if d.len() > MAX_DESC_LEN {
            return Err(ApiError::BadRequest(format!(
                "description exceeds {MAX_DESC_LEN}-char limit"
            )));
        }
    }
    let availability = body.availability.as_deref().unwrap_or("available");
    if !VALID_AVAILABILITY.contains(&availability) {
        return Err(ApiError::BadRequest(format!(
            "invalid availability `{availability}`"
        )));
    }
    let currency = body.currency.as_deref().unwrap_or("USD");

    // T-070 — validate + normalise dimensions JSONB. Absent / null pass
    // through as None; a present object goes through the shape check
    // and lands as the normalised form (unit defaulted to "cm",
    // unknown keys rejected). See core::validation::dimensions_v1.
    let dimensions: Option<serde_json::Value> = match body.dimensions.as_ref() {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => Some(ml_art_core::validation::dimensions_v1(v).map_err(ApiError::BadRequest)?),
    };

    let row: ArtworkRow = sqlx::query_as(
        r#"
        INSERT INTO artworks (
            artist_id, title, description, year_created, medium,
            dimensions, price_cents, currency, availability,
            external_url, status
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'draft')
        RETURNING
            id, title, status, medium, price_cents, currency, availability,
            NULL::text AS primary_s3_key,
            created_at, updated_at, published_at
        "#,
    )
    .bind(artist_id)
    .bind(title)
    .bind(description)
    .bind(body.year_created)
    .bind(body.medium.as_deref())
    .bind(&dimensions)
    .bind(body.price_cents)
    .bind(currency)
    .bind(availability)
    .bind(body.external_url.as_deref())
    .fetch_one(&state.pool)
    .await?;

    Ok((StatusCode::CREATED, Json(row.into_summary())))
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/studio/artworks/:id
// ─────────────────────────────────────────────────────────────────────────────

pub async fn detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<StudioArtworkDetail>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    let row: Option<ArtworkDetailRow> = sqlx::query_as(
        r#"
        SELECT
            a.id, a.title, a.status, a.medium,
            a.price_cents, a.currency, a.availability,
            a.description, a.year_created, a.dimensions, a.external_url,
            a.created_at, a.updated_at, a.published_at
        FROM artworks a
        WHERE a.id = $1
          AND a.artist_id = $2
          AND a.deleted_at IS NULL
        "#,
    )
    .bind(id)
    .bind(artist_id)
    .fetch_optional(&state.pool)
    .await?;
    let row = row.ok_or(ApiError::NotFound)?;

    let images = fetch_images(&state.pool, id).await?;

    Ok(Json(row.into_detail(images)))
}

// ─────────────────────────────────────────────────────────────────────────────
// PATCH /v1/studio/artworks/:id
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PatchArtwork {
    // Each clearable field uses `deserialize_double_option` so an
    // explicit JSON `null` lands as `Some(None)` (clear column),
    // distinct from "key absent" (`None` — leave column alone).
    // Without the helper, serde's default would collapse both into
    // `None` and the "clear via null" branch in the SQL CASE WHEN
    // below would silently never fire. See
    // `api/crates/api-search/src/serde_helpers.rs` for the contract.
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::deserialize_double_option"
    )]
    pub title: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::deserialize_double_option"
    )]
    pub description: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::deserialize_double_option"
    )]
    pub year_created: Option<Option<i32>>,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::deserialize_double_option"
    )]
    pub medium: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::deserialize_double_option"
    )]
    pub dimensions: Option<Option<serde_json::Value>>,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::deserialize_double_option"
    )]
    pub price_cents: Option<Option<i64>>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub availability: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::deserialize_double_option"
    )]
    pub external_url: Option<Option<String>>,
    /// `draft` ↔ `published` ↔ `archived`. Setting `published` from
    /// `draft` stamps `published_at = now()` (handled in SQL via COALESCE).
    #[serde(default)]
    pub status: Option<String>,
}

pub async fn patch(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    AuthedUser(user): AuthedUser,
    Json(body): Json<PatchArtwork>,
) -> Result<Json<StudioArtworkSummary>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    if let Some(Some(t)) = &body.title {
        if t.trim().len() > MAX_TITLE_LEN {
            return Err(ApiError::BadRequest(format!(
                "title exceeds {MAX_TITLE_LEN}-char limit"
            )));
        }
    }
    if let Some(s) = &body.status {
        if !VALID_STATUS.contains(&s.as_str()) {
            return Err(ApiError::BadRequest(format!("invalid status `{s}`")));
        }
    }
    if let Some(a) = &body.availability {
        if !VALID_AVAILABILITY.contains(&a.as_str()) {
            return Err(ApiError::BadRequest(format!("invalid availability `{a}`")));
        }
    }

    // T-070 — validate dimensions when the patch carries a value.
    // T-072 — `deserialize_double_option` on the struct now means
    // `Some(None)` (explicit null) is distinct from `None` (absent),
    // so the three patch states all behave correctly:
    //   - body.dimensions = None         → leave column alone
    //   - body.dimensions = Some(None)   → SET dimensions = NULL  (clear)
    //   - body.dimensions = Some(Some(v))→ SET dimensions = v     (update)
    // Validation only fires for the third case.
    let dimensions_present = body.dimensions.is_some();
    let dimensions_value: Option<serde_json::Value> = match body.dimensions {
        Some(Some(ref v)) => {
            Some(ml_art_core::validation::dimensions_v1(v).map_err(ApiError::BadRequest)?)
        }
        _ => None,
    };

    // Bool-flag-per-Option<Option<_>> pattern lets the caller explicitly
    // set a field to NULL by passing JSON `null`, vs not touching it by
    // omitting the key. Same shape as `me/collections::patch`.
    let row: Option<ArtworkRow> = sqlx::query_as(
        r#"
        UPDATE artworks SET
            title         = CASE WHEN $3::boolean THEN $4 ELSE title END,
            description   = CASE WHEN $5::boolean THEN $6 ELSE description END,
            year_created  = CASE WHEN $7::boolean THEN $8::int ELSE year_created END,
            medium        = CASE WHEN $9::boolean THEN $10 ELSE medium END,
            dimensions    = CASE WHEN $11::boolean THEN $12::jsonb ELSE dimensions END,
            price_cents   = CASE WHEN $13::boolean THEN $14::bigint ELSE price_cents END,
            currency      = COALESCE($15, currency),
            availability  = COALESCE($16, availability),
            external_url  = CASE WHEN $17::boolean THEN $18 ELSE external_url END,
            status        = COALESCE($19, status),
            published_at  = CASE
                                WHEN $19 = 'published' AND published_at IS NULL
                                THEN now()
                                ELSE published_at
                            END,
            updated_at    = now()
        WHERE id = $1
          AND artist_id = $2
          AND deleted_at IS NULL
        RETURNING
            id, title, status, medium, price_cents, currency, availability,
            (SELECT s3_key FROM artwork_images
              WHERE artwork_id = artworks.id AND is_primary) AS primary_s3_key,
            created_at, updated_at, published_at
        "#,
    )
    .bind(id)
    .bind(artist_id)
    .bind(body.title.is_some())
    .bind(body.title.flatten().map(|s| s.trim().to_string()))
    .bind(body.description.is_some())
    .bind(body.description.flatten().map(|s| s.trim().to_string()))
    .bind(body.year_created.is_some())
    .bind(body.year_created.flatten())
    .bind(body.medium.is_some())
    .bind(body.medium.flatten())
    .bind(dimensions_present)
    .bind(dimensions_value)
    .bind(body.price_cents.is_some())
    .bind(body.price_cents.flatten())
    .bind(body.currency.as_deref())
    .bind(body.availability.as_deref())
    .bind(body.external_url.is_some())
    .bind(body.external_url.flatten())
    .bind(body.status.as_deref())
    .fetch_optional(&state.pool)
    .await?;

    let row = row.ok_or(ApiError::NotFound)?;
    Ok(Json(row.into_summary()))
}

// ─────────────────────────────────────────────────────────────────────────────
// DELETE /v1/studio/artworks/:id   (soft-delete)
// ─────────────────────────────────────────────────────────────────────────────

pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    AuthedUser(user): AuthedUser,
) -> Result<StatusCode, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;
    let res = sqlx::query(
        r#"
        UPDATE artworks SET deleted_at = now()
        WHERE id = $1
          AND artist_id = $2
          AND deleted_at IS NULL
        "#,
    )
    .bind(id)
    .bind(artist_id)
    .execute(&state.pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/studio/artworks/:id/images
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AddImage {
    /// S3/MinIO key, e.g. `uploads/<uuid>.jpg`. Validation that this key
    /// was actually minted by our upload endpoint waits on T-010.
    pub s3_key: String,
    #[serde(default)]
    pub is_primary: Option<bool>,
    #[serde(default)]
    pub display_order: Option<i32>,
    #[serde(default)]
    pub width: Option<i32>,
    #[serde(default)]
    pub height: Option<i32>,
}

pub async fn add_image(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    AuthedUser(user): AuthedUser,
    Json(body): Json<AddImage>,
) -> Result<(StatusCode, Json<StudioImage>), ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    // Confirm the artwork exists and is owned by this artist before
    // writing anything.
    let owned: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM artworks
           WHERE id = $1 AND artist_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(id)
    .bind(artist_id)
    .fetch_optional(&state.pool)
    .await?;
    if owned.is_none() {
        return Err(ApiError::NotFound);
    }

    let s3_key = body.s3_key.trim();
    if s3_key.is_empty() {
        return Err(ApiError::BadRequest("s3_key must not be empty".into()));
    }

    // First image lands as primary unless the caller says otherwise; any
    // other image defaults to non-primary. The DB has a partial UNIQUE
    // index `(artwork_id) WHERE is_primary` so a conflicting "make this
    // primary too" would fail at INSERT time; callers must demote the
    // existing primary first (PATCH workflow, future ticket).
    let existing_primary: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM artwork_images
           WHERE artwork_id = $1 AND is_primary"#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let is_primary = body.is_primary.unwrap_or(existing_primary.is_none());
    if is_primary && existing_primary.is_some() {
        return Err(ApiError::BadRequest(
            "primary image already set; demote it first".into(),
        ));
    }

    let display_order = body.display_order.unwrap_or(0);

    // Prefer pixel dimensions from the `uploads` row over anything
    // the client sent. The api probes bytes on `/v1/uploads/image`
    // (header-only via the `imagesize` crate), so the upload row is
    // the trustworthy source — client-supplied dims could be spoofed
    // and there's no reason to accept them. Falls back to body.width
    // / body.height when the s3_key isn't from an upload (e.g. seed
    // imports under `demo/`) or the upload row predates migration
    // 0020 and has NULL dims.
    let (width, height) = if s3_key.starts_with("uploads/") {
        let from_upload: Option<(Option<i32>, Option<i32>)> =
            sqlx::query_as("SELECT width, height FROM uploads WHERE s3_key = $1")
                .bind(s3_key)
                .fetch_optional(&state.pool)
                .await?;
        match from_upload {
            Some((Some(w), Some(h))) => (Some(w), Some(h)),
            _ => (body.width, body.height),
        }
    } else {
        (body.width, body.height)
    };

    let row: ImageRow = sqlx::query_as(
        r#"
        INSERT INTO artwork_images
            (artwork_id, s3_key, width, height, is_primary, display_order)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING
            id, s3_key, width, height, is_primary, display_order, moderation_status, moderation_reason
        "#,
    )
    .bind(id)
    .bind(s3_key)
    .bind(width)
    .bind(height)
    .bind(is_primary)
    .bind(display_order)
    .fetch_one(&state.pool)
    .await?;

    // Whenever the *primary* image lands, generate the artwork embedding
    // so vector search can find the work. Non-primary images don't
    // change the artwork's vector representation.
    //
    // Fast path: when the s3_key comes from `/v1/uploads/image` (prefix
    // `uploads/`), the `uploads` row already has the embedding — same
    // bytes, same model, same version. Copy it rather than re-embedding
    // via Jina. This (a) avoids paying for the embed twice, (b) bypasses
    // the dev-only "Jina can't reach localhost MinIO" limitation, and
    // (c) is just a SQL SELECT instead of an HTTP round trip.
    //
    // Slow path: any other s3_key (seed data via the WikiArt importer,
    // or a future direct artworks-bucket write) goes through the
    // standard URL embed.
    if is_primary && state.embedder.enabled() {
        let copied = if s3_key.starts_with("uploads/") {
            try_copy_embedding_from_upload(&state.pool, &state.embedder, s3_key, id).await?
        } else {
            false
        };
        if !copied {
            let image_url = url_for_s3_key(s3_key);
            artwork_embeddings::process_image(&state.pool, &state.embedder, id, &image_url)
                .await
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("embed failed: {e}")))?;
        }
    }

    // T-008: enqueue a moderation job. The row lands as
    // `moderation_status='pending'`; the worker flips it to
    // `approved` or `rejected`. Idempotency key on the image id so
    // double-enqueue (retry, duplicate POST) dedups in the `jobs` table.
    state
        .jobs
        .enqueue(
            JobEvent::ArtworkImageModerate {
                artwork_image_id: row.id,
            },
            EnqueueOpts {
                idempotency_key: Some(format!("moderate:artwork_image:{}", row.id)),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("enqueue moderation: {e}")))?;

    Ok((StatusCode::CREATED, Json(row.into_studio_image())))
}

/// Try to copy an existing `uploads.embedding` into `artwork_embeddings`
/// for this artwork. Returns `Ok(true)` when the copy happened,
/// `Ok(false)` when there's no matching upload row (or it has no
/// embedding yet — shouldn't happen since the upload endpoint embeds
/// inline, but we're defensive).
///
/// Used to avoid re-embedding bytes we just embedded in
/// `/v1/uploads/image`. The (model_name, model_version) on the upload
/// row matches the current embedder's (asserted via the read of those
/// accessors), so the embedding is directly portable.
async fn try_copy_embedding_from_upload(
    pool: &ml_art_core::db::Pool,
    embedder: &ml_art_core::embedder::Embedder,
    s3_key: &str,
    artwork_id: Uuid,
) -> Result<bool, ApiError> {
    use pgvector::Vector;
    let row: Option<(Option<Vector>,)> =
        sqlx::query_as(r#"SELECT embedding FROM uploads WHERE s3_key = $1"#)
            .bind(s3_key)
            .fetch_optional(pool)
            .await?;
    let Some((Some(vector),)) = row else {
        return Ok(false);
    };
    artwork_embeddings::write(
        pool,
        artwork_id,
        embedder.model_name(),
        embedder.model_version(),
        &vector,
    )
    .await?;
    Ok(true)
}

// ─────────────────────────────────────────────────────────────────────────────
// DELETE /v1/studio/artworks/:id/images/:image_id
// ─────────────────────────────────────────────────────────────────────────────

pub async fn remove_image(
    State(state): State<Arc<AppState>>,
    Path((id, image_id)): Path<(Uuid, Uuid)>,
    AuthedUser(user): AuthedUser,
) -> Result<StatusCode, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    // Delete only if the parent artwork is owned by this artist.
    let res = sqlx::query(
        r#"
        DELETE FROM artwork_images ai
        USING artworks a
        WHERE ai.id = $1
          AND a.id = ai.artwork_id
          AND a.artist_id = $2
          AND a.deleted_at IS NULL
          AND ai.artwork_id = $3
        "#,
    )
    .bind(image_id)
    .bind(artist_id)
    .bind(id)
    .execute(&state.pool)
    .await?;

    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers + row types
// ─────────────────────────────────────────────────────────────────────────────

async fn fetch_images(pool: &sqlx::PgPool, artwork_id: Uuid) -> Result<Vec<StudioImage>, ApiError> {
    let rows: Vec<ImageRow> = sqlx::query_as(
        r#"
        SELECT id, s3_key, width, height, is_primary, display_order, moderation_status, moderation_reason
        FROM artwork_images
        WHERE artwork_id = $1
        ORDER BY is_primary DESC, display_order ASC
        "#,
    )
    .bind(artwork_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(ImageRow::into_studio_image).collect())
}

#[derive(FromRow)]
struct ArtworkRow {
    id: Uuid,
    title: Option<String>,
    status: String,
    medium: Option<String>,
    price_cents: Option<i64>,
    currency: String,
    availability: String,
    primary_s3_key: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    published_at: Option<DateTime<Utc>>,
}

impl ArtworkRow {
    fn into_summary(self) -> StudioArtworkSummary {
        StudioArtworkSummary {
            id: self.id,
            title: self.title,
            status: self.status,
            medium: self.medium,
            price_cents: self.price_cents,
            currency: self.currency,
            availability: self.availability,
            primary_image_url: self.primary_s3_key.map(|k| url_for_s3_key(&k)),
            created_at: self.created_at,
            updated_at: self.updated_at,
            published_at: self.published_at,
        }
    }
}

#[derive(FromRow)]
struct ArtworkDetailRow {
    id: Uuid,
    title: Option<String>,
    status: String,
    medium: Option<String>,
    price_cents: Option<i64>,
    currency: String,
    availability: String,
    description: Option<String>,
    year_created: Option<i32>,
    dimensions: Option<serde_json::Value>,
    external_url: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    published_at: Option<DateTime<Utc>>,
}

impl ArtworkDetailRow {
    fn into_detail(self, images: Vec<StudioImage>) -> StudioArtworkDetail {
        // Derive `primary_image_url` from the images list so callers can
        // render the summary section without rummaging through `images`.
        let primary_image_url = images.iter().find(|i| i.is_primary).map(|i| i.url.clone());
        StudioArtworkDetail {
            summary: StudioArtworkSummary {
                id: self.id,
                title: self.title,
                status: self.status,
                medium: self.medium,
                price_cents: self.price_cents,
                currency: self.currency,
                availability: self.availability,
                primary_image_url,
                created_at: self.created_at,
                updated_at: self.updated_at,
                published_at: self.published_at,
            },
            description: self.description,
            year_created: self.year_created,
            dimensions: self.dimensions,
            external_url: self.external_url,
            images,
        }
    }
}

#[derive(FromRow)]
struct ImageRow {
    id: Uuid,
    s3_key: String,
    width: Option<i32>,
    height: Option<i32>,
    is_primary: bool,
    display_order: i32,
    moderation_status: String,
    moderation_reason: Option<String>,
}

impl ImageRow {
    fn into_studio_image(self) -> StudioImage {
        StudioImage {
            url: url_for_s3_key(&self.s3_key),
            id: self.id,
            s3_key: self.s3_key,
            width: self.width,
            height: self.height,
            is_primary: self.is_primary,
            display_order: self.display_order,
            moderation_status: self.moderation_status,
            moderation_reason: self.moderation_reason,
        }
    }
}
