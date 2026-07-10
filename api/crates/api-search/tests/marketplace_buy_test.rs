//! M-05 — buyer-surface backend: artwork `purchasable` flag (drives the
//! Buy button) + the buyer's order-detail endpoint (confirmation page).
//!
//! The full pay loop (Stripe Checkout) is covered at Tier 2 (M-10); this
//! proves the two new server-side pieces the web layer depends on.

mod common;

use common::{app_with_test_auth, get_json, get_json_authed, send_authed, MIGRATOR};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

const BLUE_MORNING: &str = "bbb11111-1111-1111-1111-111111111111";
const ALICE_ARTIST: &str = "aaa11111-1111-1111-1111-111111111111";
/// Test User (non-artist) — the buyer. Bearer = `test-` + clerk id.
const BUYER_BEARER: &str = "test-user_test_99";
const BOB_BEARER: &str = "test-user_test_bob";
const TEST_USER: &str = "99999999-9999-9999-9999-999999999999";

fn uuid(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artwork_not_purchasable_by_default(pool: PgPool) {
    // Seed artwork has no weight / ships-from / GBP price, and the artist
    // isn't Stripe-onboarded.
    let app = app_with_test_auth(pool.clone());
    let (status, body): (_, Value) = get_json(app, &format!("/v1/artworks/{BLUE_MORNING}")).await;
    assert_eq!(status, 200);
    assert_eq!(body["purchasable"], false);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artwork_purchasable_when_all_requirements_met(pool: PgPool) {
    sqlx::query(
        "UPDATE artworks
         SET weight_grams = 2000, ships_from_country = 'GB', price_gbp_cents = 100000,
             dimensions = '{\"width_cm\":30,\"height_cm\":40}'::jsonb
         WHERE id = $1",
    )
    .bind(uuid(BLUE_MORNING))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE artists SET stripe_charges_enabled = true WHERE id = $1")
        .bind(uuid(ALICE_ARTIST))
        .execute(&pool)
        .await
        .unwrap();

    let app = app_with_test_auth(pool.clone());
    let (status, body): (_, Value) = get_json(app, &format!("/v1/artworks/{BLUE_MORNING}")).await;
    assert_eq!(status, 200);
    assert_eq!(body["purchasable"], true);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn missing_stripe_onboarding_blocks_purchasable(pool: PgPool) {
    // All artwork fields present, but the artist hasn't onboarded → not
    // purchasable (defence for the "list before you can be paid" case).
    sqlx::query(
        "UPDATE artworks
         SET weight_grams = 2000, ships_from_country = 'GB', price_gbp_cents = 100000,
             dimensions = '{\"width_cm\":30,\"height_cm\":40}'::jsonb
         WHERE id = $1",
    )
    .bind(uuid(BLUE_MORNING))
    .execute(&pool)
    .await
    .unwrap();

    let app = app_with_test_auth(pool.clone());
    let (status, body): (_, Value) = get_json(app, &format!("/v1/artworks/{BLUE_MORNING}")).await;
    assert_eq!(status, 200);
    assert_eq!(body["purchasable"], false);
}

async fn insert_pending_order(pool: &PgPool) -> Uuid {
    sqlx::query_scalar(
        r#"
        INSERT INTO orders (
            buyer_user_id, artwork_id, artist_id,
            amount_cents_gbp, commission_cents_gbp, status, shipping_address
        )
        VALUES ($1, $2, $3, 100000, 15000, 'pending',
                '{"name":"Test User","line1":"1 A St","city":"London",
                  "postal_code":"E1 6AN","country":"GB"}'::jsonb)
        RETURNING id
        "#,
    )
    .bind(uuid(TEST_USER))
    .bind(uuid(BLUE_MORNING))
    .bind(uuid(ALICE_ARTIST))
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn order_detail_visible_to_buyer(pool: PgPool) {
    let order_id = insert_pending_order(&pool).await;

    let app = app_with_test_auth(pool.clone());
    let (status, body): (_, Value) =
        get_json_authed(app, &format!("/v1/orders/{order_id}"), BUYER_BEARER).await;
    assert_eq!(status, 200);
    assert_eq!(body["status"], "pending");
    assert_eq!(body["amount_cents_gbp"], 100000);
    assert_eq!(body["currency"], "gbp");
    assert_eq!(body["artwork"]["artist_name"], "Alice Test");
    assert_eq!(body["shipping_address"]["country"], "GB");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn my_orders_lists_only_own_orders(pool: PgPool) {
    insert_pending_order(&pool).await;

    // Buyer sees their order.
    let app = app_with_test_auth(pool.clone());
    let (status, body): (_, Value) = get_json_authed(app, "/v1/me/orders", BUYER_BEARER).await;
    assert_eq!(status, 200);
    let orders = body.as_array().unwrap();
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0]["artwork"]["artist_name"], "Alice Test");

    // A different user sees none of it.
    let app = app_with_test_auth(pool.clone());
    let (_, body): (_, Value) = get_json_authed(app, "/v1/me/orders", BOB_BEARER).await;
    assert_eq!(body.as_array().unwrap().len(), 0);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn order_detail_hidden_from_other_users(pool: PgPool) {
    let order_id = insert_pending_order(&pool).await;

    // Bob isn't the buyer → 404 (never 403, so we don't leak existence).
    let app = app_with_test_auth(pool.clone());
    let (status, _) = send_authed(
        app,
        "GET",
        &format!("/v1/orders/{order_id}"),
        BOB_BEARER,
        None,
    )
    .await;
    assert_eq!(status, 404);
}
