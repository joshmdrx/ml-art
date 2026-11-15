//! T-086 — consent-first draft profiles (admin side).
//!
//! When a gallery or studio replies "yes, show me an example", an admin
//! builds a draft profile for them, edits it through the ordinary studio
//! UI, then sends a private preview link. The organisation claims the
//! page (it becomes theirs, still unpublished) or removes it. Nothing
//! about a draft is ever public until they claim and publish it.
//! See decisions.md 2026-11-15 — "Revive the claim flow, consent-first".
//!
//! Endpoints, all `AdminUser`-gated and audited:
//!   - `GET    /v1/admin/drafts`                   — list drafts, newest first
//!   - `POST   /v1/admin/drafts`                   — create (consent note required)
//!   - `POST   /v1/admin/drafts/:id/edit`          — start editing it in the studio
//!   - `DELETE /v1/admin/drafts/editing`           — stop editing
//!   - `POST   /v1/admin/drafts/:id/preview-link`  — mint a private link (90 days)
//!   - `POST   /v1/admin/drafts/:id/takedown`      — remove an unclaimed draft
//!
//! Invariants enforced elsewhere: drafts are excluded from the artist
//! approval queue and can't be made `active` while unclaimed
//! (`admin::artists::transition`, `studio::settings::patch`). The
//! public claim / remove side lives in `crate::preview`.

use crate::extractors::AdminUser;
use crate::onboarding::generate_unique_slug;
use crate::studio::is_unclaimed_draft;
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use ml_art_core::{
    admin::{action, audit, target},
    db::Pool,
    error::ApiError,
    preview_token,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

const MAX_DISPLAY_NAME_LEN: usize = 100;
const MAX_LOCATION_LEN: usize = 200;
const MAX_WEBSITE_LEN: usize = 500;
const MAX_CONSENT_NOTE_LEN: usize = 500;
/// Preview links expire after this many days (99-deferred.md spec).
pub const PREVIEW_LINK_DAYS: i64 = 90;

// ─────────────────────────────────────────────────────────────────────────────
// Wire shapes
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, FromRow)]
pub struct DraftItem {
    pub id: Uuid,
    pub slug: String,
    pub display_name: String,
    pub entity_type: String,
    pub location: Option<String>,
    pub website_url: Option<String>,
    pub consent_note: String,
    /// True once the organisation has claimed it (it's theirs now).
    pub claimed: bool,
    pub status: String,
    pub artwork_count: i64,
    /// Expiry of the current live preview link, if there is one.
    pub live_link_expires_at: Option<DateTime<Utc>>,
    /// True when the calling admin is editing this draft in the studio.
    pub editing: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub items: Vec<DraftItem>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub display_name: String,
    /// "gallery" (default — drafts are for organisations) or "individual".
    #[serde(default)]
    pub entity_type: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub website_url: Option<String>,
    /// How the organisation agreed to an example page. Required.
    pub consent_note: String,
}

