# ml-art Makefile
#
# Run `make` (or `make help`) for the full list. Conventions:
#   - lifecycle targets    : up / down / nuke / migrate / seed / dev
#   - per-service runners  : api / web (foreground; use separate terminals)
#   - test targets         : test-api / test-web / test-e2e / test-ml / test
#   - hygiene              : check / fmt / lint / typecheck
#
# All paths are repo-rooted. Don't add `cd` to recipes — use `-C` instead so
# parallel `make -j` doesn't trip over shared cwd.

SHELL          := /bin/bash
.SHELLFLAGS    := -eu -o pipefail -c

COMPOSE        := docker compose -f docker-compose.dev.yml
DB_URL         := postgres://ml_art:dev@localhost:5433/ml_art_dev
WIKIART_DIR    := spikes/2026-05-modifier-deltas/data/wikiart

API_PORT       := 9100
WEB_PORT       := 3000

# ─── help (default target) ──────────────────────────────────────────────────
.DEFAULT_GOAL := help

.PHONY: help
help:
	@echo "ml-art — local-dev targets"
	@echo ""
	@echo "  Setup:"
	@echo "    make setup        first-time: up + migrate + seed"
	@echo "    make up           start docker services (postgres, minio, mailhog)"
	@echo "    make migrate      apply all db/migrations/*.sql"
	@echo "    make seed         seed the WikiArt demo corpus (idempotent)"
	@echo "    make seed-reset   wipe demo content + re-seed"
	@echo ""
	@echo "  Running:"
	@echo "    make dev          start api + web together (foreground; Ctrl-C stops both)"
	@echo "    make api          run the Rust API in the foreground"
	@echo "    make web          run Next.js in the foreground"
	@echo "    make status       show which services are listening"
	@echo ""
	@echo "  Shutdown:"
	@echo "    make down         stop docker services (keeps data)"
	@echo "    make nuke         stop docker AND wipe volumes (data loss)"
	@echo ""
	@echo "  Testing:"
	@echo "    make test         run api + web + ml tests (no e2e)"
	@echo "    make test-api     cargo test --workspace"
	@echo "    make test-web     pnpm vitest"
	@echo "    make test-e2e     playwright (needs api+web running)"
	@echo "    make test-ml      uv run pytest"
	@echo "    make test-all     everything, e2e included"
	@echo ""
	@echo "  Hygiene:"
	@echo "    make check        fmt-check + lint + typecheck (no tests)"
	@echo "    make fmt          auto-format everything"
	@echo ""
	@echo "  Prod deploy (requires cargo-lambda + AWS SSO login):"
	@echo "    make deploy-api          build + push api-search to Lambda"
	@echo "    make deploy-api-check    cargo lambda build only (no upload)"
	@echo "    make deploy-jobs         build + push jobs-lambda to Lambda"
	@echo "    make deploy-jobs-check   cargo lambda build only (no upload)"
	@echo "    make deploy-web          build (OpenNext) + push web + sync assets + invalidate"
	@echo "    make deploy-web-check    opennextjs-aws build only (no upload)"
	@echo "    make smoke-prod          curl-based smoke suite (read-only, ~10s)"
	@echo ""
	@echo "  Util:"
	@echo "    make psql         open a psql shell in the postgres container"
	@echo "    make logs         tail logs from all docker services"
	@echo "    make logs-api     tail /tmp/api.log if api was started via make dev"
	@echo "    make logs-web     tail /tmp/web.log if web was started via make dev"

# ─── lifecycle ──────────────────────────────────────────────────────────────
.PHONY: up
up:
	@$(COMPOSE) up -d
	@echo "✔ docker services up — postgres:5433, minio:9000/9001, mailhog:2025/8025"

.PHONY: down
down:
	@$(COMPOSE) down
	@echo "✔ docker services stopped (volumes kept)"
	@scripts/kill-port.sh $(API_PORT) api
	@scripts/kill-port.sh $(WEB_PORT) web

.PHONY: nuke
nuke:
	@read -p "This will wipe all local Postgres + MinIO data. Continue? [y/N] " yn; \
	  case "$$yn" in y|Y) $(COMPOSE) down -v && echo "✔ wiped";; *) echo "aborted";; esac

.PHONY: migrate
migrate:
	@scripts/migrate.sh

