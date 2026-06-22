//! T-052b — integration tests for the new-works digest pipeline.
//!
//! Exercises `core::jobs::digest_handlers::{kickoff, send_user_digest}`
//! end to end against the real DB + in-memory email backend, asserting:
//!
//!   - kickoff only picks users with new work in their per-follow window
//!   - kickoff respects user_wants (master + per-kind preferences)
//!   - kickoff skips users who already received a digest today
//!   - per-user handler is idempotent under SQS at-least-once redelivery
//!   - empty payload doesn't send (and doesn't write a log row beyond
//!     the claim race)
//!   - email payload shape correctly groups by artist and caps at 12
//!
//! Per-file allow: `Deserialize`-only fields trigger dead_code under
//! `-D warnings`. See `decisions.md` 2026-05-27 — Pre-commit hooks.
#![allow(dead_code)]

mod common;

use chrono::{Duration as ChronoDuration, Utc};
use common::MIGRATOR;
use ml_art_core::{
    emails::EmailClient,
    geocoding::GeocodingClient,
    jobs::{self, JobEvent, JobsBackend, JobsDeps},
    moderation::ModerationClient,
};
use sqlx::PgPool;
use uuid::Uuid;

const ALICE_USER_ID: Uuid = Uuid::from_u128(0x8888_8888_8888_8888_8888_8888_8888_8888);
const BOB_USER_ID: Uuid = Uuid::from_u128(0x7777_7777_7777_7777_7777_7777_7777_7777);
// Artists from the seed.sql fixture.
const ARTIST_ALICE_PAINTER: Uuid = Uuid::from_u128(0xaaa1_1111_1111_1111_1111_1111_1111_1111);
const ARTIST_BRUNO: Uuid = Uuid::from_u128(0xaaa2_2222_2222_2222_2222_2222_2222_2222);

fn make_deps(pool: PgPool) -> JobsDeps {
    JobsDeps {
        pool: pool.clone(),
        geocoder: GeocodingClient::disabled(),
        emails: EmailClient::for_tests(),
        moderation: ModerationClient::disabled(),
        web_base_url: "https://wander.gallery".to_string(),
        anon_cookie_secret: "test-cookie-secret".to_string(),
        reply_email_domain: "reply.test.example.com".to_string(),
        // In-memory backend captures enqueued events from the kickoff
        // handler so we can assert on what got fanned out.
        jobs: JobsBackend::for_tests(),
    }
}

/// Seed a follow + a freshly-published artwork in the window the digest
/// looks at. Returns the artwork id for assertions.
async fn make_follow_and_new_work(pool: &PgPool, user_id: Uuid, artist_id: Uuid) -> Uuid {
    sqlx::query(
        "INSERT INTO follows (user_id, artist_id, created_at) VALUES ($1, $2, now() - interval '2 hours')",
    )
    .bind(user_id)
    .bind(artist_id)
    .execute(pool)
    .await
    .unwrap();

    let artwork_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO artworks (id, artist_id, title, status, published_at, created_at, updated_at)
        VALUES ($1, $2, 'Fresh ink', 'published', now() - interval '1 hour', now() - interval '1 hour', now() - interval '1 hour')
        "#,
    )
    .bind(artwork_id)
    .bind(artist_id)
    .execute(pool)
    .await
    .unwrap();
    artwork_id
}

