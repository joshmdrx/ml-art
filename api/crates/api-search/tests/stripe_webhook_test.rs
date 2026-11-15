//! M-03 — Stripe webhook receiver (`POST /v1/webhooks/stripe`).
//!
//! Exercises the handler-specific logic against committed DB state: the
//! `account.updated` → artist-flag transition, replay dedup on the Stripe
//! event id, and the ignore path for event types we don't act on. The
//! signature-verification primitive itself is unit-tested in
//! `ml_art_core::stripe`; `Config::for_tests` leaves the webhook secret
//! unset, so the handler takes its documented dev "skip verification"
//! path and these tests POST unsigned bodies.

mod common;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use common::{app_with_postgres_jobs, MIGRATOR};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

const ALICE_ARTIST: &str = "aaa11111-1111-1111-1111-111111111111";
const ACCOUNT_ID: &str = "acct_test_alice";

/// Link the seeded artist to a Connect account so `account.updated` has
/// something to match on.
async fn link_stripe_account(pool: &PgPool) {
    sqlx::query("UPDATE artists SET stripe_account_id = $1 WHERE id = $2")
        .bind(ACCOUNT_ID)
        .bind(uuid::Uuid::parse_str(ALICE_ARTIST).unwrap())
        .execute(pool)
        .await
        .unwrap();
}

/// POST a raw JSON event to the Stripe webhook. Fresh router per call
/// (`oneshot` consumes the `Router`) over the shared pool.
async fn post_stripe(pool: &PgPool, body: &Value) -> (StatusCode, Value) {
    let app = app_with_postgres_jobs(pool.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/stripe")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("router oneshot");
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let parsed = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, parsed)
}

