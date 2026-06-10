# `infra/` — Terraform for the prod stack

Source of truth for everything in AWS. The whole stack — web, API,
jobs, storage, CDN, DNS — runs on AWS and is described here. The
only externally-managed pieces are the **database** (Neon) and the
**3rd-party SaaS keys** (Clerk, Mapbox, Jina, Resend); see "What's
NOT in here" at the bottom.

> Architectural rationale: see `decisions.md` 2026-05-24 — "All-AWS
> infra over Vercel" — single cloud, single IaC story, predictable
> cost scaling.

## What's in here

| File | Purpose |
|---|---|
| `versions.tf` | Terraform + provider versions, S3 backend config |
| `variables.tf` | Every input value, with descriptions + sane defaults |
| `locals.tf` | Derived names (api_domain, bucket names, common tags) |
| `main.tf` | Module composition + the root-level budget alarm |
| `outputs.tf` | Values you copy into Vercel / DNS / runbooks |
| `terraform.tfvars.example` | Copy to `terraform.tfvars` (gitignored) and edit |
| `modules/dns/` | Route53 hosted zone + ACM certs (web, api, images) |
| `modules/secrets/` | SSM Parameter Store containers |
| `modules/storage/` | S3 + CloudFront for image delivery |
| `modules/jobs/` | SQS queue + jobs-lambda |
| `modules/api/` | api-search Lambda + Function URL + WAF + CloudFront |
| `modules/web/` | OpenNext server Lambda + S3 assets + CloudFront on apex |

