//! T-061 first-session taste calibrator.
//!
//! Two endpoints powering a 5-pair "this or that" UX shown to new
//! visitors:
//!
//! - `GET /v1/calibrate/pairs` — sample N pairs of artworks from
//!   far-apart semantic neighbourhoods (T-057 output). One artwork
//!   per pair side comes from each cluster's most-central
//!   representative.
//! - `POST /v1/calibrate/pick` — log the user's choice as a
//!   `calibration_pick` event. T-055 picks it up (weight 2.0) on the
//!   next taste-vector refresh.
//!
//! The picks are stored as events, not as a separate taste-vector
//! store. Anonymous picks fold into the user's taste vector at
//! sign-in via T-033's anon-merge handler. See `decisions.md`
//! 2026-06-26 — T-061 design choices.

use crate::AppState;
use axum::{extract::State, http::HeaderMap, Json};
use ml_art_core::{
    auth::{OptionalAnonId, User},
    error::ApiError,
    events::{self, EventName},
    images::url_for_s3_key,
};
use pgvector::Vector;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AuthedUser;

/// How many `(left, right)` pairs the GET endpoint returns. Five is
/// short enough to feel like a quick task and long enough to give
/// T-055 a meaningful starter signal (5 picks × weight 2.0 = 10
/// total signal, which clears the `interaction_count >= 10` gate for
/// downstream personalisation in one session).
pub const PAIRS_PER_SESSION: usize = 5;

/// Upper bound on candidate neighbourhoods we pull. Even with hundreds
/// of clusters we only need ~2× the pair count after the greedy
/// far-apart selection prunes them. Bounds the pairwise distance
/// computation at `O(MAX^2)` which is fine for MAX up to ~50.
const MAX_CANDIDATE_NEIGHBOURHOODS: i64 = 30;