async fn artist_flags(pool: &PgPool) -> (bool, bool, bool) {
    sqlx::query_as(
        "SELECT stripe_charges_enabled, stripe_payouts_enabled, stripe_onboarded_at IS NOT NULL
         FROM artists WHERE id = $1",
    )
    .bind(uuid::Uuid::parse_str(ALICE_ARTIST).unwrap())
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn account_updated_flips_artist_flags(pool: PgPool) {
    link_stripe_account(&pool).await;

    let event = json!({
        "id": "evt_acct_1",
        "type": "account.updated",
        "data": { "object": {
            "id": ACCOUNT_ID,
            "charges_enabled": true,
            "payouts_enabled": true,
        }},
    });
    let (status, json) = post_stripe(&pool, &event).await;
    assert_eq!(status, StatusCode::OK, "body: {json}");
    assert_eq!(json["status"], "processed");

    let (charges, payouts, onboarded) = artist_flags(&pool).await;
    assert!(charges, "charges_enabled flipped on");
    assert!(payouts, "payouts_enabled flipped on");
    assert!(
        onboarded,
        "stripe_onboarded_at stamped when charges went live"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn duplicate_event_id_is_idempotent(pool: PgPool) {
    link_stripe_account(&pool).await;

    let event = json!({
        "id": "evt_dupe_1",
        "type": "account.updated",
        "data": { "object": {
            "id": ACCOUNT_ID, "charges_enabled": true, "payouts_enabled": true,
        }},
    });

    let (_, first) = post_stripe(&pool, &event).await;
    assert_eq!(first["status"], "processed");
    let (_, second) = post_stripe(&pool, &event).await;
    assert_eq!(second["status"], "duplicate", "replay is a no-op ack");

    // Exactly one event row recorded despite two deliveries.
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM stripe_webhook_events WHERE event_id = $1",
    )
    .bind("evt_dupe_1")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn checkout_completed_marks_paid_and_enqueues_notifications(pool: PgPool) {
    // A pending order the webhook will match on its checkout session id.
    let order_id: uuid::Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO orders (
            buyer_user_id, artwork_id, artist_id,
            amount_cents_gbp, commission_cents_gbp, status,
            shipping_address, stripe_checkout_session_id
        )
        VALUES ('99999999-9999-9999-9999-999999999999',
                'bbb11111-1111-1111-1111-111111111111',
                'aaa11111-1111-1111-1111-111111111111',
                100000, 15000, 'pending', '{"country":"GB"}'::jsonb, 'cs_test_1')
        RETURNING id
        "#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let event = json!({
        "id": "evt_co_1",
        "type": "checkout.session.completed",
        "data": { "object": { "id": "cs_test_1", "payment_intent": "pi_test_1" } },
    });
    let (status, body) = post_stripe(&pool, &event).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["status"], "processed");

    // Order advanced to paid + payment intent captured + payout set
    // deterministically (amount 100000 - commission 15000 = 85000). The
    // Stripe fee stays null here: no STRIPE_SECRET_KEY in tests, so the
    // post-commit backfill is skipped — exactly the graceful-degrade path.
    let (st, pi, payout, fee): (String, Option<String>, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT status, stripe_payment_intent_id, payout_cents_gbp, stripe_fee_cents_gbp \
         FROM orders WHERE id = $1",
    )
    .bind(order_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(st, "paid");
    assert_eq!(pi.as_deref(), Some("pi_test_1"));
    assert_eq!(payout, Some(85000));
    assert_eq!(fee, None);

    // Two notification jobs enqueued (buyer confirmation + artist sale).
    let jobs: i64 =
        sqlx::query_scalar("SELECT count(*)::bigint FROM jobs WHERE kind = 'order_notify'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(jobs, 2);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn prod_without_webhook_secret_fails_closed(pool: PgPool) {
    // Outside dev, an unset STRIPE_WEBHOOK_SECRET must NOT fall through to
    // the unsigned "skip verification" path — that would let anyone forge
    // order/artist state transitions.
    let mut cfg = ml_art_core::config::Config::for_tests(String::new());
    cfg.env = ml_art_core::config::Environment::Prod;
    assert!(cfg.stripe_webhook_secret.is_none());

    link_stripe_account(&pool).await;
    let forged = json!({
        "id": "evt_forged_1",
        "type": "account.updated",
        "data": { "object": { "id": ACCOUNT_ID, "charges_enabled": true, "payouts_enabled": true } },
    });
    let resp = common::app_with_postgres_jobs_cfg(pool.clone(), cfg)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/stripe")
                .header("content-type", "application/json")
                .body(Body::from(forged.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

    // The forged event changed nothing: flags still off, no dedup row.
    assert_eq!(artist_flags(&pool).await, (false, false, false));
    let events: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM stripe_webhook_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(events, 0);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn unknown_event_type_is_ignored(pool: PgPool) {
    let event = json!({
        "id": "evt_unknown_1",
        "type": "customer.created",
        "data": { "object": { "id": "cus_123" } },
    });
    let (status, json) = post_stripe(&pool, &event).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "ignored");
}

/// A paid order whose Stripe fee isn't known yet — the state
/// checkout.session.completed leaves when the balance transaction hasn't
/// been attached.
async fn insert_fee_pending_order(pool: &PgPool, payment_intent: &str) -> uuid::Uuid {
    sqlx::query_scalar(
        r#"
        INSERT INTO orders (
            buyer_user_id, artwork_id, artist_id,
            amount_cents_gbp, commission_cents_gbp, payout_cents_gbp, status,
            shipping_address, stripe_payment_intent_id
        )
        VALUES ('99999999-9999-9999-9999-999999999999',
                'bbb11111-1111-1111-1111-111111111111',
                'aaa11111-1111-1111-1111-111111111111',
                5000, 750, 4250, 'paid', '{"country":"GB"}'::jsonb, $1)
        RETURNING id
        "#,
    )
    .bind(payment_intent)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn event_order(pool: &PgPool, event_id: &str) -> Option<uuid::Uuid> {
    sqlx::query_scalar("SELECT order_id FROM stripe_webhook_events WHERE event_id = $1")
        .bind(event_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn settled_charge_updated_targets_the_fee_pending_order(pool: PgPool) {
    let order_id = insert_fee_pending_order(&pool, "pi_fee_1").await;
    let event = json!({
        "id": "evt_cu_1",
        "type": "charge.updated",
        "data": { "object": { "id": "ch_1", "payment_intent": "pi_fee_1", "balance_transaction": "txn_1" } },
    });
    let (status, body) = post_stripe(&pool, &event).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["status"], "processed");
    // Linked to the order → the post-commit fee backfill ran for it (the
    // fetch itself no-ops here: no STRIPE_SECRET_KEY in tests).
    assert_eq!(event_order(&pool, "evt_cu_1").await, Some(order_id));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn unsettled_charge_updated_is_a_noop(pool: PgPool) {
    insert_fee_pending_order(&pool, "pi_fee_2").await;
    let event = json!({
        "id": "evt_cu_2",
        "type": "charge.updated",
        "data": { "object": { "id": "ch_2", "payment_intent": "pi_fee_2", "balance_transaction": null } },
    });
    let (status, _) = post_stripe(&pool, &event).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(event_order(&pool, "evt_cu_2").await, None);
}
