-- T-008c — Surface moderation rejection reason in studio.
--
-- We've been logging the rejection labels ("Explicit Nudity",
-- "Violence", etc.) from the moderation handler but not persisting
-- them — so artists who get a `rejected` status have no idea why,
-- and we have no audit trail when the studio asks. Add a freeform
-- `moderation_reason` column so the handler can write the labels
-- alongside the status flip, and the studio API can read them back.
--
-- Nullable: pending + approved rows have no reason; only set when
-- a verdict comes back rejected (or, optionally, on approval with
-- soft signals — that's a later call). Single text column rather
-- than a normalized labels table because we don't query by label
-- in the foreseeable v1 surface; we display.

ALTER TABLE artwork_images
    ADD COLUMN moderation_reason text;
