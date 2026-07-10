//! T-083 — admin surface helpers.
//!
//! Two pieces:
//!   - `audit::record` — one chokepoint that every admin mutation calls
//!     before applying. Writes a row to `admin_audit_log` with the
//!     before/after JSON snapshots; subsequent failures of the mutation
//!     itself leave the audit row in place (intentional: "tried to
//!     approve but the row was already gone" is itself audit signal).
//!   - `AdminAction` / `AdminTargetKind` — small enums that give the
//!     action + target_kind text columns a typed shape inside Rust. The
//!     column itself stays text so we can add actions without
//!     migrations.
//!
//! The `AdminUser` extractor lives in the binary crate (api-search)
//! because of axum orphan rules — same as `AuthedUser`. See
//! `api-search/src/extractors.rs`.

use crate::error::ApiError;
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

pub mod audit {
    use super::*;

    /// Record an admin action. Pass `None` for `admin_user_id` when the
    /// action is performed by a system context (background job, seed
    /// migration). All other writers should pass `Some(admin.id)`.
    ///
    /// Writes the audit row *before* the mutation it audits — if the
    /// mutation then fails, the audit still records the attempt. This
    /// is intentional: an admin's intent-to-mutate is worth keeping
    /// even when the mutation is a no-op or 404.
    #[allow(clippy::too_many_arguments)]
    pub async fn record<B: Serialize, A: Serialize>(
        pool: &PgPool,
        admin_user_id: Option<Uuid>,
        action: &str,
        target_kind: &str,
        target_id: Option<Uuid>,
        before: Option<&B>,
        after: Option<&A>,
        context: Option<serde_json::Value>,
    ) -> Result<(), ApiError> {
        let before_json = before
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("audit before serialize: {e}")))?;
        let after_json = after
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("audit after serialize: {e}")))?;
        sqlx::query(
            r#"
            INSERT INTO admin_audit_log
                (admin_user_id, action, target_kind, target_id,
                 before_jsonb, after_jsonb, context)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(admin_user_id)
        .bind(action)
        .bind(target_kind)
        .bind(target_id)
        .bind(before_json)
        .bind(after_json)
        .bind(context)
        .execute(pool)
        .await?;
        Ok(())
    }
}

/// Action names recorded in `admin_audit_log.action`. Strings rather
/// than an enum on the SQL side so adding an action doesn't require a
/// migration; the Rust-side constants here are the canonical list.
pub mod action {
    pub const ARTIST_APPROVE: &str = "artist.approve";
    pub const ARTIST_DECLINE: &str = "artist.decline";
    pub const ARTIST_PAUSE: &str = "artist.pause";
    pub const ARTIST_UNPAUSE: &str = "artist.unpause";
    pub const IMAGE_OVERRIDE_APPROVE: &str = "image.override_approve";
    pub const VENUE_APPROVE: &str = "venue.approve";
    pub const VENUE_DECLINE: &str = "venue.decline";
    /// M-08 — admin-initiated marketplace refund.
    pub const ORDER_REFUND: &str = "order.refund";
}

pub mod target {
    pub const ARTIST: &str = "artist";
    pub const IMAGE: &str = "image";
    pub const VENUE: &str = "venue";
    pub const USER: &str = "user";
    pub const ORDER: &str = "order";
}
