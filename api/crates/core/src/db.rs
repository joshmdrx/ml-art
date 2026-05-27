//! Postgres connection pool.

use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

pub type Pool = PgPool;

pub async fn make_pool(database_url: &str) -> Result<Pool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(8)
        .min_connections(0)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Some(Duration::from_secs(60)))
        // Lambdas can sit warm with idle connections; Neon free tier scales to
        // zero, so we want to release connections after a minute of idle.
        .connect(database_url)
        .await
}
