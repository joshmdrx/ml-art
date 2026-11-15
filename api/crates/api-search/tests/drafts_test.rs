// T-086 — consent-first draft profiles: admin side (`/v1/admin/drafts`)
// and the token-gated public side (`/v1/preview/:token`).
//
// Coverage:
//   - Create: admin-only, consent note required, lands pending + unowned.
//   - Never public: public profile 404s, excluded from the approval
//     queue, approve + studio publish toggle both refused (409).
//   - Edit-as: an admin's studio calls resolve to the draft while
//     editing; stopping (or a claim) ends it.
//   - Preview links: show draft artworks, one live link per draft,
//     malformed / unknown / expired tokens all 404, only a hash stored.
//   - Claim: links the signed-in user, stays unpublished, kills the
//     link, refuses accounts that already have a profile.
//   - Remove (via link) + takedown (admin): soft-delete + audit trail.

#![allow(dead_code)]

mod common;

use common::{
    app_with_test_auth, get_json_authed, get_status, get_status_authed, send_authed, send_json,
    MIGRATOR,
};
use serde_json::Value;
use sqlx::PgPool;

const ADMIN: &str = "test-user_test_admin";
const ALICE: &str = "test-user_test_alice"; // artist already
const BOB: &str = "test-user_test_bob"; // signed-in, no profile
const BOB_USER_ID: &str = "77777777-7777-7777-7777-777777777777";

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes)
        .unwrap_or_else(|e| panic!("decode: {e}\nbody: {}", String::from_utf8_lossy(bytes)))
}

/// Create a draft as the admin; returns (id, slug).
async fn create_draft(pool: &PgPool, name: &str) -> (String, String) {
    let body = format!(
        r#"{{"display_name":"{name}","location":"Deptford, London","website_url":"https://example.org","consent_note":"Replied yes to the founding-studio email"}}"#
    );
    let (status, bytes) = send_authed(
        app_with_test_auth(pool.clone()),
        "POST",
        "/v1/admin/drafts",
        ADMIN,
        Some(&body),
    )
    .await;
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&bytes));
    let v = json(&bytes);
    (
        v["id"].as_str().unwrap().to_string(),
        v["slug"].as_str().unwrap().to_string(),
    )
}

/// Mint a preview link as the admin; returns the plaintext token.
async fn mint_link(pool: &PgPool, draft_id: &str) -> String {
    let (status, bytes) = send_authed(
        app_with_test_auth(pool.clone()),
        "POST",
        &format!("/v1/admin/drafts/{draft_id}/preview-link"),
        ADMIN,
        None,
    )
    .await;
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&bytes));
    let v = json(&bytes);
    let token = v["token"].as_str().unwrap().to_string();
    assert_eq!(v["path"].as_str().unwrap(), format!("/preview/{token}"));
    token
}

