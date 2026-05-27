mod common;

use common::{app_keyword_only, get_json, get_status, MIGRATOR};
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Deserialize, Debug)]
struct Neighborhood {
    slug: String,
    name: String,
    artwork_count: i32,
    representative_image_urls: Vec<String>,
    is_featured: bool,
}

#[derive(Deserialize, Debug)]
struct Page<T> {
    items: Vec<T>,
}

#[derive(Deserialize, Debug)]
struct Summary {
    title: Option<String>,
}

#[derive(Deserialize, Debug)]
struct Detail {
    neighborhood: Neighborhood,
    artworks: Page<Summary>,
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn neighborhoods_index_returns_seeded(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, page): (_, Page<Neighborhood>) = get_json(app, "/v1/neighborhoods").await;
    assert_eq!(status, 200);
    assert_eq!(page.items.len(), 1);
    let n = &page.items[0];
    assert_eq!(n.slug, "test-vibes");
    assert_eq!(n.name, "Test Vibes");
    assert!(n.is_featured);
    assert_eq!(n.artwork_count, 3);
    // 3 representative images derived from rep_artwork_ids.
    assert_eq!(n.representative_image_urls.len(), 3);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn neighborhood_detail_includes_first_page(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, detail): (_, Detail) = get_json(app, "/v1/neighborhoods/test-vibes").await;
    assert_eq!(status, 200);
    assert_eq!(detail.neighborhood.slug, "test-vibes");
    // 3 published artworks linked via neighborhood_artworks.
    assert_eq!(detail.artworks.items.len(), 3);
    let titles: Vec<_> = detail
        .artworks
        .items
        .iter()
        .filter_map(|a| a.title.clone())
        .collect();
    assert!(titles.contains(&"Blue Morning".to_string()));
    assert!(titles.contains(&"Crimson Field".to_string()));
    assert!(titles.contains(&"Stone Form I".to_string()));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn neighborhood_missing_slug_is_404(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, _) = get_status(app, "/v1/neighborhoods/no-such-neighborhood").await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// Filter params (T-023) — same shape `/v1/search` accepts, minus location
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn neighborhood_detail_filters_by_medium(pool: PgPool) {
    let app = app_keyword_only(pool);

    // test-vibes contains: Blue Morning (Painting), Crimson Field (Painting),
    // Stone Form I (Sculpture).
    let (status, painting): (_, Detail) =
        get_json(app.clone(), "/v1/neighborhoods/test-vibes?medium=Painting").await;
    assert_eq!(status, 200);
    assert_eq!(painting.artworks.items.len(), 2);

    let (_, sculpture): (_, Detail) =
        get_json(app, "/v1/neighborhoods/test-vibes?medium=Sculpture").await;
    assert_eq!(sculpture.artworks.items.len(), 1);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn neighborhood_detail_filters_by_availability(pool: PgPool) {
    let app = app_keyword_only(pool);

    let (_, available): (_, Detail) = get_json(
        app.clone(),
        "/v1/neighborhoods/test-vibes?availability=available",
    )
    .await;
    assert_eq!(available.artworks.items.len(), 2); // Blue Morning + Crimson Field

    let (_, inquire): (_, Detail) =
        get_json(app, "/v1/neighborhoods/test-vibes?availability=inquire").await;
    assert_eq!(inquire.artworks.items.len(), 1); // Stone Form I
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn neighborhood_detail_filters_by_price_range(pool: PgPool) {
    let app = app_keyword_only(pool);

    // Blue Morning = $1k, Crimson Field = $2.5k, Stone Form I = no price.
    // price_max=200_000 cents = $2,000 — only Blue Morning qualifies. Rows
    // with NULL price_cents fail the `>= / <=` comparison so they drop out.
    let (_, cheap): (_, Detail) =
        get_json(app.clone(), "/v1/neighborhoods/test-vibes?price_max=200000").await;
    assert_eq!(cheap.artworks.items.len(), 1);

    let (_, mid): (_, Detail) = get_json(
        app,
        "/v1/neighborhoods/test-vibes?price_min=150000&price_max=300000",
    )
    .await;
    assert_eq!(mid.artworks.items.len(), 1); // Crimson Field
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn neighborhood_detail_filters_combine(pool: PgPool) {
    let app = app_keyword_only(pool);
    // Medium=Painting (2) AND availability=available (2) AND price_max=150000 (1).
    let (_, detail): (_, Detail) = get_json(
        app,
        "/v1/neighborhoods/test-vibes?medium=Painting&availability=available&price_max=150000",
    )
    .await;
    assert_eq!(detail.artworks.items.len(), 1);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn neighborhood_detail_empty_filters_no_op(pool: PgPool) {
    // Empty strings for `medium` / `availability` should NOT filter — they
    // come through as `""` from a "All" UI selection. Match the same lenient
    // treatment `/v1/search` gives them.
    let app = app_keyword_only(pool);
    let (status, detail): (_, Detail) =
        get_json(app, "/v1/neighborhoods/test-vibes?medium=&availability=").await;
    assert_eq!(status, 200);
    assert_eq!(detail.artworks.items.len(), 3);
}
