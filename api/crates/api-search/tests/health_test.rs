mod common;

use common::{app_keyword_only, get_json, MIGRATOR};
use serde_json::Value;
use sqlx::PgPool;

#[sqlx::test(migrator = "MIGRATOR")]
async fn health_returns_ok_and_db_true(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, body): (_, Value) = get_json(app, "/v1/health").await;
    assert_eq!(status, 200);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["db"], true);
    // Disabled embedder by default in tests.
    assert_eq!(body["embedder_enabled"], false);
}
