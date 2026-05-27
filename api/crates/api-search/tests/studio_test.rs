// Deserialize-only contract structs trigger dead_code under `-D warnings`.
// See `decisions.md` 2026-05-27 — Pre-commit hooks.
#![allow(dead_code)]

mod common;

use common::{
    app_with_auth_and_fixed_vector, app_with_test_auth, get_json_authed, send_authed, MIGRATOR,
};
use pgvector::Vector;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

const ALICE: &str = "test-user_test_alice";
const BOB: &str = "test-user_test_bob";
const ARTWORK_BLUE_MORNING: &str = "bbb11111-1111-1111-1111-111111111111";
const ARTWORK_STONE_FORM: &str = "bbb33333-3333-3333-3333-333333333333"; // owned by bruno

// ─────────────────────────────────────────────────────────────────────────────
// Wire shapes for parsing
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct StudioArtist {
    slug: String,
    display_name: String,
    status: String,
}

#[derive(Deserialize, Debug)]
struct ArtworkSummary {
    id: String,
    title: Option<String>,
    status: String,
    primary_image_url: Option<String>,
}

#[derive(Deserialize, Debug)]
struct Page<T> {
    items: Vec<T>,
}

#[derive(Deserialize, Debug)]
struct ArtworkDetail {
    id: String,
    title: Option<String>,
    status: String,
    description: Option<String>,
    images: Vec<Image>,
}

