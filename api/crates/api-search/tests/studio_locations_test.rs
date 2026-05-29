//! T-038 G3 — integration tests for `/v1/studio/locations`.
//!
//! Geocoding is intentionally inert in these tests: the helpers we use
//! (`app_with_test_auth`) construct `GeocodingClient::disabled()`, so
//! POST / PATCH return rows with NULL lat/lng. That's *exactly* the
//! "Locating…" state the studio UI surfaces, and lets us assert ownership
//! and validation behavior deterministically without waiting on a real
//! HTTP round trip. The full geocode → row-update path is covered in
//! `tests/geocoding_test.rs`.

mod common;

use common::{app_with_test_auth, get_json_authed, send_authed, MIGRATOR};
use serde::Deserialize;
use sqlx::PgPool;

const ALICE: &str = "test-user_test_alice";
const BOB: &str = "test-user_test_bob";

#[derive(Debug, Deserialize)]
struct Location {
    id: String,
    kind: String,
    name: String,
    address: String,
    city: Option<String>,
    country: Option<String>,
    lat: Option<f64>,
    lng: Option<f64>,
    website_url: Option<String>,
    display_order: i32,
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/studio/locations
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn list_returns_all_alice_locations_including_pending(pool: PgPool) {
    // Alice has 2 seeded rows: one geocoded gallery + one pre-geocode
    // studio. Both must appear here (unlike the public artist payload
    // which hides the pending one).
    let app = app_with_test_auth(pool);
    let (status, items): (_, Vec<Location>) =
        get_json_authed(app, "/v1/studio/locations", ALICE).await;
    assert_eq!(status, 200);
    assert_eq!(items.len(), 2);

    // Sort is by display_order ASC — gallery is order 0, studio is 1.
    assert_eq!(items[0].kind, "gallery");
    assert!(items[0].lat.is_some(), "geocoded row has lat");
    assert_eq!(items[1].kind, "studio");
    assert!(items[1].lat.is_none(), "pending row has NULL lat");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn list_requires_auth(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = send_authed(app, "GET", "/v1/studio/locations", "bogus-token", None).await;
    assert_eq!(status, 401);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn list_for_non_artist_user_is_404(pool: PgPool) {
    // Bob has a users row but no artists row pointing at him.
    let app = app_with_test_auth(pool);
    let (status, _) = send_authed(app, "GET", "/v1/studio/locations", BOB, None).await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/studio/locations
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn create_returns_201_with_pending_row(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let body = r#"{
        "kind": "gallery",
        "name": "New Test Gallery",
        "address": "10 Brand New St, London",
        "website_url": "https://new-gallery.example"
    }"#;
    let (status, bytes) = send_authed(app, "POST", "/v1/studio/locations", ALICE, Some(body)).await;
    assert_eq!(status, 201);

    let loc: Location = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(loc.kind, "gallery");
    assert_eq!(loc.name, "New Test Gallery");
    assert_eq!(loc.address, "10 Brand New St, London");
    assert_eq!(
        loc.website_url.as_deref(),
        Some("https://new-gallery.example")
    );
    // Geocoder is disabled in tests, so the row comes back un-geocoded.
    assert!(loc.lat.is_none());
    assert!(loc.lng.is_none());
    assert!(loc.city.is_none());
    // display_order auto-assigned to "after existing rows" — alice's
    // seeded studio is at order 1, so the new row should be order 2.
    assert_eq!(loc.display_order, 2);

    // Sanity: the row really landed in Postgres under alice's artist id.
    let count: (i64,) = sqlx::query_as(
        "SELECT count(*)::bigint FROM artist_locations
           WHERE artist_id = 'aaa11111-1111-1111-1111-111111111111'
             AND deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count.0, 3);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn create_rejects_unknown_kind(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = r#"{"kind": "event", "name": "Foo", "address": "1 Test St"}"#;
    let (status, _) = send_authed(app, "POST", "/v1/studio/locations", ALICE, Some(body)).await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn create_rejects_empty_name(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = r#"{"kind": "studio", "name": "   ", "address": "1 Test St"}"#;
    let (status, _) = send_authed(app, "POST", "/v1/studio/locations", ALICE, Some(body)).await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn create_rejects_bare_website_url(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = r#"{
        "kind": "gallery",
        "name": "Foo Gallery",
        "address": "1 Test St",
        "website_url": "foo.example"
    }"#;
    let (status, _) = send_authed(app, "POST", "/v1/studio/locations", ALICE, Some(body)).await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn create_by_non_artist_is_404(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = r#"{"kind": "studio", "name": "Bob's Place", "address": "1 Test St"}"#;
    let (status, _) = send_authed(app, "POST", "/v1/studio/locations", BOB, Some(body)).await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// PATCH /v1/studio/locations/:id
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn patch_updates_fields(pool: PgPool) {
    let app = app_with_test_auth(pool);
    // Alice's geocoded gallery.
    let url = "/v1/studio/locations/ddd11111-1111-1111-1111-111111111111";
    let body = r#"{
        "name": "Renamed Gallery",
        "website_url": "https://renamed.example"
    }"#;
    let (status, bytes) = send_authed(app, "PATCH", url, ALICE, Some(body)).await;
    assert_eq!(status, 200);
    let loc: Location = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(loc.name, "Renamed Gallery");
    assert_eq!(loc.website_url.as_deref(), Some("https://renamed.example"));
    // Address didn't change, so lat/lng must NOT be cleared.
    assert!(loc.lat.is_some());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn patch_address_clears_geocode(pool: PgPool) {
    let app = app_with_test_auth(pool);
    // Alice's gallery starts with lat=51.5155, lng=-0.0922. Editing
    // the address must clear lat/lng/city/country/geocoded_at so the
    // (test-disabled) geocoder can re-run against the new value.
    let url = "/v1/studio/locations/ddd11111-1111-1111-1111-111111111111";
    let body = r#"{"address": "999 Different Lane, Manchester"}"#;
    let (status, bytes) = send_authed(app, "PATCH", url, ALICE, Some(body)).await;
    assert_eq!(status, 200);
    let loc: Location = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(loc.address, "999 Different Lane, Manchester");
    assert!(loc.lat.is_none(), "lat cleared after address change");
    assert!(loc.lng.is_none(), "lng cleared after address change");
    assert!(loc.city.is_none());
    assert!(loc.country.is_none());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn patch_can_null_website_url(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let url = "/v1/studio/locations/ddd11111-1111-1111-1111-111111111111";
    let body = r#"{"website_url": null}"#;
    let (status, bytes) = send_authed(app, "PATCH", url, ALICE, Some(body)).await;
    assert_eq!(status, 200);
    let loc: Location = serde_json::from_slice(&bytes).unwrap();
    assert!(loc.website_url.is_none());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn alice_cannot_patch_brunos_location(pool: PgPool) {
    // Bruno owns ddd33333. Alice shouldn't be able to reach it. The
    // ownership filter is `artist_id = $current` so the UPDATE matches
    // zero rows and we return 404 (not 403, to avoid leaking existence).
    let app = app_with_test_auth(pool);
    let url = "/v1/studio/locations/ddd33333-3333-3333-3333-333333333333";
    let body = r#"{"name": "Hijack attempt"}"#;
    let (status, _) = send_authed(app, "PATCH", url, ALICE, Some(body)).await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// DELETE /v1/studio/locations/:id
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn delete_soft_deletes_the_row(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let url = "/v1/studio/locations/ddd11111-1111-1111-1111-111111111111";
    let (status, _) = send_authed(app.clone(), "DELETE", url, ALICE, None).await;
    assert_eq!(status, 200);

    // After delete, list should drop the row (alice now has 1 instead of 2).
    let (_, items): (_, Vec<Location>) = get_json_authed(app, "/v1/studio/locations", ALICE).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, "studio");

    // Row is still in the table, just soft-deleted.
    let row: (Option<chrono::DateTime<chrono::Utc>>,) = sqlx::query_as(
        "SELECT deleted_at FROM artist_locations
           WHERE id = 'ddd11111-1111-1111-1111-111111111111'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(row.0.is_some(), "deleted_at stamped");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn delete_alice_cannot_delete_brunos_location(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let url = "/v1/studio/locations/ddd33333-3333-3333-3333-333333333333";
    let (status, _) = send_authed(app, "DELETE", url, ALICE, None).await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn delete_unknown_id_is_404(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let url = "/v1/studio/locations/00000000-0000-0000-0000-000000000000";
    let (status, _) = send_authed(app, "DELETE", url, ALICE, None).await;
    assert_eq!(status, 404);
}

// Suppress dead-code from the `id` field — it's part of the struct
// shape we ingest from JSON but not directly asserted on.
#[test]
fn _id_field_is_part_of_deserialization() {
    let raw = r#"{
        "id": "ddd11111-1111-1111-1111-111111111111",
        "kind": "gallery",
        "name": "x", "address": "y", "city": null, "country": null,
        "lat": null, "lng": null, "website_url": null, "display_order": 0
    }"#;
    let loc: Location = serde_json::from_str(raw).unwrap();
    assert_eq!(loc.id, "ddd11111-1111-1111-1111-111111111111");
}
