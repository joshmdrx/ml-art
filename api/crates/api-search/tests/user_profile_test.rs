//! T-055 integration tests — taste-vector refresh against a real
//! Postgres pool with the standard seed fixture.
//!
//! The seed gives us:
//!   - alice (88...), bob (77...)
//!   - 5 artworks with one-hot embeddings at positions 0–4
//!     (bbb1...→0, bbb2...→1, bbb3...→2, bbb4...→3, bbb5...→4)
//!
//! Tests insert events into the `events` table directly rather than
//! going through the emit→queue→handler path; the emit path is
//! covered by `events_test.rs`. Here we just need (event_name,
//! user_id, properties) rows to exist.

mod common;

use common::MIGRATOR;
use ml_art_core::{
    jobs::{self, EnqueueOpts, HandlerError, JobEvent, JobsBackend, JobsDeps},
    user_profile,
};
use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

const ALICE: Uuid = Uuid::from_u128(0x8888_8888_8888_8888_8888_8888_8888_8888);
const BOB: Uuid = Uuid::from_u128(0x7777_7777_7777_7777_7777_7777_7777_7777);

const ARTWORK_POS_0: Uuid = Uuid::from_u128(0xbbb1_1111_1111_1111_1111_1111_1111_1111);
const ARTWORK_POS_1: Uuid = Uuid::from_u128(0xbbb2_2222_2222_2222_2222_2222_2222_2222);
const ARTWORK_POS_2: Uuid = Uuid::from_u128(0xbbb3_3333_3333_3333_3333_3333_3333_3333);

/// Insert one event row directly. The schema allows `properties` as any
/// jsonb; we just include the `artwork_id` that the join expects.
async fn insert_event(
    pool: &PgPool,
    user_id: Uuid,
    name: &str,
    artwork_id: Uuid,
    occurred_at_offset_days: f64,
) {
    sqlx::query(
        r#"
        INSERT INTO events (user_id, event_name, properties, context, occurred_at)
        VALUES ($1, $2, $3, '{}'::jsonb, now() - ($4 || ' days')::interval)
        "#,
    )
    .bind(user_id)
    .bind(name)
    .bind(serde_json::json!({ "artwork_id": artwork_id }))
    .bind(occurred_at_offset_days.to_string())
    .execute(pool)
    .await
    .unwrap();
}

