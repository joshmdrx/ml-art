//! `POST /v1/uploads/image` — visual search (and future studio) upload entry point.
//!
//! T-010 Phase A. Accepts a multipart image, validates it, PUTs to the
//! `uploads/` bucket, writes a row to `uploads`, embeds inline via the
//! T-036 pipeline, and returns enough info for the client to either
//! drop the upload into a search query or attach it to an artwork in
//! the studio.
//!
//! Identity:
//!   - Signed-in: `user_id` populated, `anonymous_id` NULL
//!   - Anonymous: `anonymous_id` from `X-Anonymous-Id`, `user_id` NULL
//!   - Both: we tolerate either; the cleanup job (future) uses
//!     `expires_at` to evict transient anon uploads
//!
//! Limits:
//!   - 10MB body cap (set at the route layer; we re-check after parse
//!     because the Multipart streamer doesn't know our cap upfront)
//!   - Content-type allowlist: jpeg / png / webp. Anything else 400s
//!
//! Embedding:
//!   - The handler calls `embedder.embed_image_from_url` against the
//!     freshly-PUT object. Jina's worker fetches it from MinIO /
//!     CloudFront, so the public URL must already be reachable
//!   - The vector is written into `uploads.embedding` on the same row.
//!     Visual search (`T-010` Phase B) reads from there
//!   - **Dev limitation:** `http://localhost:9000` is not reachable from
//!     Jina's workers, so live uploads from dev hit a 502 at the embed
//!     step (DB row + S3 PUT succeed; only the embed UPDATE doesn't
//!     fire). For real end-to-end work in dev, tunnel MinIO publicly
//!     (ngrok / cloudflared) and override `UPLOADS_PUBLIC_URL_PREFIX`.
//!     The cleanup job (`expires_at`-driven) evicts orphaned rows
//!
//! Rate limiting: the route attaches the existing `inquiry_limit`
//! policy at 20/hr (per the `03-api-data-spec.md` table for
//! `/uploads/image`). Implemented separately in `lib.rs`.

use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    Json,
};
use ml_art_core::{
    auth::{OptionalAnonId, User},
    error::ApiError,
};
use pgvector::Vector;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AuthedUser;
use crate::AppState;

const MAX_BYTES: usize = 10 * 1024 * 1024; // 10 MB
const ALLOWED_CONTENT_TYPES: &[&str] = &["image/jpeg", "image/png", "image/webp"];

/// Response from a successful upload. `image_url` lets the client
/// preview the upload immediately; `upload_id` is what the search
/// endpoint accepts in Phase B (`?image_upload_id=<id>`).
#[derive(Serialize)]
pub struct UploadAck {
    pub upload_id: Uuid,
    pub s3_key: String,
    pub image_url: String,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: Option<AuthedUser>,
    OptionalAnonId(anon_id): OptionalAnonId,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<UploadAck>), ApiError> {
    // Image embedding is required (not best-effort), so refuse the
    // upload up front if we'd just write a row with no embedding.
    if !state.embedder.enabled() {
        return Err(ApiError::Internal(anyhow::anyhow!(
            "embedder not configured; refusing upload"
        )));
    }

    let user: Option<User> = auth.map(|AuthedUser(u)| u);

    // Pull the file field. The form name is `image`; anything else
    // 400s. We only honor the first matching field (no batch uploads
    // through this endpoint).
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut content_type: Option<String> = None;
    let mut filename_ext: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(format!("malformed multipart: {e}")))?
    {
        if field.name() == Some("image") {
            content_type = field.content_type().map(|s| s.to_string());
            filename_ext = field
                .file_name()
                .and_then(|n| n.rsplit('.').next())
                .map(|e| e.to_lowercase());
            let bytes = field
                .bytes()
                .await
                .map_err(|e| ApiError::BadRequest(format!("reading upload bytes: {e}")))?;
            if bytes.len() > MAX_BYTES {
                return Err(ApiError::BadRequest(format!(
                    "upload exceeds {MAX_BYTES}-byte limit"
                )));
            }
            file_bytes = Some(bytes.to_vec());
            break;
        }
    }

    let bytes = file_bytes.ok_or_else(|| {
        ApiError::BadRequest(
            "missing `image` field; expected multipart form with one file part".into(),
        )
    })?;
    let content_type = content_type
        .ok_or_else(|| ApiError::BadRequest("upload field has no content-type".into()))?;
    if !ALLOWED_CONTENT_TYPES.contains(&content_type.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "content-type `{content_type}` not allowed; need jpeg / png / webp"
        )));
    }
    if bytes.is_empty() {
        return Err(ApiError::BadRequest("upload is empty".into()));
    }

    // Mint the row + s3_key first so we have an id to PUT against.
    // Extension comes from the original filename if it looks
    // reasonable, otherwise we derive it from content_type.
    let upload_id = Uuid::new_v4();
    let ext = match filename_ext.as_deref() {
        Some(e) if e == "jpg" || e == "jpeg" || e == "png" || e == "webp" => e.to_string(),
        _ => match content_type.as_str() {
            "image/jpeg" => "jpg".to_string(),
            "image/png" => "png".to_string(),
            "image/webp" => "webp".to_string(),
            // Validated above; the catch-all is defensive.
            _ => unreachable!(),
        },
    };
    let s3_key = format!("uploads/{upload_id}.{ext}");

    // PUT to S3/MinIO before we touch the DB — if S3 fails, we don't
    // want an orphan `uploads` row.
    state
        .object_store
        .put(&s3_key, bytes, &content_type)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("s3 put: {e}")))?;

    // Insert the upload row. Embedding stays NULL until the Jina call
    // succeeds (next step).
    sqlx::query(
        r#"
        INSERT INTO uploads (id, s3_key, anonymous_id, user_id)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(upload_id)
    .bind(&s3_key)
    .bind(anon_id)
    .bind(user.as_ref().map(|u| u.id))
    .execute(&state.pool)
    .await?;

    // Embed inline. Same pattern as T-036's `process_image` but the
    // destination row is `uploads`, not `artwork_embeddings`, so we
    // call the embedder directly and UPDATE.
    let image_url = state.object_store.public_url(&s3_key);
    let vector: Vector = state
        .embedder
        .embed_image_from_url(&image_url)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("embed upload: {e}")))?;
    sqlx::query("UPDATE uploads SET embedding = $1 WHERE id = $2")
        .bind(&vector)
        .bind(upload_id)
        .execute(&state.pool)
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(UploadAck {
            upload_id,
            s3_key,
            image_url,
        }),
    ))
}
