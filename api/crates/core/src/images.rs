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

    #[test]
    fn url_for_s3_key_default_base() {
        // No env override → falls back to MinIO default.
        std::env::remove_var("IMAGE_BASE_URL");
        let url = url_for_s3_key("demo/alice/1.jpg");
        assert_eq!(url, "http://localhost:9000/artworks/demo/alice/1.jpg");
    }

    #[test]
    fn url_for_s3_key_respects_env_override() {
        // Restore-on-drop in case other tests run in the same process.
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                std::env::remove_var("IMAGE_BASE_URL");
            }
        }
        let _guard = Guard;
        std::env::set_var("IMAGE_BASE_URL", "https://cdn.example.com/v1");
        let url = url_for_s3_key("demo/alice/1.jpg");
        assert_eq!(url, "https://cdn.example.com/v1/demo/alice/1.jpg");
    }
}
