//! Object storage — S3 in prod, MinIO in dev, in-memory in tests.
//!
//! Wraps the AWS SDK with a small surface (`put`, `set_retention`,
//! `public_url`) and a test-mode constructor that stores bytes in memory. Same shape
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
//! ## Retention tags (T-089)
//!
//! The uploads bucket holds two kinds of object under the same
//! `uploads/<uuid>.<ext>` keys: throwaway visual-search uploads and
//! permanent artist artwork images (T-012 Phase 1). The bucket's
//! lifecycle rule expires objects after 90 days **only when they carry
//! the tag `state=pending`** (see `infra/modules/storage/main.tf`).
//!
//! - [`ObjectStore::put`] writes every new object as `state=pending`.
//! - [`ObjectStore::set_retention`] flips it to `state=attached` once
//!   a durable row (`artwork_images`) references the key, and back to
//!   `pending` when the last reference is removed.
//!
//! Untagged objects never expire, so a missed tag fails safe (we keep
//! bytes) rather than losing an artist's image. See `decisions.md`
//! 2026-11-17.
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
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{Tag, Tagging};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Object tag key the bucket lifecycle rule filters on.
pub const RETENTION_TAG_KEY: &str = "state";

/// Lifecycle state of an object in the uploads bucket, stored as the
/// `state` object tag. Only [`Retention::Pending`] objects are expired
/// by the bucket lifecycle rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    /// Freshly uploaded or no longer referenced. Expires after 90 days.
    Pending,
    /// Referenced by an `artwork_images` row. Kept forever.
    Attached,
}

impl Retention {
    /// Tag value as written to S3 (`state=<value>`).
    pub fn as_tag_value(self) -> &'static str {
        match self {
            Retention::Pending => "pending",
            Retention::Attached => "attached",
        }
    }

    /// URL-query form accepted by PutObject's `x-amz-tagging` header.
    fn as_tagging_header(self) -> String {
        format!("{RETENTION_TAG_KEY}={}", self.as_tag_value())
    }
}

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
struct MemoryObject {
    bytes: Vec<u8>,
    content_type: String,
    retention: Retention,
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
                loader = loader.credentials_provider(Credentials::new(
                    ak,
                    sk,
                    None,
                    None,
                    "ml-art-static",
                ));
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

    /// PUT a new object, tagged `state=pending` so the bucket lifecycle
    /// rule expires it unless something later calls
    /// [`ObjectStore::set_retention`] with [`Retention::Attached`].
    /// Returns the `s3_key` (the caller usually already knows it; the
    /// return value is here for ergonomic chaining).
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
                    .tagging(Retention::Pending.as_tagging_header())
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
                        retention: Retention::Pending,
                    },
                );
                Ok(key.to_string())
            }
        }
    }

    /// Replace the object's `state` tag. Returns `Ok(false)` when the
    /// key doesn't exist (S3 `NoSuchKey`), so callers can turn a bad
    /// client-supplied key into a 400 instead of a 500.
    ///
    /// PutObjectTagging replaces the whole tag set; `state` is the
    /// only tag we use, so that's fine.
    pub async fn set_retention(&self, key: &str, retention: Retention) -> anyhow::Result<bool> {
        match &*self.inner {
            Inner::Real { client, bucket, .. } => {
                let tagging = Tagging::builder()
                    .tag_set(
                        Tag::builder()
                            .key(RETENTION_TAG_KEY)
                            .value(retention.as_tag_value())
                            .build()?,
                    )
                    .build()?;
                let res = client
                    .put_object_tagging()
                    .bucket(bucket)
                    .key(key)
                    .tagging(tagging)
                    .send()
                    .await;
                match res {
                    Ok(_) => Ok(true),
                    Err(e) if e.code() == Some("NoSuchKey") => Ok(false),
                    Err(e) => Err(anyhow::anyhow!(
                        "s3 put-object-tagging bucket={bucket} key={key}: {e:?}"
                    )),
                }
            }
            Inner::Memory { store, .. } => {
                let mut g = store.lock().expect("object_store mutex poisoned");
                match g.get_mut(key) {
                    Some(obj) => {
                        obj.retention = retention;
                        Ok(true)
                    }
                    None => Ok(false),
                }
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

    /// Test-only: the object's current retention tag. `None` if the
    /// store isn't the in-memory variant or the key doesn't exist.
    #[doc(hidden)]
    pub fn test_retention(&self, key: &str) -> Option<Retention> {
        match &*self.inner {
            Inner::Memory { store, .. } => store.lock().ok()?.get(key).map(|o| o.retention),
            Inner::Real { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_tag_values_match_lifecycle_filter() {
        // `infra/modules/storage/main.tf` filters on `state=pending`.
        // Renaming these strands every object in the bucket.
        assert_eq!(RETENTION_TAG_KEY, "state");
        assert_eq!(Retention::Pending.as_tag_value(), "pending");
        assert_eq!(Retention::Attached.as_tag_value(), "attached");
        assert_eq!(Retention::Pending.as_tagging_header(), "state=pending");
    }

    #[tokio::test]
    async fn put_tags_pending_and_set_retention_flips_it() {
        let store = ObjectStore::for_tests("uploads");
        store
            .put("uploads/a.jpg", vec![1, 2, 3], "image/jpeg")
            .await
            .unwrap();
        assert_eq!(
            store.test_retention("uploads/a.jpg"),
            Some(Retention::Pending)
        );

        assert!(store
            .set_retention("uploads/a.jpg", Retention::Attached)
            .await
            .unwrap());
        assert_eq!(
            store.test_retention("uploads/a.jpg"),
            Some(Retention::Attached)
        );

        assert!(store
            .set_retention("uploads/a.jpg", Retention::Pending)
            .await
            .unwrap());
        assert_eq!(
            store.test_retention("uploads/a.jpg"),
            Some(Retention::Pending)
        );
    }

    #[tokio::test]
    async fn set_retention_on_missing_key_reports_not_found() {
        let store = ObjectStore::for_tests("uploads");
        let found = store
            .set_retention("uploads/missing.jpg", Retention::Attached)
            .await
            .unwrap();
        assert!(!found);
        assert_eq!(store.test_retention("uploads/missing.jpg"), None);
    }
}
