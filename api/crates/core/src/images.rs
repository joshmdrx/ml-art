//! Image URL helpers.
//!
//! Two-bucket setup in dev + prod:
//!
//! - **`artworks/`** — original WikiArt seed dump + (eventually) the
//!   "official" home for artwork imagery. Fronted by `IMAGE_BASE_URL`
//!   which in dev is `http://localhost:9000/artworks` (MinIO host +
//!   bucket name) and in prod is the CloudFront distribution that
//!   fronts the artworks bucket. Keys inside this bucket don't have
//!   the bucket name prefix (e.g. `wikiart/<artist>/<file>.jpg`).
//! - **`uploads/`** — visual-search uploads from `/v1/uploads/image`
//!   AND, since T-012 Phase 1, new artwork-image uploads from artists.
//!   Fronted by `UPLOADS_PUBLIC_URL_PREFIX` (`localhost:9000/uploads`
//!   in dev — MinIO host + bucket name). Keys in this bucket DO carry
//!   an `uploads/` prefix (legacy artifact of T-010 Phase A); the
//!   prefix is what tells us "render via uploads bucket, not artworks."
//!
//! That means the public URL for an `uploads/`-prefixed key looks
//! like `http://localhost:9000/uploads/uploads/abc.png` — host +
//! bucket + key, where the second `/uploads/` is the key's prefix.
//! Cosmetic wart; the file really is at that path in MinIO's
//! `<endpoint>/<bucket>/<key>` URL scheme. If we ever clean up the
//! upload handler to mint bare-UUID keys, this collapses to a single
//! `/uploads/` and we drop the doubled segment.
//!
//! Extracted because the same six-line snippet was duplicated across
//! `artwork.rs`, `neighborhoods.rs`, and `me/collections.rs`. See
//! `decisions.md` 2026-05-27 — code review pass + CONTRIBUTING.md.

const DEFAULT_IMAGE_BASE_URL: &str = "http://localhost:9000/artworks";
const DEFAULT_UPLOADS_URL_PREFIX: &str = "http://localhost:9000/uploads";

/// Build a public URL for an `artwork_images.s3_key`. Routes
/// `uploads/`-prefixed keys to the uploads bucket; everything else to
/// the artworks bucket. The prefix on the key is preserved (NOT
/// stripped) so the final URL maps to MinIO's `<endpoint>/<bucket>/<key>`
/// — see the module docs for why we accept the doubled `/uploads/`.
/// Reads env vars on every call so test code can override per-test.
pub fn url_for_s3_key(s3_key: &str) -> String {
    if s3_key.starts_with("uploads/") {
        let base = std::env::var("UPLOADS_PUBLIC_URL_PREFIX")
            .unwrap_or_else(|_| DEFAULT_UPLOADS_URL_PREFIX.to_string());
        return format!("{base}/{s3_key}");
    }
    let base =
        std::env::var("IMAGE_BASE_URL").unwrap_or_else(|_| DEFAULT_IMAGE_BASE_URL.to_string());
    format!("{base}/{s3_key}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All env-mutating cases live in one test because Rust runs
    /// `#[test]`s in parallel by default; splitting them caused a flaky
    /// race where one case saw a `set_var` from another mid-flight.
    /// One test, sequential assertions, no race.
    #[test]
    fn url_for_s3_key_routes_buckets_and_honors_env() {
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                std::env::remove_var("IMAGE_BASE_URL");
                std::env::remove_var("UPLOADS_PUBLIC_URL_PREFIX");
            }
        }
        let _guard = Guard;

        // 1. No env → falls back to MinIO defaults; artworks key.
        std::env::remove_var("IMAGE_BASE_URL");
        std::env::remove_var("UPLOADS_PUBLIC_URL_PREFIX");
        assert_eq!(
            url_for_s3_key("demo/alice/1.jpg"),
            "http://localhost:9000/artworks/demo/alice/1.jpg"
        );

        // 2. `uploads/` prefix routes to the uploads bucket. The
        // prefix is kept on the key so the final URL maps to MinIO's
        // `<endpoint>/<bucket>/<key>` scheme — the doubled
        // `/uploads/uploads/` is intentional. See module docs.
        assert_eq!(
            url_for_s3_key("uploads/abc-123.jpg"),
            "http://localhost:9000/uploads/uploads/abc-123.jpg"
        );

        // 3. Env override on artworks side.
        std::env::set_var("IMAGE_BASE_URL", "https://cdn.example.com/v1");
        assert_eq!(
            url_for_s3_key("demo/alice/1.jpg"),
            "https://cdn.example.com/v1/demo/alice/1.jpg"
        );

        // 4. Env override on uploads side. Prefix on the key is still
        // preserved.
        std::env::set_var("UPLOADS_PUBLIC_URL_PREFIX", "https://cdn.example.com/up");
        assert_eq!(
            url_for_s3_key("uploads/abc-123.jpg"),
            "https://cdn.example.com/up/uploads/abc-123.jpg"
        );
    }
}
