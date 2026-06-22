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

/// Probe an image byte slice for pixel dimensions. Returns `None` if
/// the format can't be identified (we accept jpeg / png / webp via the
/// upload validation upstream, but a corrupt file slips through).
///
/// Header-only — `imagesize::blob_size` reads the first ~50 bytes and
/// returns without decoding pixel data, so this is essentially free per
/// upload. Used to populate `uploads.width`/`height` so the studio
/// attach can copy them onto `artwork_images` for layout reservation
/// (CLS prevention on the public artwork page).
pub fn probe_image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    imagesize::blob_size(bytes)
        .ok()
        .and_then(|s| Some((u32::try_from(s.width).ok()?, u32::try_from(s.height).ok()?)))
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

    /// Smallest valid PNG — 1×1 pixel. Built inline rather than reading
    /// from disk so the test stays hermetic. Sourced from the minimal
    /// PNG specification: signature + IHDR (width=1, height=1, bit
    /// depth=8, color type=2 RGB) + IDAT (compressed single zero pixel)
    /// + IEND. Verified bytes by hand against the spec.
    const ONE_PX_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // signature
        0x00, 0x00, 0x00, 0x0D, // IHDR length
        0x49, 0x48, 0x44, 0x52, // "IHDR"
        0x00, 0x00, 0x00, 0x01, // width = 1
        0x00, 0x00, 0x00, 0x01, // height = 1
        0x08, 0x02, 0x00, 0x00, 0x00, // bit depth, color, ...
        0x90, 0x77, 0x53, 0xDE, // CRC
        0x00, 0x00, 0x00, 0x0C, // IDAT length
        0x49, 0x44, 0x41, 0x54, // "IDAT"
        0x08, 0x99, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0xCB, 0xD3, 0x07,
        0x9E, // IDAT CRC
        0x00, 0x00, 0x00, 0x00, // IEND length
        0x49, 0x45, 0x4E, 0x44, // "IEND"
        0xAE, 0x42, 0x60, 0x82, // IEND CRC
    ];

    #[test]
    fn probe_dimensions_reads_png_header() {
        assert_eq!(probe_image_dimensions(ONE_PX_PNG), Some((1, 1)));
    }

    #[test]
    fn probe_dimensions_rejects_non_image() {
        assert_eq!(probe_image_dimensions(b"not an image"), None);
        assert_eq!(probe_image_dimensions(&[]), None);
    }

    #[test]
    fn probe_dimensions_rejects_truncated_png() {
        // Just the signature, no IHDR — imagesize should bail.
        let truncated = &ONE_PX_PNG[..8];
        assert_eq!(probe_image_dimensions(truncated), None);
    }
}
