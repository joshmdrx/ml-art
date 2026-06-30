// T-081.1 — venues + venue_artworks integration tests.
//
// Coverage:
//   - Studio CRUD: create / list / detail / patch / delete + 404 on
//     wrong owner.
//   - Slug collision returns 409.
//   - Invitation flow: invite creates pending; artist sees it in
//     /v1/studio/venue-requests; accept / decline transitions; only
//     accepted rows show on public surfaces.
//   - Re-invite after decline reopens to pending.
//   - Public list filters on status='active' (pending_review hidden).
//   - Public detail 404 for pending venues.
//   - Cascade-clear: deleting an artwork removes its venue_artworks
//     rows (ON DELETE CASCADE).

#![allow(dead_code)]

mod common;

use common::{app_with_test_auth, get_json_authed, send_authed, MIGRATOR};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

const OWNER_BEARER: &str = "test-user_test_99"; // generic test user — the venue owner
const ARTIST_BEARER: &str = "test-user_test_alice"; // signed-in artist
const OWNER_USER_ID: &str = "99999999-9999-9999-9999-999999999999";
const ALICE_USER_ID: &str = "88888888-8888-8888-8888-888888888888";
const ALICE_ARTIST_ID: &str = "aaa11111-1111-1111-1111-111111111111";
const BLUE_MORNING: &str = "bbb11111-1111-1111-1111-111111111111";
const CRIMSON_FIELD: &str = "bbb22222-2222-2222-2222-222222222222";

#[derive(Deserialize, Debug)]
struct Venue {
    id: String,
    slug: String,
    name: String,
    status: String,
    address: Option<String>,
    owner_user_id: String,
}

#[derive(Deserialize, Debug)]
struct VenueArtwork {
    artwork_id: String,
    status: String,
}

#[derive(Deserialize, Debug)]
struct VenueRequest {
    venue_id: String,
    artwork_id: String,
    status: String,
}

#[derive(Deserialize, Debug)]
struct PublicListResponse {
    items: Vec<PublicVenue>,
}

#[derive(Deserialize, Debug)]
struct PublicVenue {
    slug: String,
}

#[derive(Deserialize, Debug)]
struct PublicDetail {
    slug: String,
    artworks: Vec<PublicArtwork>,
}

#[derive(Deserialize, Debug)]
struct PublicArtwork {
    artwork_id: String,
}

async fn create_venue(app: &axum::Router, bearer: &str, name: &str) -> Venue {
    let body = format!(
        r#"{{"name":"{name}","kind":"gallery","address":"1 Test St, London"}}"#
    );
    let (status, bytes) =
        send_authed(app.clone(), "POST", "/v1/studio/venues", bearer, Some(&body)).await;
    assert_eq!(status, 201, "create venue: {}", String::from_utf8_lossy(&bytes));
    serde_json::from_slice(&bytes).unwrap()
}

