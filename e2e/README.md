# e2e — Playwright golden-path tests

End-to-end tests against the full local stack.

## Running locally

```bash
# 1. Stack up
docker compose -f ../docker-compose.dev.yml up -d
# 2. Migrations + seed (assumed already done; otherwise see top-level README)
# 3. API
(cd ../api && DATABASE_URL=postgres://ml_art:dev@localhost:5433/ml_art_dev cargo run -p api-search) &
# 4. Web
(cd ../web && pnpm dev) &
# 5. Wait for both, then:
pnpm install
pnpm exec playwright install --with-deps chromium
pnpm test
```

`pnpm test:ui` opens Playwright's interactive runner; `pnpm report` opens
the last HTML report.

## What's covered

8 flows in `tests/`, each ~30 lines. They run against the **real seeded
demo data** (2000 WikiArt artworks across 27 synthetic studios) rather
than a separate test fixture, so they double as a sanity check on the
seed itself.

## What's NOT covered

- Auth flows (Clerk integration not built yet)
- Visual search upload
- Save / inquiry / studio flows

Add coverage when those features land.
