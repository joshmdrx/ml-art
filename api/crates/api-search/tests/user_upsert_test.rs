//! `core::auth::insert_or_fetch_user` — the slow path of the lazy
//! Clerk→`users` upsert that runs on a user's first authenticated request.
//!
//! Regression for 2026-11-15: a first sign-in 500'd on the
//! `users_email_key` index (ON CONFLICT only arbitrates `clerk_user_id`)
//! because the email was already held by a different Clerk identity. Now
//! a 409. The concurrency test guards the same-identity path.

mod common;

use common::MIGRATOR;
use ml_art_core::{auth::insert_or_fetch_user, error::ApiError};
use sqlx::PgPool;

#[sqlx::test(migrator = "MIGRATOR")]
async fn concurrent_first_requests_create_exactly_one_user(pool: PgPool) {
    let tasks: Vec<_> = (0..16)
        .map(|_| {
            let pool = pool.clone();
            tokio::spawn(async move {
                insert_or_fetch_user(&pool, "user_race_test", "race@example.com", false).await
            })
        })
        .collect();

    let mut ids = Vec::new();
    for t in tasks {
        let user = t
            .await
            .expect("task panicked")
            .expect("every concurrent first request must succeed");
        ids.push(user.id);
    }
    ids.dedup();
    assert_eq!(ids.len(), 1, "all requests resolve to the same user row");

    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM users WHERE clerk_user_id = 'user_race_test'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rows, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn same_email_under_a_different_clerk_account_is_conflict(pool: PgPool) {
    insert_or_fetch_user(&pool, "user_original", "taken@example.com", false)
        .await
        .unwrap();

    let err = insert_or_fetch_user(&pool, "user_recreated", "taken@example.com", false)
        .await
        .expect_err("a second Clerk account can't silently claim the email");
    assert!(matches!(err, ApiError::Conflict(_)), "got {err:?}");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn repeat_sign_in_returns_the_existing_user(pool: PgPool) {
    let first = insert_or_fetch_user(&pool, "user_repeat", "repeat@example.com", false)
        .await
        .unwrap();
    let again = insert_or_fetch_user(&pool, "user_repeat", "repeat@example.com", false)
        .await
        .unwrap();
    assert_eq!(first.id, again.id);
}
