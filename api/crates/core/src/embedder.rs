//! Multimodal embedder.
//!
//! Production path: HTTP call to Jina, behind a Postgres-backed cache.
//! Dev path (no `JINA_API_KEY`): always returns `None` from `embed_text`.
//! Callers must treat the optional return as "degrade to keyword search".

use crate::db::Pool;
use chrono::{DateTime, Utc};
use pgvector::Vector;
use serde::Serialize;
use std::sync::Arc;

#[derive(Clone)]
pub struct Embedder {
    inner: Arc<Inner>,
}

struct Inner {
    api_key: Option<String>,
    model_name: String,
    model_version: String,
    http: reqwest::Client,
    pool: Pool,
    /// Test-only escape hatch. When `Some`, `embed_text` returns this vector
    /// directly without hitting Jina. Production code never sets it; tests
    /// construct via `Embedder::with_fixed_vector(...)`.
    fixed_vector: Option<Vector>,
}

impl Embedder {
    pub fn new(
        pool: Pool,
        api_key: Option<String>,
        model_name: String,
        model_version: String,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("reqwest client");
        Self {
            inner: Arc::new(Inner {
                api_key,
                model_name,
                model_version,
                http,
                pool,
                fixed_vector: None,
            }),
        }
    }

    /// Construct an embedder that returns `vector` for every text query,
    /// without ever calling Jina or touching the cache. Intended for
    /// integration tests; production should never call this.
    pub fn with_fixed_vector(
        pool: Pool,
        model_name: String,
        model_version: String,
        vector: Vector,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                api_key: Some("test-stub".to_string()),
                model_name,
                model_version,
                http: reqwest::Client::new(),
                pool,
                fixed_vector: Some(vector),
            }),
        }
    }

    /// Construct a fully-disabled embedder. `embed_text` always returns None;
    /// search degrades to keyword-only. Used by tests that don't exercise
    /// the vector path.
    pub fn disabled(pool: Pool) -> Self {
        Self {
            inner: Arc::new(Inner {
                api_key: None,
                model_name: "jinaai/jina-clip-v2".to_string(),
                model_version: "local".to_string(),
                http: reqwest::Client::new(),
                pool,
                fixed_vector: None,
            }),
        }
    }

    pub fn enabled(&self) -> bool {
        self.inner.api_key.is_some()
    }

    /// Embed a text query, using the cache when possible.
    ///
    /// Returns `None` if no API key is configured (dev-friendly: callers
    /// degrade to keyword-only search).
    pub async fn embed_text(&self, query: &str) -> anyhow::Result<Option<Vector>> {
        if self.inner.api_key.is_none() {
            return Ok(None);
        }

        // Test escape hatch: short-circuit before any HTTP / DB activity.
        if let Some(v) = &self.inner.fixed_vector {
            return Ok(Some(v.clone()));
        }

        let normalized = normalize_query(query);

        // Cache hit?
        if let Some(vec) = lookup_cache(
            &self.inner.pool,
            &normalized,
            &self.inner.model_name,
            &self.inner.model_version,
        )
        .await?
        {
            return Ok(Some(vec));
        }

        // Cache miss → call Jina.
        let vec = self.call_jina_text(&normalized).await?;
        upsert_cache(
            &self.inner.pool,
            &normalized,
            &self.inner.model_name,
            &self.inner.model_version,
            &vec,
        )
        .await?;
        Ok(Some(vec))
    }

    async fn call_jina_text(&self, text: &str) -> anyhow::Result<Vector> {
        #[derive(Serialize)]
        struct Body<'a> {
            model: &'a str,
            input: Vec<JinaInput<'a>>,
            embedding_type: &'a str,
        }
        #[derive(Serialize)]
        struct JinaInput<'a> {
            text: &'a str,
        }

        let key = self
            .inner
            .api_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("jina api key not configured"))?;

        // Jina's HTTP API takes the bare model name ("jina-clip-v2"), while we
        // store the full Hugging Face id ("jinaai/jina-clip-v2") in our DB
        // because that's what the Python tooling reports. Translate at the
        // boundary so the rest of the system stays consistent.
        let api_model = self
            .inner
            .model_name
            .rsplit('/')
            .next()
            .unwrap_or(&self.inner.model_name);

        let body = Body {
            model: api_model,
            input: vec![JinaInput { text }],
            embedding_type: "float",
        };

        let resp = self
            .inner
            .http
            .post("https://api.jina.ai/v1/embeddings")
            .bearer_auth(key)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        #[derive(serde::Deserialize)]
        struct JinaResp {
            data: Vec<JinaData>,
        }
        #[derive(serde::Deserialize)]
        struct JinaData {
            embedding: Vec<f32>,
        }
        let parsed: JinaResp = resp.json().await?;
        let first = parsed
            .data
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty embedding response"))?;

        Ok(Vector::from(first.embedding))
    }
}

fn normalize_query(q: &str) -> String {
    q.trim().to_lowercase()
}

async fn lookup_cache(
    pool: &Pool,
    query_text: &str,
    model_name: &str,
    model_version: &str,
) -> Result<Option<Vector>, sqlx::Error> {
    let row: Option<(Vector,)> = sqlx::query_as(
        r#"
        UPDATE query_embedding_cache
           SET last_used_at = now(),
               hit_count = hit_count + 1
         WHERE query_text = $1
           AND model_name = $2
           AND model_version = $3
        RETURNING embedding
        "#,
    )
    .bind(query_text)
    .bind(model_name)
    .bind(model_version)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.0))
}

async fn upsert_cache(
    pool: &Pool,
    query_text: &str,
    model_name: &str,
    model_version: &str,
    embedding: &Vector,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO query_embedding_cache
            (query_text, model_name, model_version, embedding)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (query_text, model_name, model_version) DO UPDATE
           SET last_used_at = now(),
               hit_count = query_embedding_cache.hit_count + 1
        "#,
    )
    .bind(query_text)
    .bind(model_name)
    .bind(model_version)
    .bind(embedding)
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(dead_code)]
fn _ts_compile_check() -> Option<DateTime<Utc>> {
    None
}
