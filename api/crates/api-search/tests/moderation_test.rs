// T-008 integration tests — exercise:
//
// 1. Adding an image enqueues a moderation job in the `jobs` table.
// 2. A freshly-added image lands as `moderation_status='pending'` and is
//    hidden from public surfaces until approved.
// 3. The moderation handler flips `pending → approved` (Disabled client)
//    or `pending → rejected` (Test client with a canned bad result), and
//    public surfaces update accordingly.
// 4. Idempotency: enqueueing the same image twice produces one job.

// Deserialize-only contract structs trigger dead_code under `-D warnings`.
#![allow(dead_code)]

mod common;

use common::{
    app_with_auth_fixed_vector_postgres_jobs, app_with_keyword_only_postgres_jobs, get_json,
    send_authed, MIGRATOR,
};
use ml_art_core::{
    db::Pool,
    jobs::{self, JobEvent},
    moderation::{moderate_artwork_image, ModerationClient, ModerationResult},
};
use pgvector::Vector;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

const ALICE: &str = "test-user_test_alice";
const ARTWORK_BLUE_MORNING: &str = "bbb11111-1111-1111-1111-111111111111";

fn unit_vector_at(pos: usize) -> Vector {
    let mut v = vec![0.0_f32; 1024];
    v[pos] = 1.0;
    Vector::from(v)
}

#[derive(Deserialize, Debug)]
struct ArtworkSummary {
    id: String,
}

#[derive(Deserialize, Debug)]
struct Image {
    id: String,
    s3_key: String,
    moderation_status: String,
}

#[derive(Deserialize, Debug)]
struct ArtworkFull {
    id: String,
    images: Vec<PublicImage>,
}

#[derive(Deserialize, Debug)]
struct PublicImage {
    id: String,
    url: String,
}