async fn fetch_taste(pool: &PgPool, user_id: Uuid) -> Option<(Vector, i32)> {
    sqlx::query_as::<_, (Option<Vector>, i32)>(
        "SELECT taste_embedding, interaction_count FROM user_profiles WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .unwrap()
    .and_then(|(v, c)| v.map(|vv| (vv, c)))
}

fn deps_for_handler(pool: PgPool) -> JobsDeps {
    JobsDeps {
        pool: pool.clone(),
        geocoder: ml_art_core::geocoding::GeocodingClient::disabled(),
        emails: ml_art_core::emails::EmailClient::disabled("test@example.invalid".to_string()),
        moderation: ml_art_core::moderation::ModerationClient::disabled(),
        web_base_url: "http://test.invalid".to_string(),
        anon_cookie_secret: "test-secret".to_string(),
        reply_email_domain: "reply.test".to_string(),
        jobs: JobsBackend::for_tests(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn refresh_with_no_events_skips_write(pool: PgPool) {
    let result = user_profile::refresh_user(&pool, ALICE).await.unwrap();
    assert!(!result.updated, "no events → no write");
    assert_eq!(result.interaction_count, 0);
    assert!(
        fetch_taste(&pool, ALICE).await.is_none(),
        "no profile row written"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn refresh_with_single_save_points_at_that_artwork(pool: PgPool) {
    // Alice saves the position-0 artwork. The resulting taste vector
    // should be (almost) the unit vector at position 0.
    insert_event(&pool, ALICE, "artwork_saved", ARTWORK_POS_0, 1.0).await;

    let result = user_profile::refresh_user(&pool, ALICE).await.unwrap();
    assert!(result.updated);
    assert_eq!(result.interaction_count, 1);

    let (taste, count) = fetch_taste(&pool, ALICE).await.unwrap();
    let v = taste.to_vec();
    assert!((v[0] - 1.0).abs() < 1e-4, "v[0] should be ~1.0, got {}", v[0]);
    assert_eq!(count, 1);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn refresh_blends_multiple_events(pool: PgPool) {
    // One inquiry on position-1 (weight 5) and three views on position-0
    // (weight 0.5 each). The normalised result should sit closer to
    // position 1 than position 0.
    insert_event(&pool, ALICE, "inquiry_submitted", ARTWORK_POS_1, 2.0).await;
    insert_event(&pool, ALICE, "artwork_viewed", ARTWORK_POS_0, 0.5).await;
    insert_event(&pool, ALICE, "artwork_viewed", ARTWORK_POS_0, 1.0).await;
    insert_event(&pool, ALICE, "artwork_viewed", ARTWORK_POS_0, 1.5).await;

    let result = user_profile::refresh_user(&pool, ALICE).await.unwrap();
    assert!(result.updated);
    assert_eq!(result.interaction_count, 4);

    let (taste, _) = fetch_taste(&pool, ALICE).await.unwrap();
    let v = taste.to_vec();
    assert!(
        v[1] > v[0],
        "inquiry should dominate views: v[0]={}, v[1]={}",
        v[0],
        v[1]
    );
    assert!(v[0] > 0.0, "view contribution shouldn't be zeroed");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn refresh_ignores_non_contributing_events(pool: PgPool) {
    // search_executed has no artwork_id; artist_followed has artist_id
    // not artwork_id; inquiry_started fires before submit. None of
    // these should drive a JOIN to embeddings.
    insert_event(&pool, ALICE, "search_executed", ARTWORK_POS_0, 0.5).await;
    insert_event(&pool, ALICE, "inquiry_started", ARTWORK_POS_1, 0.5).await;
    insert_event(&pool, ALICE, "neighborhood_viewed", ARTWORK_POS_2, 0.5).await;

    let result = user_profile::refresh_user(&pool, ALICE).await.unwrap();
    assert!(!result.updated, "no contributing events → no write");
    assert_eq!(result.interaction_count, 0);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn refresh_is_idempotent_in_quick_succession(pool: PgPool) {
    insert_event(&pool, ALICE, "artwork_saved", ARTWORK_POS_2, 0.1).await;

    let first = user_profile::refresh_user(&pool, ALICE).await.unwrap();
    let second = user_profile::refresh_user(&pool, ALICE).await.unwrap();
    assert!(first.updated && second.updated);
    assert_eq!(first.interaction_count, second.interaction_count);

    // The vector itself should be (essentially) the same — decay
    // moved by ~milliseconds between calls, so the direction is stable.
    let (v1, _) = fetch_taste(&pool, ALICE).await.unwrap();
    let v1 = v1.to_vec();
    let (v2, _) = fetch_taste(&pool, ALICE).await.unwrap();
    let v2 = v2.to_vec();
    let dot: f32 = v1.iter().zip(v2.iter()).map(|(a, b)| a * b).sum();
    assert!(dot > 0.9999, "vectors should be ~identical: dot = {}", dot);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn refresh_isolates_users(pool: PgPool) {
    // Alice's events don't change Bob's profile and vice-versa.
    insert_event(&pool, ALICE, "artwork_saved", ARTWORK_POS_0, 0.5).await;
    insert_event(&pool, BOB, "artwork_saved", ARTWORK_POS_2, 0.5).await;

    user_profile::refresh_user(&pool, ALICE).await.unwrap();
    user_profile::refresh_user(&pool, BOB).await.unwrap();

    let (alice_v, _) = fetch_taste(&pool, ALICE).await.unwrap();
    let (bob_v, _) = fetch_taste(&pool, BOB).await.unwrap();
    let av = alice_v.to_vec();
    let bv = bob_v.to_vec();
    assert!(av[0] > 0.9 && bv[2] > 0.9);
    assert!(
        av[2] < 0.01 && bv[0] < 0.01,
        "leakage between users: alice[2]={}, bob[0]={}",
        av[2],
        bv[0]
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn users_with_recent_activity_dedups_and_excludes_anon(pool: PgPool) {
    // Two events for alice, one for bob, one anonymous (no user_id).
    insert_event(&pool, ALICE, "artwork_viewed", ARTWORK_POS_0, 0.1).await;
    insert_event(&pool, ALICE, "artwork_saved", ARTWORK_POS_1, 0.2).await;
    insert_event(&pool, BOB, "artwork_viewed", ARTWORK_POS_2, 0.3).await;
    sqlx::query(
        r#"
        INSERT INTO events (anonymous_id, event_name, properties, context, occurred_at)
        VALUES (gen_random_uuid(), 'artwork_viewed',
                jsonb_build_object('artwork_id', $1::text),
                '{}'::jsonb, now())
        "#,
    )
    .bind(ARTWORK_POS_0)
    .execute(&pool)
    .await
    .unwrap();

    let yesterday = chrono::Utc::now() - chrono::Duration::days(1);
    let mut ids = user_profile::users_with_recent_activity(&pool, yesterday)
        .await
        .unwrap();
    ids.sort();
    let mut expected = vec![ALICE, BOB];
    expected.sort();
    assert_eq!(ids, expected, "anon excluded, dups collapsed");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn handler_arm_dispatches_to_refresh(pool: PgPool) {
    // End-to-end through the job dispatch: enqueue
    // JobEvent::UserProfileRefresh, hand it to `jobs::handle`, observe
    // the user_profiles row appear.
    insert_event(&pool, ALICE, "artwork_saved", ARTWORK_POS_1, 0.5).await;

    let deps = deps_for_handler(pool.clone());
    let result = jobs::handle(JobEvent::UserProfileRefresh { user_id: ALICE }, &deps).await;
    assert!(matches!(result, Ok(())), "handler returned {:?}", result);

    let (taste, count) = fetch_taste(&pool, ALICE).await.expect("profile written");
    let v = taste.to_vec();
    assert!((v[1] - 1.0).abs() < 1e-4);
    assert_eq!(count, 1);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn handler_returns_ok_for_user_with_no_events(pool: PgPool) {
    // A scheduled trigger shouldn't blow up when the user has no
    // qualifying events — they just don't get a profile row.
    let deps = deps_for_handler(pool.clone());
    let result = jobs::handle(JobEvent::UserProfileRefresh { user_id: ALICE }, &deps).await;
    assert!(
        matches!(result, Ok(())),
        "handler should succeed even with no events: {:?}",
        result
    );
    assert!(fetch_taste(&pool, ALICE).await.is_none());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn enqueue_persists_user_profile_refresh_variant(pool: PgPool) {
    // Smoke test: the new variant lands a pending row tagged with the
    // right kind discriminator. (The full payload round-trip is
    // exercised by the worker poll loop in `jobs-worker`.)
    let backend = JobsBackend::postgres(pool.clone());
    backend
        .enqueue(
            JobEvent::UserProfileRefresh { user_id: ALICE },
            EnqueueOpts::default(),
        )
        .await
        .unwrap();

    let row: (String, String, serde_json::Value) =
        sqlx::query_as("SELECT kind, status, payload FROM jobs ORDER BY created_at DESC LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.0, "user_profile_refresh");
    assert_eq!(row.1, "pending");
    assert_eq!(row.2["user_id"], serde_json::json!(ALICE));
}

// Reference the HandlerError type so the compiler doesn't grumble about
// the import on releases where every test uses Ok(())
#[allow(dead_code)]
fn _ensure_handler_error_in_scope(e: HandlerError) -> HandlerError {
    e
}

// ─────────────────────────────────────────────────────────────────────────────
// T-055.2 — kickoff (fan-out)
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn kickoff_enqueues_one_refresh_per_active_user(pool: PgPool) {
    // Alice has two events, Bob has one, no anonymous events.
    // Kickoff should enqueue exactly two UserProfileRefresh jobs.
    insert_event(&pool, ALICE, "artwork_viewed", ARTWORK_POS_0, 0.1).await;
    insert_event(&pool, ALICE, "artwork_saved", ARTWORK_POS_1, 0.2).await;
    insert_event(&pool, BOB, "inquiry_submitted", ARTWORK_POS_2, 0.3).await;

    let deps = deps_for_handler(pool.clone());
    jobs::handle(JobEvent::UserProfileRefreshKickoff {}, &deps)
        .await
        .expect("kickoff handler should succeed");

    // The `for_tests()` JobsBackend captures everything enqueued so we
    // can assert on shape without touching the jobs table.
    let captured = deps.jobs.captured();
    assert_eq!(captured.len(), 2, "two active users → two jobs");

    let mut user_ids: Vec<Uuid> = captured
        .iter()
        .map(|e| match e {
            JobEvent::UserProfileRefresh { user_id } => *user_id,
            other => panic!("expected UserProfileRefresh, got {:?}", other),
        })
        .collect();
    user_ids.sort();
    let mut expected = vec![ALICE, BOB];
    expected.sort();
    assert_eq!(user_ids, expected);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn kickoff_ignores_anonymous_events(pool: PgPool) {
    // Two anonymous events, no signed-in events. Kickoff enqueues
    // nothing — anonymous taste folds in via the T-033 anon-merge
    // handler at sign-in.
    sqlx::query(
        r#"
        INSERT INTO events (anonymous_id, event_name, properties, context, occurred_at)
        VALUES
          (gen_random_uuid(), 'artwork_viewed',
              jsonb_build_object('artwork_id', $1::text),
              '{}'::jsonb, now()),
          (gen_random_uuid(), 'artwork_saved',
              jsonb_build_object('artwork_id', $2::text),
              '{}'::jsonb, now())
        "#,
    )
    .bind(ARTWORK_POS_0)
    .bind(ARTWORK_POS_1)
    .execute(&pool)
    .await
    .unwrap();

    let deps = deps_for_handler(pool.clone());
    jobs::handle(JobEvent::UserProfileRefreshKickoff {}, &deps)
        .await
        .unwrap();

    assert!(
        deps.jobs.captured().is_empty(),
        "anonymous-only activity → no refresh jobs"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn calibration_pick_feeds_taste_vector(pool: PgPool) {
    // T-061 → T-055 wiring: a `calibration_pick` event with an
    // `artwork_id` should drive the user's taste vector at weight 2.0,
    // identical to any other artwork-linked event.
    insert_event(&pool, ALICE, "calibration_pick", ARTWORK_POS_2, 0.1).await;

    let result = user_profile::refresh_user(&pool, ALICE).await.unwrap();
    assert!(result.updated, "calibration pick should drive a refresh");
    assert_eq!(result.interaction_count, 1);

    let (taste, _) = fetch_taste(&pool, ALICE).await.unwrap();
    let v = taste.to_vec();
    assert!(
        (v[2] - 1.0).abs() < 1e-4,
        "expected unit at pos 2, got v[2] = {}",
        v[2]
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn kickoff_skips_stale_users(pool: PgPool) {
    // Alice's events are 2 days old → outside the 25h lookback. She
    // shouldn't get a refresh job. Bob's are fresh → he should.
    insert_event(&pool, ALICE, "artwork_saved", ARTWORK_POS_0, 2.0).await;
    insert_event(&pool, BOB, "artwork_saved", ARTWORK_POS_1, 0.1).await;

    let deps = deps_for_handler(pool.clone());
    jobs::handle(JobEvent::UserProfileRefreshKickoff {}, &deps)
        .await
        .unwrap();

    let captured = deps.jobs.captured();
    assert_eq!(captured.len(), 1, "only the fresh user gets a job");
    match &captured[0] {
        JobEvent::UserProfileRefresh { user_id } => assert_eq!(*user_id, BOB),
        other => panic!("unexpected event: {:?}", other),
    }
}
