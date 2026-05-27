// Per-file allow: `Deserialize`-only fields trigger `dead_code` under
// `-D warnings`, but we keep them to document the JSON contract the API
// returns. See `decisions.md` 2026-05-27 — Pre-commit hooks.
#![allow(dead_code)]

mod common;

use common::{app_keyword_only, get_json, get_status, MIGRATOR};
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Deserialize, Debug)]
struct Summary {
    title: Option<String>,
    artist_slug: String,
    primary_image_url: Option<String>,
    currency: String,
    availability: String,
}

#[derive(Deserialize, Debug)]
struct Page {
    items: Vec<Summary>,
    next_cursor: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// No-query path: shape, ordering, draft exclusion
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_no_query_returns_published_artworks_newest_first(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, page): (_, Page) = get_json(app, "/v1/search?limit=10").await;
    assert_eq!(status, 200);
    // 5 published artworks; draft "Hidden Sketch" must not appear.
    assert_eq!(page.items.len(), 5);
    let titles: Vec<_> = page.items.iter().filter_map(|i| i.title.clone()).collect();
    assert!(!titles.iter().any(|t| t == "Hidden Sketch"));
    // Newest-first by default — "Linocut Study" (1 day ago) comes before "Blue Morning" (5 days ago)
    assert_eq!(page.items[0].title.as_deref(), Some("Linocut Study"));
    assert_eq!(
        page.items.last().unwrap().title.as_deref(),
        Some("Blue Morning")
    );
    assert!(page.next_cursor.is_none());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_respects_limit(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?limit=2").await;
    assert_eq!(page.items.len(), 2);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_returns_primary_image_url(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?limit=5").await;
    for item in &page.items {
        let url = item
            .primary_image_url
            .as_ref()
            .expect("each fixture artwork has a primary image");
        assert!(
            url.contains("test/"),
            "expected test fixture s3_key in URL, got {url}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Keyword search (embedder disabled)
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_keyword_matches_title(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?q=blue&limit=10").await;
    // "Blue Morning" should match; nothing else has "blue" in tsvector.
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].title.as_deref(), Some("Blue Morning"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_keyword_matches_medium(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?q=sculpture&limit=10").await;
    // Both Stone Forms are sculptures.
    assert_eq!(page.items.len(), 2);
    for item in &page.items {
        assert_eq!(item.artist_slug, "bruno-test");
    }
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_keyword_no_match_is_empty(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?q=ocelot&limit=10").await;
    assert!(page.items.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// Filters
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_medium_filter(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?medium=Print&limit=10").await;
    // Only "Linocut Study" is published with medium=Print.
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].title.as_deref(), Some("Linocut Study"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_price_range_filter(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?price_min=100000&price_max=300000").await;
    // Blue Morning (100k), Crimson Field (250k) — Stone Form II (500k) excluded.
    let titles: Vec<_> = page.items.iter().filter_map(|i| i.title.clone()).collect();
    assert!(titles.contains(&"Blue Morning".to_string()));
    assert!(titles.contains(&"Crimson Field".to_string()));
    assert!(!titles.contains(&"Stone Form II".to_string()));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_availability_filter(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?availability=sold").await;
    // Only Stone Form II is sold.
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].title.as_deref(), Some("Stone Form II"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Geographic filters
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_location_filter_string_match(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?location=berlin").await;
    // Bruno's two sculptures; Alice (London) and Carmen (no location) excluded.
    for item in &page.items {
        assert_eq!(item.artist_slug, "bruno-test");
    }
    assert_eq!(page.items.len(), 2);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_near_me_haversine(pool: PgPool) {
    let app = app_keyword_only(pool);
    // Center on Berlin, 50km radius — Bruno only.
    let (_, page): (_, Page) = get_json(
        app,
        "/v1/search?near_lat=52.52&near_lng=13.405&near_radius_km=50",
    )
    .await;
    for item in &page.items {
        assert_eq!(item.artist_slug, "bruno-test");
    }
    assert!(!page.items.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_nearest_sort_requires_coords(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, _) = get_status(app, "/v1/search?sort=nearest").await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_near_lat_without_lng_is_400(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, _) = get_status(app, "/v1/search?near_lat=51.5").await;
    assert_eq!(status, 400);
}

// ─────────────────────────────────────────────────────────────────────────────
// Currency / metadata preserved
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_preserves_currency(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?q=stone").await;
    for item in &page.items {
        assert_eq!(item.currency, "EUR");
    }
}
