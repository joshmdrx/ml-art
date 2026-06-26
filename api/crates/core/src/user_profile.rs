//! T-055 — user taste vector refresh.
//!
//! Builds a single user's taste embedding from their behavioural event
//! stream and persists it to `user_profiles.taste_embedding`. The
//! computed vector is the L2-normalised weighted sum of artwork
//! embeddings, with each event's weight decayed by time-since-event.
//!
//! Downstream consumers:
//! - T-056 personalised search re-rank (HNSW nearest-neighbour to this
//!   vector, blended into the RRF fusion).
//! - T-060 Discover Weekly digest (sample nearest + far-cluster artworks).
//! - Cluster-of-a-user (sort neighbourhoods by sim-to-taste).
//!
//! ## Math
//!
//! For each qualifying event `e` with associated artwork embedding
//! `v_e` and base weight `w_e`:
//!
//!   taste = Σ (w_e · decay(days_old(e)) · v_e)
//!   taste := taste / ||taste||
//!
//! `decay(d) = 0.95 ^ (d/7)` — a soft half-life of ~13 weeks. Old
//! engagement still counts but recent activity dominates.
//!
//! ## What's not in v1
//!
//! - **`artist_followed` / `artist_unfollowed`.** These point at an
//!   artist, not an artwork; the natural interpretation is `weight ·
//!   centroid(artist's artworks)`. Skipped for v1 to keep the SQL
//!   simple — add when the first real follow events appear.
//! - **Anonymous users.** `user_profiles.user_id` FKs into `users`, so
//!   pre-signin taste lives implicitly in the event stream until
//!   T-033's anon-merge handler links it to a user record. T-061 (the
//!   first-session calibrator) is the natural home for an anonymous-
//!   taste-store if we need one before sign-in.
//! - **`modifier_applied`, `visual_search_uploaded`.** No artwork id in
//!   `properties`. Modifier weights would need a modifier-vector
//!   lookup; visual uploads would need the upload's own embedding.
//!   Deferred to a refinement.

use crate::db::Pool;
use pgvector::Vector;
use uuid::Uuid;

/// Base weights per event kind, before time decay. Mirrors the
/// `T-055` TODO entry; tune as taste-vector quality is measured
/// against real engagement.
pub const WEIGHT_INQUIRY: f32 = 5.0;
pub const WEIGHT_SAVE: f32 = 3.0;
pub const WEIGHT_VIEW: f32 = 0.5;

/// How far back to look for contributing events. Older events still
/// contribute via the decay, but capping the window keeps the query
/// bounded as the events table grows.
pub const LOOKBACK_DAYS: i32 = 90;

/// Soft half-life: weight is multiplied by this each week of age. A
/// 90-day-old event therefore counts ~52% as much as a fresh one
/// (0.95^(90/7) ≈ 0.52).
pub const WEEKLY_DECAY: f32 = 0.95;

/// Minimum signal magnitude before we persist a taste vector. Floats
/// this small are almost certainly noise (e.g. one view-event with
/// max decay) and would normalise into a meaningless direction.
const MIN_SIGNAL_NORM: f32 = 1e-6;

/// Base weight contributed by a single event of the given name.
/// `None` means "this event doesn't contribute" (search, started-but-
/// not-submitted, etc.) and so doesn't drive a JOIN to embeddings.
///
/// Returns a signed weight: unsaves subtract, mirroring the original
/// save. That keeps the vector consistent with the user's *current*
/// state — saving then later unsaving cancels out, modulo decay.
pub fn base_weight(event_name: &str) -> Option<f32> {
    match event_name {
        "inquiry_submitted" => Some(WEIGHT_INQUIRY),
        "artwork_saved" => Some(WEIGHT_SAVE),
        "artwork_unsaved" => Some(-WEIGHT_SAVE),
        "artwork_viewed" => Some(WEIGHT_VIEW),
        _ => None,
    }
}

/// One row of the (event, embedding) join — the unit of accumulation.
/// `embedding` is the artwork's 1024-d vector; `days_old` is a real
/// number of days since the event (so the decay is continuous, not
/// stepped at week boundaries).
#[derive(Debug, Clone)]
pub struct EventContribution {
    pub event_name: String,
    pub embedding: Vec<f32>,
    pub days_old: f64,
}

/// What `refresh_user` writes back. Returned so callers can log the
/// result without re-reading the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshResult {
    /// True if we wrote a fresh taste vector. False means the user
    /// had no qualifying events (or their signal was below the noise
    /// floor) — we don't touch the row in that case.
    pub updated: bool,
    /// Count of events that contributed to this run's vector. Same
    /// number written to `user_profiles.interaction_count`. Downstream
    /// surfaces gate on `>= 10` to enable personalised retrieval.
    pub interaction_count: i32,
}

