//! Behavioural events writer (T-050).
//!
//! Every interesting user action — search, view, save, follow, inquire —
//! lands here as a row in the `events` table (migration 0006). The
//! taste-vector, "for you" re-rank, neighbourhoods, and digest features
//! all read from it.
//!
//! ## Architecture
//!
//! Call sites emit via `events::emit(&jobs, ...)`. That enqueues a
//! `JobEvent::EventLog` onto the existing jobs queue (SQS in prod,
//! Postgres in dev). The handler in `core::jobs::handle` INSERTs into
//! the `events` table. The storage destination is intentionally
//! abstracted behind the queue so we can swap Postgres for S3 Parquet
//! in the cold tier without touching emit sites — see `decisions.md`
//! 2026-06-17 (Event storage).
//!
//! Emit is **best-effort**: any failure is logged at `WARN` and
//! swallowed. We never want the analytics layer to break a real
//! user-facing request.
//!
//! ## What goes in `properties` vs `context`
//!
//! - **`properties`** — event-meaningful data (artwork_id, search
//!   query, modifier codes, position in grid). Call-site supplies it
//!   via `event_log()`'s `properties: serde_json::json!({...})`.
//! - **`context`** — environment / forensic data (IP, user-agent).
//!   Extracted once from the request headers via
//!   [`extract_request_context`] — call sites just thread it through.
//!
//! ## PII
//!
//! `context.ip` and `context.user_agent` are PII under GDPR / UK GDPR.
//! Do NOT log them outside this table; do NOT echo them in API
//! responses. Retention + DSAR / right-to-delete is a deferred ticket
//! — see `decisions.md` for the privacy-policy + cookie-consent
//! follow-ups.
//!
//! ## Adding a new event
//!
//! 1. Add a variant to [`EventName`].
//! 2. Map it in [`EventName::as_str`].
//! 3. Document the expected `properties` shape on the variant.
//! 4. If client-side, allowlist it in `api-search::events::ALLOWED_*`.

use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::jobs::{EnqueueOpts, JobEvent, JobsBackend};

/// The closed taxonomy of event names the platform records. Adding
/// one is an additive change; renaming is structural (downstream
/// readers expect stable names).
///
/// Serialised form is `snake_case` — matches the `events.event_name`
/// column convention from migration 0006.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventName {
    /// `GET /v1/search` — `properties`: `{q, filters, result_count, sort}`.
    SearchExecuted,
    /// `GET /v1/artworks/:id` — `properties`: `{artwork_id, from?}`.
    ArtworkViewed,
    /// `me/collections::add_artwork` — `properties`: `{artwork_id, collection_id}`.
    ArtworkSaved,
    /// `me/collections::remove_artwork` — same shape as `ArtworkSaved`.
    ArtworkUnsaved,
    /// Client-side. Fired when the inquiry modal opens (NOT when
    /// submit succeeds — that's `InquirySubmitted`). Measures funnel.
    InquiryStarted,
    /// `POST /v1/artworks/:id/inquiries` — `properties`: `{artwork_id, artist_id, anonymous}`.
    InquirySubmitted,
    /// Client-side. Fired when the modifier bar applies one or more
    /// modifiers. `properties`: `{codes: [..]}`.
    ModifierApplied,
    /// `POST /v1/uploads/image` — `properties`: `{upload_id, bytes_size}`.
    VisualSearchUploaded,
    /// `GET /v1/artists/:slug` — `properties`: `{artist_id, slug}`.
    ArtistViewed,
    /// `GET /v1/neighborhoods/:slug` — `properties`: `{neighborhood_id, slug}`.
    NeighborhoodViewed,
    /// `POST /v1/me/follows/:artist_id` — `properties`: `{artist_id}`.
    ArtistFollowed,
    /// `DELETE /v1/me/follows/:artist_id` — `properties`: `{artist_id}`.
    ArtistUnfollowed,
    /// T-061 first-session calibrator. `POST /v1/calibrate/pick`.
    /// `properties`: `{pair_id, artwork_id, rejected_artwork_id}`.
    /// `artwork_id` is the chosen one — the field name matches what
    /// T-055's taste-vector SQL JOIN expects, so calibration picks
    /// feed straight into the user's taste vector at weight 2.0.
    CalibrationPick,
}