// ─────────────────────────────────────────────────────────────────────────────
// Response types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalibrateArtwork {
    pub artwork_id: Uuid,
    pub title: Option<String>,
    pub artist_name: String,
    pub artist_slug: String,
    pub image_url: String,
    /// Which neighbourhood this artwork represents — useful for the
    /// pick event's analytics and for the client to render "from
    /// '<slug>'" if it wants.
    pub neighborhood_slug: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalibratePair {
    /// Client-side index 0..N-1. Echoed back on POST so the operator
    /// can join `pair_id` across the two events.
    pub id: String,
    pub left: CalibrateArtwork,
    pub right: CalibrateArtwork,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairsResponse {
    pub pairs: Vec<CalibratePair>,
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/calibrate/pairs
// ─────────────────────────────────────────────────────────────────────────────

#[derive(FromRow)]
struct NeighborhoodCandidate {
    slug: String,
    cluster_centroid: Vector,
    representative_artwork_ids: Vec<Uuid>,
}

pub async fn pairs(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PairsResponse>, ApiError> {
    // Pull semantic neighbourhoods with a populated centroid + at
    // least one representative artwork. Featured / larger clusters
    // first — those are the broadest visual neighborhoods and the
    // most-recognisable for cold-start picks.
    let candidates: Vec<NeighborhoodCandidate> = sqlx::query_as(
        r#"
        SELECT
            slug,
            cluster_centroid,
            representative_artwork_ids
        FROM neighborhoods
        WHERE kind = 'semantic'
          AND cluster_centroid IS NOT NULL
          AND cardinality(representative_artwork_ids) > 0
        ORDER BY is_featured DESC, artwork_count DESC
        LIMIT $1
        "#,
    )
    .bind(MAX_CANDIDATE_NEIGHBOURHOODS)
    .fetch_all(&state.pool)
    .await?;

    let selected = greedy_far_apart(&candidates, PAIRS_PER_SESSION);

    // Each `(left_idx, right_idx)` refers into `candidates`. Collect
    // the artwork ids the client will see so we can batch-fetch their
    // display data in one query.
    let needed_ids: Vec<Uuid> = selected
        .iter()
        .flat_map(|(l, r)| [candidates[*l].representative_artwork_ids[0], candidates[*r].representative_artwork_ids[0]])
        .collect();

    let display = fetch_artwork_display(&state, &needed_ids).await?;

    let pairs: Vec<CalibratePair> = selected
        .into_iter()
        .enumerate()
        .filter_map(|(idx, (l, r))| {
            let l_id = candidates[l].representative_artwork_ids[0];
            let r_id = candidates[r].representative_artwork_ids[0];
            let mut left = display.iter().find(|d| d.artwork_id == l_id)?.clone();
            let mut right = display.iter().find(|d| d.artwork_id == r_id)?.clone();
            left.neighborhood_slug = candidates[l].slug.clone();
            right.neighborhood_slug = candidates[r].slug.clone();
            Some(CalibratePair {
                id: idx.to_string(),
                left,
                right,
            })
        })
        .collect();

    Ok(Json(PairsResponse { pairs }))
}

/// Greedy farthest-pair selection: pick the first candidate, find the
/// one farthest from it, pair them, remove both, repeat. Stops when
/// fewer than two candidates remain or we've produced `wanted` pairs.
///
/// Not optimal (max-weight matching would do better) but the cluster
/// counts are small enough that O(n²) is trivial and the greedy
/// approximation already produces visually-distinct pairs. Wraps
/// indices into `candidates` so callers don't need to clone the
/// Vector data.
fn greedy_far_apart(
    candidates: &[NeighborhoodCandidate],
    wanted: usize,
) -> Vec<(usize, usize)> {
    if candidates.len() < 2 {
        return Vec::new();
    }
    let mut remaining: Vec<usize> = (0..candidates.len()).collect();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    while pairs.len() < wanted && remaining.len() >= 2 {
        let pivot_idx = remaining[0];
        let pivot = &candidates[pivot_idx].cluster_centroid.as_slice();
        // Argmax of distance over the rest.
        let (best_pos, _) = remaining
            .iter()
            .enumerate()
            .skip(1)
            .map(|(pos, &c)| {
                let other = candidates[c].cluster_centroid.as_slice();
                (pos, euclidean_sq(pivot, other))
            })
            .max_by(|(_, d1), (_, d2)| d1.partial_cmp(d2).unwrap_or(std::cmp::Ordering::Equal))
            .expect("remaining has ≥2 items so skip(1) yields ≥1");
        let partner_idx = remaining[best_pos];
        pairs.push((pivot_idx, partner_idx));
        // Remove both, larger index first so the earlier removal doesn't shift it.
        remaining.swap_remove(best_pos);
        remaining.swap_remove(0);
    }
    pairs
}

/// Squared euclidean distance — same ordering as euclidean, avoids a
/// sqrt per pair.
fn euclidean_sq(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let d = x - y;
            d * d
        })
        .sum()
}

#[derive(Debug, Clone, FromRow)]
struct ArtworkDisplayRow {
    artwork_id: Uuid,
    title: Option<String>,
    artist_name: String,
    artist_slug: String,
    s3_key: String,
}

async fn fetch_artwork_display(
    state: &AppState,
    ids: &[Uuid],
) -> Result<Vec<CalibrateArtwork>, ApiError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<ArtworkDisplayRow> = sqlx::query_as(
        r#"
        SELECT
            a.id            AS artwork_id,
            a.title         AS title,
            ar.display_name AS artist_name,
            ar.slug         AS artist_slug,
            ai.s3_key       AS s3_key
        FROM artworks a
        JOIN artists ar ON ar.id = a.artist_id
        JOIN artwork_images ai
            ON ai.artwork_id = a.id
           AND ai.is_primary
           AND ai.moderation_status = 'approved'
        WHERE a.id = ANY($1)
          AND a.deleted_at IS NULL
          AND a.status = 'published'
        "#,
    )
    .bind(ids)
    .fetch_all(&state.pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| CalibrateArtwork {
            artwork_id: r.artwork_id,
            title: r.title,
            artist_name: r.artist_name,
            artist_slug: r.artist_slug,
            image_url: url_for_s3_key(&r.s3_key),
            // Filled in by the caller from the candidates list.
            neighborhood_slug: String::new(),
        })
        .collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/calibrate/pick
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PickRequest {
    pub pair_id: String,
    pub chosen_artwork_id: Uuid,
    pub rejected_artwork_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct PickResponse {
    pub ok: bool,
}

pub async fn pick(
    State(state): State<Arc<AppState>>,
    auth: Option<AuthedUser>,
    OptionalAnonId(anon_id): OptionalAnonId,
    headers: HeaderMap,
    Json(body): Json<PickRequest>,
) -> Result<Json<PickResponse>, ApiError> {
    let user: Option<User> = auth.map(|AuthedUser(u)| u);

    // Both identities attached for crosswalk on sign-in (T-033). The
    // event's `artwork_id` field is what T-055's SQL JOIN expects — so
    // calibration picks feed straight into the taste vector with no
    // special-case handling in the refresh path.
    events::emit(
        &state.jobs,
        events::event_log(
            EventName::CalibrationPick,
            anon_id,
            user.as_ref().map(|u| u.id),
            serde_json::json!({
                "pair_id": body.pair_id,
                "artwork_id": body.chosen_artwork_id,
                "rejected_artwork_id": body.rejected_artwork_id,
            }),
            events::extract_request_context(&headers),
        ),
    )
    .await;

    Ok(Json(PickResponse { ok: true }))
}
