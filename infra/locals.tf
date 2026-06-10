/**
 * Computed names + composite values used across modules. Putting
 * them here instead of inline keeps the module boundaries clean and
 * means a tweak to the naming convention is one edit, not twenty.
 */

locals {
  # Used as the prefix on every named resource (Lambda fn, IAM role,
  # bucket, log group, …). Two reasons we don't just use
  # `var.project_name` directly: (1) we want the environment in the
  # name for parallel staging/prod stacks, (2) some resources (S3
  # buckets) need to be globally unique so we want a stable prefix.
  name_prefix = "${var.project_name}-${var.environment}"

  # Hostnames the various pieces serve at. Everything is AWS-backed:
  # the web app is OpenNext-on-Lambda fronted by CloudFront on the
  # apex, the API + image CDN sit on subdomains.
  web_domain    = var.domain_name
  api_domain    = "${var.api_subdomain}.${var.domain_name}"
  images_domain = "${var.images_subdomain}.${var.domain_name}"

  # S3 bucket names. Globally unique → prefix with name_prefix.
  # Suffixes are vars so a future "artworks-v2" bucket migration is
  # configurable without renaming everything else.
  artworks_bucket   = "${local.name_prefix}-${var.artworks_bucket_suffix}"
  uploads_bucket    = "${local.name_prefix}-${var.uploads_bucket_suffix}"
  web_assets_bucket = "${local.name_prefix}-${var.web_assets_bucket_suffix}"

  # Common tags applied to every resource. Augments the provider-
  # level default_tags (versions.tf) with the per-resource context
  # we can compute here.
  common_tags = {
    Project     = var.project_name
    Environment = var.environment
    Domain      = var.domain_name
  }
}
