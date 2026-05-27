# db/ — Postgres schema

Source of truth for the database schema. Migrations are plain `.sql` files,
applied in lexicographic order by `sqlx-cli` (or any other sequential runner).

## Layout

```
db/
├── migrations/
│   ├── 0001_init.sql              users, artists, extensions
│   ├── 0002_artworks.sql          artworks, images, embeddings, tags
│   ├── 0003_collections.sql       user collections + saved artworks
│   ├── 0004_inquiries_uploads.sql inquiries + visual-search uploads
│   ├── 0005_neighborhoods.sql     curated neighborhoods (algorithmic later)
│   ├── 0006_events_profiles.sql   behavioral events + taste embeddings
│   └── 0007_ml_artifacts.sql      import sources, LLM artifacts, eval set
└── README.md
```

## Running migrations

Locally against the dev Postgres in docker-compose:

```bash
# from the repo root, with docker compose up running:
cargo install sqlx-cli --no-default-features --features postgres
export DATABASE_URL="postgres://ml_art:dev@localhost:5432/ml_art_dev"
sqlx migrate run --source db/migrations
```

Or with any other tool that runs `.sql` files in order (psql in a loop, dbmate, etc.).

## Conventions

- One file per logical group of tables; numbered sequentially.
- Never rewrite a committed migration. Add a new migration with the change.
- Extensions (`pgvector`, `uuid-ossp`, `pgcrypto`) created in `0001_init.sql`
  via `CREATE EXTENSION IF NOT EXISTS`, so first-run on a fresh Postgres works.
- All identifiers are `snake_case`.
- All `id` columns are `uuid PRIMARY KEY DEFAULT gen_random_uuid()`.
- Timestamps are `timestamptz NOT NULL DEFAULT now()` unless they represent an
  optional event that hasn't happened yet (e.g. `deleted_at`).
- Soft-delete: `deleted_at` on `artists`, `artworks`, `user_collections`, `users`.
  All public read queries filter `WHERE deleted_at IS NULL`.

## V1 deviations from the spec

- `events` is **not partitioned** in v1. Schema is otherwise identical; convert
  to monthly partitioning when scale demands. See `0006_events_profiles.sql`.
- `artwork_images` stores one row per uploaded original. Variants (thumb /
  medium / full) are served via an image proxy at the CDN edge, not as rows.

## Demo data

`artworks.is_demo` flags seeded demo content (WikiArt / Met). Production reads
filter `is_demo = false`; staging and local include it; the seed script sets
`is_demo = true` on every row it creates.
