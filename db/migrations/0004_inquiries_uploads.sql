-- 0004_inquiries_uploads.sql
-- inquiries (anonymous + authed), uploads (visual search).

CREATE TABLE inquiries (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    artwork_id          uuid NOT NULL REFERENCES artworks(id),
    artist_id           uuid NOT NULL REFERENCES artists(id),
    from_user_id        uuid REFERENCES users(id),  -- null for anonymous
    from_email          text NOT NULL,
    from_name           text NOT NULL,
    message             text NOT NULL,
    budget_range        jsonb,
    delivery_channel    text CHECK (delivery_channel IN ('email', 'platform', 'external')),
    -- Anonymous-inquiry verification flow:
    verification_token  text UNIQUE,
    verified_at         timestamptz,
    delivered_at        timestamptz,
    created_at          timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX inquiries_artist_created_idx ON inquiries (artist_id, created_at DESC);
CREATE INDEX inquiries_verification_token_idx ON inquiries (verification_token)
    WHERE verification_token IS NOT NULL AND verified_at IS NULL;

CREATE TABLE uploads (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    s3_key              text NOT NULL,
    anonymous_id        uuid,
    user_id             uuid REFERENCES users(id),
    embedding           vector(1024),
    moderation_status   text NOT NULL DEFAULT 'pending'
        CHECK (moderation_status IN ('pending', 'approved', 'rejected')),
    created_at          timestamptz NOT NULL DEFAULT now(),
    expires_at          timestamptz NOT NULL DEFAULT (now() + interval '24 hours')
);

CREATE INDEX uploads_expires_at_idx ON uploads (expires_at);
