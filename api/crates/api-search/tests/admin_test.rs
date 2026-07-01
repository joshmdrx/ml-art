// T-083.1 — admin surface (`/v1/admin/*`) integration tests.
//
// Coverage:
//   - Auth gates: 401 without bearer, 403 with non-admin bearer.
//   - List endpoint: status filter default + explicit, paginated.
//   - Approve: pending → active, audit row written, admin_user_id set.
//   - Approve: idempotent re-application (already-active → 200 no audit row).
//   - Approve: illegal source-state (rejected → active) is 409.
//   - Decline: pending → rejected, audit row written.
//   - Pause + Unpause: active → paused → active round-trip.
//   - Unknown artist: 404 (not 403 — the auth gate fires before lookup).

// Deserialize-only structs trip `dead_code` under -D warnings; the
// fields are part of the wire contract we're asserting.
#![allow(dead_code)]

mod common;

use common::{app_with_test_auth, get_json_authed, send_authed, MIGRATOR};
use serde::Deserialize;
use sqlx::PgPool;

const ADMIN_BEARER: &str = "test-user_test_admin";
const ALICE_BEARER: &str = "test-user_test_alice"; // non-admin user
const PENDING_ARTIST: &str = "aaa44444-4444-4444-4444-444444444444"; // dora-pending
const ACTIVE_ARTIST: &str = "aaa11111-1111-1111-1111-111111111111"; // alice
const UNKNOWN_ARTIST: &str = "ffffffff-ffff-ffff-ffff-ffffffffffff";

#[derive(Deserialize, Debug)]
struct ListPage {
    items: Vec<AdminArtistItem>,
    next_cursor: Option<String>,
}

#[derive(Deserialize, Debug)]
struct AdminArtistItem {
    id: String,
    slug: String,
    status: String,
    artwork_count: i64,
}

