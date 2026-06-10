/**
 * Every value that might vary between environments OR that someone
 * might reasonably want to tweak without reading the module code.
 *
 * Rule of thumb: if a string or number shows up in `main.tf` or any
 * module, ask "would I ever want this different?" — if yes, it
 * belongs here, not inline.
 *
 * Defaults are chosen for the prod stack (sized small for v1; cheap
 * to bump up later). Override in `terraform.tfvars`.
 */

# ─────────────────────────────────────────────────────────────────────────────
# Project identity — used in resource naming, tags, log groups, etc.
# ─────────────────────────────────────────────────────────────────────────────

variable "project_name" {
  description = "Short slug used as the prefix on every named AWS resource. Lowercase, dash-separated."
  type        = string
  default     = "ml-art"
  validation {
    condition     = can(regex("^[a-z][a-z0-9-]{1,30}$", var.project_name))
    error_message = "project_name must be lowercase alphanumeric + dashes, 2–31 chars."
  }
}

variable "environment" {
  description = "Deployment environment slug. Tags + resource suffixes use this so a future `staging` apply doesn't collide with prod."
  type        = string
  default     = "prod"
  validation {
    condition     = contains(["prod", "staging", "dev"], var.environment)
    error_message = "environment must be one of: prod, staging, dev."
  }
}

variable "aws_region" {
  description = "Primary AWS region. CloudFront-only resources auto-route to us-east-1 via the aliased provider regardless."
  type        = string
  default     = "us-east-1"
}

# ─────────────────────────────────────────────────────────────────────────────
# Domain — the gating variable. Set this once in tfvars; every URL
# in the stack derives from it via locals.tf.
# ─────────────────────────────────────────────────────────────────────────────

variable "domain_name" {
  description = "Root domain (apex). The web app serves here, api.<domain> serves the Rust API, images.<domain> is the CloudFront image CDN. The domain itself is registered out-of-band (Cloudflare Registrar, etc.); Terraform manages the Route53 hosted zone + records."
  type        = string
  # No default — force the operator to confirm what domain we're
  # spending money against.
}

variable "api_subdomain" {
  description = "Subdomain hosting the Rust API. Composed with domain_name as <api_subdomain>.<domain_name>."
  type        = string
  default     = "api"
}

variable "images_subdomain" {
  description = "Subdomain hosting the CloudFront image CDN. Composed as <images_subdomain>.<domain_name>. Kept separate from the API surface so a misconfigured WAF on the API can't blackhole images."
  type        = string
  default     = "images"
}

# ─────────────────────────────────────────────────────────────────────────────
# API runtime — Lambda Function URL backing api.<domain>.
# ─────────────────────────────────────────────────────────────────────────────

variable "api_lambda_memory_mb" {
  description = "Lambda memory size for the api-search binary. AWS scales CPU proportionally; 512MB is plenty for the Axum + sqlx + Jina-call workload and stays inside the free-tier compute envelope."
  type        = number
  default     = 512
}

variable "api_lambda_timeout_s" {
  description = "Lambda invocation timeout. Search requests p99 ~1s with the embedder hot; 30s is a generous ceiling that still bounds runaway queries."
  type        = number
  default     = 30
}

variable "api_lambda_architecture" {
  description = "Lambda arch — ARM64 is ~20% cheaper per GB-s and has comparable Rust performance once you've cross-compiled. Switch to x86_64 if cross-compile is causing pain locally."
  type        = string
  default     = "arm64"
  validation {
    condition     = contains(["arm64", "x86_64"], var.api_lambda_architecture)
    error_message = "api_lambda_architecture must be arm64 or x86_64."
  }
}

# ─────────────────────────────────────────────────────────────────────────────
# Web runtime — OpenNext-built Next.js server on Lambda, fronted by
# CloudFront on the apex domain. The static-assets bucket is sized
# automatically off the build output; only the server fn is tuned here.
# ─────────────────────────────────────────────────────────────────────────────

variable "web_lambda_memory_mb" {
  description = "Memory for the OpenNext server Lambda (SSR + RSC). 1024MB is the sweet spot for Next.js cold-start in Node 20 — going lower hurts TTFB more than the cost saving justifies."
  type        = number
  default     = 1024
}