.PHONY: seed
seed:
	@$(MAKE) -s _seed-check
	@cd ml && uv run python -m ml_art.seed \
		--data-dir $(WIKIART_DIR) \
		--database-url "$(DB_URL)"

.PHONY: seed-reset
seed-reset:
	@cd ml && uv run python -m ml_art.seed \
		--data-dir $(WIKIART_DIR) \
		--database-url "$(DB_URL)" \
		--reset

.PHONY: _seed-check
_seed-check:
	@if [ ! -d "ml/$(WIKIART_DIR)" ] || [ -z "$$(ls -A ml/$(WIKIART_DIR) 2>/dev/null)" ]; then \
		echo "✘ WikiArt corpus not found at ml/$(WIKIART_DIR)"; \
		echo "  Fetch it first:"; \
		echo "    cd ml && uv run python -m ml_art.datasets.wikiart \\"; \
		echo "      --out $(WIKIART_DIR) --per-style 80 --max-total 2000"; \
		exit 1; \
	fi

.PHONY: setup
setup: up migrate seed
	@echo ""
	@echo "✔ Setup complete. Next:"
	@echo "    make dev    # api + web together"

# ─── runners ────────────────────────────────────────────────────────────────
.PHONY: dev
dev: up
	@scripts/dev.sh

.PHONY: api
api:
	@cd api && PORT=$(API_PORT) cargo run -p api-search

.PHONY: web
web:
	@cd web && pnpm dev

.PHONY: status
status:
	@scripts/status.sh

# ─── testing ────────────────────────────────────────────────────────────────
.PHONY: test
test: test-api test-web test-ml

.PHONY: test-all
test-all: test test-e2e

.PHONY: test-api
test-api:
	@cd api && DATABASE_URL="$(DB_URL)" cargo test --workspace

.PHONY: test-web
test-web:
	@cd web && pnpm test

.PHONY: test-e2e
test-e2e:
	@cd e2e && pnpm test

.PHONY: test-ml
test-ml:
	@cd ml && uv run pytest

# ─── hygiene ────────────────────────────────────────────────────────────────
.PHONY: check
check: check-api check-web check-ml

.PHONY: check-api
check-api:
	@cd api && cargo fmt --all --check
	@cd api && cargo clippy --workspace --all-targets -- -D warnings

.PHONY: check-web
check-web:
	@cd web && pnpm typecheck
	@cd web && pnpm lint

.PHONY: check-ml
check-ml:
	@cd ml && uv run ruff check ml_art tests

.PHONY: fmt
fmt:
	@cd api && cargo fmt --all
	@cd ml && uv run ruff format ml_art tests || true

# ─── prod deploy ────────────────────────────────────────────────────────────
# Build + ship the Rust lambdas to AWS. Requires cargo-lambda installed
# locally and an active SSO session (`aws sso login --profile ml-art`).
# See infra/POST_DEPLOY.md for the one-time setup.

.PHONY: deploy-api
deploy-api:
	@scripts/deploy-api.sh

.PHONY: deploy-api-check
deploy-api-check:
	@scripts/deploy-api.sh --check

.PHONY: deploy-jobs
deploy-jobs:
	@scripts/deploy-jobs.sh

.PHONY: deploy-jobs-check
deploy-jobs-check:
	@scripts/deploy-jobs.sh --check

.PHONY: deploy-web
deploy-web:
	@scripts/deploy-web.sh

.PHONY: deploy-web-check
deploy-web-check:
	@scripts/deploy-web.sh --check

# Production smoke suite (T-075). Read-only curl assertions over the
# public surface; ~10s end-to-end. Auto-runs at the tail of
# deploy-api and deploy-web so a bad deploy fails loud at the deploy
# step. Also runnable on its own (`make smoke-prod`) before / after
# any manual change.
.PHONY: smoke-prod
smoke-prod:
	@scripts/smoke-prod.sh

# ─── utilities ──────────────────────────────────────────────────────────────
.PHONY: psql
psql:
	@docker exec -it ml-art-postgres psql -U ml_art -d ml_art_dev

.PHONY: logs
logs:
	@$(COMPOSE) logs -f

.PHONY: logs-api
logs-api:
	@test -f /tmp/api.log && tail -f /tmp/api.log || echo "no /tmp/api.log — start with 'make dev'"

.PHONY: logs-web
logs-web:
	@test -f /tmp/web.log && tail -f /tmp/web.log || echo "no /tmp/web.log — start with 'make dev'"
