-- Pixel dimensions for `/v1/uploads/image` uploads.
--
-- The upload handler probes the image bytes (header-only, via the
-- `imagesize` crate) before the S3 PUT and stamps these columns on
-- the row. The studio attach handler (`POST /v1/studio/artworks/:id/images`)
-- then carries them across to `artwork_images.width/height` whenever
-- the s3_key starts with `uploads/` — single source of truth for
-- pixel dims, not spoofable by clients.
--
-- Both nullable: legacy upload rows (pre-this-migration) won't have
-- them, and a corrupt-bytes upload that slipped past mime validation
-- would also probe `None`. NULL means "we don't know" and downstream
-- code falls back to letting the browser compute aspect ratio at
-- render time (with layout shift).

ALTER TABLE uploads
    ADD COLUMN width  integer,
    ADD COLUMN height integer;