impl EventName {
    /// Stable snake_case string for the `events.event_name` column.
    /// Hand-written instead of serde-derived so the handler can match
    /// without re-serialising.
    pub fn as_str(&self) -> &'static str {
        match self {
            EventName::SearchExecuted => "search_executed",
            EventName::ArtworkViewed => "artwork_viewed",
            EventName::ArtworkSaved => "artwork_saved",
            EventName::ArtworkUnsaved => "artwork_unsaved",
            EventName::InquiryStarted => "inquiry_started",
            EventName::InquirySubmitted => "inquiry_submitted",
            EventName::ModifierApplied => "modifier_applied",
            EventName::VisualSearchUploaded => "visual_search_uploaded",
            EventName::ArtistViewed => "artist_viewed",
            EventName::NeighborhoodViewed => "neighborhood_viewed",
            EventName::ArtistFollowed => "artist_followed",
            EventName::ArtistUnfollowed => "artist_unfollowed",
            EventName::CalibrationPick => "calibration_pick",
        }
    }
}

/// Extract the request-context fields we attach to every event:
/// client IP and user-agent. Both are PII (see the module docs).
///
/// The IP comes from `X-Forwarded-For` — CloudFront sets this with the
/// client's true IP as the leftmost entry. `axum::extract::ConnectInfo`
/// would give the proxy's IP, which is useless for our purposes.
///
/// Empty / unset → JSON null in the field, not absent — keeps the
/// `context` shape stable across rows for downstream queries.
pub fn extract_request_context(headers: &HeaderMap) -> Value {
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty());
    serde_json::json!({
        "ip": ip,
        "user_agent": user_agent,
    })
}

/// Build a `JobEvent::EventLog` ready for enqueue. Saves callers from
/// repeating the struct-literal shape; centralises any future
/// per-event scrubbing / canonicalisation.
pub fn event_log(
    name: EventName,
    anonymous_id: Option<Uuid>,
    user_id: Option<Uuid>,
    properties: Value,
    context: Value,
) -> JobEvent {
    JobEvent::EventLog {
        name,
        anonymous_id,
        user_id,
        properties,
        context,
    }
}

/// Best-effort enqueue. Logs at WARN on failure but never propagates
/// the error — analytics must never break a real request.
///
/// Synchronous (not `tokio::spawn`'d) so the event is in the queue
/// before the response leaves the Lambda. SQS sendMessage is
/// single-digit-ms; the latency cost is invisible to the user.
pub async fn emit(jobs: &JobsBackend, event: JobEvent) {
    if let Err(e) = jobs.enqueue(event, EnqueueOpts::default()).await {
        tracing::warn!(error = ?e, "event emit failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn event_name_roundtrips_through_serde() {
        let n = EventName::ArtworkViewed;
        let s = serde_json::to_string(&n).unwrap();
        assert_eq!(s, "\"artwork_viewed\"");
        let back: EventName = serde_json::from_str(&s).unwrap();
        assert_eq!(back, n);
    }

    #[test]
    fn event_name_as_str_matches_serde_form() {
        for n in [
            EventName::SearchExecuted,
            EventName::ArtworkViewed,
            EventName::ArtworkSaved,
            EventName::ArtworkUnsaved,
            EventName::InquiryStarted,
            EventName::InquirySubmitted,
            EventName::ModifierApplied,
            EventName::VisualSearchUploaded,
            EventName::ArtistViewed,
            EventName::NeighborhoodViewed,
            EventName::ArtistFollowed,
            EventName::ArtistUnfollowed,
        ] {
            // serde produces "\"snake_case\""; trim the quotes.
            let serde_form = serde_json::to_string(&n).unwrap();
            let inner = &serde_form[1..serde_form.len() - 1];
            assert_eq!(inner, n.as_str(), "{n:?}");
        }
    }

    #[test]
    fn extract_request_context_picks_first_forwarded_for() {
        let mut h = HeaderMap::new();
        h.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.4, 70.41.3.18, 150.172.238.178"),
        );
        h.insert("user-agent", HeaderValue::from_static("Mozilla/5.0"));
        let ctx = extract_request_context(&h);
        assert_eq!(ctx["ip"], "203.0.113.4");
        assert_eq!(ctx["user_agent"], "Mozilla/5.0");
    }

    #[test]
    fn extract_request_context_missing_headers_yield_null() {
        let h = HeaderMap::new();
        let ctx = extract_request_context(&h);
        assert!(ctx["ip"].is_null());
        assert!(ctx["user_agent"].is_null());
    }

    #[test]
    fn extract_request_context_empty_string_treated_as_missing() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", HeaderValue::from_static(""));
        h.insert("user-agent", HeaderValue::from_static(""));
        let ctx = extract_request_context(&h);
        assert!(ctx["ip"].is_null());
        assert!(ctx["user_agent"].is_null());
    }

    #[test]
    fn extract_request_context_trims_whitespace() {
        let mut h = HeaderMap::new();
        h.insert(
            "x-forwarded-for",
            HeaderValue::from_static("  10.0.0.1  , 10.0.0.2"),
        );
        let ctx = extract_request_context(&h);
        assert_eq!(ctx["ip"], "10.0.0.1");
    }
}
