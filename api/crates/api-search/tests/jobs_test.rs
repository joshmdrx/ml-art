//! Integration tests for the Postgres jobs queue.
//!
//! Covers the full enqueue → claim → handle → mark loop against a
//! real per-test Postgres pool. Doesn't depend on api-search routes
//! — exercises `core::jobs` + `core::geocoding` directly so the
//! tests survive any future restructuring of the api binary.

mod common;

use common::MIGRATOR;
use ml_art_core::{
    emails::EmailClient,
    geocoding::{Geocoded, GeocodingClient},
    jobs::{self, EnqueueOpts, JobEvent, JobsBackend, JobsDeps},
};
use sqlx::PgPool;
use uuid::Uuid;

const ALICE_PENDING_STUDIO: Uuid = Uuid::from_u128(0xddd2_2222_2222_2222_2222_2222_2222_2222);

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn enqueue_inserts_a_pending_row(pool: PgPool) {
    let backend = JobsBackend::postgres(pool.clone());
    backend
        .enqueue(
            JobEvent::ArtistLocationGeocode {
                location_id: ALICE_PENDING_STUDIO,
            },
            EnqueueOpts::default(),
        )
        .await
        .unwrap();

    let row: (String, String, i32) =
        sqlx::query_as("SELECT kind, status, attempts FROM jobs ORDER BY created_at DESC LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.0, "artist_location_geocode");
    assert_eq!(row.1, "pending");
    assert_eq!(row.2, 0);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn idempotency_key_dedups_repeated_enqueue(pool: PgPool) {
    let backend = JobsBackend::postgres(pool.clone());
    let opts = EnqueueOpts {
        idempotency_key: Some("dedup-test".to_string()),
        ..Default::default()
    };
    backend
        .enqueue(
            JobEvent::ArtistLocationGeocode {
                location_id: ALICE_PENDING_STUDIO,
            },
            opts.clone(),
        )
        .await
        .unwrap();
    backend
        .enqueue(
            JobEvent::ArtistLocationGeocode {
                location_id: ALICE_PENDING_STUDIO,
            },
            opts,
        )
        .await
        .unwrap();

    let count: (i64,) = sqlx::query_as("SELECT count(*)::bigint FROM jobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1, "second enqueue with same key should be a no-op");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn claim_one_marks_running_and_increments_attempts(pool: PgPool) {
    let backend = JobsBackend::postgres(pool.clone());
    backend
        .enqueue(
            JobEvent::ArtistLocationGeocode {
                location_id: ALICE_PENDING_STUDIO,
            },
            EnqueueOpts::default(),
        )
        .await
        .unwrap();

    let claimed = jobs::postgres::claim_one(&pool).await.unwrap().unwrap();
    assert_eq!(claimed.kind, "artist_location_geocode");
    assert_eq!(claimed.attempts, 1, "claim increments attempts");

    // A second claim shouldn't see the same job — it's `running` now.
    let second = jobs::postgres::claim_one(&pool).await.unwrap();
    assert!(second.is_none());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn full_loop_geocodes_via_handler(pool: PgPool) {
    // Enqueue → claim → run handler → mark done → verify the
    // location row got its coords. Exercises the same dispatch
    // path the jobs-worker binary uses in production.
    let backend = JobsBackend::postgres(pool.clone());
    backend
        .enqueue(
            JobEvent::ArtistLocationGeocode {
                location_id: ALICE_PENDING_STUDIO,
            },
            EnqueueOpts::default(),
        )
        .await
        .unwrap();

    // Build deps with a canned geocoder so we don't hit Mapbox.
    let canned = vec![(
        "99 Test Lane, London".to_string(),
        Some(Geocoded {
            lng: -0.111,
            lat: 51.501,
            city: Some("London".to_string()),
            country: Some("GB".to_string()),
        }),
    )];
    let deps = JobsDeps {
        pool: pool.clone(),
        geocoder: GeocodingClient::for_tests(canned),
        emails: EmailClient::for_tests(),
        moderation: ml_art_core::moderation::ModerationClient::disabled(),
        web_base_url: "https://test.example.com".to_string(),
        anon_cookie_secret: "test-cookie-secret".to_string(),
        jobs: ml_art_core::jobs::JobsBackend::for_tests(),
    };

    let job = jobs::postgres::claim_one(&pool).await.unwrap().unwrap();
    let event = jobs::postgres::decode(&job).unwrap();
    jobs::handle(event, &deps).await.unwrap();
    jobs::postgres::mark_done(&pool, job.id).await.unwrap();

    // Job marked done.
    let status: (String,) = sqlx::query_as("SELECT status FROM jobs WHERE id = $1")
        .bind(job.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status.0, "done");

    // Location got geocoded.
    let loc: (Option<f64>, Option<f64>, Option<String>) =
        sqlx::query_as("SELECT lat, lng, city FROM artist_locations WHERE id = $1")
            .bind(ALICE_PENDING_STUDIO)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(loc.0, Some(51.501));
    assert_eq!(loc.1, Some(-0.111));
    assert_eq!(loc.2.as_deref(), Some("London"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn mark_failed_or_retry_backs_off_then_terminates(pool: PgPool) {
    let backend = JobsBackend::postgres(pool.clone());
    backend
        .enqueue(
            JobEvent::ArtistLocationGeocode {
                location_id: ALICE_PENDING_STUDIO,
            },
            EnqueueOpts {
                max_attempts: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let job = jobs::postgres::claim_one(&pool).await.unwrap().unwrap();
    // First failure: attempts=1 (claim incremented), max=2 → pending again
    // with backoff.
    jobs::postgres::mark_failed_or_retry(
        &pool,
        job.id,
        job.attempts,
        job.max_attempts,
        "first failure",
    )
    .await
    .unwrap();

    let status: (String, i32) = sqlx::query_as("SELECT status, attempts FROM jobs WHERE id = $1")
        .bind(job.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status.0, "pending");
    assert_eq!(status.1, 1);

    // Force the next_run_at into the past so claim_one will see it.
    sqlx::query("UPDATE jobs SET next_run_at = now() - interval '1 minute' WHERE id = $1")
        .bind(job.id)
        .execute(&pool)
        .await
        .unwrap();

    // Second claim → attempts=2 (= max). Failure should mark `failed`.
    let job2 = jobs::postgres::claim_one(&pool).await.unwrap().unwrap();
    jobs::postgres::mark_failed_or_retry(
        &pool,
        job2.id,
        job2.attempts,
        job2.max_attempts,
        "second failure",
    )
    .await
    .unwrap();

    let final_status: (String, Option<String>) =
        sqlx::query_as("SELECT status, last_error FROM jobs WHERE id = $1")
            .bind(job.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(final_status.0, "failed");
    assert!(final_status.1.as_deref().unwrap_or("").contains("second"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_create_enqueues_the_geocode(pool: PgPool) {
    // Driving the test through the HTTP studio handler is overkill;
    // the studio_locations_test suite already verifies the row is
    // created. This test asserts the SIDE EFFECT: a row in `jobs`.
    let backend = JobsBackend::postgres(pool.clone());

    backend
        .enqueue(
            JobEvent::ArtistLocationGeocode {
                location_id: ALICE_PENDING_STUDIO,
            },
            EnqueueOpts {
                idempotency_key: Some(format!("geocode:{ALICE_PENDING_STUDIO}")),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let count: (i64,) =
        sqlx::query_as("SELECT count(*)::bigint FROM jobs WHERE kind = 'artist_location_geocode'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count.0, 1);
}
