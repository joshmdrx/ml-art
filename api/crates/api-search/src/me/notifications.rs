//! T-068 — `/v1/me/notification-preferences` GET + PATCH.
//!
//! The settings UI fetches a full preference map (all user-facing
//! kinds, defaults filled in) + the master kill switch, then sends
//! diffs back via PATCH. PATCH is a partial update — omitted fields
//! aren't touched, so the client can submit just the toggle the user
//! flipped without round-tripping the whole map.
//!
//! See `core::notifications` for the kind enum + `user_wants` helper
//! that callers use to gate sends.

use axum::{extract::State, http::StatusCode, Json};
use ml_art_core::{error::ApiError, notifications::NotificationKind};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::extractors::AuthedUser;
use crate::AppState;

/// Response shape — what the settings UI reads on first load.
/// `kinds` always contains every user-facing kind, with default=true
/// filled in where the user hasn't overridden.
#[derive(Debug, Serialize)]
pub struct PreferencesResponse {
    /// Master kill switch. False = no notification emails of any kind
    /// (transactional still goes through).
    pub global_enabled: bool,
    /// Per-kind state. Keys are the snake_case kind names.
    pub kinds: HashMap<String, bool>,
}

/// Request shape for PATCH. Both fields optional — omit one to leave
/// it untouched. `kinds` is a sparse map; only the entries you send
/// get upserted.
#[derive(Debug, Deserialize)]
pub struct PreferencesPatch {
    #[serde(default)]
    pub global_enabled: Option<bool>,
    #[serde(default)]
    pub kinds: Option<HashMap<String, bool>>,
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<PreferencesResponse>, ApiError> {
    let prefs = load_preferences(&state.pool, user.id).await?;
    Ok(Json(prefs))
}

pub async fn patch(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Json(body): Json<PreferencesPatch>,
) -> Result<Json<PreferencesResponse>, ApiError> {
    // Validate every kind in the request before touching the DB —
    // partial application of a bad payload would be worse than
    // rejecting the whole thing.
    if let Some(kinds) = &body.kinds {
        for kind_str in kinds.keys() {
            let kind = NotificationKind::from_str(kind_str)
                .map_err(|_| ApiError::BadRequest("unknown notification kind".into()))?;
            if kind.is_transactional() {
                return Err(ApiError::BadRequest(
                    "transactional kinds can't be opted out of".into(),
                ));
            }
        }
    }

    let mut tx = state.pool.begin().await?;

    if let Some(global) = body.global_enabled {
        sqlx::query("UPDATE users SET global_email_notifications_enabled = $1 WHERE id = $2")
            .bind(global)
            .bind(user.id)
            .execute(&mut *tx)
            .await?;
    }

    if let Some(kinds) = body.kinds {
        for (kind_str, enabled) in kinds {
            sqlx::query(
                r#"
                INSERT INTO notification_preferences (user_id, kind, enabled, updated_at)
                VALUES ($1, $2, $3, now())
                ON CONFLICT (user_id, kind) DO UPDATE
                    SET enabled = $3, updated_at = now()
                "#,
            )
            .bind(user.id)
            .bind(&kind_str)
            .bind(enabled)
            .execute(&mut *tx)
            .await?;
        }
    }

    tx.commit().await?;

    let prefs = load_preferences(&state.pool, user.id).await?;
    Ok(Json(prefs))
}

async fn load_preferences(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
) -> Result<PreferencesResponse, ApiError> {
    let global: bool =
        sqlx::query_scalar("SELECT global_email_notifications_enabled FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await?;

    let rows: Vec<(String, bool)> =
        sqlx::query_as("SELECT kind, enabled FROM notification_preferences WHERE user_id = $1")
            .bind(user_id)
            .fetch_all(pool)
            .await?;
    let overrides: HashMap<String, bool> = rows.into_iter().collect();

    // Always return every user-facing kind. Default-on if no override
    // row. Transactional kinds aren't in this map at all (no toggle).
    let mut kinds = HashMap::new();
    for k in NotificationKind::user_facing() {
        let value = overrides.get(k.as_str()).copied().unwrap_or(true);
        kinds.insert(k.as_str().to_string(), value);
    }

    Ok(PreferencesResponse {
        global_enabled: global,
        kinds,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/notifications/unsubscribe  (NO auth — token is the credential)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UnsubscribeBody {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct UnsubscribeResponse {
    /// What was switched off, so the web confirmation page can render
    /// it ("you've been unsubscribed from {friendly}").
    pub kind: String,
    pub friendly_label: &'static str,
}

/// Verifies the signed token, flips that `(user_id, kind)` preference
/// to disabled, and returns the friendly kind name for the confirmation
/// page. Idempotent — clicking the link twice still 200s.
///
/// No `AuthedUser` extractor — the token IS the auth. The signature
/// is checked against `ANON_COOKIE_SECRET` (the same HMAC key we use
/// for anon-id signing; rotating it invalidates all outstanding
/// unsubscribe links along with all anon cookies, which is acceptable
/// because a rotation is a security event anyway).
pub async fn unsubscribe(
    State(state): State<Arc<AppState>>,
    Json(body): Json<UnsubscribeBody>,
) -> Result<Json<UnsubscribeResponse>, ApiError> {
    use ml_art_core::notifications::{verify_unsubscribe_token, UnsubscribeError};

    let secret = state.cfg.anon_cookie_secret.as_bytes();
    let (user_id, kind) = verify_unsubscribe_token(&body.token, secret).map_err(|e| match e {
        UnsubscribeError::Expired => {
            ApiError::BadRequest("This unsubscribe link has expired.".into())
        }
        _ => ApiError::BadRequest("This unsubscribe link isn't valid.".into()),
    })?;

    // Transactional kinds can't be unsubscribed from — token shouldn't
    // ever encode one, but belt-and-braces.
    if kind.is_transactional() {
        return Err(ApiError::BadRequest(
            "This kind of email can't be unsubscribed from.".into(),
        ));
    }

    sqlx::query(
        r#"
        INSERT INTO notification_preferences (user_id, kind, enabled, updated_at)
        VALUES ($1, $2, false, now())
        ON CONFLICT (user_id, kind) DO UPDATE
            SET enabled = false, updated_at = now()
        "#,
    )
    .bind(user_id)
    .bind(kind.as_str())
    .execute(&state.pool)
    .await?;

    Ok(Json(UnsubscribeResponse {
        kind: kind.as_str().to_string(),
        friendly_label: kind.label(),
    }))
}

// Convenience for the one-click POST path that returns 204 (no body)
// instead of the friendly response — Gmail's RFC 8058 one-click flow
// expects 2xx with no required shape.
pub async fn unsubscribe_oneclick(
    state: State<Arc<AppState>>,
    body: Json<UnsubscribeBody>,
) -> Result<StatusCode, ApiError> {
    let _ = unsubscribe(state, body).await?;
    Ok(StatusCode::NO_CONTENT)
}
