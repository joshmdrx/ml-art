//! T-038 G5 — integration tests for `/v1/search/map`.

mod common;

use common::{app_keyword_only, get_json, get_status, MIGRATOR};
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Debug, Deserialize)]
struct Pin {
    location_id: String,
    #[allow(dead_code)]
    lat: f64,
    #[allow(dead_code)]
    lng: f64,
    name: String,
    kind: String,
    city: Option<String>,
    country: Option<String>,
    artist: PinArtist,
}

#[derive(Debug, Deserialize)]
struct PinArtist {
    slug: String,
    display_name: String,
    primary_image_url: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Happy path — no filters
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn returns_geocoded_pins_only(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, pins): (_, Vec<Pin>) = get_json(app, "/v1/search/map").await;
    assert_eq!(status, 200);
    // Seed has 3 location rows: 2 geocoded (alice gallery + bruno gallery)
    // + 1 pre-geocode (alice studio). Only the geocoded ones come back.
    assert_eq!(pins.len(), 2);

    let names: Vec<&str> = pins.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"Test Gallery London"));
    assert!(names.contains(&"Berlin Project Space"));

    // Pin shape sanity.
    let london = pins
        .iter()
        .find(|p| p.city.as_deref() == Some("London"))
        .unwrap();
    assert_eq!(london.kind, "gallery");
    assert_eq!(london.country.as_deref(), Some("GB"));
    assert_eq!(london.artist.slug, "alice-test");
    assert_eq!(london.artist.display_name, "Alice Test");
    assert!(london.artist.primary_image_url.is_some());
    assert!(uuid::Uuid::parse_str(&london.location_id).is_ok());
}

// ─────────────────────────────────────────────────────────────────────────────
// Bbox filter
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn bbox_filters_to_london_only(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, pins): (_, Vec<Pin>) =
        get_json(app, "/v1/search/map?bbox=-1.0,51.0,1.0,52.0").await;
    assert_eq!(status, 200);
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].city.as_deref(), Some("London"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn bbox_filters_to_berlin_only(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, pins): (_, Vec<Pin>) =
        get_json(app, "/v1/search/map?bbox=12.0,52.0,14.5,53.5").await;
    assert_eq!(status, 200);
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].city.as_deref(), Some("Berlin"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn bbox_outside_any_pin_returns_empty(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, pins): (_, Vec<Pin>) = get_json(app, "/v1/search/map?bbox=-50,30,-40,40").await;
    assert_eq!(status, 200);
    assert!(pins.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn malformed_bbox_is_400(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, _) = get_status(app, "/v1/search/map?bbox=not,a,real,bbox").await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn inverted_bbox_is_400(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, _) = get_status(app, "/v1/search/map?bbox=10,0,5,1").await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn out_of_range_bbox_is_400(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, _) = get_status(app, "/v1/search/map?bbox=0,-91,1,1").await;
    assert_eq!(status, 400);
}

// ─────────────────────────────────────────────────────────────────────────────
// Filters — location, medium, q
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn location_filter_matches_city_substring(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, pins): (_, Vec<Pin>) = get_json(app, "/v1/search/map?location=berlin").await;
    assert_eq!(status, 200);
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].city.as_deref(), Some("Berlin"));
}