// ─────────────────────────────────────────────────────────────────────────────
// Studio CRUD
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn venues_create_starts_pending_review(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let v = create_venue(&app, OWNER_BEARER, "Test Gallery One").await;
    assert_eq!(v.name, "Test Gallery One");
    assert_eq!(v.slug, "test-gallery-one");
    assert_eq!(v.status, "pending_review");
    assert_eq!(v.owner_user_id, OWNER_USER_ID);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn venues_create_requires_auth(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = send_authed(app, "POST", "/v1/studio/venues", "", Some(r#"{"name":"X","kind":"gallery"}"#)).await;
    assert_eq!(status, 401);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn venues_slug_collision_is_409(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let _ = create_venue(&app, OWNER_BEARER, "Test Gallery").await;
    let (status, _) = send_authed(
        app,
        "POST",
        "/v1/studio/venues",
        OWNER_BEARER,
        Some(r#"{"name":"Test Gallery","kind":"shop"}"#),
    )
    .await;
    assert_eq!(status, 409);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn venues_list_own_only(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let _ = create_venue(&app, OWNER_BEARER, "Mine A").await;
    let _ = create_venue(&app, OWNER_BEARER, "Mine B").await;
    let _ = create_venue(&app, ARTIST_BEARER, "Not Mine").await;

    let (status, list): (_, Vec<Venue>) =
        get_json_authed(app, "/v1/studio/venues", OWNER_BEARER).await;
    assert_eq!(status, 200);
    assert_eq!(list.len(), 2);
    for v in &list {
        assert_eq!(v.owner_user_id, OWNER_USER_ID);
    }
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn venues_detail_404_for_non_owner(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let v = create_venue(&app, OWNER_BEARER, "Owner Only").await;
    let uri = format!("/v1/studio/venues/{}", v.id);

    // Owner can read.
    let (status, _) = send_authed(app.clone(), "GET", &uri, OWNER_BEARER, None).await;
    assert_eq!(status, 200);

    // Different user gets 404 — leaks no info.
    let (status, _) = send_authed(app, "GET", &uri, ARTIST_BEARER, None).await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn venues_patch_updates_name(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let v = create_venue(&app, OWNER_BEARER, "Old Name").await;
    let uri = format!("/v1/studio/venues/{}", v.id);
    let (status, bytes) = send_authed(
        app,
        "PATCH",
        &uri,
        OWNER_BEARER,
        Some(r#"{"name":"New Name"}"#),
    )
    .await;
    assert_eq!(status, 200);
    let updated: Venue = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(updated.name, "New Name");
    assert_eq!(updated.slug, "old-name"); // slug stays — only name changed
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn venues_delete_soft_deletes(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let v = create_venue(&app, OWNER_BEARER, "To Delete").await;
    let uri = format!("/v1/studio/venues/{}", v.id);
    let (status, _) = send_authed(app.clone(), "DELETE", &uri, OWNER_BEARER, None).await;
    assert_eq!(status, 204);

    // Vanishes from list_own.
    let (_, list): (_, Vec<Venue>) =
        get_json_authed(app, "/v1/studio/venues", OWNER_BEARER).await;
    assert_eq!(list.len(), 0);

    // Soft-delete: row still exists.
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM venues WHERE id = $1 AND deleted_at IS NOT NULL)",
    )
    .bind(Uuid::parse_str(&v.id).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(exists);
}

// ─────────────────────────────────────────────────────────────────────────────
// Invitation flow
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn venue_artwork_invite_then_artist_accepts(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let v = create_venue(&app, OWNER_BEARER, "Invite Gallery").await;

    // Owner invites Alice's "Blue Morning".
    let uri = format!("/v1/studio/venues/{}/artworks/{}", v.id, BLUE_MORNING);
    let (status, _) = send_authed(app.clone(), "POST", &uri, OWNER_BEARER, None).await;
    assert_eq!(status, 204);

    // Alice sees a pending request.
    let (status, reqs): (_, Vec<VenueRequest>) =
        get_json_authed(app.clone(), "/v1/studio/venue-requests", ARTIST_BEARER).await;
    assert_eq!(status, 200);
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].artwork_id, BLUE_MORNING);
    assert_eq!(reqs[0].status, "pending");

    // Accept.
    let accept_uri = format!(
        "/v1/studio/venue-requests/{}/{}/accept",
        v.id, BLUE_MORNING
    );
    let (status, _) = send_authed(app.clone(), "POST", &accept_uri, ARTIST_BEARER, None).await;
    assert_eq!(status, 204);

    // Inbox now empty (only pending shows).
    let (_, reqs): (_, Vec<VenueRequest>) =
        get_json_authed(app, "/v1/studio/venue-requests", ARTIST_BEARER).await;
    assert_eq!(reqs.len(), 0);

    // venue_artworks row is `accepted`.
    let status: String =
        sqlx::query_scalar("SELECT status FROM venue_artworks WHERE venue_id=$1 AND artwork_id=$2")
            .bind(Uuid::parse_str(&v.id).unwrap())
            .bind(Uuid::parse_str(BLUE_MORNING).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "accepted");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn venue_artist_decline_then_reinvite_reopens_pending(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let v = create_venue(&app, OWNER_BEARER, "Persistent Gallery").await;
    let invite_uri = format!("/v1/studio/venues/{}/artworks/{}", v.id, BLUE_MORNING);
    let decline_uri = format!(
        "/v1/studio/venue-requests/{}/{}/decline",
        v.id, BLUE_MORNING
    );

    let (status, _) = send_authed(app.clone(), "POST", &invite_uri, OWNER_BEARER, None).await;
    assert_eq!(status, 204);
    let (status, _) = send_authed(app.clone(), "POST", &decline_uri, ARTIST_BEARER, None).await;
    assert_eq!(status, 204);

    // Re-invite: row flips back to pending.
    let (status, _) = send_authed(app, "POST", &invite_uri, OWNER_BEARER, None).await;
    assert_eq!(status, 204);

    let row_status: String =
        sqlx::query_scalar("SELECT status FROM venue_artworks WHERE venue_id=$1 AND artwork_id=$2")
            .bind(Uuid::parse_str(&v.id).unwrap())
            .bind(Uuid::parse_str(BLUE_MORNING).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row_status, "pending");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn venue_artwork_invite_unknown_artwork_404(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let v = create_venue(&app, OWNER_BEARER, "Some Gallery").await;
    let uri = format!(
        "/v1/studio/venues/{}/artworks/ffffffff-ffff-ffff-ffff-ffffffffffff",
        v.id
    );
    let (status, _) = send_authed(app, "POST", &uri, OWNER_BEARER, None).await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn venue_artwork_uninvite_removes_row(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let v = create_venue(&app, OWNER_BEARER, "Maybe Gallery").await;
    let uri = format!("/v1/studio/venues/{}/artworks/{}", v.id, BLUE_MORNING);
    let (s, _) = send_authed(app.clone(), "POST", &uri, OWNER_BEARER, None).await;
    assert_eq!(s, 204);
    let (s, _) = send_authed(app, "DELETE", &uri, OWNER_BEARER, None).await;
    assert_eq!(s, 204);

    let row_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM venue_artworks WHERE venue_id=$1 AND artwork_id=$2",
    )
    .bind(Uuid::parse_str(&v.id).unwrap())
    .bind(Uuid::parse_str(BLUE_MORNING).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row_count, 0);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn venue_artwork_cascade_clear_on_artwork_delete(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let v = create_venue(&app, OWNER_BEARER, "Cascade Gallery").await;
    let uri = format!("/v1/studio/venues/{}/artworks/{}", v.id, CRIMSON_FIELD);
    let (s, _) = send_authed(app, "POST", &uri, OWNER_BEARER, None).await;
    assert_eq!(s, 204);

    // Hard-delete the artwork to test the FK cascade rule.
    sqlx::query("DELETE FROM artworks WHERE id = $1")
        .bind(Uuid::parse_str(CRIMSON_FIELD).unwrap())
        .execute(&pool)
        .await
        .unwrap();

    let row_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM venue_artworks WHERE artwork_id=$1",
    )
    .bind(Uuid::parse_str(CRIMSON_FIELD).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row_count, 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// Public reads
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn public_list_hides_pending_review(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let _ = create_venue(&app, OWNER_BEARER, "Hidden Gallery").await;

    let (status, page): (_, PublicListResponse) =
        get_json_authed(app, "/v1/venues", "").await;
    assert_eq!(status, 200);
    assert!(
        page.items.is_empty(),
        "pending_review venues must not appear publicly"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn public_detail_only_shows_accepted_artworks(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let v = create_venue(&app, OWNER_BEARER, "Mixed Gallery").await;

    // Flip the venue to active so it's publicly readable.
    sqlx::query("UPDATE venues SET status='active' WHERE id = $1")
        .bind(Uuid::parse_str(&v.id).unwrap())
        .execute(&pool)
        .await
        .unwrap();

    // One accepted artwork; one pending.
    sqlx::query(
        "INSERT INTO venue_artworks (venue_id, artwork_id, status, decided_at)
         VALUES ($1, $2, 'accepted', now()), ($1, $3, 'pending', NULL)",
    )
    .bind(Uuid::parse_str(&v.id).unwrap())
    .bind(Uuid::parse_str(BLUE_MORNING).unwrap())
    .bind(Uuid::parse_str(CRIMSON_FIELD).unwrap())
    .execute(&pool)
    .await
    .unwrap();

    let (status, detail): (_, PublicDetail) =
        get_json_authed(app, &format!("/v1/venues/{}", v.slug), "").await;
    assert_eq!(status, 200);
    assert_eq!(detail.slug, v.slug);
    assert_eq!(detail.artworks.len(), 1, "only accepted artworks surface");
    assert_eq!(detail.artworks[0].artwork_id, BLUE_MORNING);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn public_detail_pending_review_is_404(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let v = create_venue(&app, OWNER_BEARER, "Still Pending").await;
    let (status, _) = send_authed(app, "GET", &format!("/v1/venues/{}", v.slug), "", None).await;
    assert_eq!(status, 404);
}