// ─────────────────────────────────────────────────────────────────────────────
// Create
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn drafts_create_is_admin_only(pool: PgPool) {
    let body = r#"{"display_name":"Cell","consent_note":"yes"}"#;
    let (status, _) = send_authed(
        app_with_test_auth(pool),
        "POST",
        "/v1/admin/drafts",
        ALICE,
        Some(body),
    )
    .await;
    assert_eq!(status, 403);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn drafts_create_requires_consent_note(pool: PgPool) {
    let body = r#"{"display_name":"Cell","consent_note":"   "}"#;
    let (status, _) = send_authed(
        app_with_test_auth(pool),
        "POST",
        "/v1/admin/drafts",
        ADMIN,
        Some(body),
    )
    .await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn drafts_create_then_list(pool: PgPool) {
    let (id, slug) = create_draft(&pool, "Cell Project Space").await;
    assert_eq!(slug, "cell-project-space");

    let (status, list): (_, Value) =
        get_json_authed(app_with_test_auth(pool.clone()), "/v1/admin/drafts", ADMIN).await;
    assert_eq!(status, 200);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    let d = &items[0];
    assert_eq!(d["id"], id.as_str());
    assert_eq!(d["entity_type"], "gallery");
    assert_eq!(d["status"], "pending");
    assert_eq!(d["claimed"], false);
    assert_eq!(
        d["consent_note"],
        "Replied yes to the founding-studio email"
    );
    assert!(d["live_link_expires_at"].is_null());

    let (user_id, prebuilt): (Option<uuid::Uuid>, bool) =
        sqlx::query_as("SELECT user_id, is_prebuilt FROM artists WHERE slug = $1")
            .bind(&slug)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(user_id.is_none());
    assert!(prebuilt);

    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM admin_audit_log WHERE action = 'draft.create' AND target_id = $1::uuid",
    )
    .bind(&id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// Never public while unclaimed
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn draft_is_hidden_from_public_page_and_admin_queue(pool: PgPool) {
    let (id, slug) = create_draft(&pool, "Bearspace").await;

    let (status, _) = get_status(
        app_with_test_auth(pool.clone()),
        &format!("/v1/artists/{slug}"),
    )
    .await;
    assert_eq!(status, 404);

    let (_, queue): (_, Value) = get_json_authed(
        app_with_test_auth(pool.clone()),
        "/v1/admin/artists?status=pending",
        ADMIN,
    )
    .await;
    let ids: Vec<&str> = queue["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&id.as_str()), "draft leaked into queue");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn draft_cannot_be_approved(pool: PgPool) {
    let (id, _) = create_draft(&pool, "Bearspace").await;
    let (status, _) = send_authed(
        app_with_test_auth(pool.clone()),
        "POST",
        &format!("/v1/admin/artists/{id}/approve"),
        ADMIN,
        None,
    )
    .await;
    assert_eq!(status, 409);
    let s: String = sqlx::query_scalar("SELECT status FROM artists WHERE id = $1::uuid")
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(s, "pending");
}

// ─────────────────────────────────────────────────────────────────────────────
// Edit-as through the studio
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_editing_routes_studio_to_the_draft(pool: PgPool) {
    let (id, slug) = create_draft(&pool, "APT Gallery").await;

    // Before editing: the admin has no artist of their own → 404.
    let (status, _) =
        get_status_authed(app_with_test_auth(pool.clone()), "/v1/studio/me", ADMIN).await;
    assert_eq!(status, 404);

    let (status, _) = send_authed(
        app_with_test_auth(pool.clone()),
        "POST",
        &format!("/v1/admin/drafts/{id}/edit"),
        ADMIN,
        None,
    )
    .await;
    assert_eq!(status, 200);

    let (status, me): (_, Value) =
        get_json_authed(app_with_test_auth(pool.clone()), "/v1/studio/me", ADMIN).await;
    assert_eq!(status, 200);
    assert_eq!(me["slug"], slug.as_str());
    assert_eq!(me["editing_draft"], true);

    // Studio writes land on the draft.
    let (status, _) = send_authed(
        app_with_test_auth(pool.clone()),
        "POST",
        "/v1/studio/artworks",
        ADMIN,
        Some(r#"{"title":"Common Ghosts"}"#),
    )
    .await;
    assert_eq!(status, 201);
    let owner: uuid::Uuid =
        sqlx::query_scalar("SELECT artist_id FROM artworks WHERE title = 'Common Ghosts'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(owner.to_string(), id);

    // …but the publish toggle is refused while unclaimed.
    let (status, _) = send_authed(
        app_with_test_auth(pool.clone()),
        "PATCH",
        "/v1/studio/settings",
        ADMIN,
        Some(r#"{"status":"active"}"#),
    )
    .await;
    assert_eq!(status, 409);

    let (status, _) = send_authed(
        app_with_test_auth(pool.clone()),
        "DELETE",
        "/v1/admin/drafts/editing",
        ADMIN,
        None,
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) =
        get_status_authed(app_with_test_auth(pool.clone()), "/v1/studio/me", ADMIN).await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn non_admin_editing_pointer_is_ignored(pool: PgPool) {
    // Defence in depth: even if a non-admin row somehow points at a
    // draft, studio resolution ignores it.
    let (id, _) = create_draft(&pool, "APT Gallery").await;
    sqlx::query("UPDATE users SET editing_draft_artist_id = $1::uuid WHERE id = $2::uuid")
        .bind(&id)
        .bind(BOB_USER_ID)
        .execute(&pool)
        .await
        .unwrap();
    let (status, _) =
        get_status_authed(app_with_test_auth(pool.clone()), "/v1/studio/me", BOB).await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// Preview links
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn preview_link_shows_draft_including_draft_artworks(pool: PgPool) {
    let (id, _) = create_draft(&pool, "Jealous Gallery").await;
    sqlx::query(
        "INSERT INTO artworks (artist_id, title, status) VALUES ($1::uuid, 'Clearance Sale', 'draft')",
    )
    .bind(&id)
    .execute(&pool)
    .await
    .unwrap();
    let token = mint_link(&pool, &id).await;

    let (status, bytes) = get_status(
        app_with_test_auth(pool.clone()),
        &format!("/v1/preview/{token}"),
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&bytes));
    let v = json(&bytes);
    assert_eq!(v["artist"]["display_name"], "Jealous Gallery");
    assert_eq!(v["artist"]["entity_type"], "gallery");
    let titles: Vec<&str> = v["artworks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, vec!["Clearance Sale"]);
    assert!(v["expires_at"].is_string());

    // Only the hash is stored.
    let stored: String = sqlx::query_scalar(
        "SELECT token_hash FROM artist_preview_links WHERE artist_id = $1::uuid",
    )
    .bind(&id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_ne!(stored, token);
    assert_eq!(stored, ml_art_core::preview_token::hash(&token));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn reminting_a_link_revokes_the_old_one(pool: PgPool) {
    let (id, _) = create_draft(&pool, "Print Club").await;
    let old = mint_link(&pool, &id).await;
    let new = mint_link(&pool, &id).await;
    let (s_old, _) = get_status(
        app_with_test_auth(pool.clone()),
        &format!("/v1/preview/{old}"),
    )
    .await;
    let (s_new, _) = get_status(
        app_with_test_auth(pool.clone()),
        &format!("/v1/preview/{new}"),
    )
    .await;
    assert_eq!(s_old, 404);
    assert_eq!(s_new, 200);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn bad_tokens_all_404(pool: PgPool) {
    let (id, _) = create_draft(&pool, "Print Club").await;
    let token = mint_link(&pool, &id).await;

    // Malformed, unknown (well-formed), and expired all look the same.
    let unknown = "0".repeat(64);
    for t in ["nope", unknown.as_str()] {
        let (status, _) = get_status(
            app_with_test_auth(pool.clone()),
            &format!("/v1/preview/{t}"),
        )
        .await;
        assert_eq!(status, 404, "token {t}");
    }
    sqlx::query("UPDATE artist_preview_links SET expires_at = now() - interval '1 day'")
        .execute(&pool)
        .await
        .unwrap();
    let (status, _) = get_status(
        app_with_test_auth(pool.clone()),
        &format!("/v1/preview/{token}"),
    )
    .await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn links_cannot_be_minted_for_non_drafts(pool: PgPool) {
    // alice-test is a normal, self-onboarded artist.
    let (status, _) = send_authed(
        app_with_test_auth(pool),
        "POST",
        "/v1/admin/drafts/aaa11111-1111-1111-1111-111111111111/preview-link",
        ADMIN,
        None,
    )
    .await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// Claim
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn claim_links_the_account_and_keeps_it_unpublished(pool: PgPool) {
    let (id, slug) = create_draft(&pool, "Kingsgate Workshops").await;
    // Admin is mid-edit when the claim lands.
    send_authed(
        app_with_test_auth(pool.clone()),
        "POST",
        &format!("/v1/admin/drafts/{id}/edit"),
        ADMIN,
        None,
    )
    .await;
    let token = mint_link(&pool, &id).await;

    let (status, bytes) = send_authed(
        app_with_test_auth(pool.clone()),
        "POST",
        &format!("/v1/preview/{token}/claim"),
        BOB,
        None,
    )
    .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&bytes));
    let v = json(&bytes);
    assert_eq!(v["slug"], slug.as_str());
    assert_eq!(v["status"], "pending");

    let owner: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT user_id FROM artists WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(owner.unwrap().to_string(), BOB_USER_ID);

    // Bob's studio is now this page; the link is spent; still not public.
    let (_, me): (_, Value) =
        get_json_authed(app_with_test_auth(pool.clone()), "/v1/studio/me", BOB).await;
    assert_eq!(me["slug"], slug.as_str());
    assert_eq!(me["editing_draft"], false);
    let (status, _) = get_status(
        app_with_test_auth(pool.clone()),
        &format!("/v1/preview/{token}"),
    )
    .await;
    assert_eq!(status, 404);
    let (status, _) = get_status(
        app_with_test_auth(pool.clone()),
        &format!("/v1/artists/{slug}"),
    )
    .await;
    assert_eq!(status, 404);

    // The admin's editing session ended with the claim.
    let (status, _) =
        get_status_authed(app_with_test_auth(pool.clone()), "/v1/studio/me", ADMIN).await;
    assert_eq!(status, 404);

    // Once it's theirs, the owner can publish it.
    let (status, _) = send_authed(
        app_with_test_auth(pool.clone()),
        "PATCH",
        "/v1/studio/settings",
        BOB,
        Some(r#"{"status":"active"}"#),
    )
    .await;
    assert_eq!(status, 200);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn claim_requires_sign_in(pool: PgPool) {
    let (id, _) = create_draft(&pool, "Kingsgate Workshops").await;
    let token = mint_link(&pool, &id).await;
    let (status, _) = send_json(
        app_with_test_auth(pool),
        "POST",
        &format!("/v1/preview/{token}/claim"),
        None,
    )
    .await;
    assert_eq!(status, 401);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn claim_refuses_accounts_that_already_have_a_profile(pool: PgPool) {
    let (id, _) = create_draft(&pool, "Kingsgate Workshops").await;
    let token = mint_link(&pool, &id).await;
    let (status, _) = send_authed(
        app_with_test_auth(pool.clone()),
        "POST",
        &format!("/v1/preview/{token}/claim"),
        ALICE,
        None,
    )
    .await;
    assert_eq!(status, 409);
    // Link still live for the right person.
    let (status, _) = get_status(
        app_with_test_auth(pool.clone()),
        &format!("/v1/preview/{token}"),
    )
    .await;
    assert_eq!(status, 200);
}

// ─────────────────────────────────────────────────────────────────────────────
// Remove + takedown
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn remove_via_link_deletes_the_draft(pool: PgPool) {
    let (id, _) = create_draft(&pool, "Lewisham Arthouse").await;
    sqlx::query(
        "INSERT INTO artworks (artist_id, title, status) VALUES ($1::uuid, 'Kiln', 'draft')",
    )
    .bind(&id)
    .execute(&pool)
    .await
    .unwrap();
    let token = mint_link(&pool, &id).await;

    let (status, _) = send_json(
        app_with_test_auth(pool.clone()),
        "POST",
        &format!("/v1/preview/{token}/remove"),
        None,
    )
    .await;
    assert_eq!(status, 204);

    let (status, _) = get_status(
        app_with_test_auth(pool.clone()),
        &format!("/v1/preview/{token}"),
    )
    .await;
    assert_eq!(status, 404);
    let (artist_gone, artworks_live): (bool, i64) = sqlx::query_as(
        "SELECT (SELECT deleted_at IS NOT NULL FROM artists WHERE id = $1::uuid),
                (SELECT count(*) FROM artworks WHERE artist_id = $1::uuid AND deleted_at IS NULL)",
    )
    .bind(&id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(artist_gone);
    assert_eq!(artworks_live, 0);
    let via: String = sqlx::query_scalar(
        "SELECT context->>'via' FROM admin_audit_log WHERE action = 'draft.takedown' AND target_id = $1::uuid",
    )
    .bind(&id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(via, "preview_link");

    // Gone from the admin list too.
    let (_, list): (_, Value) =
        get_json_authed(app_with_test_auth(pool.clone()), "/v1/admin/drafts", ADMIN).await;
    assert!(list["items"].as_array().unwrap().is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admin_takedown_only_applies_to_unclaimed_drafts(pool: PgPool) {
    let (id, _) = create_draft(&pool, "Chisenhale Studios").await;
    let token = mint_link(&pool, &id).await;
    send_authed(
        app_with_test_auth(pool.clone()),
        "POST",
        &format!("/v1/preview/{token}/claim"),
        BOB,
        None,
    )
    .await;
    let (status, _) = send_authed(
        app_with_test_auth(pool.clone()),
        "POST",
        &format!("/v1/admin/drafts/{id}/takedown"),
        ADMIN,
        None,
    )
    .await;
    assert_eq!(status, 404, "claimed pages belong to the organisation");

    let (other, _) = create_draft(&pool, "Cob Gallery").await;
    let (status, _) = send_authed(
        app_with_test_auth(pool.clone()),
        "POST",
        &format!("/v1/admin/drafts/{other}/takedown"),
        ADMIN,
        None,
    )
    .await;
    assert_eq!(status, 204);
}

// ─────────────────────────────────────────────────────────────────────────────
// Follow-ups: admins can't claim, first publish takes artworks live,
// admin assign-by-email
// ─────────────────────────────────────────────────────────────────────────────

const TEST99_EMAIL: &str = "test@example.com"; // user_test_99, no profile
const TEST99: &str = "test-user_test_99";

async fn claim_as(pool: &PgPool, token: &str, bearer: &str) -> u16 {
    send_authed(
        app_with_test_auth(pool.clone()),
        "POST",
        &format!("/v1/preview/{token}/claim"),
        bearer,
        None,
    )
    .await
    .0
    .as_u16()
}

async fn insert_artwork(pool: &PgPool, draft_id: &str, title: &str) {
    sqlx::query("INSERT INTO artworks (artist_id, title, status) VALUES ($1::uuid, $2, 'draft')")
        .bind(draft_id)
        .bind(title)
        .execute(pool)
        .await
        .unwrap();
}

async fn artwork_status(pool: &PgPool, title: &str) -> (String, bool) {
    sqlx::query_as("SELECT status, published_at IS NOT NULL FROM artworks WHERE title = $1")
        .bind(title)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn set_status(pool: &PgPool, bearer: &str, status: &str) -> u16 {
    send_authed(
        app_with_test_auth(pool.clone()),
        "PATCH",
        "/v1/studio/settings",
        bearer,
        Some(&format!(r#"{{"status":"{status}"}}"#)),
    )
    .await
    .0
    .as_u16()
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn admins_cannot_claim_a_preview(pool: PgPool) {
    let (id, _) = create_draft(&pool, "Bearspace").await;
    let token = mint_link(&pool, &id).await;
    assert_eq!(claim_as(&pool, &token, ADMIN).await, 403);
    // Still unclaimed, link still live.
    let (status, _) = get_status(
        app_with_test_auth(pool.clone()),
        &format!("/v1/preview/{token}"),
    )
    .await;
    assert_eq!(status, 200);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn first_publish_of_a_claimed_page_publishes_its_draft_artworks(pool: PgPool) {
    let (id, slug) = create_draft(&pool, "Jealous Gallery").await;
    insert_artwork(&pool, &id, "Built by admin").await;
    let token = mint_link(&pool, &id).await;
    assert_eq!(claim_as(&pool, &token, BOB).await, 200);

    // Claiming alone publishes nothing.
    assert_eq!(artwork_status(&pool, "Built by admin").await.0, "draft");

    assert_eq!(set_status(&pool, BOB, "active").await, 200);
    assert_eq!(
        artwork_status(&pool, "Built by admin").await,
        ("published".to_string(), true)
    );
    let (status, bytes) = get_status(
        app_with_test_auth(pool.clone()),
        &format!("/v1/artists/{slug}"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        json(&bytes)["artworks"]["items"].as_array().unwrap().len(),
        1
    );

    // Later publishes respect the owner's own drafts.
    insert_artwork(&pool, &id, "Owner's own draft").await;
    assert_eq!(set_status(&pool, BOB, "paused").await, 200);
    assert_eq!(set_status(&pool, BOB, "active").await, 200);
    assert_eq!(artwork_status(&pool, "Owner's own draft").await.0, "draft");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn onboarding_complete_also_publishes_claimed_draft_artworks(pool: PgPool) {
    let (id, _) = create_draft(&pool, "Print Club").await;
    insert_artwork(&pool, &id, "Edition of 50").await;
    let token = mint_link(&pool, &id).await;
    assert_eq!(claim_as(&pool, &token, BOB).await, 200);
    let (status, _) = send_authed(
        app_with_test_auth(pool.clone()),
        "POST",
        "/v1/onboarding/complete",
        BOB,
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(artwork_status(&pool, "Edition of 50").await.0, "published");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn self_onboarded_artists_keep_their_drafts_on_publish(pool: PgPool) {
    // dora-pending is an ordinary (non-prebuilt) pending artist: the
    // T-086 artwork publish must not apply to them.
    sqlx::query(
        "UPDATE artists SET user_id = $1::uuid WHERE id = 'aaa44444-4444-4444-4444-444444444444'",
    )
    .bind(BOB_USER_ID)
    .execute(&pool)
    .await
    .unwrap();
    insert_artwork(&pool, "aaa44444-4444-4444-4444-444444444444", "Dora sketch").await;
    assert_eq!(set_status(&pool, BOB, "active").await, 200);
    assert_eq!(artwork_status(&pool, "Dora sketch").await.0, "draft");
}

async fn assign(pool: &PgPool, draft_id: &str, email: &str, bearer: &str) -> (u16, Value) {
    let (status, bytes) = send_authed(
        app_with_test_auth(pool.clone()),
        "POST",
        &format!("/v1/admin/drafts/{draft_id}/assign"),
        bearer,
        Some(&format!(r#"{{"email":"{email}"}}"#)),
    )
    .await;
    let v = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status.as_u16(), v)
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn assign_gives_an_unclaimed_draft_to_an_account(pool: PgPool) {
    let (id, _) = create_draft(&pool, "Kingsgate Workshops").await;
    let token = mint_link(&pool, &id).await;

    assert_eq!(assign(&pool, &id, "bob@example.com", ALICE).await.0, 403);
    assert_eq!(assign(&pool, &id, "nobody@example.com", ADMIN).await.0, 400);
    // Alice already has a page: one page per account.
    assert_eq!(assign(&pool, &id, "alice@example.com", ADMIN).await.0, 409);

    let (status, item) = assign(&pool, &id, "BOB@example.com", ADMIN).await;
    assert_eq!(status, 200, "{item}");
    assert_eq!(item["claimed"], true);
    assert_eq!(item["owner_email"], "bob@example.com");

    // The link is spent; Bob's studio is this page.
    let (status, _) = get_status(
        app_with_test_auth(pool.clone()),
        &format!("/v1/preview/{token}"),
    )
    .await;
    assert_eq!(status, 404);
    let (_, me): (_, Value) =
        get_json_authed(app_with_test_auth(pool.clone()), "/v1/studio/me", BOB).await;
    assert_eq!(me["id"], id.as_str());

    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM admin_audit_log WHERE action = 'draft.assign' AND target_id = $1::uuid",
    )
    .bind(&id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 1);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn assign_fixes_a_wrong_claim_until_the_page_is_published(pool: PgPool) {
    let (id, _) = create_draft(&pool, "Chisenhale Studios").await;
    let token = mint_link(&pool, &id).await;
    assert_eq!(claim_as(&pool, &token, BOB).await, 200);

    // Wrong person claimed: move it to the right account.
    let (status, item) = assign(&pool, &id, TEST99_EMAIL, ADMIN).await;
    assert_eq!(status, 200, "{item}");
    assert_eq!(item["owner_email"], TEST99_EMAIL);
    let bob_is_artist: bool = sqlx::query_scalar("SELECT is_artist FROM users WHERE id = $1::uuid")
        .bind(BOB_USER_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!bob_is_artist);
    let (status, _) =
        get_status_authed(app_with_test_auth(pool.clone()), "/v1/studio/me", BOB).await;
    assert_eq!(status, 404);

    // Once the owner publishes, it's theirs for good.
    assert_eq!(set_status(&pool, TEST99, "active").await, 200);
    assert_eq!(assign(&pool, &id, "bob@example.com", ADMIN).await.0, 409);
}