#[derive(Debug, Serialize)]
pub struct EditingResponse {
    pub editing_draft_artist_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct PreviewLinkResponse {
    /// Plaintext token — shown once; only its hash is stored.
    pub token: String,
    /// Web path to send to the organisation: `/preview/<token>`.
    pub path: String,
    pub expires_at: DateTime<Utc>,
}

/// Audit snapshot. Never includes a token.
#[derive(Debug, Serialize)]
struct DraftAudit<'a> {
    id: Uuid,
    display_name: &'a str,
    consent_note: &'a str,
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

pub async fn list(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
) -> Result<Json<ListResponse>, ApiError> {
    let items = fetch_drafts(&state.pool, admin.id, None).await?;
    Ok(Json(ListResponse { items }))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Json(body): Json<CreateBody>,
) -> Result<(StatusCode, Json<DraftItem>), ApiError> {
    let display_name = body.display_name.trim();
    if display_name.is_empty() {
        return Err(ApiError::BadRequest("display_name is required".into()));
    }
    if display_name.chars().count() > MAX_DISPLAY_NAME_LEN {
        return Err(ApiError::BadRequest(format!(
            "display_name exceeds {MAX_DISPLAY_NAME_LEN}-char limit"
        )));
    }
    let consent_note = body.consent_note.trim();
    if consent_note.is_empty() {
        return Err(ApiError::BadRequest(
            "consent_note is required: record how the organisation agreed to an example page"
                .into(),
        ));
    }
    if consent_note.chars().count() > MAX_CONSENT_NOTE_LEN {
        return Err(ApiError::BadRequest(format!(
            "consent_note exceeds {MAX_CONSENT_NOTE_LEN}-char limit"
        )));
    }
    let entity_type = body
        .entity_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("gallery");
    if !matches!(entity_type, "individual" | "gallery") {
        return Err(ApiError::BadRequest(
            "entity_type must be 'individual' or 'gallery'".into(),
        ));
    }
    let location = body
        .location
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if location.is_some_and(|l| l.chars().count() > MAX_LOCATION_LEN) {
        return Err(ApiError::BadRequest(format!(
            "location exceeds {MAX_LOCATION_LEN}-char limit"
        )));
    }
    let website_url = body
        .website_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(u) = website_url {
        if u.chars().count() > MAX_WEBSITE_LEN {
            return Err(ApiError::BadRequest(format!(
                "website_url exceeds {MAX_WEBSITE_LEN}-char limit"
            )));
        }
        if !u.starts_with("http://") && !u.starts_with("https://") {
            return Err(ApiError::BadRequest(
                "website_url must start with http:// or https://".into(),
            ));
        }
    }

    let slug = generate_unique_slug(&state.pool, display_name).await?;

    // Mint the id up front so the audit row (consent trail included) is
    // written before the draft exists, matching every other admin
    // mutation. If the audit write fails, no draft is created.
    let id = Uuid::new_v4();
    audit::record(
        &state.pool,
        Some(admin.id),
        action::DRAFT_CREATE,
        target::ARTIST,
        Some(id),
        None::<&()>,
        Some(&DraftAudit {
            id,
            display_name,
            consent_note,
        }),
        None,
    )
    .await?;

    sqlx::query(
        r#"
        INSERT INTO artists (
            id, user_id, slug, display_name, location, website_url,
            inquiry_preferences, status, entity_type,
            is_prebuilt, prebuilt_consent_note
        )
        VALUES ($1, NULL, $2, $3, $4, $5, '{"type":"platform"}'::jsonb, 'pending', $6, true, $7)
        "#,
    )
    .bind(id)
    .bind(&slug)
    .bind(display_name)
    .bind(location)
    .bind(website_url)
    .bind(entity_type)
    .bind(consent_note)
    .execute(&state.pool)
    .await?;

    let item = fetch_drafts(&state.pool, admin.id, Some(id))
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("draft {id} vanished after insert")))?;
    Ok((StatusCode::CREATED, Json(item)))
}

pub async fn start_editing(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<EditingResponse>, ApiError> {
    require_unclaimed_draft(&state.pool, id).await?;
    audit::record(
        &state.pool,
        Some(admin.id),
        action::DRAFT_EDIT_START,
        target::ARTIST,
        Some(id),
        None::<&()>,
        None::<&()>,
        None,
    )
    .await?;
    sqlx::query("UPDATE users SET editing_draft_artist_id = $2, updated_at = now() WHERE id = $1")
        .bind(admin.id)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(EditingResponse {
        editing_draft_artist_id: Some(id),
    }))
}

/// Not audited: ending your own editing session changes no data.
pub async fn stop_editing(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
) -> Result<Json<EditingResponse>, ApiError> {
    sqlx::query(
        "UPDATE users SET editing_draft_artist_id = NULL, updated_at = now() WHERE id = $1",
    )
    .bind(admin.id)
    .execute(&state.pool)
    .await?;
    Ok(Json(EditingResponse {
        editing_draft_artist_id: None,
    }))
}

