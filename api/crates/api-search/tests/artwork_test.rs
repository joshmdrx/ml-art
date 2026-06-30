mod common;

use common::{app_keyword_only, get_json, get_status, MIGRATOR};
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Deserialize, Debug)]
struct Artist {
    slug: String,
    display_name: String,
}

#[derive(Deserialize, Debug)]
struct Image {
    url: String,
    is_primary: bool,
}

#[derive(Deserialize, Debug)]
struct Artwork {
    title: Option<String>,
    description: Option<String>,
    medium: Option<String>,
    price_cents: Option<i64>,
    currency: String,
    availability: String,
    artist: Artist,
    images: Vec<Image>,
    #[serde(default)]
    venues: Vec<ArtworkVenue>,
}

#[derive(Deserialize, Debug)]
struct ArtworkVenue {
    slug: String,
    name: String,
}

#[derive(Deserialize, Debug)]
struct SimSummary {
    artist_slug: String,
}

#[derive(Deserialize, Debug)]
struct SimPage {
    items: Vec<SimSummary>,
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artwork_detail_happy_path(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, artwork): (_, Artwork) =
        get_json(app, "/v1/artworks/bbb11111-1111-1111-1111-111111111111").await;
    assert_eq!(status, 200);
    // T-081 — venues is always present (empty when none accepted).
    assert!(artwork.venues.is_empty());
    assert_eq!(artwork.title.as_deref(), Some("Blue Morning"));
    assert_eq!(artwork.medium.as_deref(), Some("Painting"));
    assert_eq!(artwork.price_cents, Some(100000));
    assert_eq!(artwork.currency, "USD");
    assert_eq!(artwork.availability, "available");
    assert_eq!(artwork.artist.slug, "alice-test");
    assert_eq!(artwork.artist.display_name, "Alice Test");
    assert_eq!(artwork.images.len(), 1);
    assert!(artwork.images[0].is_primary);
    assert!(artwork.images[0].url.contains("test/alice/1.jpg"));
    assert!(artwork.description.is_some());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artwork_with_null_price_serializes_null(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, artwork): (_, Artwork) =
        get_json(app, "/v1/artworks/bbb33333-3333-3333-3333-333333333333").await;
    assert_eq!(status, 200);
    assert!(artwork.price_cents.is_none());
    assert_eq!(artwork.availability, "inquire");
    assert_eq!(artwork.currency, "EUR");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artwork_draft_returns_404(pool: PgPool) {
    // bbb66666 is the draft "Hidden Sketch" — must not be reachable.
    let app = app_keyword_only(pool);
    let (status, _) = get_status(app, "/v1/artworks/bbb66666-6666-6666-6666-666666666666").await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artwork_missing_uuid_returns_404(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, _) = get_status(app, "/v1/artworks/00000000-0000-0000-0000-000000000000").await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artwork_surfaces_accepted_venues(pool: PgPool) {
    // T-081 — set up an active venue + an accepted venue_artworks row,
    // then assert /v1/artworks/:id includes the venue ref.
    let venue_id = sqlx::types::Uuid::parse_str("dddddddd-dddd-dddd-dddd-dddddddddddd").unwrap();
    sqlx::query(
        r#"
        INSERT INTO venues (id, slug, name, kind, owner_user_id, status, city, country)
        VALUES ($1, 'positive-gallery', 'Positive Gallery', 'gallery',
                '99999999-9999-9999-9999-999999999999', 'active',
                'London', 'GB')
        "#,
    )
    .bind(venue_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO venue_artworks (venue_id, artwork_id, status, decided_at)
         VALUES ($1, 'bbb11111-1111-1111-1111-111111111111'::uuid, 'accepted', now())",
    )
    .bind(venue_id)
    .execute(&pool)
    .await
    .unwrap();

    let app = app_keyword_only(pool);
    let (status, artwork): (_, Artwork) =
        get_json(app, "/v1/artworks/bbb11111-1111-1111-1111-111111111111").await;
    assert_eq!(status, 200);
    assert_eq!(artwork.venues.len(), 1);
    assert_eq!(artwork.venues[0].slug, "positive-gallery");
    assert_eq!(artwork.venues[0].name, "Positive Gallery");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn similar_excludes_anchor_and_same_artist_by_default(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, page): (_, SimPage) = get_json(
        app,
        "/v1/artworks/bbb11111-1111-1111-1111-111111111111/similar?limit=10",
    )
    .await;
    assert_eq!(status, 200);
    // Anchor is by alice-test; default excludes same-artist, so no alice rows.
    for item in &page.items {
        assert_ne!(item.artist_slug, "alice-test");
    }
    // 3 published non-alice artworks (Bruno x2, Carmen x1) → 3 results.
    assert_eq!(page.items.len(), 3);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn similar_with_include_same_artist_returns_other_alice(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, page): (_, SimPage) = get_json(
        app,
        "/v1/artworks/bbb11111-1111-1111-1111-111111111111/similar?limit=10&include_same_artist=true",
    )
    .await;
    assert_eq!(status, 200);
    // 4 other published artworks total; include_same_artist includes Crimson Field.
    let alice_count = page
        .items
        .iter()
        .filter(|i| i.artist_slug == "alice-test")
        .count();
    assert_eq!(alice_count, 1);
    assert_eq!(page.items.len(), 4);
}
