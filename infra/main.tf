/**
 * Composition root for the ml-art prod stack.
 *
 * Each module owns one concern and exposes the outputs the next
 * needs (e.g. `dns` outputs `cloudflare_zone_id` for `api` + `storage`
 * + `web` to attach CNAME records to).
 *
 * Ordering reflects what depends on what; Terraform sorts the
 * actual apply graph itself but reading top-to-bottom matches the
 * runtime data flow:
 *
 *   1. dns        — hosted zone + ACM certs (gating; everything else
 *                   needs the cert ARN)
 *   2. secrets    — SSM parameters for runtime config + 3rd-party keys
 *   3. storage    — S3 + CloudFront for image delivery
 *   4. jobs       — SQS + jobs-lambda (independent of api)
 *   5. api        — Lambda Function URL + WAF + CloudFront
 *   6. web        — OpenNext Lambda + S3 assets + CloudFront on apex
 *
 * Each module takes the bare-minimum inputs; we don't pass `var.*`
 * straight through unless the module genuinely owns that decision.
 */

module "dns" {
  source = "./modules/dns"

  domain_name   = var.domain_name
  web_domain    = local.web_domain
  api_domain    = local.api_domain
  images_domain = local.images_domain

  providers = {
    aws           = aws
    aws.us_east_1 = aws.us_east_1
  }
}

module "secrets" {
  source = "./modules/secrets"

  name_prefix = local.name_prefix
}

module "storage" {
  source = "./modules/storage"

  name_prefix        = local.name_prefix
  artworks_bucket    = local.artworks_bucket
  uploads_bucket     = local.uploads_bucket
  images_domain      = local.images_domain
  cloudflare_zone_id = module.dns.cloudflare_zone_id
  acm_cert_arn       = module.dns.images_cert_arn

  providers = {
    aws           = aws
    aws.us_east_1 = aws.us_east_1
  }
}

module "jobs" {
  source = "./modules/jobs"

  name_prefix                = local.name_prefix
  lambda_memory_mb           = var.jobs_lambda_memory_mb
  lambda_timeout_s           = var.jobs_lambda_timeout_s
  queue_visibility_timeout_s = var.jobs_queue_visibility_timeout_s
  max_receive_count          = var.jobs_max_receive_count
  uploads_bucket_arn         = module.storage.uploads_bucket_arn
  uploads_bucket_name        = module.storage.uploads_bucket_name
  artworks_bucket_arn        = module.storage.artworks_bucket_arn
  config_parameter_path      = module.secrets.parameter_path_prefix
}

module "api" {
  source = "./modules/api"

  name_prefix             = local.name_prefix
  api_domain              = local.api_domain
  lambda_memory_mb        = var.api_lambda_memory_mb
  lambda_timeout_s        = var.api_lambda_timeout_s
  lambda_architecture     = var.api_lambda_architecture
  waf_rate_limit_per_5min = var.waf_rate_limit_per_5min
  cloudflare_zone_id      = module.dns.cloudflare_zone_id
  acm_cert_arn            = module.dns.api_cert_arn
  jobs_queue_arn          = module.jobs.queue_arn
  jobs_queue_url          = module.jobs.queue_url
  uploads_bucket_arn      = module.storage.uploads_bucket_arn
  uploads_bucket_name     = module.storage.uploads_bucket_name
  artworks_bucket_arn     = module.storage.artworks_bucket_arn
  config_parameter_path   = module.secrets.parameter_path_prefix

  providers = {
    aws           = aws
    aws.us_east_1 = aws.us_east_1
  }
}

module "web" {
  source = "./modules/web"

  name_prefix             = local.name_prefix
  web_domain              = local.web_domain
  web_assets_bucket       = local.web_assets_bucket
  lambda_memory_mb        = var.web_lambda_memory_mb
  lambda_timeout_s        = var.web_lambda_timeout_s
  lambda_architecture     = var.web_lambda_architecture
  cloudflare_zone_id      = module.dns.cloudflare_zone_id
  acm_cert_arn            = module.dns.web_cert_arn
  api_url                 = "https://${local.api_domain}"
  images_cdn_url          = "https://${local.images_domain}"
  config_parameter_path   = module.secrets.parameter_path_prefix
  waf_rate_limit_per_5min = var.waf_rate_limit_per_5min

  providers = {
    aws           = aws
    aws.us_east_1 = aws.us_east_1
  }
}

module "observability" {
  source = "./modules/observability"

  name_prefix                    = local.name_prefix
  alert_email                    = var.budget_alert_email
  api_lambda_name                = module.api.lambda_function_name
  web_lambda_name                = module.web.server_lambda_name
  jobs_lambda_name               = module.jobs.lambda_function_name
  jobs_dlq_name                  = module.jobs.dlq_name
  api_cloudfront_distribution_id = module.api.cloudfront_distribution_id
}

# ─────────────────────────────────────────────────────────────────────────────
# AWS Budgets — T-015. Lives at the root rather than in a module
# because it's a single small resource that consumes outputs from
# every other module would be overkill to plumb through.
# ─────────────────────────────────────────────────────────────────────────────

resource "aws_budgets_budget" "monthly" {
  name              = "${local.name_prefix}-monthly"
  budget_type       = "COST"
  limit_amount      = var.monthly_budget_usd
  limit_unit        = "USD"
  time_unit         = "MONTHLY"
  time_period_start = "2026-01-01_00:00"

  # 80% actual: "you're burning faster than expected, look now".
  notification {
    comparison_operator        = "GREATER_THAN"
    threshold                  = 80
    threshold_type             = "PERCENTAGE"
    notification_type          = "ACTUAL"
    subscriber_email_addresses = [var.budget_alert_email]
  }

  # 100% forecast: "we're projected to overspend the cap this month".
  # Forecasts can be noisy in the first few days of the month — that's
  # fine; better a false positive than missing a real overrun.
  notification {
    comparison_operator        = "GREATER_THAN"
    threshold                  = 100
    threshold_type             = "PERCENTAGE"
    notification_type          = "FORECASTED"
    subscriber_email_addresses = [var.budget_alert_email]
  }
}
