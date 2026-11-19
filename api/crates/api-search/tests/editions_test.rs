//! T-090 — limited editions, checkout holds, and sold-out auto-refunds.
//!
//! Covers the stock rules end to end against Postgres:
//!   - `orders::reserve` (the checkout's hold step, testable without
//!     Stripe): last copy held by one buyer blocks another until the hold
//!     lapses; the same buyer reuses / replaces their hold; editions count
//!     online + offline sales;
//!   - the payment webhook: editions sell out on the last copy, and a
//!     payment that can't be fulfilled is cancelled + queued a sold-out
//!     email instead of a sale (the Stripe refund itself is skipped in
//!     tests — no key, dev env);
//!   - refunds re-list an edition copy; expired sessions release holds;
//!   - the studio editor's edition / shipping fields and their guards.

mod common;

use api_search::orders::{reserve, Reservation};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use common::{app_with_postgres_jobs, app_with_test_auth, send_authed, MIGRATOR};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const ARTWORK: &str = "bbb11111-1111-1111-1111-111111111111"; // alice's
const ARTIST: &str = "aaa11111-1111-1111-1111-111111111111";
const BUYER_A: &str = "99999999-9999-9999-9999-999999999999";
const BUYER_B: &str = "77777777-7777-7777-7777-777777777777"; // bob
const ALICE: &str = "test-user_test_alice";

fn id(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}

fn address(line1: &str) -> Value {
    json!({
        "name": "A Buyer", "line1": line1, "line2": null,
        "city": "London", "postal_code": "EC1A 1BB", "country": "GB"
    })
}

/// Put the seeded artwork on sale; `edition` None = original.
async fn on_sale(pool: &PgPool, edition: Option<i32>, offline: i32) {
    sqlx::query(
        "UPDATE artworks SET availability = 'available', status = 'published', \
         edition_size = $2, editions_sold_offline = $3 WHERE id = $1",
    )
    .bind(id(ARTWORK))
    .bind(edition)
    .bind(offline)
    .execute(pool)
    .await
    .unwrap();
}

async fn try_reserve(pool: &PgPool, buyer: &str, line1: &str) -> Result<Reservation, String> {
    reserve(
        pool,
        id(ARTWORK),
        id(buyer),
        id(ARTIST),
        &address(line1),
        10_000,
        1_500,
    )
    .await
    .map_err(|e| e.to_string())
}

/// Give a reserved order a session id so the webhook can match it.
async fn set_session(pool: &PgPool, order_id: Uuid, session: &str) {
    sqlx::query("UPDATE orders SET stripe_checkout_session_id = $2 WHERE id = $1")
        .bind(order_id)
        .bind(session)
        .execute(pool)
        .await
        .unwrap();
}

