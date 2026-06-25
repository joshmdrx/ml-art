//! `POST /v1/events` — batched client-side event ingestion (T-050.3).
//!
//! Server-side handlers emit events directly via `core::events::emit`.
//! Client-side actions (modifier-button clicks, "user opened the
//! inquiry modal", future page-impression events) batch + POST here
//! every few seconds + on tab close.
//!
//! Trust model: the client supplies the event name + properties, but
//! the **identity** (`anonymous_id` from the cookie, `user_id` from
//! the Bearer token) is derived server-side. A malicious client can
//! report fake `modifier_applied` for their own anon, but can't
//! attribute events to other users. The event-name allowlist also
//! prevents clients from polluting analytics with events the server
//! should be the sole source of (`artwork_saved`, `inquiry_submitted`
//! etc.) — those are server-side-only, full stop.
//!
//! Rate-limited by the per-anon search policy (60/min/anon) since
//! events traffic is roughly correlated with browsing intensity.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use ml_art_core::{
    auth::OptionalAnonId,
    error::ApiError,
    events::{self, EventName},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::extractors::AuthedUser;
use crate::AppState;

/// Hard cap on events per batch. The web batches 10-50 typically;
/// anything past 50 is either a bug or abuse — reject so the analytics
/// path can't be used to amplify a single request into hundreds of
/// jobs-queue writes.
const MAX_BATCH: usize = 50;

/// The set of event names a CLIENT is allowed to report. Everything
/// else lives on the server-side allowlist (see `core::events`).
///
/// Names not in this set 400. Add deliberately — accepting an event
/// from the client is a trust statement that no server signal could
/// have given us the same data.
const CLIENT_ALLOWED: &[EventName] = &[EventName::ModifierApplied, EventName::InquiryStarted];

#[derive(Debug, Deserialize)]
pub struct ClientEvent {
    /// Snake_case name. Validated against `CLIENT_ALLOWED`.
    pub name: EventName,
    /// Free-form event-specific properties. Capped at the axum default
    /// body-size limit (2 MiB across the whole request); per-event
    /// payload should stay under a kilobyte in normal use.
    #[serde(default)]
    pub properties: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct IngestBody {
    pub events: Vec<ClientEvent>,
}

/// Accepts a batch, fans out one `JobEvent::EventLog` per entry. Returns
/// 202 Accepted with no body — analytics is fire-and-forget on the
/// client side anyway.
pub async fn ingest(
    State(state): State<Arc<AppState>>,
    auth: Option<AuthedUser>,
    OptionalAnonId(anon_id): OptionalAnonId,
    headers: HeaderMap,
    Json(body): Json<IngestBody>,
) -> Result<StatusCode, ApiError> {
    if body.events.is_empty() {
        return Err(ApiError::BadRequest("events: must not be empty".into()));
    }
    if body.events.len() > MAX_BATCH {
        return Err(ApiError::BadRequest(format!(
            "events: max {MAX_BATCH} per batch (got {})",
            body.events.len()
        )));
    }
    // Allowlist check BEFORE doing any enqueue work — any reject
    // means none of the batch lands. Otherwise a partial-success
    // batch leaves analytics in a half-state we'd have to reason about.
    for ev in &body.events {
        if !CLIENT_ALLOWED.contains(&ev.name) {
            return Err(ApiError::BadRequest(format!(
                "events: name {:?} is not allowed from the client",
                ev.name
            )));
        }
    }

    // Identity is derived server-side from the request, never from
    // the client. A signed-in user with an anon cookie gets both
    // attached (T-033's merge logic uses the crosswalk).
    let user_id = auth.map(|AuthedUser(u)| u.id);
    let context = events::extract_request_context(&headers);

    // Per-event enqueue. We could batch into a single SQS sendMessageBatch
    // (cap 10) for prod cost optimisation, but at this scale (<10k events/
    // day projected) the simpler per-event path stays well inside the SQS
    // free tier. Revisit if we ever burn through it.
    for ev in body.events {
        events::emit(
            &state.jobs,
            events::event_log(ev.name, anon_id, user_id, ev.properties, context.clone()),
        )
        .await;
    }

    Ok(StatusCode::ACCEPTED)
}