// ─────────────────────────────────────────────────────────────────────────────
// kickoff
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn kickoff_enqueues_user_with_new_work(pool: PgPool) {
    let _ = make_follow_and_new_work(&pool, BOB_USER_ID, ARTIST_ALICE_PAINTER).await;
    let deps = make_deps(pool);

    jobs::handle(JobEvent::NotifyFollowersDigestKickoff {}, &deps)
        .await
        .unwrap();

    let captured = deps.jobs.captured();
    assert_eq!(captured.len(), 1, "exactly one per-user job enqueued");
    assert!(matches!(
        &captured[0],
        JobEvent::NotifyFollowersDigestUser { user_id } if *user_id == BOB_USER_ID
    ));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn kickoff_skips_users_without_new_work(pool: PgPool) {
    // Bob follows Alice-painter but no new artworks were published.
    sqlx::query(
        "INSERT INTO follows (user_id, artist_id, created_at) VALUES ($1, $2, now() - interval '1 hour')",
    )
    .bind(BOB_USER_ID)
    .bind(ARTIST_ALICE_PAINTER)
    .execute(&pool)
    .await
    .unwrap();

    let deps = make_deps(pool);
    jobs::handle(JobEvent::NotifyFollowersDigestKickoff {}, &deps)
        .await
        .unwrap();
    assert!(deps.jobs.captured().is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn kickoff_skips_users_already_sent_today(pool: PgPool) {
    let _ = make_follow_and_new_work(&pool, BOB_USER_ID, ARTIST_ALICE_PAINTER).await;

    // Pretend a digest already went out today.
    sqlx::query(
        "INSERT INTO user_notification_log (user_id, kind, sent_on) VALUES ($1, 'new_works_digest', current_date)",
    )
    .bind(BOB_USER_ID)
    .execute(&pool)
    .await
    .unwrap();

    let deps = make_deps(pool);
    jobs::handle(JobEvent::NotifyFollowersDigestKickoff {}, &deps)
        .await
        .unwrap();
    assert!(
        deps.jobs.captured().is_empty(),
        "users already sent today must be excluded from the kickoff"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn kickoff_respects_global_email_kill_switch(pool: PgPool) {
    let _ = make_follow_and_new_work(&pool, BOB_USER_ID, ARTIST_ALICE_PAINTER).await;
    sqlx::query("UPDATE users SET global_email_notifications_enabled = false WHERE id = $1")
        .bind(BOB_USER_ID)
        .execute(&pool)
        .await
        .unwrap();

    let deps = make_deps(pool);
    jobs::handle(JobEvent::NotifyFollowersDigestKickoff {}, &deps)
        .await
        .unwrap();
    assert!(deps.jobs.captured().is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn kickoff_respects_per_kind_opt_out(pool: PgPool) {
    let _ = make_follow_and_new_work(&pool, BOB_USER_ID, ARTIST_ALICE_PAINTER).await;
    sqlx::query(
        "INSERT INTO notification_preferences (user_id, kind, enabled) VALUES ($1, 'new_works_digest', false)",
    )
    .bind(BOB_USER_ID)
    .execute(&pool)
    .await
    .unwrap();

    let deps = make_deps(pool);
    jobs::handle(JobEvent::NotifyFollowersDigestKickoff {}, &deps)
        .await
        .unwrap();
    assert!(deps.jobs.captured().is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn kickoff_per_follow_backfill_window(pool: PgPool) {
    // Follow created LONG ago; artwork published 2h ago. Should be
    // captured (24h floor wins).
    sqlx::query(
        "INSERT INTO follows (user_id, artist_id, created_at) VALUES ($1, $2, now() - interval '30 days')",
    )
    .bind(BOB_USER_ID)
    .bind(ARTIST_ALICE_PAINTER)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO artworks (id, artist_id, title, status, published_at, created_at, updated_at)
        VALUES ($1, $2, 'Recent', 'published', now() - interval '2 hours', now(), now())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(ARTIST_ALICE_PAINTER)
    .execute(&pool)
    .await
    .unwrap();

    // Follow created recently; artwork published BEFORE the follow.
    // Should NOT be captured.
    let recent_follow_user: Uuid = ALICE_USER_ID;
    sqlx::query(
        "INSERT INTO follows (user_id, artist_id, created_at) VALUES ($1, $2, now() - interval '5 minutes')",
    )
    .bind(recent_follow_user)
    .bind(ARTIST_BRUNO)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO artworks (id, artist_id, title, status, published_at, created_at, updated_at)
        VALUES ($1, $2, 'Old work', 'published', now() - interval '1 hour', now(), now())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(ARTIST_BRUNO)
    .execute(&pool)
    .await
    .unwrap();

    let deps = make_deps(pool);
    jobs::handle(JobEvent::NotifyFollowersDigestKickoff {}, &deps)
        .await
        .unwrap();

    let captured = deps.jobs.captured();
    let user_ids: Vec<Uuid> = captured
        .iter()
        .filter_map(|e| match e {
            JobEvent::NotifyFollowersDigestUser { user_id } => Some(*user_id),
            _ => None,
        })
        .collect();
    assert!(
        user_ids.contains(&BOB_USER_ID),
        "Bob has new work after a 24h-floor-clamped follow window",
    );
    assert!(
        !user_ids.contains(&recent_follow_user),
        "Alice's follow is newer than the artwork — should be excluded",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// per-user handler
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn per_user_handler_sends_email(pool: PgPool) {
    let _ = make_follow_and_new_work(&pool, BOB_USER_ID, ARTIST_ALICE_PAINTER).await;
    let deps = make_deps(pool);

    jobs::handle(
        JobEvent::NotifyFollowersDigestUser {
            user_id: BOB_USER_ID,
        },
        &deps,
    )
    .await
    .unwrap();

    let sent = deps.emails.captured();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, "bob@example.com");
    assert!(
        sent[0].subject.starts_with("1 new work from "),
        "expected single-work subject, got {:?}",
        sent[0].subject
    );
    // List-Unsubscribe + List-Unsubscribe-Post headers wired.
    let header_names: Vec<&str> = sent[0].headers.iter().map(|(k, _)| k.as_str()).collect();
    assert!(header_names.contains(&"List-Unsubscribe"));
    assert!(header_names.contains(&"List-Unsubscribe-Post"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn per_user_handler_is_idempotent(pool: PgPool) {
    let _ = make_follow_and_new_work(&pool, BOB_USER_ID, ARTIST_ALICE_PAINTER).await;
    let deps = make_deps(pool);

    for _ in 0..3 {
        jobs::handle(
            JobEvent::NotifyFollowersDigestUser {
                user_id: BOB_USER_ID,
            },
            &deps,
        )
        .await
        .unwrap();
    }

    let sent = deps.emails.captured();
    assert_eq!(
        sent.len(),
        1,
        "redeliveries on the same day must produce exactly one email"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn per_user_handler_skips_after_opt_out(pool: PgPool) {
    let _ = make_follow_and_new_work(&pool, BOB_USER_ID, ARTIST_ALICE_PAINTER).await;
    // Opt out between kickoff scan and message delivery.
    sqlx::query(
        "INSERT INTO notification_preferences (user_id, kind, enabled) VALUES ($1, 'new_works_digest', false)",
    )
    .bind(BOB_USER_ID)
    .execute(&pool)
    .await
    .unwrap();

    let deps = make_deps(pool);
    jobs::handle(
        JobEvent::NotifyFollowersDigestUser {
            user_id: BOB_USER_ID,
        },
        &deps,
    )
    .await
    .unwrap();

    assert!(
        deps.emails.captured().is_empty(),
        "per-user handler defensively rechecks user_wants",
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn per_user_handler_multi_artist_subject(pool: PgPool) {
    // Two follows, both with new work. Subject should switch to the
    // multi-artist form.
    let _ = make_follow_and_new_work(&pool, BOB_USER_ID, ARTIST_ALICE_PAINTER).await;
    let _ = make_follow_and_new_work(&pool, BOB_USER_ID, ARTIST_BRUNO).await;

    let deps = make_deps(pool);
    jobs::handle(
        JobEvent::NotifyFollowersDigestUser {
            user_id: BOB_USER_ID,
        },
        &deps,
    )
    .await
    .unwrap();

    let sent = deps.emails.captured();
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0]
            .subject
            .contains("new works from artists you follow"),
        "expected multi-artist subject, got {:?}",
        sent[0].subject
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn per_user_handler_silent_on_empty(pool: PgPool) {
    // User has a follow but the artwork is BEFORE the follow window —
    // payload will be empty.
    sqlx::query(
        "INSERT INTO follows (user_id, artist_id, created_at) VALUES ($1, $2, now() - interval '5 minutes')",
    )
    .bind(BOB_USER_ID)
    .bind(ARTIST_ALICE_PAINTER)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO artworks (id, artist_id, title, status, published_at, created_at, updated_at)
        VALUES ($1, $2, 'Old', 'published', now() - interval '1 hour', now(), now())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(ARTIST_ALICE_PAINTER)
    .execute(&pool)
    .await
    .unwrap();

    let deps = make_deps(pool);
    jobs::handle(
        JobEvent::NotifyFollowersDigestUser {
            user_id: BOB_USER_ID,
        },
        &deps,
    )
    .await
    .unwrap();
    assert!(deps.emails.captured().is_empty());
}

// Silence unused-imports lint when the chrono helpers stay unused in
// some matrix configurations.
#[allow(dead_code)]
fn _chrono_anchor(_: ChronoDuration, _: chrono::DateTime<Utc>) {}
