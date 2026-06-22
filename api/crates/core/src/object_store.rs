//! Object storage — S3 in prod, MinIO in dev, in-memory in tests.
//!
//! Wraps the AWS SDK with a small surface (`put`, `presigned_get_url`)
//! and a test-mode constructor that stores bytes in memory. Same shape
//! as `Embedder::with_fixed_vector` — explicit test variant rather
//! than an env flag, so production binaries can't accidentally use the
//! stub.
//!
//! ## Configuration
//!
//! Reads from the SDK's default chain (env vars, instance metadata,
//! shared config). For dev/MinIO we override `S3_ENDPOINT` to
//! `http://localhost:9000` and pass static creds via the standard AWS
//! env vars (`AWS_ACCESS_KEY_ID=dev`, `AWS_SECRET_ACCESS_KEY=devpassword`,
//! `AWS_REGION=us-east-1` — region is mandatory for the SDK even when
//! pointing at MinIO).
//!
//! ## Why a wrapper instead of using `aws-sdk-s3::Client` directly
//!
//! Two reasons:
//!   1. The handler shouldn't know whether it's talking to MinIO or
//!      real S3; that's the configuration layer's problem
//!   2. Tests need a working stub. Mocking `aws-sdk-s3::Client` is
//!      possible but verbose; an enum (`Real { client }` / `Memory { … }`)
//!      keeps the test path tight

use aws_credential_types::Credentials;
use aws_sdk_s3::config::{BehaviorVersion, Region};
use aws_sdk_s3::primitives::ByteStream;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct ObjectStore {
    inner: Arc<Inner>,
}

enum Inner {
    /// Production path. Talks to whatever S3-compatible endpoint the
    /// SDK was configured against (real S3 in prod, MinIO in dev).
    Real {
        client: aws_sdk_s3::Client,
        bucket: String,
        /// Public URL prefix for objects in this bucket. In dev this
        /// is `http://localhost:9000/<bucket>`; in prod it's the
        /// CloudFront distribution that fronts the bucket.
        public_url_prefix: String,
    },
    /// Test path. Stores bytes in memory keyed by `s3_key`. Public
    /// URLs are synthesized as `https://test.example.com/<bucket>/<key>`
    /// so callers can still build URLs that look plausible.
    Memory {
        bucket: String,
        store: Arc<Mutex<HashMap<String, MemoryObject>>>,
    },
}

#[derive(Clone)]
#[allow(dead_code)] // Reserved for richer test assertions later (size, type, ...).
struct MemoryObject {
    bytes: Vec<u8>,
    content_type: String,
}

impl ObjectStore {
    /// Build a production-style `ObjectStore`. `endpoint_url` is the
    /// override knob — `Some("http://localhost:9000")` in dev, `None`
    /// in prod (defaults to real S3).
    pub async fn new(
        bucket: String,
        public_url_prefix: String,
        endpoint_url: Option<String>,
        region: String,
        access_key: Option<String>,
        secret_key: Option<String>,
    ) -> Self {
        let mut loader =
            aws_config::defaults(BehaviorVersion::latest()).region(Region::new(region));

        if let Some(url) = endpoint_url.as_deref() {
            loader = loader.endpoint_url(url);
        }
        // Static creds are ONLY for MinIO (dev). In Lambda the runtime
        // already injects AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY +
        // AWS_SESSION_TOKEN from the role's STS session — but the
        // single-line ctor here can't see the session token, so passing
        // ak/sk without it produces a broken static-cred override that
        // S3 rejects with AccessDenied (surfaces as "service error").
        // Gate on `endpoint_url` so prod always uses the default chain.
        if endpoint_url.is_some() {
            if let (Some(ak), Some(sk)) = (access_key.as_deref(), secret_key.as_deref()) {
                loader = loader
                    .credentials_provider(Credentials::new(ak, sk, None, None, "ml-art-static"));
            }
        }

        let cfg = loader.load().await;

        // MinIO requires path-style addressing (no virtual-hosted style).
        // Force it whenever we're using a custom endpoint.
        let s3_cfg = aws_sdk_s3::config::Builder::from(&cfg)
            .force_path_style(endpoint_url.is_some())
            .build();

        Self {
            inner: Arc::new(Inner::Real {
                client: aws_sdk_s3::Client::from_conf(s3_cfg),
                bucket,
                public_url_prefix,
            }),
        }
    }

    /// In-memory store for tests. No HTTP, no real S3. Each instance is
    /// independent — tests don't share state unless they pass the same
    /// `ObjectStore`.
    pub fn for_tests(bucket: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Inner::Memory {
                bucket: bucket.into(),
                store: Arc::new(Mutex::new(HashMap::new())),
            }),
        }
    }

    /// PUT a new object. Returns the `s3_key` (the caller usually
    /// already knows it; the return value is here for ergonomic
    /// chaining).
    pub async fn put(
        &self,
        key: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> anyhow::Result<String> {
        match &*self.inner {
            Inner::Real { client, bucket, .. } => {
                client
                    .put_object()
                    .bucket(bucket)
                    .key(key)
                    .content_type(content_type)
                    .body(ByteStream::from(bytes))
                    .send()
                    .await
                    // `Display` on SdkError::ServiceError is just
                    // "service error"; the underlying code (AccessDenied,
                    // NoSuchBucket, etc.) lives in the source chain.
                    // Use `{:?}` so the actual reason reaches CloudWatch.
                    .map_err(|e| anyhow::anyhow!("s3 put bucket={bucket} key={key}: {e:?}"))?;
                Ok(key.to_string())
            }
            Inner::Memory { store, .. } => {
                let mut g = store.lock().expect("object_store mutex poisoned");
                g.insert(
                    key.to_string(),
                    MemoryObject {
                        bytes,
                        content_type: content_type.to_string(),
                    },
                );
                Ok(key.to_string())
            }
        }
    }

    /// Build the public URL for an object. Mirrors what
    /// `core::images::url_for_s3_key` does for the `artworks` bucket
    /// but is bound to the bucket this `ObjectStore` was built with.
    pub fn public_url(&self, key: &str) -> String {
        match &*self.inner {
            Inner::Real {
                public_url_prefix, ..
            } => format!("{public_url_prefix}/{key}"),
            Inner::Memory { bucket, .. } => {
                format!("https://test.example.com/{bucket}/{key}")
            }
        }
    }

    /// Whether the store actually persists. Tests can use this to skip
    /// assertions that depend on a real backend (we don't have any
    /// today, but the hook is here for future moderation tests).
    pub fn is_real(&self) -> bool {
        matches!(*self.inner, Inner::Real { .. })
    }

    /// Test-only: peek at what got stored. Production paths never
    /// call this. Returns `None` if the store isn't the in-memory
    /// variant or the key doesn't exist.
    #[doc(hidden)]
    pub fn test_get(&self, key: &str) -> Option<(Vec<u8>, String)> {
        match &*self.inner {
            Inner::Memory { store, .. } => {
                let g = store.lock().ok()?;
                g.get(key)
                    .map(|o| (o.bytes.clone(), o.content_type.clone()))
            }
            Inner::Real { .. } => None,
        }
    }
}
