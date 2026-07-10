//! M-08 — admin order queue + refund gates. The refund's Stripe call
//! isn't hermetic (needs a live key + a real charge), so the success
//! path is covered at M-10; here we prove the queryable surface + the
//! admin/config gates.

mod common;

use common::{app_with_test_auth, get_json_authed, send_authed, MIGRATOR};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

const ADMIN_BEARER: &str = "test-user_test_admin";
const ALICE_BEARER: &str = "test-user_test_alice"; // non-admin for this surface
const BLUE_MORNING: &str = "bbb11111-1111-1111-1111-111111111111";
const ALICE_ARTIST: &str = "aaa11111-1111-1111-1111-111111111111";
const TEST_USER: &str = "99999999-9999-9999-9999-999999999999";

async fn insert_order(pool: &PgPool, status: &str, payment_intent: Option<&str>) -> Uuid {
    sqlx::query_scalar(
        r#"
        INSERT INTO orders (
            buyer_user_id, artwork_id, artist_id,
            amount_cents_gbp, commission_cents_gbp, status,
            shipping_address, stripe_payment_intent_id
        )
        VALUES ('99999999-9999-9999-9999-999999999999',
                'bbb11111-1111-1111-1111-111111111111',
                'aaa11111-1111-1111-1111-111111111111',
                100000, 15000, $1, '{"country":"GB"}'::jsonb, $2)
        RETURNING id
        "#,
    )
    .bind(status)
    .bind(payment_intent)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn list_returns_orders_and_filters_by_status(pool: PgPool) {
    let _ = (BLUE_MORNING, ALICE_ARTIST, TEST_USER); // documented ids
    insert_order(&pool, "paid", None).await;
    insert_order(&pool, "pending", None).await;

    let app = app_with_test_auth(pool.clone());
    let (status, body): (_, Value) = get_json_authed(app, "/v1/admin/orders", ADMIN_BEARER).await;
    assert_eq!(status, 200);
    assert_eq!(body.as_array().unwrap().len(), 2);

    // Status filter.
    let app = app_with_test_auth(pool.clone());
    let (status, body): (_, Value) =
        get_json_authed(app, "/v1/admin/orders?status=paid", ADMIN_BEARER).await;
    assert_eq!(status, 200);
    let orders = body.as_array().unwrap();
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0]["status"], "paid");
    assert_eq!(orders[0]["artist_name"], "Alice Test");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn detail_returns_full_order(pool: PgPool) {
    let order_id = insert_order(&pool, "paid", Some("pi_test_1")).await;

    let app = app_with_test_auth(pool.clone());
    let (status, body): (_, Value) =
        get_json_authed(app, &format!("/v1/admin/orders/{order_id}"), ADMIN_BEARER).await;
    assert_eq!(status, 200);
    assert_eq!(body["status"], "paid");
    assert_eq!(body["stripe_payment_intent_id"], "pi_test_1");
    assert_eq!(body["buyer_email"], "test@example.com");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn non_admin_forbidden(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = send_authed(app, "GET", "/v1/admin/orders", ALICE_BEARER, None).await;
    assert_eq!(status, 403);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn refund_503_when_stripe_unconfigured(pool: PgPool) {
    // for_tests config has no Stripe key → the refund endpoint answers
    // 503 at the entry rather than half-applying.
    let order_id = insert_order(&pool, "paid", Some("pi_test_1")).await;
    let app = app_with_test_auth(pool);
    let (status, _) = send_authed(
        app,
        "POST",
        &format!("/v1/admin/orders/{order_id}/refund"),
        ADMIN_BEARER,
        Some(r#"{"reason":"not-as-described"}"#),
    )
    .await;
    assert_eq!(status, 503);
}