#[derive(Deserialize, Debug)]
struct Image {
    id: String,
    s3_key: String,
    url: String,
    is_primary: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// /v1/studio/me
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_me_returns_linked_artist(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, artist): (_, StudioArtist) = get_json_authed(app, "/v1/studio/me", ALICE).await;
    assert_eq!(status, 200);
    assert_eq!(artist.slug, "alice-test");
    assert_eq!(artist.display_name, "Alice Test");
    assert_eq!(artist.status, "active");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_me_404s_for_non_artist_user(pool: PgPool) {
    // Bob is a signed-in user with no `artists.user_id` link. Studio
    // must return 404, not 401, so we don't leak "yes there are artists
    // but you're not one."
    let app = app_with_test_auth(pool);
    let (status, _) = common::get_status_authed(app, "/v1/studio/me", BOB).await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_me_401s_without_auth(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = common::get_status(app, "/v1/studio/me").await;
    assert_eq!(status, 401);
}

// ─────────────────────────────────────────────────────────────────────────────
// /v1/studio/artworks GET
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_list_returns_only_my_artworks(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, page): (_, Page<ArtworkSummary>) =
        get_json_authed(app, "/v1/studio/artworks", ALICE).await;
    assert_eq!(status, 200);
    // Alice owns Blue Morning + Crimson Field; she does NOT own Bruno's
    // Stone Form pieces or Carmen's Linocut Study or the Hidden Sketch draft.
    let titles: Vec<_> = page.items.iter().filter_map(|a| a.title.clone()).collect();
    assert!(titles.contains(&"Blue Morning".to_string()));
    assert!(titles.contains(&"Crimson Field".to_string()));
    assert!(!titles.contains(&"Stone Form I".to_string()));
    assert!(!titles.contains(&"Linocut Study".to_string()));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_list_includes_drafts(pool: PgPool) {
    // Add a draft for alice and verify it shows up (the public /artists
    // endpoint hides drafts; studio must surface them).
    let app = app_with_test_auth(pool.clone());
    sqlx::query(
        r#"
        INSERT INTO artworks (id, artist_id, title, status)
        VALUES (gen_random_uuid(), $1, 'Draft Test', 'draft')
        "#,
    )
    .bind(uuid::Uuid::parse_str("aaa11111-1111-1111-1111-111111111111").unwrap())
    .execute(&pool)
    .await
    .unwrap();

    let (_, page): (_, Page<ArtworkSummary>) =
        get_json_authed(app, "/v1/studio/artworks", ALICE).await;
    let draft = page
        .items
        .iter()
        .find(|a| a.title.as_deref() == Some("Draft Test"));
    assert!(draft.is_some(), "draft should be visible to its artist");
    assert_eq!(draft.unwrap().status, "draft");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_status_filter(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (_, drafts): (_, Page<ArtworkSummary>) =
        get_json_authed(app.clone(), "/v1/studio/artworks?status=draft", ALICE).await;
    let (_, published): (_, Page<ArtworkSummary>) =
        get_json_authed(app, "/v1/studio/artworks?status=published", ALICE).await;
    // Alice's seed has no drafts; both her artworks are published.
    assert_eq!(drafts.items.len(), 0);
    assert!(published.items.iter().all(|a| a.status == "published"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_list_401_for_non_artist(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = common::get_status_authed(app, "/v1/studio/artworks", BOB).await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// /v1/studio/artworks POST (create)
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_create_defaults_to_draft(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = json!({
        "title": "Untitled Study",
        "medium": "Watercolour",
        "price_cents": 75000,
        "currency": "GBP",
    })
    .to_string();
    let (status, bytes) = send_authed(app, "POST", "/v1/studio/artworks", ALICE, Some(&body)).await;
    assert_eq!(status, 201);
    let created: ArtworkSummary = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(created.title.as_deref(), Some("Untitled Study"));
    assert_eq!(created.status, "draft");
    assert!(created.primary_image_url.is_none());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_create_rejects_bad_availability(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = json!({
        "title": "Bad",
        "availability": "stolen",
    })
    .to_string();
    let (status, _) = send_authed(app, "POST", "/v1/studio/artworks", ALICE, Some(&body)).await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_create_404s_for_non_artist(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = json!({"title": "Bob's first work"}).to_string();
    let (status, _) = send_authed(app, "POST", "/v1/studio/artworks", BOB, Some(&body)).await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// /v1/studio/artworks/:id PATCH
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_patch_updates_title_and_medium(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = json!({"title": "Blue Morning (rev)", "medium": "Oil on linen"}).to_string();
    let (status, bytes) = send_authed(
        app,
        "PATCH",
        &format!("/v1/studio/artworks/{ARTWORK_BLUE_MORNING}"),
        ALICE,
        Some(&body),
    )
    .await;
    assert_eq!(status, 200);
    let updated: ArtworkSummary = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(updated.title.as_deref(), Some("Blue Morning (rev)"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_patch_status_to_published_stamps_published_at(pool: PgPool) {
    // Create a draft, flip it to published, observe published_at populates.
    let app = app_with_test_auth(pool.clone());
    let create_body = json!({"title": "Draft → Published"}).to_string();
    let (_, bytes) = send_authed(
        app.clone(),
        "POST",
        "/v1/studio/artworks",
        ALICE,
        Some(&create_body),
    )
    .await;
    let created: ArtworkSummary = serde_json::from_slice(&bytes).unwrap();

    let patch_body = json!({"status": "published"}).to_string();
    let (status, _) = send_authed(
        app,
        "PATCH",
        &format!("/v1/studio/artworks/{}", created.id),
        ALICE,
        Some(&patch_body),
    )
    .await;
    assert_eq!(status, 200);

    // Direct DB check that published_at is stamped.
    let (published_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT published_at FROM artworks WHERE id = $1")
            .bind(uuid::Uuid::parse_str(&created.id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        published_at.is_some(),
        "published_at should populate on status='published'"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_patch_rejects_bad_status(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = json!({"status": "private"}).to_string();
    let (status, _) = send_authed(
        app,
        "PATCH",
        &format!("/v1/studio/artworks/{ARTWORK_BLUE_MORNING}"),
        ALICE,
        Some(&body),
    )
    .await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_alice_cannot_patch_brunos_artwork(pool: PgPool) {
    // Cross-artist access must 404 — both "doesn't exist" and "exists
    // but you can't touch it" collapse to the same response so we
    // don't leak Bruno's catalog to Alice.
    let app = app_with_test_auth(pool);
    let body = json!({"title": "Pwn'd"}).to_string();
    let (status, _) = send_authed(
        app,
        "PATCH",
        &format!("/v1/studio/artworks/{ARTWORK_STONE_FORM}"),
        ALICE,
        Some(&body),
    )
    .await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// /v1/studio/artworks/:id DELETE
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_delete_soft_deletes(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let (status, _) = send_authed(
        app.clone(),
        "DELETE",
        &format!("/v1/studio/artworks/{ARTWORK_BLUE_MORNING}"),
        ALICE,
        None,
    )
    .await;
    assert_eq!(status, 204);

    // Second delete → 404 (the row is now soft-deleted; the filter
    // `deleted_at IS NULL` excludes it).
    let (status, _) = send_authed(
        app,
        "DELETE",
        &format!("/v1/studio/artworks/{ARTWORK_BLUE_MORNING}"),
        ALICE,
        None,
    )
    .await;
    assert_eq!(status, 404);

    // Public detail endpoint must also 404 — soft-deleted means hidden.
    let (deleted_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT deleted_at FROM artworks WHERE id = $1")
            .bind(uuid::Uuid::parse_str(ARTWORK_BLUE_MORNING).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(deleted_at.is_some());
}

// ─────────────────────────────────────────────────────────────────────────────
// /v1/studio/artworks/:id/images
// ─────────────────────────────────────────────────────────────────────────────

fn unit_vector_at(pos: usize) -> Vector {
    let mut v = vec![0.0_f32; 1024];
    v[pos] = 1.0;
    Vector::from(v)
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_add_image_first_is_primary_and_embeds(pool: PgPool) {
    // Use the fixed-vector app so the inline `process_image` call
    // succeeds without hitting Jina. Create a fresh artwork (no
    // existing primary), then add its first image. Asserts: the row
    // lands, is_primary=true by default, and an embedding row appears.
    let app = app_with_auth_and_fixed_vector(pool.clone(), unit_vector_at(321));

    let created: ArtworkSummary = {
        let body = json!({"title": "Embed Me"}).to_string();
        let (_, bytes) = send_authed(
            app.clone(),
            "POST",
            "/v1/studio/artworks",
            ALICE,
            Some(&body),
        )
        .await;
        serde_json::from_slice(&bytes).unwrap()
    };

    let add_body = json!({"s3_key": "uploads/test-embed.jpg"}).to_string();
    let (status, bytes) = send_authed(
        app,
        "POST",
        &format!("/v1/studio/artworks/{}/images", created.id),
        ALICE,
        Some(&add_body),
    )
    .await;
    assert_eq!(status, 201);
    let img: Image = serde_json::from_slice(&bytes).unwrap();
    assert!(img.is_primary, "first image defaults to primary");
    assert!(img.url.ends_with("/uploads/test-embed.jpg"));

    // Embedding row landed via T-036's process_image.
    let (n,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM artwork_embeddings WHERE artwork_id = $1")
            .bind(uuid::Uuid::parse_str(&created.id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 1, "primary image add should create one embedding row");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_add_image_rejects_second_primary(pool: PgPool) {
    // Blue Morning already has a primary image (from the seed).
    // Adding another with is_primary=true must fail before INSERT.
    let app = app_with_auth_and_fixed_vector(pool, unit_vector_at(1));
    let body = json!({"s3_key": "uploads/another.jpg", "is_primary": true}).to_string();
    let (status, _) = send_authed(
        app,
        "POST",
        &format!("/v1/studio/artworks/{ARTWORK_BLUE_MORNING}/images"),
        ALICE,
        Some(&body),
    )
    .await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_remove_image(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());

    // Find the primary image id for Blue Morning.
    let (image_id,): (uuid::Uuid,) =
        sqlx::query_as("SELECT id FROM artwork_images WHERE artwork_id = $1 AND is_primary")
            .bind(uuid::Uuid::parse_str(ARTWORK_BLUE_MORNING).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();

    let (status, _) = send_authed(
        app,
        "DELETE",
        &format!("/v1/studio/artworks/{ARTWORK_BLUE_MORNING}/images/{image_id}"),
        ALICE,
        None,
    )
    .await;
    assert_eq!(status, 204);

    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM artwork_images WHERE id = $1")
        .bind(image_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_remove_image_404s_for_cross_artist(pool: PgPool) {
    // Pick an image belonging to Bruno's artwork. Alice attempts delete
    // → 404 (ownership boundary).
    let app = app_with_test_auth(pool.clone());
    let (image_id, art_id): (uuid::Uuid, uuid::Uuid) = sqlx::query_as(
        r#"SELECT ai.id, ai.artwork_id
           FROM artwork_images ai
           JOIN artworks a ON a.id = ai.artwork_id
           WHERE a.artist_id = $1
           LIMIT 1"#,
    )
    .bind(uuid::Uuid::parse_str("aaa22222-2222-2222-2222-222222222222").unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();

    let (status, _) = send_authed(
        app,
        "DELETE",
        &format!("/v1/studio/artworks/{art_id}/images/{image_id}"),
        ALICE,
        None,
    )
    .await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// Detail endpoint
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_detail_returns_images(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, detail): (_, ArtworkDetail) = get_json_authed(
        app,
        &format!("/v1/studio/artworks/{ARTWORK_BLUE_MORNING}"),
        ALICE,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(detail.status, "published");
    assert_eq!(detail.title.as_deref(), Some("Blue Morning"));
    assert_eq!(detail.images.len(), 1);
    assert!(detail.images[0].is_primary);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn studio_artworks_detail_404s_for_cross_artist(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = common::get_status_authed(
        app,
        &format!("/v1/studio/artworks/{ARTWORK_STONE_FORM}"),
        ALICE,
    )
    .await;
    assert_eq!(status, 404);
}
