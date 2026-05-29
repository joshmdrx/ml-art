mod common;

use common::{app_keyword_only, get_json, get_status, MIGRATOR};
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Deserialize, Debug)]
struct Artist {
    slug: String,
    display_name: String,
    bio: Option<String>,
    location: Option<String>,
    city: Option<String>,
    country: Option<String>,
    representative_image_urls: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct Summary {
    title: Option<String>,
    artist_slug: String,
}

#[derive(Deserialize, Debug)]
struct Page {
    items: Vec<Summary>,
}

#[derive(Deserialize, Debug)]
struct Detail {
    artist: Artist,
    artworks: Page,
    #[serde(default)]
    locations: Vec<Location>,
}

#[derive(Deserialize, Debug)]
struct Location {
    kind: String,
    name: String,
    address: String,
    city: Option<String>,
    country: Option<String>,
    lat: Option<f64>,
    lng: Option<f64>,
    website_url: Option<String>,
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artist_detail_happy_path(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, detail): (_, Detail) = get_json(app, "/v1/artists/alice-test").await;
    assert_eq!(status, 200);
    assert_eq!(detail.artist.slug, "alice-test");
    assert_eq!(detail.artist.display_name, "Alice Test");
    assert_eq!(detail.artist.city.as_deref(), Some("London"));
    assert_eq!(detail.artist.country.as_deref(), Some("GB"));
    assert!(detail.artist.bio.is_some());
    // Alice has 2 published artworks; both must appear.
    assert_eq!(detail.artworks.items.len(), 2);
    for item in &detail.artworks.items {
        assert_eq!(item.artist_slug, "alice-test");
    }
    // Up to 3 representative URLs derived from the artworks' primaries.
    assert!(!detail.artist.representative_image_urls.is_empty());
    assert!(detail.artist.representative_image_urls.len() <= 3);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artist_without_location_returns_nulls(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, detail): (_, Detail) = get_json(app, "/v1/artists/carmen-test").await;
    assert_eq!(status, 200);
    assert!(detail.artist.location.is_none());
    assert!(detail.artist.city.is_none());
    assert!(detail.artist.country.is_none());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artist_artworks_exclude_drafts(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, detail): (_, Detail) = get_json(app, "/v1/artists/carmen-test").await;
    // Carmen has 1 published + 1 draft. Draft must not appear.
    assert_eq!(detail.artworks.items.len(), 1);
    assert_eq!(
        detail.artworks.items[0].title.as_deref(),
        Some("Linocut Study")
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artist_missing_slug_is_404(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, _) = get_status(app, "/v1/artists/does-not-exist").await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// T-038 — artist_locations on the artist detail payload
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artist_detail_returns_geocoded_locations(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, detail): (_, Detail) = get_json(app, "/v1/artists/alice-test").await;
    assert_eq!(status, 200);

    // Alice has 2 location rows in the fixture; only the geocoded gallery
    // row should appear on the public payload. The pre-geocode studio row
    // (lat/lng NULL) must be hidden.
    assert_eq!(detail.locations.len(), 1);
    let loc = &detail.locations[0];
    assert_eq!(loc.kind, "gallery");
    assert_eq!(loc.name, "Test Gallery London");
    assert_eq!(loc.address, "1 Test St, London EC1A 1AA");
    assert_eq!(loc.city.as_deref(), Some("London"));
    assert_eq!(loc.country.as_deref(), Some("GB"));
    assert!(loc.lat.is_some());
    assert!(loc.lng.is_some());
    assert_eq!(
        loc.website_url.as_deref(),
        Some("https://test-gallery.example")
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artist_detail_empty_locations_for_artist_without_any(pool: PgPool) {
    // Carmen has no `artist_locations` rows.
    let app = app_keyword_only(pool);
    let (status, detail): (_, Detail) = get_json(app, "/v1/artists/carmen-test").await;
    assert_eq!(status, 200);
    assert!(detail.locations.is_empty());
}