#[derive(Deserialize, Debug)]
struct StatusRow {
    id: String,
    status: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Auth gates
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_list_requires_bearer(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = send_authed(app, "GET", "/v1/admin/artists", "", None).await;
    // Bearer is empty → 401.
    assert_eq!(status, 401);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_list_non_admin_is_403(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = send_authed(app, "GET", "/v1/admin/artists", ALICE_BEARER, None).await;
    assert_eq!(status, 403);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_approve_non_admin_is_403(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let uri = format!("/v1/admin/artists/{PENDING_ARTIST}/approve");
    let (status, _) = send_authed(app, "POST", &uri, ALICE_BEARER, None).await;
    assert_eq!(status, 403);
}

// ─────────────────────────────────────────────────────────────────────────────
// List endpoint
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_list_defaults_to_pending(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, page): (_, ListPage) =
        get_json_authed(app, "/v1/admin/artists", ADMIN_BEARER).await;
    assert_eq!(status, 200);
    // dora-pending + franz-pending have status='pending'. franz is the
    // dedicated E2E admin-decline target — see fixtures/seed.sql.
    assert_eq!(page.items.len(), 2);
    let slugs: std::collections::HashSet<&str> =
        page.items.iter().map(|i| i.slug.as_str()).collect();
    assert!(slugs.contains("dora-pending"));
    assert!(slugs.contains("franz-pending"));
    for it in &page.items {
        assert_eq!(it.status, "pending");
        assert_eq!(it.artwork_count, 0);
    }
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_list_active_filter(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, page): (_, ListPage) =
        get_json_authed(app, "/v1/admin/artists?status=active", ADMIN_BEARER).await;
    assert_eq!(status, 200);
    // alice, bruno, carmen, greta are active in seed. (greta is the
    // dedicated E2E pause/unpause target — see fixtures/seed.sql.)
    assert_eq!(page.items.len(), 4);
    for it in &page.items {
        assert_eq!(it.status, "active");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Approve / decline / pause / unpause
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_approve_pending_artist_transitions_to_active(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let uri = format!("/v1/admin/artists/{PENDING_ARTIST}/approve");
    let (status, bytes) = send_authed(app, "POST", &uri, ADMIN_BEARER, None).await;
    assert_eq!(status, 200);
    let row: StatusRow = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(row.status, "active");

    // Audit row written, admin_user_id set to the admin caller.
    let (action, admin_id): (String, Option<uuid::Uuid>) = sqlx::query_as(
        "SELECT action, admin_user_id FROM admin_audit_log
         WHERE target_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(uuid::Uuid::parse_str(PENDING_ARTIST).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(action, "artist.approve");
    assert_eq!(
        admin_id.unwrap(),
        uuid::Uuid::parse_str("66666666-6666-6666-6666-666666666666").unwrap()
    );

    // DB state matches the response.
    let db_status: String =
        sqlx::query_scalar("SELECT status FROM artists WHERE id = $1")
            .bind(uuid::Uuid::parse_str(PENDING_ARTIST).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(db_status, "active");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_approve_already_active_is_idempotent_no_audit(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let uri = format!("/v1/admin/artists/{ACTIVE_ARTIST}/approve");
    let (status, _) = send_authed(app, "POST", &uri, ADMIN_BEARER, None).await;
    assert_eq!(status, 200);

    let audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM admin_audit_log WHERE target_id = $1",
    )
    .bind(uuid::Uuid::parse_str(ACTIVE_ARTIST).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        audit_count, 0,
        "no-op transition must not clutter the audit log"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_approve_active_after_decline_is_409(pool: PgPool) {
    // Set up: take dora-pending → rejected first.
    sqlx::query("UPDATE artists SET status='rejected' WHERE id=$1")
        .bind(uuid::Uuid::parse_str(PENDING_ARTIST).unwrap())
        .execute(&pool)
        .await
        .unwrap();

    let app = app_with_test_auth(pool);
    let uri = format!("/v1/admin/artists/{PENDING_ARTIST}/approve");
    let (status, _) = send_authed(app, "POST", &uri, ADMIN_BEARER, None).await;
    // Illegal source state — re-pending them first via a UI affordance,
    // not direct approve.
    assert_eq!(status, 409);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_decline_pending_artist_writes_audit(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let uri = format!("/v1/admin/artists/{PENDING_ARTIST}/decline");
    let (status, bytes) = send_authed(app, "POST", &uri, ADMIN_BEARER, None).await;
    assert_eq!(status, 200);
    let row: StatusRow = serde_json::from_slice(&bytes).unwrap();
    // Schema CHECK pins the column value to `rejected`; the wire word
    // ("decline") is on the action name.
    assert_eq!(row.status, "rejected");

    let action: String = sqlx::query_scalar(
        "SELECT action FROM admin_audit_log
         WHERE target_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(uuid::Uuid::parse_str(PENDING_ARTIST).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(action, "artist.decline");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_pause_unpause_round_trip(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());

    // active → paused
    let uri = format!("/v1/admin/artists/{ACTIVE_ARTIST}/pause");
    let (status, _) = send_authed(app.clone(), "POST", &uri, ADMIN_BEARER, None).await;
    assert_eq!(status, 200);
    let paused: String = sqlx::query_scalar("SELECT status FROM artists WHERE id=$1")
        .bind(uuid::Uuid::parse_str(ACTIVE_ARTIST).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(paused, "paused");

    // paused → active
    let uri = format!("/v1/admin/artists/{ACTIVE_ARTIST}/unpause");
    let (status, _) = send_authed(app, "POST", &uri, ADMIN_BEARER, None).await;
    assert_eq!(status, 200);
    let active: String = sqlx::query_scalar("SELECT status FROM artists WHERE id=$1")
        .bind(uuid::Uuid::parse_str(ACTIVE_ARTIST).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(active, "active");

    // Two audit rows for the two transitions.
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM admin_audit_log WHERE target_id = $1",
    )
    .bind(uuid::Uuid::parse_str(ACTIVE_ARTIST).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit_count, 2);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_approve_unknown_artist_is_404(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let uri = format!("/v1/admin/artists/{UNKNOWN_ARTIST}/approve");
    let (status, _) = send_authed(app, "POST", &uri, ADMIN_BEARER, None).await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// T-083.3 — image moderation override queue
// ─────────────────────────────────────────────────────────────────────────────

const REJECTED_IMAGE: &str = "eee11111-1111-1111-1111-111111111111";

#[derive(Deserialize, Debug)]
struct ImageListPage {
    items: Vec<AdminImageItem>,
}

#[derive(Deserialize, Debug)]
struct AdminImageItem {
    id: String,
    moderation_status: String,
    moderation_reason: Option<String>,
    artist_slug: String,
}

#[derive(Deserialize, Debug)]
struct ImageStatusRow {
    id: String,
    moderation_status: String,
    moderation_reason: Option<String>,
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_images_list_requires_admin(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = send_authed(app, "GET", "/v1/admin/images", ALICE_BEARER, None).await;
    assert_eq!(status, 403);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_images_list_defaults_to_rejected(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, page): (_, ImageListPage) =
        get_json_authed(app, "/v1/admin/images", ADMIN_BEARER).await;
    assert_eq!(status, 200);
    assert_eq!(page.items.len(), 1);
    let it = &page.items[0];
    assert_eq!(it.id, REJECTED_IMAGE);
    assert_eq!(it.moderation_status, "rejected");
    assert_eq!(it.moderation_reason.as_deref(), Some("EXPLICIT_NUDITY"));
    assert_eq!(it.artist_slug, "carmen-test");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_image_override_flips_status_and_clears_reason(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let uri = format!("/v1/admin/images/{REJECTED_IMAGE}/override");
    let (status, bytes) = send_authed(app, "POST", &uri, ADMIN_BEARER, None).await;
    assert_eq!(status, 200);
    let row: ImageStatusRow = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(row.moderation_status, "approved");
    assert!(row.moderation_reason.is_none());

    // Audit row written, target_kind=image.
    let (action, target_kind): (String, String) = sqlx::query_as(
        "SELECT action, target_kind FROM admin_audit_log
         WHERE target_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(uuid::Uuid::parse_str(REJECTED_IMAGE).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(action, "image.override_approve");
    assert_eq!(target_kind, "image");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_image_override_already_approved_is_noop(pool: PgPool) {
    // Pick the existing approved primary image on alice's Blue Morning.
    // It's `approved` in the seed, so override is the no-op branch.
    let id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM artwork_images WHERE artwork_id = $1 AND is_primary = true")
            .bind(uuid::Uuid::parse_str("bbb11111-1111-1111-1111-111111111111").unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();

    let app = app_with_test_auth(pool.clone());
    let uri = format!("/v1/admin/images/{id}/override");
    let (status, _) = send_authed(app, "POST", &uri, ADMIN_BEARER, None).await;
    assert_eq!(status, 200);

    let audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM admin_audit_log WHERE target_id = $1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit_count, 0);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_image_override_unknown_image_is_404(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let uri = format!("/v1/admin/images/{UNKNOWN_ARTIST}/override"); // any unknown uuid
    let (status, _) = send_authed(app, "POST", &uri, ADMIN_BEARER, None).await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// T-083.5 — audit log read-only viewer
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct AuditLogPage {
    items: Vec<AuditLogEntry>,
}

#[derive(Deserialize, Debug)]
struct AuditLogEntry {
    action: String,
    target_kind: String,
    admin_email: Option<String>,
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_audit_log_requires_admin(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = send_authed(app, "GET", "/v1/admin/audit-log", ALICE_BEARER, None).await;
    assert_eq!(status, 403);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_audit_log_returns_recent_actions(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());

    // Perform a transition to seed an audit row.
    let uri = format!("/v1/admin/artists/{PENDING_ARTIST}/approve");
    let (status, _) = send_authed(app.clone(), "POST", &uri, ADMIN_BEARER, None).await;
    assert_eq!(status, 200);

    // Audit log lists it with the admin email joined in.
    let (status, page): (_, AuditLogPage) =
        get_json_authed(app, "/v1/admin/audit-log", ADMIN_BEARER).await;
    assert_eq!(status, 200);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].action, "artist.approve");
    assert_eq!(page.items[0].target_kind, "artist");
    assert_eq!(page.items[0].admin_email.as_deref(), Some("admin@example.com"));
}
