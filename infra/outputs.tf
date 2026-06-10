/**
 * Outputs surfaced by `terraform output`.
 *
 * What lives here vs in a module's `outputs.tf`:
 *   - module outputs: values needed by *other* modules.
 *   - root outputs (this file): values the operator needs to copy
 *     into the registrar, paste into CI secrets, or reference from
 *     a runbook.
 */

output "web_url" {
  description = "Full URL the Next.js app serves at (apex). Useful for smoke tests + as the canonical link in runbooks."
  value       = "https://${local.web_domain}"
}

output "api_url" {
  description = "Full URL the Rust API serves at. The web module wires this into the OpenNext server Lambda as NEXT_PUBLIC_API_BASE_URL automatically."
  value       = "https://${local.api_domain}"
}

output "images_cdn_url" {
  description = "CloudFront origin for image delivery. Wire into the API's IMAGE_BASE_URL + UPLOADS_PUBLIC_URL_PREFIX (SSM)."
  value       = "https://${local.images_domain}"
}

output "web_cloudfront_distribution_id" {
  description = "Web CloudFront distribution — for `aws cloudfront create-invalidation` on deploy."
  value       = module.web.cloudfront_distribution_id
}

output "web_assets_bucket_name" {
  description = "S3 bucket holding OpenNext static output. CI does `aws s3 sync .open-next/assets/ s3://<this>/ --delete` on deploy."
  value       = module.web.assets_bucket_name
}

output "web_server_lambda_name" {
  description = "Server Lambda function name. CI does `aws lambda update-function-code` against this on deploy."
  value       = module.web.server_lambda_name
}

output "cloudflare_zone_id" {
  description = "Cloudflare zone ID for the project domain. Pinned-the-registrar's-nameservers approach — Cloudflare Registrar mandates Cloudflare DNS, so we manage records there via the cloudflare TF provider."
  value       = module.dns.cloudflare_zone_id
}

output "jobs_queue_url" {
  description = "SQS queue URL the api-search binary enqueues against. Wire into the API's JOBS_QUEUE_URL (SSM)."
  value       = module.jobs.queue_url
}

output "ssm_parameter_path_prefix" {
  description = "Root of the SSM parameter tree (e.g. /ml-art/prod/). Both Lambdas read from here via their IAM policies; the operator populates the secret values out-of-band."
  value       = module.secrets.parameter_path_prefix
}
