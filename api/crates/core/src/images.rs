//! Image URL helpers.
//!
//! Today: `s3_key` → `${IMAGE_BASE_URL}/{s3_key}`. In dev that points at
//! MinIO; in staging/prod it points at the CloudFront distribution that
//! fronts the production S3 bucket. The function reads the env var on
//! every call rather than caching, so test code can override per-test
//! via `std::env::set_var` if needed.
//!
//! Extracted because the same six-line snippet was duplicated across
//! `artwork.rs`, `neighborhoods.rs`, and `me/collections.rs`. See
//! `decisions.md` 2026-05-27 — code review pass + CONTRIBUTING.md.

const DEFAULT_IMAGE_BASE_URL: &str = "http://localhost:9000/artworks";

/// Build a public URL for an `artwork_images.s3_key`. Defaults to local
/// MinIO when `IMAGE_BASE_URL` isn't set (i.e., in dev + tests).
pub fn url_for_s3_key(s3_key: &str) -> String {
    let base =
        std::env::var("IMAGE_BASE_URL").unwrap_or_else(|_| DEFAULT_IMAGE_BASE_URL.to_string());
    format!("{base}/{s3_key}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both env-mutating cases live in one test because Rust runs `#[test]`s
    /// in parallel by default; splitting them caused a flaky race where the
    /// `default_base` case saw a `set_var` from the `respects_env_override`
    /// case mid-flight. One test, sequential assertions, no race.
    #[test]
    fn url_for_s3_key_default_and_override() {
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                std::env::remove_var("IMAGE_BASE_URL");
            }
        }
        let _guard = Guard;

        // 1. No env → falls back to MinIO default.
        std::env::remove_var("IMAGE_BASE_URL");
        assert_eq!(
            url_for_s3_key("demo/alice/1.jpg"),
            "http://localhost:9000/artworks/demo/alice/1.jpg"
        );

        // 2. Env set → that wins.
        std::env::set_var("IMAGE_BASE_URL", "https://cdn.example.com/v1");
        assert_eq!(
            url_for_s3_key("demo/alice/1.jpg"),
            "https://cdn.example.com/v1/demo/alice/1.jpg"
        );
    }
}