variable "web_lambda_timeout_s" {
  description = "Per-request timeout. SSR pages should resolve in <2s; 10s gives headroom for the slow tail without letting a runaway request burn budget."
  type        = number
  default     = 10
}

variable "web_lambda_architecture" {
  description = "ARM64 is cheaper per GB-s and Node 20 has a clean arm64 runtime — no cross-compile pain since OpenNext bundles JS, not native binaries."
  type        = string
  default     = "arm64"
  validation {
    condition     = contains(["arm64", "x86_64"], var.web_lambda_architecture)
    error_message = "web_lambda_architecture must be arm64 or x86_64."
  }
}

variable "web_assets_bucket_suffix" {
  description = "S3 bucket holding OpenNext's static output (.next/static + public/). Composed as <project_name>-<environment>-<web_assets_bucket_suffix>."
  type        = string
  default     = "web-assets"
}

# ─────────────────────────────────────────────────────────────────────────────
# Jobs (jobs-lambda + SQS) — the eventually-consistent side.
# ─────────────────────────────────────────────────────────────────────────────

variable "jobs_lambda_memory_mb" {
  description = "Lambda memory for the jobs handler. Same workload shape as the API (sqlx + the occasional Resend / Rekognition call). 512MB is fine."
  type        = number
  default     = 512
}

variable "jobs_lambda_timeout_s" {
  description = "Per-event timeout. Most handlers finish in <2s; geocoding + email-send sit around 1s; image moderation depends on Rekognition latency."
  type        = number
  default     = 60
}

variable "jobs_queue_visibility_timeout_s" {
  description = "SQS visibility timeout — must be at least as long as `jobs_lambda_timeout_s` or the queue redelivers in-flight messages. Set to 6x lambda timeout per AWS recommendation."
  type        = number
  default     = 360
}

variable "jobs_max_receive_count" {
  description = "How many times a job retries before going to the DLQ. Matches the `jobs.max_attempts` column default on the Postgres backend so behaviour is symmetric across drivers."
  type        = number
  default     = 5
}

# ─────────────────────────────────────────────────────────────────────────────
# Images / S3 — bucket names need to be globally unique, so we
# prefix with project + environment.
# ─────────────────────────────────────────────────────────────────────────────

variable "artworks_bucket_suffix" {
  description = "S3 bucket holding the WikiArt seed + (eventually) studio-uploaded artwork images. Composed as <project_name>-<environment>-<artworks_bucket_suffix>."
  type        = string
  default     = "artworks"
}

variable "uploads_bucket_suffix" {
  description = "S3 bucket holding visual-search uploads + (since T-012 Phase 1) new artwork-image uploads. Lifecycle rule on this one — see modules/storage."
  type        = string
  default     = "uploads"
}

variable "tfstate_bucket_suffix" {
  description = "Mirror of the bucket hardcoded in versions.tf's backend block. Kept here so other resources (e.g. budget alarms) can reference the name without duplicating the literal."
  type        = string
  default     = "tfstate"
}

# ─────────────────────────────────────────────────────────────────────────────
# Cost guardrails — T-015.
# ─────────────────────────────────────────────────────────────────────────────

variable "monthly_budget_usd" {
  description = "AWS Budgets monthly threshold. An alert fires at 80% actual + at 100% forecast. v1 sized for an essentially-idle stack."
  type        = number
  default     = 20
}

variable "budget_alert_email" {
  description = "Email address that receives the AWS Budgets alarm. Pulled out as a variable because it's the only piece of PII in the entire TF tree."
  type        = string
  # No default — force the operator to wire a real address.
}

# ─────────────────────────────────────────────────────────────────────────────
# WAF — T-034.
# ─────────────────────────────────────────────────────────────────────────────

variable "waf_rate_limit_per_5min" {
  description = "AWS WAF rate-based rule, requests-per-5-minutes-per-IP. v1 sized generously — this is the volumetric tier (per-user limits go on Vercel + the API layer). Drops abusive bursts before they hit the Function URL."
  type        = number
  default     = 1000
}