#[derive(Deserialize, Debug)]
struct Page<T> {
    items: Vec<T>,
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Add-image enqueues a moderation job
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn add_image_enqueues_artwork_image_moderate(pool: PgPool) {
    let app = app_with_auth_fixed_vector_postgres_jobs(pool.clone(), unit_vector_at(99));

    // Create a fresh artwork so we don't trip the "primary already set"
    // path on Blue Morning.
    let created: ArtworkSummary = {
        let (_, bytes) = send_authed(
            app.clone(),
            "POST",
            "/v1/studio/artworks",
            ALICE,
            Some(&json!({"title": "Moderate Me"}).to_string()),
        )
        .await;
        serde_json::from_slice(&bytes).unwrap()
    };

    let add_body = json!({"s3_key": "uploads/moderate-me.jpg"}).to_string();
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
    assert_eq!(
        img.moderation_status, "pending",
        "fresh row defaults to pending; worker flips it"
    );

    // One job in the queue, kind matches, payload references this row.
    let (kind, payload): (String, serde_json::Value) =
        sqlx::query_as("SELECT kind, payload FROM jobs WHERE idempotency_key = $1")
            .bind(format!("moderate:artwork_image:{}", img.id))
            .fetch_one(&pool)
            .await
            .expect("moderation job present");
    assert_eq!(kind, "artwork_image_moderate");
    assert_eq!(
        payload["artwork_image_id"].as_str(),
        Some(img.id.as_str()),
        "payload references the image just inserted"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Pending images are hidden from public surfaces
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn pending_images_hidden_from_artwork_detail(pool: PgPool) {
    // Insert a second (non-primary) pending image directly so we can
    // observe the public filter without standing up the full add-image
    // flow.
    let new_image_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO artwork_images
               (id, artwork_id, s3_key, is_primary, display_order, moderation_status)
           VALUES ($1, $2, 'test/alice/pending.jpg', false, 1, 'pending')"#,
    )
    .bind(new_image_id)
    .bind(Uuid::parse_str(ARTWORK_BLUE_MORNING).unwrap())
    .execute(&pool)
    .await
    .unwrap();

    let app = app_with_keyword_only_postgres_jobs(pool.clone());
    let (status, full): (_, ArtworkFull) =
        get_json(app, &format!("/v1/artworks/{ARTWORK_BLUE_MORNING}")).await;
    assert_eq!(status, 200);

    // Seed image (approved) shows; new pending image does NOT.
    assert_eq!(full.images.len(), 1, "pending image filtered out");
    assert!(
        !full.images.iter().any(|i| i.id == new_image_id.to_string()),
        "pending image is hidden"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Handler approves (Disabled) or rejects (canned) the row
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn moderation_handler_approves_via_disabled_client(pool: PgPool) {
    let id = insert_pending_image(&pool, "test/alice/needs-mod.jpg").await;
    let client = ModerationClient::disabled();
    moderate_artwork_image(&client, &pool, id).await.unwrap();

    let (status,): (String,) =
        sqlx::query_as("SELECT moderation_status FROM artwork_images WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "approved");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn moderation_handler_rejects_via_canned_client(pool: PgPool) {
    let id = insert_pending_image(&pool, "test/alice/bad.jpg").await;
    let client = ModerationClient::for_tests(vec![(
        "test/alice/bad.jpg".to_string(),
        ModerationResult::rejected(vec![
            "Explicit Nudity".to_string(),
            "Suggestive".to_string(),
        ]),
    )]);
    moderate_artwork_image(&client, &pool, id).await.unwrap();

    let (status, reason): (String, Option<String>) = sqlx::query_as(
        "SELECT moderation_status, moderation_reason FROM artwork_images WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "rejected");
    // Labels are persisted comma-joined so the studio can render the
    // "why was this rejected" line without an extra join. T-008c.
    assert_eq!(reason.as_deref(), Some("Explicit Nudity, Suggestive"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn moderation_handler_clears_reason_on_re_approve(pool: PgPool) {
    // Belt-and-braces: a row previously stamped as rejected getting
    // re-run through Disabled (or a more-permissive Real client)
    // should clear the reason so the studio doesn't keep showing a
    // stale "why" against an approved image.
    let id = insert_pending_image(&pool, "test/alice/maybe.jpg").await;
    sqlx::query(
        "UPDATE artwork_images SET moderation_status='rejected', moderation_reason='Old Label' WHERE id = $1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();

    let client = ModerationClient::disabled();
    moderate_artwork_image(&client, &pool, id).await.unwrap();

    let (status, reason): (String, Option<String>) = sqlx::query_as(
        "SELECT moderation_status, moderation_reason FROM artwork_images WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "approved");
    assert!(reason.is_none());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn moderation_handler_is_noop_for_missing_row(pool: PgPool) {
    // Image id that doesn't exist — handler must not error so the
    // worker doesn't loop on a permanently-deleted row.
    let id = Uuid::new_v4();
    let client = ModerationClient::disabled();
    moderate_artwork_image(&client, &pool, id).await.unwrap();
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. End-to-end: enqueue → handle → row flips approved → public surface
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn add_image_flips_approved_after_worker_runs(pool: PgPool) {
    let app = app_with_auth_fixed_vector_postgres_jobs(pool.clone(), unit_vector_at(101));

    // Create fresh artwork + add image (lands as pending).
    let created: ArtworkSummary = {
        let (_, bytes) = send_authed(
            app.clone(),
            "POST",
            "/v1/studio/artworks",
            ALICE,
            Some(&json!({"title": "E2E Mod", "status": "published"}).to_string()),
        )
        .await;
        serde_json::from_slice(&bytes).unwrap()
    };
    let (_, bytes) = send_authed(
        app,
        "POST",
        &format!("/v1/studio/artworks/{}/images", created.id),
        ALICE,
        Some(&json!({"s3_key": "uploads/e2e-mod.jpg"}).to_string()),
    )
    .await;
    let img: Image = serde_json::from_slice(&bytes).unwrap();

    // Drive the worker manually: claim the job + dispatch the handler.
    let claimed = jobs::postgres::claim_one(&pool)
        .await
        .unwrap()
        .expect("a job was pending");
    let event = jobs::postgres::decode(&claimed).unwrap();
    assert!(matches!(event, JobEvent::ArtworkImageModerate { .. }));

    let deps = jobs::JobsDeps {
        pool: pool.clone(),
        geocoder: ml_art_core::geocoding::GeocodingClient::disabled(),
        emails: ml_art_core::emails::EmailClient::disabled("noreply@test".to_string()),
        moderation: ModerationClient::disabled(),
        web_base_url: "http://localhost:3000".to_string(),
        anon_cookie_secret: "test-cookie-secret".to_string(),
        jobs: jobs::JobsBackend::for_tests(),
    };
    jobs::handle(event, &deps).await.unwrap();
    jobs::postgres::mark_done(&pool, claimed.id).await.unwrap();

    // Row is approved.
    let (status,): (String,) =
        sqlx::query_as("SELECT moderation_status FROM artwork_images WHERE id = $1")
            .bind(Uuid::parse_str(&img.id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "approved");
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Idempotency: re-enqueueing dedups by `idempotency_key`
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn double_enqueue_with_same_key_dedups(pool: PgPool) {
    let backend = ml_art_core::jobs::JobsBackend::postgres(pool.clone());
    let image_id = Uuid::new_v4();
    let key = format!("moderate:artwork_image:{image_id}");
    let opts = ml_art_core::jobs::EnqueueOpts {
        idempotency_key: Some(key.clone()),
        ..Default::default()
    };
    backend
        .enqueue(
            JobEvent::ArtworkImageModerate {
                artwork_image_id: image_id,
            },
            opts.clone(),
        )
        .await
        .unwrap();
    backend
        .enqueue(
            JobEvent::ArtworkImageModerate {
                artwork_image_id: image_id,
            },
            opts,
        )
        .await
        .unwrap();

    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM jobs WHERE idempotency_key = $1")
        .bind(&key)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "second enqueue with same key is a no-op");
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

async fn insert_pending_image(pool: &Pool, s3_key: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO artwork_images
               (id, artwork_id, s3_key, is_primary, display_order, moderation_status)
           VALUES ($1, $2, $3, false, 9, 'pending')"#,
    )
    .bind(id)
    .bind(Uuid::parse_str(ARTWORK_BLUE_MORNING).unwrap())
    .bind(s3_key)
    .execute(pool)
    .await
    .unwrap();
    id
}