The module bodies are scaffolded but mostly empty (`# TODO(deploy-track)`
comments mark the resources that haven't landed yet). Each module
fills in across follow-up commits as we wire the deploy track. The
scaffold compiles cleanly (`terraform validate`) but has nothing
to apply yet.

## Architecture (the agreed-upon shape)

```
                            <domain>           api.<domain>         images.<domain>
                               │                    │                     │
                       ┌───────┴────────┐   ┌───────┴────────┐   ┌────────┴───────┐
                       │ CloudFront     │   │ CloudFront     │   │ CloudFront     │
                       │ (web)          │   │ + WAF (T-034)  │   │ (images)       │
                       └─┬────────────┬─┘   └────────┬───────┘   └────────┬───────┘
              ┌──────────┘            │              │                    │
              │                       │              │                    │
        /_next/static/*       default (dynamic)      │                    │
        /public/*                     │              │                    │
              │                       │              │                    │
       ┌──────▼─────┐          ┌──────▼─────────┐ ┌──▼─────────────────┐ ┌▼─────────────┐
       │ S3         │          │ APIG HTTP API  │ │ APIG HTTP API      │ │ S3           │
       │ web-assets │          └──────┬─────────┘ └─────────┬──────────┘ │ artworks/    │
       └────────────┘                 │                     │            │ uploads/     │
                               ┌──────▼─────────┐  ┌────────▼───────────┐└──────────────┘
                               │ Lambda         │  │ Lambda             │
                               │ web-server     │  │ api-search (Rust)  │
                               │ (OpenNext;     │  └─────────┬──────────┘
                               │  Node 20)      │            │
                               └────────────────┘            │
                                                   ┌─────────▼──────────┐
                                                   │ SQS (JobsBackend)  │
                                                   └─────────┬──────────┘
                                                             │
                                                   ┌─────────▼──────────┐
                                                   │ Lambda             │
                                                   │ jobs-lambda (Rust) │
                                                   │ - geocoding        │
                                                   │ - emails (Resend)  │
                                                   │ - moderation       │
                                                   │   (Rekognition)    │
                                                   └────────────────────┘

DNS: Cloudflare (Registrar mandates Cloudflare nameservers) → CNAMEs → CloudFront
External: Neon (Postgres + pgvector) · Clerk · Mapbox · Jina · Resend
```

Rationale:
- `decisions.md` 2026-05-24 — "All-AWS infra over Vercel" (OpenNext on Lambda)
- `decisions.md` 2026-05-24 — "Rust Lambdas for the API"
- `decisions.md` 2026-05-29 — "Jobs queue" (Postgres ↔ SQS swap)
- `decisions.md` 2026-06-10 — "Cloudflare for DNS" (Registrar lockout)
- `decisions.md` 2026-06-10 — "API Gateway over Lambda Function URL" (new-account block)

## One-time bootstrap

State lives in S3 + DynamoDB. Those two resources need to exist
**before** `terraform init` can read the backend config. Run these
once, by hand, against the AWS account you intend to deploy into:

```sh
# Names match versions.tf's backend block. If you change them,
# change them in BOTH places.
BUCKET="ml-art-tfstate"
TABLE="ml-art-tfstate-lock"
REGION="us-east-1"

aws s3api create-bucket \
  --bucket "$BUCKET" \
  --region "$REGION"

aws s3api put-bucket-versioning \
  --bucket "$BUCKET" \
  --versioning-configuration Status=Enabled

aws s3api put-bucket-encryption \
  --bucket "$BUCKET" \
  --server-side-encryption-configuration \
    '{"Rules":[{"ApplyServerSideEncryptionByDefault":{"SSEAlgorithm":"AES256"}}]}'

aws s3api put-public-access-block \
  --bucket "$BUCKET" \
  --public-access-block-configuration \
    BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true

aws dynamodb create-table \
  --table-name "$TABLE" \
  --attribute-definitions AttributeName=LockID,AttributeType=S \
  --key-schema AttributeName=LockID,KeyType=HASH \
  --billing-mode PAY_PER_REQUEST \
  --region "$REGION"
```

## Day-to-day workflow

```sh
cd infra/

# First time:
cp terraform.tfvars.example terraform.tfvars
$EDITOR terraform.tfvars   # set domain_name + budget_alert_email

terraform init             # downloads providers + connects to S3 backend
terraform plan -out plan   # review every change before apply
terraform apply plan       # apply the plan you reviewed (no surprise drift)
```

After the first apply that creates the Route53 hosted zone:

```sh
terraform output -raw hosted_zone_name_servers
# Paste these NS records at your registrar.
# After ~5 min the domain resolves through Route53 and the rest of
# the stack (ACM cert validation, CloudFront, etc.) unblocks.
```

After SSM parameters are created (empty containers):

```sh
# Set each secret value out-of-band. The TF lifecycle{} on these
# resources ignores the `value` attribute so this won't be reverted
# on the next `terraform apply`.
aws ssm put-parameter \
  --name "$(terraform output -raw ssm_parameter_path_prefix)database_url" \
  --value "postgres://..." \
  --type SecureString \
  --overwrite
# ...repeat for each key listed in modules/secrets/main.tf
```

## Conventions

- **Variables everywhere.** Anything that's a name, sizing, or
  region lives in `variables.tf`. The rule of thumb is: if a string
  shows up inline in a module body, it should probably be a variable.
- **Module inputs are the minimum needed.** Don't pass `var.foo`
  through to a module unless the module owns the decision; pass
  the computed value from `locals.tf` instead.
- **Defaults are prod-shaped.** Override in `terraform.tfvars` for
  weirder environments.
- **State backend is S3** (not Terraform Cloud) — keeps the whole
  stack inside AWS, single bill, no extra account to manage.
- **Resource naming**: `<project>-<env>-<purpose>` via `local.name_prefix`.
  Hyphen-separated, lowercase. S3 buckets follow the same shape so
  they stay sortable in the console.

## What's NOT in here

These pieces are managed outside Terraform on purpose:

- **Neon (Postgres)** — created once via the Neon console, connection
  string goes into SSM. No need to manage DB-level config in TF;
  schema changes flow through `db/migrations/`. If we ever switch
  the DB to RDS, the instance lands in this TF; the SSM
  `database_url` parameter switches over and the lambdas don't notice.
- **Clerk** — auth project lives in the Clerk dashboard. Keys go in SSM.
- **Mapbox / Jina / Resend** — same. Keys in SSM.

**The web app deploy itself is in TF (server Lambda + assets bucket +
CloudFront).** What's _not_ in TF is the per-commit code update — CI
does `aws lambda update-function-code` + `aws s3 sync .open-next/assets/`
+ `aws cloudfront create-invalidation`, using the function name and
bucket name surfaced as Terraform outputs. The infra is declarative;
the code-on-it is a CI pipeline (see `.github/workflows/deploy-web.yml`,
TBD).