/// Compute the weighted-sum taste vector from a slice of contributions.
/// Returns `None` if the result would have norm below `MIN_SIGNAL_NORM`
/// — i.e. no meaningful direction to record.
///
/// Pure function, no IO. Easy to unit-test.
pub fn build_taste_vector(contributions: &[EventContribution]) -> Option<Vec<f32>> {
    if contributions.is_empty() {
        return None;
    }
    let dim = contributions[0].embedding.len();
    let mut acc = vec![0.0_f32; dim];

    for c in contributions {
        let Some(base) = base_weight(&c.event_name) else {
            // Caller shouldn't pass non-contributing rows in — they
            // wouldn't appear in the SQL JOIN — but the contract says
            // we tolerate it rather than panic.
            continue;
        };
        // weeks_old as f32; 0.95.powf is the natural exponential decay
        // even though the constant is < 1.
        let weeks_old = c.days_old as f32 / 7.0;
        let decay = WEEKLY_DECAY.powf(weeks_old);
        let effective = base * decay;
        debug_assert_eq!(c.embedding.len(), dim, "embedding dim mismatch");
        for (a, v) in acc.iter_mut().zip(c.embedding.iter()) {
            *a += effective * v;
        }
    }

    let norm: f32 = acc.iter().map(|x| x * x).sum::<f32>().sqrt();
    if !norm.is_finite() || norm < MIN_SIGNAL_NORM {
        return None;
    }
    for a in acc.iter_mut() {
        *a /= norm;
    }
    Some(acc)
}

/// Refresh one user's `user_profiles` row from their event stream.
/// Idempotent — running twice in quick succession produces the same
/// vector (up to float noise).
///
/// Pulls the (event, artwork-embedding) pairs in a single query and
/// accumulates in Rust because pgvector's aggregate support for
/// weighted sums would require either custom SQL or per-row arithmetic
/// over the 1024-d arrays. Both are slower than the fetch+accumulate
/// path for the row counts we expect (≤ a few thousand per user).
pub async fn refresh_user(pool: &Pool, user_id: Uuid) -> sqlx::Result<RefreshResult> {
    // Only event names with a positive `base_weight` ever appear here.
    // Hand-listed rather than computed from `base_weight` because the
    // SQL planner can use the `events_name_occurred_idx` only on a
    // literal IN-list.
    let rows: Vec<(String, Vector, f64)> = sqlx::query_as(
        r#"
        SELECT
            e.event_name,
            ae.embedding,
            -- EXTRACT(EPOCH FROM ...) returns NUMERIC in PG ≥14; cast to
            -- double-precision so sqlx decodes into f64.
            (EXTRACT(EPOCH FROM (now() - e.occurred_at)) / 86400.0)::float8 AS days_old
        FROM events e
        JOIN artwork_embeddings ae
            ON ae.artwork_id = (e.properties->>'artwork_id')::uuid
        WHERE e.user_id = $1
          AND e.occurred_at > now() - ($2 || ' days')::interval
          AND e.properties ? 'artwork_id'
          AND e.event_name IN (
              'inquiry_submitted', 'artwork_saved', 'artwork_unsaved', 'artwork_viewed'
          )
        "#,
    )
    .bind(user_id)
    .bind(LOOKBACK_DAYS.to_string())
    .fetch_all(pool)
    .await?;

    let interaction_count = i32::try_from(rows.len()).unwrap_or(i32::MAX);

    let contributions: Vec<EventContribution> = rows
        .into_iter()
        .map(|(name, emb, days_old)| EventContribution {
            event_name: name,
            embedding: emb.to_vec(),
            days_old,
        })
        .collect();

    let Some(taste) = build_taste_vector(&contributions) else {
        return Ok(RefreshResult {
            updated: false,
            interaction_count,
        });
    };

    let taste_vec = Vector::from(taste);

    sqlx::query(
        r#"
        INSERT INTO user_profiles
            (user_id, taste_embedding, interaction_count,
             last_active, profile_updated_at)
        VALUES ($1, $2, $3, now(), now())
        ON CONFLICT (user_id) DO UPDATE SET
            taste_embedding   = EXCLUDED.taste_embedding,
            interaction_count = EXCLUDED.interaction_count,
            last_active       = EXCLUDED.last_active,
            profile_updated_at = now()
        "#,
    )
    .bind(user_id)
    .bind(&taste_vec)
    .bind(interaction_count)
    .execute(pool)
    .await?;

    Ok(RefreshResult {
        updated: true,
        interaction_count,
    })
}

