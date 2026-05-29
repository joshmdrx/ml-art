//! Multimodal embedder.
//!
//! Production path: HTTP call to Jina, behind a Postgres-backed cache for
//! text queries. Image embeddings (used by `T-036`'s artwork-embedding
//! pipeline) bypass the cache — every uploaded image is unique enough
//! that a (URL → vector) cache wouldn't pay back its complexity.
//!
//! Dev path (no `JINA_API_KEY`): `embed_text` returns `None`, callers
//! degrade to keyword-only search. `embed_image_from_url` errors out —
//! image embedding is required, not best-effort, for the
//! artwork-creation flow (without it, the new artwork would never be
//! findable). Studio create handlers should gate on `embedder.enabled()`
//! before calling.

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
                model_version: "v2".to_string(),
                http: reqwest::Client::new(),
                pool,
                fixed_vector: None,
            }),
        }
    }

    pub fn enabled(&self) -> bool {
        self.inner.api_key.is_some()
    }

    /// The model label that gets persisted alongside every embedding
    /// row. Exposed so `artwork_embeddings::write` and `query_cache`
    /// stay in sync with this embedder's config without each caller
    /// holding its own copy of the strings.
    pub fn model_name(&self) -> &str {
        &self.inner.model_name
    }

    pub fn model_version(&self) -> &str {
        &self.inner.model_version
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

    /// Embed an image by URL. Used by the artwork-creation pipeline:
    /// after a fresh `artworks` row gets its primary image, we call
    /// Jina, normalize the response into a `Vector`, and write a row to
    /// `artwork_embeddings`. Bypasses the cache (see module docs).
    ///
    /// Errors if `JINA_API_KEY` is unset — image embedding is required,
    /// not best-effort. Callers should `embedder.enabled()` first.
    pub async fn embed_image_from_url(&self, url: &str) -> anyhow::Result<Vector> {
        // Test escape hatch — same shape as embed_text. Lets integration
        // tests exercise `process_image` end-to-end without hitting Jina.
        if let Some(v) = &self.inner.fixed_vector {
            return Ok(v.clone());
        }

        if self.inner.api_key.is_none() {
            anyhow::bail!("jina api key not configured; cannot embed image");
        }

        self.call_jina_image(url).await
    }

    /// Embed an image by raw bytes. We base64-encode and send as a
    /// `data:` URL in Jina's `image` field; the API supports either
    /// form. This is the right path when we already have the bytes
    /// (fresh upload) — and crucially, it works in dev where Jina's
    /// cloud workers can't reach our `localhost:9000` MinIO.
    ///
    /// In prod this is also cheaper than the URL path: no Jina → S3
    /// fetch round-trip, no race between upload completion and
    /// embed-time fetch. Worth using as the default path for
    /// just-uploaded bytes.
    pub async fn embed_image_from_bytes(
        &self,
        mime_type: &str,
        bytes: &[u8],
    ) -> anyhow::Result<Vector> {
        if let Some(v) = &self.inner.fixed_vector {
            return Ok(v.clone());
        }

        if self.inner.api_key.is_none() {
            anyhow::bail!("jina api key not configured; cannot embed image");
        }

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        // `data:<mime>;base64,<payload>` — RFC 2397.
        let data_url = format!("data:{mime_type};base64,{b64}");
        self.call_jina_image(&data_url).await
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

    /// Mirror of `call_jina_text` for the image branch. Jina accepts an
    /// `image` field that's either a public URL or a base64 data-URL. We
    /// send the URL directly because in our stack the image is already
    /// stored at a CDN-accessible address by the time we embed it (S3 →
    /// CloudFront in prod, MinIO `localhost:9000` in dev — fetchable from
    /// the Jina worker in both cases).
    async fn call_jina_image(&self, url: &str) -> anyhow::Result<Vector> {
        #[derive(Serialize)]
        struct Body<'a> {
            model: &'a str,
            input: Vec<JinaInput<'a>>,
            embedding_type: &'a str,
        }
        #[derive(Serialize)]
        struct JinaInput<'a> {
            image: &'a str,
        }

        let key = self
            .inner
            .api_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("jina api key not configured"))?;

        let api_model = self
            .inner
            .model_name
            .rsplit('/')
            .next()
            .unwrap_or(&self.inner.model_name);

        let body = Body {
            model: api_model,
            input: vec![JinaInput { image: url }],
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
