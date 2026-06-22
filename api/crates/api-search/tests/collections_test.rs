// Per-file allow: `Deserialize`-only fields trigger `dead_code` under
// `-D warnings`. See `decisions.md` 2026-05-27 — Pre-commit hooks.
#![allow(dead_code)]

mod common;

use common::{app_with_test_auth, get_json_authed, send_authed, MIGRATOR};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

const ALICE: &str = "test-user_test_alice";
const BOB: &str = "test-user_test_bob";

#[derive(Deserialize, Debug)]
struct Summary {
    id: String,
    name: String,
    is_public: bool,
    share_id: Option<String>,
    artwork_count: i32,
    cover_image_urls: Vec<String>,
    /// Surfaced by the list endpoint when `?artwork_id=` is passed; the
    /// API defaults to `false` on every other response so this is always
    /// safe to read.
    #[serde(default)]
    contains_artwork: bool,
}

#[derive(Deserialize, Debug)]
struct ListPage {
    items: Vec<Summary>,
}

#[derive(Deserialize, Debug)]
struct DetailBody {
    collection: Summary,
    artworks: ArtworksPage,
}

#[derive(Deserialize, Debug)]
struct ArtworksPage {
    items: Vec<ArtworkSummary>,
}

#[derive(Deserialize, Debug)]
struct ArtworkSummary {
    id: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Auth gate
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn collections_list_requires_auth(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = common::get_status(app, "/v1/me/collections").await;
    assert_eq!(status, 401);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn collections_create_with_bad_bearer_is_401(pool: PgPool) {
    // Use a valid body so we don't hit the JSON-validation path (422)
    // before reaching the auth check.
    let app = app_with_test_auth(pool);
    let body = json!({"name": "Anything"}).to_string();
    let (status, _) = send_authed(
        app,
        "POST",
        "/v1/me/collections",
        "garbage-not-a-test-token",
        Some(&body),
    )
    .await;
    assert_eq!(status, 401);
}

// ─────────────────────────────────────────────────────────────────────────────
// CRUD lifecycle: create → list → patch → add artwork → detail → remove → delete
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn collections_create_then_list(pool: PgPool) {
    let app = app_with_test_auth(pool);

    // Empty list initially
    let (status, page): (_, ListPage) =
        get_json_authed(app.clone(), "/v1/me/collections", ALICE).await;
    assert_eq!(status, 200);
    assert!(page.items.is_empty());

    // Create one
    let (status, created): (_, Summary) = {
        let body = json!({"name": "Favourites"}).to_string();
        let (status, bytes) = send_authed(
            app.clone(),
            "POST",
            "/v1/me/collections",
            ALICE,
            Some(&body),
        )
        .await;
        let parsed = serde_json::from_slice(&bytes).unwrap();
        (status, parsed)
    };
    assert_eq!(status, 201);
    assert_eq!(created.name, "Favourites");
    assert!(!created.is_public);
    assert!(created.share_id.is_none());
    assert_eq!(created.artwork_count, 0);

    // Now list contains it
    let (_, page): (_, ListPage) = get_json_authed(app, "/v1/me/collections", ALICE).await;
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, created.id);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn collections_create_validates_name(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = json!({"name": "   "}).to_string();
    let (status, _) = send_authed(app, "POST", "/v1/me/collections", ALICE, Some(&body)).await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn collections_patch_renames(pool: PgPool) {
    let app = app_with_test_auth(pool);

    // Create
    let (_, created): (_, Summary) = {
        let body = json!({"name": "Old"}).to_string();
        let (status, bytes) = send_authed(
            app.clone(),
            "POST",
            "/v1/me/collections",
            ALICE,
            Some(&body),
        )
        .await;
        (status, serde_json::from_slice(&bytes).unwrap())
    };

    // Patch
    let patch = json!({"name": "New name"}).to_string();
    let (status, bytes) = send_authed(
        app,
        "PATCH",
        &format!("/v1/me/collections/{}", created.id),
        ALICE,
        Some(&patch),
    )
    .await;
    assert_eq!(status, 200);
    let updated: Summary = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(updated.name, "New name");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn collections_toggle_public_mints_share_id(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (_, created): (_, Summary) = {
        let body = json!({"name": "Goes public"}).to_string();
        let (_, bytes) = send_authed(
            app.clone(),
            "POST",
            "/v1/me/collections",
            ALICE,
            Some(&body),
        )
        .await;
        ((), serde_json::from_slice(&bytes).unwrap())
    };
    assert!(created.share_id.is_none());

    let patch = json!({"is_public": true}).to_string();
    let (status, bytes) = send_authed(
        app,
        "PATCH",
        &format!("/v1/me/collections/{}", created.id),
        ALICE,
        Some(&patch),
    )
    .await;
    assert_eq!(status, 200);
    let updated: Summary = serde_json::from_slice(&bytes).unwrap();
    assert!(updated.is_public);
    assert!(updated.share_id.is_some());
    assert!(updated.share_id.unwrap().len() >= 8);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn collections_add_then_remove_artwork(pool: PgPool) {
    let app = app_with_test_auth(pool);

    let (_, created): (_, Summary) = {
        let body = json!({"name": "Loose collection"}).to_string();
        let (_, bytes) = send_authed(
            app.clone(),
            "POST",
            "/v1/me/collections",
            ALICE,
            Some(&body),
        )
        .await;
        ((), serde_json::from_slice(&bytes).unwrap())
    };

    // Add (using a fixture artwork id — Alice the painter's "Blue Morning")
    let add_body = json!({"artwork_id": "bbb11111-1111-1111-1111-111111111111"}).to_string();
    let (status, _) = send_authed(
        app.clone(),
        "POST",
        &format!("/v1/me/collections/{}/artworks", created.id),
        ALICE,
        Some(&add_body),
    )
    .await;
    assert_eq!(status, 204);

    // Detail shows the artwork
    let (_, detail): (_, DetailBody) = get_json_authed(
        app.clone(),
        &format!("/v1/me/collections/{}", created.id),
        ALICE,
    )
    .await;
    assert_eq!(detail.artworks.items.len(), 1);
    assert_eq!(detail.collection.artwork_count, 1);
    assert!(!detail.collection.cover_image_urls.is_empty());

    // Second add is idempotent (still 204, still one artwork)
    let (status, _) = send_authed(
        app.clone(),
        "POST",
        &format!("/v1/me/collections/{}/artworks", created.id),
        ALICE,
        Some(&add_body),
    )
    .await;
    assert_eq!(status, 204);

    let (_, detail): (_, DetailBody) = get_json_authed(
        app.clone(),
        &format!("/v1/me/collections/{}", created.id),
        ALICE,
    )
    .await;
    assert_eq!(detail.artworks.items.len(), 1);

    // Remove
    let (status, _) = send_authed(
        app.clone(),
        "DELETE",
        &format!(
            "/v1/me/collections/{}/artworks/bbb11111-1111-1111-1111-111111111111",
            created.id
        ),
        ALICE,
        None,
    )
    .await;
    assert_eq!(status, 204);

    let (_, detail): (_, DetailBody) =
        get_json_authed(app, &format!("/v1/me/collections/{}", created.id), ALICE).await;
    assert!(detail.artworks.items.is_empty());
    assert_eq!(detail.collection.artwork_count, 0);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn collections_delete_soft_deletes(pool: PgPool) {
    let app = app_with_test_auth(pool);

    let (_, created): (_, Summary) = {
        let body = json!({"name": "Goes away"}).to_string();
        let (_, bytes) = send_authed(
            app.clone(),
            "POST",
            "/v1/me/collections",
            ALICE,
            Some(&body),
        )
        .await;
        ((), serde_json::from_slice(&bytes).unwrap())
    };

    let (status, _) = send_authed(
        app.clone(),
        "DELETE",
        &format!("/v1/me/collections/{}", created.id),
        ALICE,
        None,
    )
    .await;
    assert_eq!(status, 204);

    // Gone from list
    let (_, page): (_, ListPage) = get_json_authed(app.clone(), "/v1/me/collections", ALICE).await;
    assert!(page.items.is_empty());

    // Gone from detail too
    let (status, _) = send_authed(
        app,
        "GET",
        &format!("/v1/me/collections/{}", created.id),
        ALICE,
        None,
    )
    .await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// Ownership boundaries — Bob must not touch Alice's collection
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn collections_bob_cannot_read_alices(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (_, created): (_, Summary) = {
        let body = json!({"name": "Alice's only"}).to_string();
        let (_, bytes) = send_authed(
            app.clone(),
            "POST",
            "/v1/me/collections",
            ALICE,
            Some(&body),
        )
        .await;
        ((), serde_json::from_slice(&bytes).unwrap())
    };

    let (status, _) = send_authed(
        app,
        "GET",
        &format!("/v1/me/collections/{}", created.id),
        BOB,
        None,
    )
    .await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn collections_bob_cannot_patch_alices(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (_, created): (_, Summary) = {
        let body = json!({"name": "Alice's only"}).to_string();
        let (_, bytes) = send_authed(
            app.clone(),
            "POST",
            "/v1/me/collections",
            ALICE,
            Some(&body),
        )
        .await;
        ((), serde_json::from_slice(&bytes).unwrap())
    };

    let patch = json!({"name": "Hijacked"}).to_string();
    let (status, _) = send_authed(
        app,
        "PATCH",
        &format!("/v1/me/collections/{}", created.id),
        BOB,
        Some(&patch),
    )
    .await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn collections_bob_cannot_add_to_alices(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (_, created): (_, Summary) = {
        let body = json!({"name": "Alice's only"}).to_string();
        let (_, bytes) = send_authed(
            app.clone(),
            "POST",
            "/v1/me/collections",
            ALICE,
            Some(&body),
        )
        .await;
        ((), serde_json::from_slice(&bytes).unwrap())
    };

    let add = json!({"artwork_id": "bbb11111-1111-1111-1111-111111111111"}).to_string();
    let (status, _) = send_authed(
        app,
        "POST",
        &format!("/v1/me/collections/{}/artworks", created.id),
        BOB,
        Some(&add),
    )
    .await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn collections_bob_cannot_delete_alices(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (_, created): (_, Summary) = {
        let body = json!({"name": "Alice's only"}).to_string();
        let (_, bytes) = send_authed(
            app.clone(),
            "POST",
            "/v1/me/collections",
            ALICE,
            Some(&body),
        )
        .await;
        ((), serde_json::from_slice(&bytes).unwrap())
    };

    let (status, _) = send_authed(
        app,
        "DELETE",
        &format!("/v1/me/collections/{}", created.id),
        BOB,
        None,
    )
    .await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// `?artwork_id=` membership flag (T-029)
// ─────────────────────────────────────────────────────────────────────────────

const ARTWORK_BLUE_MORNING: &str = "bbb11111-1111-1111-1111-111111111111";

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn collections_list_contains_artwork_flag(pool: PgPool) {
    let app = app_with_test_auth(pool);

    // Build two collections — one will hold the artwork, one won't.
    let yes: Summary = {
        let body = json!({"name": "Has it"}).to_string();
        let (_, bytes) = send_authed(
            app.clone(),
            "POST",
            "/v1/me/collections",
            ALICE,
            Some(&body),
        )
        .await;
        serde_json::from_slice(&bytes).unwrap()
    };
    let no: Summary = {
        let body = json!({"name": "Doesn't"}).to_string();
        let (_, bytes) = send_authed(
            app.clone(),
            "POST",
            "/v1/me/collections",
            ALICE,
            Some(&body),
        )
        .await;
        serde_json::from_slice(&bytes).unwrap()
    };

    // Add the artwork to `yes` only.
    let add_body = json!({"artwork_id": ARTWORK_BLUE_MORNING}).to_string();
    let (status, _) = send_authed(
        app.clone(),
        "POST",
        &format!("/v1/me/collections/{}/artworks", yes.id),
        ALICE,
        Some(&add_body),
    )
    .await;
    assert_eq!(status, 204);

    // 1. No query param → all rows must report `contains_artwork: false`.
    let (status, plain): (_, ListPage) =
        get_json_authed(app.clone(), "/v1/me/collections", ALICE).await;
    assert_eq!(status, 200);
    assert!(
        plain.items.iter().all(|s| !s.contains_artwork),
        "plain list must default contains_artwork to false"
    );

    // 2. With `?artwork_id=<bbb…>` → exactly the `yes` collection is flagged.
    let (status, filtered): (_, ListPage) = get_json_authed(
        app.clone(),
        &format!("/v1/me/collections?artwork_id={ARTWORK_BLUE_MORNING}"),
        ALICE,
    )
    .await;
    assert_eq!(status, 200);
    let yes_row = filtered.items.iter().find(|s| s.id == yes.id).unwrap();
    let no_row = filtered.items.iter().find(|s| s.id == no.id).unwrap();
    assert!(yes_row.contains_artwork, "yes-collection should be flagged");
    assert!(
        !no_row.contains_artwork,
        "no-collection should not be flagged"
    );

    // 3. With a different (real but uninvolved) artwork id → none flagged.
    let unrelated = "bbb22222-2222-2222-2222-222222222222";
    let (status, neither): (_, ListPage) = get_json_authed(
        app,
        &format!("/v1/me/collections?artwork_id={unrelated}"),
        ALICE,
    )
    .await;
    assert_eq!(status, 200);
    assert!(
        neither.items.iter().all(|s| !s.contains_artwork),
        "querying a non-member artwork should leave all flags false"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn collections_list_rejects_malformed_artwork_id(pool: PgPool) {
    // Axum's Query extractor rejects unparseable UUIDs with 400.
    let app = app_with_test_auth(pool);
    let (status, _) = send_authed(
        app,
        "GET",
        "/v1/me/collections?artwork_id=not-a-uuid",
        ALICE,
        None,
    )
    .await;
    assert_eq!(status, 400);
}

// ─────────────────────────────────────────────────────────────────────────────
// T-053 — public read by share_id
// ─────────────────────────────────────────────────────────────────────────────

/// Helper: create a public collection, add an artwork, return (share_id, collection_id).
async fn make_public_collection(app: axum::Router) -> (String, String) {
    let body = json!({"name": "Mood board", "is_public": true}).to_string();
    let (_, bytes) = send_authed(
        app.clone(),
        "POST",
        "/v1/me/collections",
        ALICE,
        Some(&body),
    )
    .await;
    let created: Summary = serde_json::from_slice(&bytes).unwrap();
    let share_id = created
        .share_id
        .clone()
        .expect("share_id minted on public create");

    let add_body = json!({"artwork_id": ARTWORK_BLUE_MORNING}).to_string();
    let (status, _) = send_authed(
        app,
        "POST",
        &format!("/v1/me/collections/{}/artworks", created.id),
        ALICE,
        Some(&add_body),
    )
    .await;
    assert_eq!(status, 204);

    (share_id, created.id)
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn public_share_happy(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (share_id, _) = make_public_collection(app.clone()).await;

    let (status, detail): (_, DetailBody) =
        common::get_json(app, &format!("/v1/collections/share/{share_id}")).await;
    assert_eq!(status, 200);
    assert!(detail.collection.is_public);
    assert_eq!(detail.collection.name, "Mood board");
    assert_eq!(detail.artworks.items.len(), 1);
    assert_eq!(detail.collection.artwork_count, 1);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn public_share_private_collection_returns_404(pool: PgPool) {
    let app = app_with_test_auth(pool);

    // Create a *private* collection. share_id is null at this point, so
    // we have no way to even craft a malicious URL — but verify that a
    // valid-looking arbitrary token still 404s.
    let body = json!({"name": "Private mood"}).to_string();
    let (_, _bytes) = send_authed(
        app.clone(),
        "POST",
        "/v1/me/collections",
        ALICE,
        Some(&body),
    )
    .await;

    let (status, _) = common::get_status(app, "/v1/collections/share/aaaaaaaaaaaa").await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn public_share_unknown_token_returns_404(pool: PgPool) {
    let app = app_with_test_auth(pool);
    // No collections created — any token must 404.
    let (status, _) = common::get_status(app, "/v1/collections/share/zzzz9999yyyy").await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn public_share_malformed_token_returns_404(pool: PgPool) {
    let app = app_with_test_auth(pool);
    // Special chars, too short, too long — all 404 (the API rejects
    // obviously-malformed tokens before hitting the DB).
    for bad in ["short", "a", "../../etc/passwd", "tokenWith!Special"] {
        let (status, _) =
            common::get_status(app.clone(), &format!("/v1/collections/share/{bad}")).await;
        assert_eq!(status, 404, "expected 404 for token {bad:?}");
    }
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn public_share_id_rotates_on_toggle_old_link_dies(pool: PgPool) {
    let app = app_with_test_auth(pool);

    // Step 1: create + go public → s1
    let (s1, collection_id) = make_public_collection(app.clone()).await;
    let (status, _) = common::get_status(app.clone(), &format!("/v1/collections/share/{s1}")).await;
    assert_eq!(status, 200, "s1 should work while public");

    // Step 2: go private → s1 dies
    let patch = json!({"is_public": false}).to_string();
    let (status, _) = send_authed(
        app.clone(),
        "PATCH",
        &format!("/v1/me/collections/{collection_id}"),
        ALICE,
        Some(&patch),
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = common::get_status(app.clone(), &format!("/v1/collections/share/{s1}")).await;
    assert_eq!(status, 404, "s1 should 404 once collection is private");

    // Step 3: go public again → new share_id (s2) ≠ s1
    let patch = json!({"is_public": true}).to_string();
    let (_, bytes) = send_authed(
        app.clone(),
        "PATCH",
        &format!("/v1/me/collections/{collection_id}"),
        ALICE,
        Some(&patch),
    )
    .await;
    let again: Summary = serde_json::from_slice(&bytes).unwrap();
    let s2 = again.share_id.expect("share_id re-minted on second toggle");
    assert_ne!(s1, s2, "share_id rotates across the private toggle");

    // s1 still 404; s2 works.
    let (status, _) = common::get_status(app.clone(), &format!("/v1/collections/share/{s1}")).await;
    assert_eq!(
        status, 404,
        "old s1 must stay dead even after re-publishing"
    );
    let (status, _detail): (_, DetailBody) =
        common::get_json(app, &format!("/v1/collections/share/{s2}")).await;
    assert_eq!(status, 200, "new s2 works");
}
