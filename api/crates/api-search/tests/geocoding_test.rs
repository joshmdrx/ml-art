//! T-038 G2 — integration tests for the geocode-and-update pipeline.
//!
//! Drives `geocode_and_update` against a real Postgres pool with a
//! stubbed `GeocodingClient::for_tests`, asserting:
//!   - success path writes lat/lng/city/country + geocoded_at
//!   - empty-result path stamps geocoded_at (no retry storm) but
//!     leaves lat/lng NULL so the row stays hidden from public surfaces
//!   - missing/deleted rows are a soft no-op (not an error)

mod common;

use common::MIGRATOR;
use ml_art_core::geocoding::{geocode_and_update, Geocoded, GeocodingClient};
use sqlx::PgPool;
use uuid::Uuid;

const ALICE_ID: Uuid = Uuid::from_u128(0xaaa1_1111_1111_1111_1111_1111_1111_1111);

/// The pre-geocode studio row alice has in the seed (lat/lng NULL).
const ALICE_PENDING_STUDIO: Uuid = Uuid::from_u128(0xddd2_2222_2222_2222_2222_2222_2222_2222);

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn geocode_writes_lat_lng_city_country(pool: PgPool) {
    // Stub returns a London address for the studio's exact address line.
    let canned = vec![(
        "99 Test Lane, London".to_string(),
        Some(Geocoded {
            lng: -0.111,
            lat: 51.501,
            city: Some("London".to_string()),
            country: Some("GB".to_string()),
        }),
    )];
    let client = GeocodingClient::for_tests(canned);

    geocode_and_update(&client, &pool, ALICE_PENDING_STUDIO)
        .await
        .unwrap();

    let row: GeocodedRow = sqlx::query_as(
        "SELECT lat, lng, city, country, geocoded_at FROM artist_locations WHERE id = $1",
    )
    .bind(ALICE_PENDING_STUDIO)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.lat, Some(51.501));
    assert_eq!(row.lng, Some(-0.111));
    assert_eq!(row.city.as_deref(), Some("London"));
    assert_eq!(row.country.as_deref(), Some("GB"));
    assert!(row.geocoded_at.is_some(), "geocoded_at should be set");
}

#[derive(sqlx::FromRow)]
struct GeocodedRow {
    lat: Option<f64>,
    lng: Option<f64>,
    city: Option<String>,
    country: Option<String>,
    geocoded_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn geocode_with_disabled_client_stamps_only_geocoded_at(pool: PgPool) {
    // Disabled client mimics MAPBOX_TOKEN unset. The row stays hidden
    // from public surfaces (lat/lng NULL) but we mark it processed.
    let client = GeocodingClient::disabled();

    geocode_and_update(&client, &pool, ALICE_PENDING_STUDIO)
        .await
        .unwrap();

    let row: GeocodedRow =
        sqlx::query_as("SELECT lat, lng, NULL::text AS city, NULL::text AS country, geocoded_at FROM artist_locations WHERE id = $1")
            .bind(ALICE_PENDING_STUDIO)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert!(row.lat.is_none(), "lat must stay NULL when no match");
    assert!(row.lng.is_none(), "lng must stay NULL when no match");
    assert!(
        row.geocoded_at.is_some(),
        "geocoded_at must be stamped so we don't retry forever"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn geocode_for_missing_row_is_noop(pool: PgPool) {
    // Row got deleted between enqueue and run — should not error.
    let client = GeocodingClient::disabled();
    let bogus = Uuid::from_u128(0xdead_dead_dead_dead_dead_dead_dead_dead);
    geocode_and_update(&client, &pool, bogus).await.unwrap();
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn geocode_re_runs_overwrite_previous_coords(pool: PgPool) {
    // The geocoded gallery row (alice's first location) already has
    // coords. If an artist edits the address (or we run a backfill),
    // we should be able to overwrite. Drive that here.
    let alice_gallery: Uuid = Uuid::from_u128(0xddd1_1111_1111_1111_1111_1111_1111_1111);

    let canned = vec![(
        "1 Test St, London EC1A 1AA".to_string(),
        Some(Geocoded {
            lng: -0.0001,
            lat: 51.0001,
            city: Some("Westminster".to_string()),
            country: Some("GB".to_string()),
        }),
    )];
    let client = GeocodingClient::for_tests(canned);

    geocode_and_update(&client, &pool, alice_gallery)
        .await
        .unwrap();

    let row: (f64, f64, Option<String>) =
        sqlx::query_as("SELECT lat, lng, city FROM artist_locations WHERE id = $1")
            .bind(alice_gallery)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.0, 51.0001);
    assert_eq!(row.1, -0.0001);
    assert_eq!(row.2.as_deref(), Some("Westminster"));
}

// `ALICE_ID` is unused for now (placeholder for future tests that
// scope by-artist). Silence dead_code without #[allow] on the const.
#[test]
fn _alice_constant_is_referenced() {
    assert_eq!(ALICE_ID.to_string(), "aaa11111-1111-1111-1111-111111111111");
}