// ─────────────────────────────────────────────────────────────────────────────
// T-041 — artist filter (?artist=<slug>) for the "See on map" CTA
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artist_filter_keeps_only_that_artists_pins(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, pins): (_, Vec<Pin>) = get_json(app, "/v1/search/map?artist=alice-test").await;
    assert_eq!(status, 200);
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].artist.slug, "alice-test");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artist_filter_unknown_slug_returns_empty(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, pins): (_, Vec<Pin>) = get_json(app, "/v1/search/map?artist=ghost-artist").await;
    assert_eq!(status, 200);
    assert!(pins.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artist_filter_composes_with_bbox(pool: PgPool) {
    // alice is in London, bruno in Berlin. ?artist=alice + a Berlin
    // bbox should return zero (alice exists but isn't in Berlin).
    let app = app_keyword_only(pool);
    let (status, pins): (_, Vec<Pin>) =
        get_json(app, "/v1/search/map?artist=alice-test&bbox=12,52,14.5,53.5").await;
    assert_eq!(status, 200);
    assert!(pins.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn medium_filter_painting_keeps_only_alice(pool: PgPool) {
    // Alice makes Paintings; Bruno makes Sculptures. The medium
    // filter is per-artist (we surface a pin if the venue's artist has
    // *any* matching artwork), so filtering by Painting should drop
    // Bruno's Berlin pin even though Bruno is geocoded.
    let app = app_keyword_only(pool);
    let (status, pins): (_, Vec<Pin>) = get_json(app, "/v1/search/map?medium=Painting").await;
    assert_eq!(status, 200);
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].artist.slug, "alice-test");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn medium_filter_sculpture_keeps_only_bruno(pool: PgPool) {
    // Symmetric — Sculpture should drop Alice and keep Bruno.
    let app = app_keyword_only(pool);
    let (status, pins): (_, Vec<Pin>) = get_json(app, "/v1/search/map?medium=Sculpture").await;
    assert_eq!(status, 200);
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].artist.slug, "bruno-test");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn medium_filter_unmatched_returns_empty(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, pins): (_, Vec<Pin>) = get_json(app, "/v1/search/map?medium=Holography").await;
    assert_eq!(status, 200);
    assert!(pins.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn q_filter_uses_artwork_tsvector(pool: PgPool) {
    // Alice's "Blue Morning" has "cobalt" in the description, so a
    // search for "cobalt" should keep her gallery and drop bruno's.
    let app = app_keyword_only(pool);
    let (status, pins): (_, Vec<Pin>) = get_json(app, "/v1/search/map?q=cobalt").await;
    assert_eq!(status, 200);
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].artist.slug, "alice-test");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn q_no_match_returns_empty(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, pins): (_, Vec<Pin>) = get_json(app, "/v1/search/map?q=xyzqwerty").await;
    assert_eq!(status, 200);
    assert!(pins.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// Hidden / non-public rows must not leak
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn pre_geocode_rows_are_hidden(pool: PgPool) {
    // Alice's studio row has lat=NULL, lng=NULL. Even with the largest
    // possible bbox the pre-geocode row must not appear (the SQL
    // filters `lat IS NOT NULL`).
    let app = app_keyword_only(pool);
    let (_, pins): (_, Vec<Pin>) = get_json(app, "/v1/search/map?bbox=-180,-90,180,90").await;
    assert!(!pins.iter().any(|p| p.name == "Studio (by appointment)"));
}

// ─────────────────────────────────────────────────────────────────────────────
// ?artist_ids= — the "map = view of grid result" thread-through path
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artist_ids_restricts_pins_to_those_artists(pool: PgPool) {
    // Seed has Alice in London + Bruno in Berlin. Pass only Alice's
    // id → only the London pin.
    let app = app_keyword_only(pool);
    let (_, pins): (_, Vec<Pin>) = get_json(
        app,
        "/v1/search/map?artist_ids=aaa11111-1111-1111-1111-111111111111",
    )
    .await;
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].city.as_deref(), Some("London"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artist_ids_composes_with_bbox(pool: PgPool) {
    // Both artists' ids, but bbox excludes Berlin → only London comes
    // back. Ensures artist_ids isn't short-circuiting the bbox filter.
    let app = app_keyword_only(pool);
    let url = "/v1/search/map?artist_ids=aaa11111-1111-1111-1111-111111111111,aaa22222-2222-2222-2222-222222222222&bbox=-1,51,1,52";
    let (_, pins): (_, Vec<Pin>) = get_json(app, url).await;
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].city.as_deref(), Some("London"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artist_ids_unknown_id_returns_empty(pool: PgPool) {
    // A real-but-not-in-DB UUID → zero matches, no error.
    let app = app_keyword_only(pool);
    let (_, pins): (_, Vec<Pin>) = get_json(
        app,
        "/v1/search/map?artist_ids=00000000-0000-0000-0000-000000000000",
    )
    .await;
    assert!(pins.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artist_ids_invalid_uuid_is_400(pool: PgPool) {
    use common::get_status;
    let app = app_keyword_only(pool);
    let (status, _) = get_status(app, "/v1/search/map?artist_ids=not-a-uuid").await;
    assert_eq!(status, 400);
}