/// User ids that have produced at least one event since `since`.
/// Used by the scheduled trigger (T-055.2) to fan out
/// `JobEvent::UserProfileRefresh` jobs. Anonymous events are
/// intentionally excluded — they'll fold into a user's taste the
/// next time they sign in (via T-033 anon-merge + the next refresh).
pub async fn users_with_recent_activity(
    pool: &Pool,
    since: chrono::DateTime<chrono::Utc>,
) -> sqlx::Result<Vec<Uuid>> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT DISTINCT user_id
        FROM events
        WHERE user_id IS NOT NULL
          AND occurred_at >= $1
        "#,
    )
    .bind(since)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_hot(dim: usize, pos: usize) -> Vec<f32> {
        let mut v = vec![0.0; dim];
        v[pos] = 1.0;
        v
    }

    #[test]
    fn base_weight_table() {
        assert_eq!(base_weight("inquiry_submitted"), Some(5.0));
        assert_eq!(base_weight("artwork_saved"), Some(3.0));
        assert_eq!(base_weight("artwork_unsaved"), Some(-3.0));
        assert_eq!(base_weight("artwork_viewed"), Some(0.5));
        // Tracked events with no taste-vector contribution.
        assert_eq!(base_weight("search_executed"), None);
        assert_eq!(base_weight("inquiry_started"), None);
        assert_eq!(base_weight("artist_followed"), None); // deferred
        // Unknown names — defensively treated as non-contributing.
        assert_eq!(base_weight("totally_made_up_event"), None);
    }

    #[test]
    fn empty_contributions_yields_none() {
        assert!(build_taste_vector(&[]).is_none());
    }

    #[test]
    fn single_save_normalises_to_unit() {
        // A single fresh save on a one-hot vector at position 0 should
        // produce a unit vector at position 0 after normalisation,
        // regardless of the base weight.
        let v = build_taste_vector(&[EventContribution {
            event_name: "artwork_saved".into(),
            embedding: one_hot(4, 0),
            days_old: 0.0,
        }])
        .expect("one event → some vector");
        assert!((v[0] - 1.0).abs() < 1e-5, "v[0] = {}", v[0]);
        for x in &v[1..] {
            assert!(x.abs() < 1e-6, "off-axis: {}", x);
        }
    }

    #[test]
    fn save_then_unsave_cancels() {
        // Same artwork, save then unsave with identical decay. Result
        // should be effectively zero → returns None (sub-noise-floor).
        let now = 1.0;
        let r = build_taste_vector(&[
            EventContribution {
                event_name: "artwork_saved".into(),
                embedding: one_hot(4, 0),
                days_old: now,
            },
            EventContribution {
                event_name: "artwork_unsaved".into(),
                embedding: one_hot(4, 0),
                days_old: now,
            },
        ]);
        assert!(r.is_none(), "save then unsave should net to zero, got {:?}", r);
    }

    #[test]
    fn inquiry_outweighs_view_in_direction() {
        // One inquiry on position-1 (weight 5) vs three views on
        // position-0 (weight 0.5 each). Inquiry should dominate, so
        // the normalised result points at position 1.
        let v = build_taste_vector(&[
            EventContribution {
                event_name: "artwork_viewed".into(),
                embedding: one_hot(4, 0),
                days_old: 0.0,
            },
            EventContribution {
                event_name: "artwork_viewed".into(),
                embedding: one_hot(4, 0),
                days_old: 0.0,
            },
            EventContribution {
                event_name: "artwork_viewed".into(),
                embedding: one_hot(4, 0),
                days_old: 0.0,
            },
            EventContribution {
                event_name: "inquiry_submitted".into(),
                embedding: one_hot(4, 1),
                days_old: 0.0,
            },
        ])
        .unwrap();
        // Inquiry weight 5 vs total view weight 1.5 → position 1 wins.
        assert!(v[1] > v[0], "inquiry should dominate: v = {:?}", v);
        // Both axes get *some* mass — view contribution shouldn't be
        // zeroed out.
        assert!(v[0] > 0.0, "view contribution lost: v[0] = {}", v[0]);
    }

    #[test]
    fn decay_diminishes_old_events() {
        // Two identical events, one fresh and one 13 weeks old. With
        // 0.95 weekly decay, the old one contributes ~52% as much.
        // The newer one should dominate.
        let v = build_taste_vector(&[
            EventContribution {
                event_name: "artwork_saved".into(),
                embedding: one_hot(4, 0),
                days_old: 0.0,
            },
            EventContribution {
                event_name: "artwork_saved".into(),
                embedding: one_hot(4, 1),
                days_old: 91.0,
            },
        ])
        .unwrap();
        assert!(
            v[0] > v[1],
            "fresh event should outweigh 13-week-old one: {:?}",
            v
        );
    }

    #[test]
    fn result_is_unit_norm() {
        let v = build_taste_vector(&[
            EventContribution {
                event_name: "artwork_saved".into(),
                embedding: one_hot(8, 2),
                days_old: 5.0,
            },
            EventContribution {
                event_name: "artwork_viewed".into(),
                embedding: one_hot(8, 4),
                days_old: 12.0,
            },
            EventContribution {
                event_name: "inquiry_submitted".into(),
                embedding: one_hot(8, 6),
                days_old: 30.0,
            },
        ])
        .unwrap();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-5,
            "expected unit norm, got {}",
            norm
        );
    }
}