pub async fn create_preview_link(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<PreviewLinkResponse>), ApiError> {
    require_unclaimed_draft(&state.pool, id).await?;

    let token = preview_token::generate();
    let expires_at = Utc::now() + Duration::days(PREVIEW_LINK_DAYS);

    audit::record(
        &state.pool,
        Some(admin.id),
        action::DRAFT_PREVIEW_LINK,
        target::ARTIST,
        Some(id),
        None::<&()>,
        Some(&serde_json::json!({ "expires_at": expires_at })),
        None,
    )
    .await?;

    // One live link per draft: revoke earlier ones, then insert, in one
    // transaction so there's never a moment with two live links.
    let mut tx = state.pool.begin().await?;
    revoke_live_links(&mut tx, id).await?;
    sqlx::query(
        r#"
        INSERT INTO artist_preview_links (artist_id, token_hash, created_by, expires_at)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(id)
    .bind(preview_token::hash(&token))
    .bind(admin.id)
    .bind(expires_at)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let path = format!("/preview/{token}");
    Ok((
        StatusCode::CREATED,
        Json(PreviewLinkResponse {
            token,
            path,
            expires_at,
        }),
    ))
}

pub async fn takedown(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    // Claimed drafts belong to the organisation now; they use the normal
    // pause / delete paths, not this one.
    require_unclaimed_draft(&state.pool, id).await?;
    audit::record(
        &state.pool,
        Some(admin.id),
        action::DRAFT_TAKEDOWN,
        target::ARTIST,
        Some(id),
        None::<&()>,
        None::<&()>,
        Some(serde_json::json!({ "via": "admin" })),
    )
    .await?;
    remove_draft(&state.pool, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers (also used by `crate::preview`)
// ─────────────────────────────────────────────────────────────────────────────

/// 404 unless `id` is an existing draft nobody has claimed.
async fn require_unclaimed_draft(pool: &Pool, id: Uuid) -> Result<(), ApiError> {
    if is_unclaimed_draft(pool, id).await? {
        Ok(())
    } else {
        Err(ApiError::NotFound)
    }
}

/// Revoke every live preview link for a draft.
pub(crate) async fn revoke_live_links(
    conn: &mut sqlx::PgConnection,
    artist_id: Uuid,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        UPDATE artist_preview_links
        SET revoked_at = now(), updated_at = now()
        WHERE artist_id = $1 AND revoked_at IS NULL AND claimed_at IS NULL
        "#,
    )
    .bind(artist_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Soft-delete an unclaimed draft and its artworks, revoke its links and
/// end any admin editing session pointing at it. One transaction.
/// Image files are left for the unclaimed-draft purge job (ticket T-087).
pub(crate) async fn remove_draft(pool: &Pool, artist_id: Uuid) -> Result<(), ApiError> {
    let mut tx = pool.begin().await?;
    let removed = sqlx::query(
        r#"
        UPDATE artists SET deleted_at = now(), updated_at = now()
        WHERE id = $1 AND is_prebuilt AND user_id IS NULL AND deleted_at IS NULL
        "#,
    )
    .bind(artist_id)
    .execute(&mut *tx)
    .await?;
    if removed.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    sqlx::query(
        "UPDATE artworks SET deleted_at = now(), updated_at = now()
         WHERE artist_id = $1 AND deleted_at IS NULL",
    )
    .bind(artist_id)
    .execute(&mut *tx)
    .await?;
    revoke_live_links(&mut tx, artist_id).await?;
    clear_editing_sessions(&mut tx, artist_id).await?;
    tx.commit().await?;
    Ok(())
}

/// End every admin editing session that points at this draft.
pub(crate) async fn clear_editing_sessions(
    conn: &mut sqlx::PgConnection,
    artist_id: Uuid,
) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE users SET editing_draft_artist_id = NULL, updated_at = now()
         WHERE editing_draft_artist_id = $1",
    )
    .bind(artist_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Drafts (claimed or not), newest first; optionally a single one.
async fn fetch_drafts(
    pool: &Pool,
    admin_id: Uuid,
    only: Option<Uuid>,
) -> Result<Vec<DraftItem>, ApiError> {
    Ok(sqlx::query_as(
        r#"
        SELECT
            ar.id, ar.slug, ar.display_name, ar.entity_type,
            ar.location, ar.website_url,
            COALESCE(ar.prebuilt_consent_note, '') AS consent_note,
            (ar.user_id IS NOT NULL) AS claimed,
            ar.status,
            COALESCE(c.cnt, 0)::bigint AS artwork_count,
            l.expires_at AS live_link_expires_at,
            (u.editing_draft_artist_id IS NOT DISTINCT FROM ar.id) AS editing,
            ar.created_at
        FROM artists ar
        JOIN users u ON u.id = $1
        LEFT JOIN (
            SELECT artist_id, COUNT(*) AS cnt
            FROM artworks
            WHERE deleted_at IS NULL
            GROUP BY artist_id
        ) c ON c.artist_id = ar.id
        LEFT JOIN LATERAL (
            SELECT expires_at
            FROM artist_preview_links
            WHERE artist_id = ar.id
              AND revoked_at IS NULL
              AND claimed_at IS NULL
              AND expires_at > now()
            ORDER BY created_at DESC
            LIMIT 1
        ) l ON true
        WHERE ar.is_prebuilt
          AND ar.deleted_at IS NULL
          AND ($2::uuid IS NULL OR ar.id = $2)
        ORDER BY ar.created_at DESC
        LIMIT 200
        "#,
    )
    .bind(admin_id)
    .bind(only)
    .fetch_all(pool)
    .await?)
}
