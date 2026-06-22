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
        reply_email_domain: "reply.test.example.com".to_string(),
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

// ── T-054 — inquiry-reply email handlers ─────────────────────────────────────

const ALICE_ARTIST: &str = "aaa11111-1111-1111-1111-111111111111";
const BLUE_MORNING: &str = "bbb11111-1111-1111-1111-111111111111";

/// Build `JobsDeps` whose `EmailClient` is the supplied capture handle.
/// Geocoder/moderation are inert — these tests only exercise the email
/// path. Secret + domain match `Config::for_tests` so a minted Reply-To
/// round-trips under the same secret.
fn email_deps(pool: PgPool, emails: EmailClient) -> JobsDeps {
    JobsDeps {
        pool,
        geocoder: GeocodingClient::for_tests(vec![]),
        emails,
        moderation: ml_art_core::moderation::ModerationClient::disabled(),
        web_base_url: "https://test.example.com".to_string(),
        anon_cookie_secret: "test-cookie-secret".to_string(),
        reply_email_domain: "reply.test.example.com".to_string(),
        jobs: JobsBackend::for_tests(),
    }
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn send_reply_uses_tokenised_reply_to(pool: PgPool) {
    let alice = Uuid::parse_str(ALICE_ARTIST).unwrap();
    let blue = Uuid::parse_str(BLUE_MORNING).unwrap();

    let inquiry_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO inquiries (artwork_id, artist_id, from_email, from_name, message)
           VALUES ($1, $2, 'buyer@example.com', 'Buyer', 'Is this available?')
           RETURNING id"#,
    )
    .bind(blue)
    .bind(alice)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Alice's outgoing studio reply (from_role defaults to 'artist').
    let reply_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO inquiry_replies (inquiry_id, artist_id, message)
           VALUES ($1, $2, 'Yes, it is!')
           RETURNING id"#,
    )
    .bind(inquiry_id)
    .bind(alice)
    .fetch_one(&pool)
    .await
    .unwrap();

    let emails = EmailClient::for_tests();
    let deps = email_deps(pool.clone(), emails.clone());
    jobs::handle(JobEvent::InquirySendReply { reply_id }, &deps)
        .await
        .unwrap();

    let sent = emails.captured();
    assert_eq!(sent.len(), 1);
    let email = &sent[0];
    // Goes to the inquirer; Reply-To is the tokenised per-inquiry address.
    assert_eq!(email.to, "buyer@example.com");
    let reply_to = email.reply_to.as_deref().expect("reply_to set");
    assert!(reply_to.starts_with("r-"), "got {reply_to}");
    assert!(
        reply_to.ends_with("@reply.test.example.com"),
        "got {reply_to}"
    );
    // …and it round-trips back to THIS inquiry under the shared secret.
    let resolved = ml_art_core::reply_address::verify(reply_to, b"test-cookie-secret");
    assert_eq!(resolved, Some(inquiry_id));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn send_reply_forward_emails_artist_with_inquirer_reply_to(pool: PgPool) {
    let alice = Uuid::parse_str(ALICE_ARTIST).unwrap();
    let blue = Uuid::parse_str(BLUE_MORNING).unwrap();

    let inquiry_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO inquiries (artwork_id, artist_id, from_email, from_name, message)
           VALUES ($1, $2, 'buyer@example.com', 'Buyer', 'Is this available?')
           RETURNING id"#,
    )
    .bind(blue)
    .bind(alice)
    .fetch_one(&pool)
    .await
    .unwrap();

    // An inquirer-inbound reply row, exactly as the webhook writes it:
    // NULL artist_id, from_role='inquirer'.
    let reply_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO inquiry_replies (inquiry_id, from_role, message, inbound_message_id)
           VALUES ($1, 'inquirer', 'Yes, still very interested!', '<m-1@mail>')
           RETURNING id"#,
    )
    .bind(inquiry_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let emails = EmailClient::for_tests();
    let deps = email_deps(pool.clone(), emails.clone());
    jobs::handle(JobEvent::InquirySendReplyForward { reply_id }, &deps)
        .await
        .unwrap();

    let sent = emails.captured();
    assert_eq!(sent.len(), 1);
    let email = &sent[0];
    // Forwarded to Alice's user email; Reply-To = the inquirer's address.
    assert_eq!(email.to, "alice@example.com");
    assert_eq!(email.reply_to.as_deref(), Some("buyer@example.com"));

    // sent_at is now set — the idempotency guard a retry would hit.
    let sent_flag: bool =
        sqlx::query_scalar("SELECT sent_at IS NOT NULL FROM inquiry_replies WHERE id = $1")
            .bind(reply_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(sent_flag, "forward should stamp sent_at");
}