async fn insert_order(pool: &PgPool, buyer: &str, status: &str, session: &str, pi: &str) -> Uuid {
    sqlx::query_scalar(
        r#"
        INSERT INTO orders (
            buyer_user_id, artwork_id, artist_id, amount_cents_gbp, commission_cents_gbp,
            status, shipping_address, stripe_checkout_session_id, stripe_payment_intent_id,
            hold_expires_at
        )
        VALUES ($1, $2, $3, 10000, 1500, $4, '{"country":"GB"}'::jsonb, $5, $6,
                now() + interval '30 minutes')
        RETURNING id
        "#,
    )
    .bind(id(buyer))
    .bind(id(ARTWORK))
    .bind(id(ARTIST))
    .bind(status)
    .bind(session)
    .bind(pi)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn post_stripe(pool: &PgPool, body: &Value) -> StatusCode {
    let resp = app_with_postgres_jobs(pool.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/stripe")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    resp.status()
}

fn completed(evt: &str, session: &str, pi: &str) -> Value {
    json!({
        "id": evt, "type": "checkout.session.completed",
        "data": { "object": { "id": session, "payment_intent": pi } },
    })
}

async fn availability(pool: &PgPool) -> String {
    sqlx::query_scalar("SELECT availability FROM artworks WHERE id = $1")
        .bind(id(ARTWORK))
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn order_state(pool: &PgPool, order_id: Uuid) -> (String, Option<String>) {
    sqlx::query_as("SELECT status, refund_reason FROM orders WHERE id = $1")
        .bind(order_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn notify_kinds(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT payload->>'kind' FROM jobs WHERE kind = 'order_notify' ORDER BY created_at",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

// ───────────────────────────── holds at checkout ─────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn last_copy_on_hold_blocks_another_buyer_until_it_lapses(pool: PgPool) {
    on_sale(&pool, None, 0).await;

    let a = try_reserve(&pool, BUYER_A, "1 Test St").await.unwrap();
    let Reservation::New(hold) = a else {
        panic!("first buyer should get a new hold, got {a:?}")
    };

    let b = try_reserve(&pool, BUYER_B, "2 Other Rd").await;
    let err = b.expect_err("second buyer must not get the original");
    assert!(err.contains("checking out the last copy"), "{err}");

    // A's hold lapses (they walked away) → B can buy.
    sqlx::query("UPDATE orders SET hold_expires_at = now() - interval '1 minute' WHERE id = $1")
        .bind(hold.order_id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        try_reserve(&pool, BUYER_B, "2 Other Rd").await,
        Ok(Reservation::New(_))
    ));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn same_buyer_reuses_or_replaces_their_hold(pool: PgPool) {
    on_sale(&pool, None, 0).await;
    let Ok(Reservation::New(first)) = try_reserve(&pool, BUYER_A, "1 Test St").await else {
        panic!("expected a new hold")
    };
    set_session(&pool, first.order_id, "cs_reuse").await;

    // Double-click / resubmit with the same address → same order + expiry,
    // so Stripe replays the same session.
    match try_reserve(&pool, BUYER_A, "1 Test St").await.unwrap() {
        Reservation::Reuse(h) => {
            assert_eq!(h.order_id, first.order_id);
            assert_eq!(h.expires_at.timestamp(), first.expires_at.timestamp());
        }
        other => panic!("expected reuse, got {other:?}"),
    }

    // New address → keep the order (and its copy), replace the session.
    match try_reserve(&pool, BUYER_A, "9 New Address").await.unwrap() {
        Reservation::Replace { hold, old_session } => {
            assert_eq!(hold.order_id, first.order_id);
            assert_eq!(old_session.as_deref(), Some("cs_reuse"));
        }
        other => panic!("expected replace, got {other:?}"),
    }

    let orders: i64 = sqlx::query_scalar("SELECT count(*) FROM orders WHERE artwork_id = $1")
        .bind(id(ARTWORK))
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(orders, 1, "no duplicate orders for one buyer");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn edition_counts_offline_and_online_sales(pool: PgPool) {
    // Edition of 3: one sold at a fair, one sold on Wander → one left.
    on_sale(&pool, Some(3), 1).await;
    insert_order(&pool, BUYER_B, "paid", "cs_prev", "pi_prev").await;

    assert!(matches!(
        try_reserve(&pool, BUYER_A, "1 Test St").await,
        Ok(Reservation::New(_))
    ));
    let err = try_reserve(&pool, "88888888-8888-8888-8888-888888888888", "3 Third St")
        .await
        .expect_err("last copy is held");
    assert!(err.contains("last copy"), "{err}");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn sold_out_edition_refuses_new_holds(pool: PgPool) {
    on_sale(&pool, Some(2), 2).await;
    let err = try_reserve(&pool, BUYER_A, "1 Test St").await.unwrap_err();
    assert!(err.contains("sold out"), "{err}");
}

// ───────────────────────────── payment webhook ──────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn edition_stays_on_sale_until_the_last_copy_sells(pool: PgPool) {
    on_sale(&pool, Some(2), 0).await;
    insert_order(&pool, BUYER_A, "pending", "cs_e1", "pi_e1").await;
    insert_order(&pool, BUYER_B, "pending", "cs_e2", "pi_e2").await;

    assert_eq!(
        post_stripe(&pool, &completed("evt_e1", "cs_e1", "pi_e1")).await,
        StatusCode::OK
    );
    assert_eq!(availability(&pool).await, "available", "1 of 2 sold");

    assert_eq!(
        post_stripe(&pool, &completed("evt_e2", "cs_e2", "pi_e2")).await,
        StatusCode::OK
    );
    assert_eq!(availability(&pool).await, "sold", "2 of 2 sold → sold out");

    // Refunding one puts a copy back on sale.
    let refunded = json!({
        "id": "evt_e_ref", "type": "charge.refunded",
        "data": { "object": { "id": "ch_e", "payment_intent": "pi_e1" } },
    });
    assert_eq!(post_stripe(&pool, &refunded).await, StatusCode::OK);
    assert_eq!(availability(&pool).await, "available");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn payment_after_sell_out_is_cancelled_not_sold(pool: PgPool) {
    on_sale(&pool, None, 0).await;
    // B already bought the original; A's (stale) checkout completes after.
    insert_order(&pool, BUYER_B, "paid", "cs_won", "pi_won").await;
    sqlx::query("UPDATE artworks SET availability = 'sold' WHERE id = $1")
        .bind(id(ARTWORK))
        .execute(&pool)
        .await
        .unwrap();
    let late = insert_order(&pool, BUYER_A, "pending", "cs_late", "pi_late").await;

    assert_eq!(
        post_stripe(&pool, &completed("evt_late", "cs_late", "pi_late")).await,
        StatusCode::OK
    );

    let (status, reason) = order_state(&pool, late).await;
    assert_eq!(status, "cancelled");
    assert_eq!(reason.as_deref(), Some("sold_out"));
    assert_eq!(availability(&pool).await, "sold");
    assert_eq!(
        notify_kinds(&pool).await,
        vec!["buyer_sold_out".to_string()],
        "buyer told + refunded; no sale emails"
    );

    // Stripe's charge.refunded for that auto-refund must not re-notify or
    // flip the cancelled order to refunded.
    let refunded = json!({
        "id": "evt_late_ref", "type": "charge.refunded",
        "data": { "object": { "id": "ch_late", "payment_intent": "pi_late" } },
    });
    assert_eq!(post_stripe(&pool, &refunded).await, StatusCode::OK);
    assert_eq!(order_state(&pool, late).await.0, "cancelled");
    assert_eq!(notify_kinds(&pool).await.len(), 1);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn payment_for_a_withdrawn_work_is_cancelled(pool: PgPool) {
    on_sale(&pool, None, 0).await;
    let order = insert_order(&pool, BUYER_A, "pending", "cs_wd", "pi_wd").await;
    // Artist marked it sold at a fair while the buyer was on Checkout.
    sqlx::query("UPDATE artworks SET availability = 'sold' WHERE id = $1")
        .bind(id(ARTWORK))
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        post_stripe(&pool, &completed("evt_wd", "cs_wd", "pi_wd")).await,
        StatusCode::OK
    );
    assert_eq!(order_state(&pool, order).await.0, "cancelled");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn expired_session_releases_the_hold(pool: PgPool) {
    on_sale(&pool, None, 0).await;
    let order = insert_order(&pool, BUYER_A, "pending", "cs_exp", "pi_exp").await;
    let expired = json!({
        "id": "evt_exp", "type": "checkout.session.expired",
        "data": { "object": { "id": "cs_exp" } },
    });
    assert_eq!(post_stripe(&pool, &expired).await, StatusCode::OK);
    assert_eq!(order_state(&pool, order).await.0, "cancelled");
    assert!(matches!(
        try_reserve(&pool, BUYER_B, "2 Other Rd").await,
        Ok(Reservation::New(_))
    ));
}

// ───────────────────────────── studio editor ────────────────────────────────

async fn patch(pool: &PgPool, body: Value) -> (StatusCode, String) {
    let (status, bytes) = send_authed(
        app_with_test_auth(pool.clone()),
        "PATCH",
        &format!("/v1/studio/artworks/{ARTWORK}"),
        ALICE,
        Some(&body.to_string()),
    )
    .await;
    (status, String::from_utf8_lossy(&bytes).to_string())
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_sets_shipping_and_edition_fields(pool: PgPool) {
    on_sale(&pool, None, 0).await;
    let (status, body) = patch(
        &pool,
        json!({"weight_grams": 1500, "ships_from_country": "gb", "edition_size": 5, "editions_sold_offline": 2}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (w, from, size, offline): (Option<i32>, Option<String>, Option<i32>, i32) = sqlx::query_as(
        "SELECT weight_grams, ships_from_country, edition_size, editions_sold_offline \
         FROM artworks WHERE id = $1",
    )
    .bind(id(ARTWORK))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (w, from.as_deref(), size, offline),
        (Some(1500), Some("GB"), Some(5), 2)
    );

    // Recording the rest as sold elsewhere sells it out…
    let (status, body) = patch(&pool, json!({"editions_sold_offline": 5})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(availability(&pool).await, "sold");
    // …and correcting the count puts it back on sale.
    let (status, _) = patch(&pool, json!({"editions_sold_offline": 4})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(availability(&pool).await, "available");

    // Back to an original clears the offline count.
    let (status, body) = patch(&pool, json!({"edition_size": null})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let offline: i32 =
        sqlx::query_scalar("SELECT editions_sold_offline FROM artworks WHERE id = $1")
            .bind(id(ARTWORK))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(offline, 0);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_rejects_impossible_edition_and_shipping_values(pool: PgPool) {
    on_sale(&pool, Some(5), 0).await;

    let (status, body) = patch(&pool, json!({"ships_from_country": "FR"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, _) = patch(&pool, json!({"edition_size": 1})).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an edition of 1 is an original"
    );
    let (status, _) = patch(&pool, json!({"weight_grams": 0})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = patch(&pool, json!({"editions_sold_offline": 6})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("more than the edition size"), "{body}");

    // Three already sold on Wander → can't shrink the edition to two.
    for (i, buyer) in [BUYER_A, BUYER_B, BUYER_A].iter().enumerate() {
        insert_order(
            &pool,
            buyer,
            "paid",
            &format!("cs_s{i}"),
            &format!("pi_s{i}"),
        )
        .await;
    }
    let (status, body) = patch(&pool, json!({"edition_size": 2})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("already sold 3"), "{body}");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn public_artwork_shows_edition_and_copies_left(pool: PgPool) {
    on_sale(&pool, Some(10), 3).await;
    insert_order(&pool, BUYER_A, "paid", "cs_pub", "pi_pub").await;
    insert_order(&pool, BUYER_B, "pending", "cs_pub2", "pi_pub2").await; // a hold isn't a sale

    let resp = common::app_keyword_only(pool.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/v1/artworks/{ARTWORK}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(resp.into_body(), 1 << 20).await.unwrap()).unwrap();
    assert_eq!(body["edition_size"], 10);
    assert_eq!(body["editions_remaining"], 6);
}
