//! M-06 — studio sales dashboard: orders list, mark-shipped transition,
//! and payout status. Scoped to the calling artist throughout.

mod common;

use common::{app_with_test_auth, get_json_authed, send_authed, MIGRATOR};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

const ALICE_ARTIST: &str = "aaa11111-1111-1111-1111-111111111111";
const BLUE_MORNING: &str = "bbb11111-1111-1111-1111-111111111111";
const TEST_USER: &str = "99999999-9999-9999-9999-999999999999";
/// Alice is the seeded artist; Bob is a non-artist user.
const ALICE_BEARER: &str = "test-user_test_alice";
const BOB_BEARER: &str = "test-user_test_bob";

fn uuid(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}

/// Insert an order for Alice's artwork in the given status.
async fn insert_order(pool: &PgPool, status: &str) -> Uuid {
    sqlx::query_scalar(
        r#"
        INSERT INTO orders (
            buyer_user_id, artwork_id, artist_id,
            amount_cents_gbp, commission_cents_gbp, status, shipping_address
        )
        VALUES ($1, $2, $3, 100000, 15000, $4,
                '{"name":"Test User","line1":"1 A St","city":"London",
                  "postal_code":"E1 6AN","country":"GB"}'::jsonb)
        RETURNING id
        "#,
    )
    .bind(uuid(TEST_USER))
    .bind(uuid(BLUE_MORNING))
    .bind(uuid(ALICE_ARTIST))
    .bind(status)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn order_status(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM orders WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn mark_shipped_transitions_paid_to_shipped(pool: PgPool) {
    let order_id = insert_order(&pool, "paid").await;

    let app = app_with_test_auth(pool.clone());
    let (status, body) = send_authed(
        app,
        "POST",
        &format!("/v1/studio/orders/{order_id}/ship"),
        ALICE_BEARER,
        Some(r#"{"carrier":"Royal Mail","tracking_number":"RM123456789GB"}"#),
    )
    .await;
    assert_eq!(status, 200, "body: {}", String::from_utf8_lossy(&body));

    assert_eq!(order_status(&pool, order_id).await, "shipped");
    let (carrier, tracking): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT tracking_carrier, tracking_number FROM orders WHERE id = $1")
            .bind(order_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(carrier.as_deref(), Some("Royal Mail"));
    assert_eq!(tracking.as_deref(), Some("RM123456789GB"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn cannot_ship_a_pending_order(pool: PgPool) {
    let order_id = insert_order(&pool, "pending").await;

    let app = app_with_test_auth(pool.clone());
    let (status, _) = send_authed(
        app,
        "POST",
        &format!("/v1/studio/orders/{order_id}/ship"),
        ALICE_BEARER,
        Some(r#"{"carrier":"DPD","tracking_number":"X1"}"#),
    )
    .await;
    assert_eq!(status, 409, "pending order can't be shipped");
    assert_eq!(order_status(&pool, order_id).await, "pending");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn non_artist_cannot_ship(pool: PgPool) {
    let order_id = insert_order(&pool, "paid").await;

    // Bob has no artist row → studio gate returns 404.
    let app = app_with_test_auth(pool.clone());
    let (status, _) = send_authed(
        app,
        "POST",
        &format!("/v1/studio/orders/{order_id}/ship"),
        BOB_BEARER,
        Some(r#"{"carrier":"DPD","tracking_number":"X1"}"#),
    )
    .await;
    assert_eq!(status, 404);
    assert_eq!(order_status(&pool, order_id).await, "paid");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_orders_list_shows_own_orders(pool: PgPool) {
    let order_id = insert_order(&pool, "paid").await;

    let app = app_with_test_auth(pool.clone());
    let (status, body): (_, Value) = get_json_authed(app, "/v1/studio/orders", ALICE_BEARER).await;
    assert_eq!(status, 200);
    let orders = body.as_array().expect("array");
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0]["id"], order_id.to_string());
    assert_eq!(orders[0]["status"], "paid");
    assert_eq!(orders[0]["buyer_name"], "Test User");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn payout_status_reflects_onboarding(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let (status, body): (_, Value) =
        get_json_authed(app, "/v1/studio/stripe/payouts", ALICE_BEARER).await;
    assert_eq!(status, 200);
    assert_eq!(body["onboarding_started"], false);
    assert_eq!(body["charges_enabled"], false);

    // Simulate onboarding started + KYC complete.
    sqlx::query(
        "UPDATE artists SET stripe_account_id = 'acct_x', stripe_charges_enabled = true WHERE id = $1",
    )
    .bind(uuid(ALICE_ARTIST))
    .execute(&pool)
    .await
    .unwrap();

    let app = app_with_test_auth(pool.clone());
    let (_, body): (_, Value) =
        get_json_authed(app, "/v1/studio/stripe/payouts", ALICE_BEARER).await;
    assert_eq!(body["onboarding_started"], true);
    assert_eq!(body["charges_enabled"], true);
}
