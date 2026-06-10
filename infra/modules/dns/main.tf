/**
 * ACM certs + Cloudflare DNS validation records.
 *
 * The domain (wander.gallery) is registered with Cloudflare Registrar,
 * which mandates Cloudflare nameservers — we don't get to point NS at
 * Route53. So DNS records live in Cloudflare; ACM certs still live in
 * AWS (us-east-1 because CloudFront).
 *
 * Three certs, all us-east-1 (CloudFront-only):
 *   - web_cert     — apex + www. SAN
 *   - api_cert     — api.<domain>
 *   - images_cert  — images.<domain>
 *
 * For each cert, ACM hands us 1–2 validation CNAME targets; we write
 * them into Cloudflare and the matching aws_acm_certificate_validation
 * resource blocks until ACM marks the cert ISSUED.
 *
 * The Cloudflare zone is assumed to already exist — Cloudflare auto-
 * created it when the domain was registered. We look it up via the
 * `cloudflare_zone` data source rather than creating it.
 */

terraform {
  required_providers {
    aws = {
      source                = "hashicorp/aws"
      configuration_aliases = [aws.us_east_1]
    }
    cloudflare = {
      source = "cloudflare/cloudflare"
    }
  }
}

variable "domain_name" {
  description = "Apex domain. Must match the Cloudflare-registered zone name."
  type        = string
}

variable "web_domain" {
  description = "FQDN the web app serves at — apex (same value as domain_name in v1). Kept as a separate input so a future `www.` switch is one variable change."
  type        = string
}

variable "api_domain" {
  description = "FQDN for the Rust API (e.g. api.wander.gallery)."
  type        = string
}

variable "images_domain" {
  description = "FQDN for the CloudFront image CDN (e.g. images.wander.gallery)."
  type        = string
}

# ─── Cloudflare zone lookup ──────────────────────────────────────────────────
# Auto-created by Cloudflare when the domain was registered. Just
# look up its ID.

data "cloudflare_zone" "this" {
  name = var.domain_name
}

# ─── ACM certs (all us-east-1, all DNS-validated) ────────────────────────────

resource "aws_acm_certificate" "web" {
  provider                  = aws.us_east_1
  domain_name               = var.web_domain
  subject_alternative_names = ["www.${var.web_domain}"]
  validation_method         = "DNS"

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_acm_certificate" "api" {
  provider          = aws.us_east_1
  domain_name       = var.api_domain
  validation_method = "DNS"

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_acm_certificate" "images" {
  provider          = aws.us_east_1
  domain_name       = var.images_domain
  validation_method = "DNS"

  lifecycle {
    create_before_destroy = true
  }
}

# ─── DNS validation records (in Cloudflare) ──────────────────────────────────
# ACM gives us a list of CNAMEs (one per domain on the cert; the web
# cert has 2 because of the www SAN). We write each into Cloudflare;
# ACM polls them, marks the cert ISSUED, and the matching
# aws_acm_certificate_validation resource unblocks.
#
# `proxied = false` is critical — these are pure DNS validations, not
# something that should go through Cloudflare's CDN.
#
# Cloudflare's record `value` field has a trailing-dot quirk: ACM
# emits values like `_xxx.acm-validations.aws.` (trailing dot) but
# Cloudflare strips it. trimsuffix() normalizes so plans are stable.

locals {
  # ACM emits one set of domain_validation_options per (domain + SAN).
  # Flatten across all three certs into a single map for one shared
  # cloudflare_record loop. Keyed by domain so the map is stable
  # across plans (vs. positional list ordering which is not).
  validation_records = merge(
    {
      for dvo in aws_acm_certificate.web.domain_validation_options :
      dvo.domain_name => {
        name  = dvo.resource_record_name
        type  = dvo.resource_record_type
        value = trimsuffix(dvo.resource_record_value, ".")
      }
    },
    {
      for dvo in aws_acm_certificate.api.domain_validation_options :
      dvo.domain_name => {
        name  = dvo.resource_record_name
        type  = dvo.resource_record_type
        value = trimsuffix(dvo.resource_record_value, ".")
      }
    },
    {
      for dvo in aws_acm_certificate.images.domain_validation_options :
      dvo.domain_name => {
        name  = dvo.resource_record_name
        type  = dvo.resource_record_type
        value = trimsuffix(dvo.resource_record_value, ".")
      }
    },
  )
}

resource "cloudflare_record" "acm_validation" {
  for_each = local.validation_records

  zone_id = data.cloudflare_zone.this.id
  name    = each.value.name
  type    = each.value.type
  content = each.value.value
  ttl     = 60
  proxied = false
  comment = "ACM cert DNS validation — managed by Terraform (modules/dns/)"

  # ACM auto-renews certs; the validation record stays put forever
  # for the same cert. If we ever rotate the cert (different domains),
  # the old validation record gets replaced cleanly.
  allow_overwrite = true
}

# ─── Wait-for-validation resources ───────────────────────────────────────────
# These block apply until ACM has marked the cert ISSUED. On the first
# apply this takes ~30 seconds — Cloudflare DNS is fast.
#
# The validation_record_fqdns input filters cloudflare_record's hostname
# to just the ones for THIS cert (the merged local has all three certs'
# validations).

resource "aws_acm_certificate_validation" "web" {
  provider        = aws.us_east_1
  certificate_arn = aws_acm_certificate.web.arn
  validation_record_fqdns = [
    for dvo in aws_acm_certificate.web.domain_validation_options :
    trimsuffix(dvo.resource_record_name, ".")
  ]

  depends_on = [cloudflare_record.acm_validation]
}

resource "aws_acm_certificate_validation" "api" {
  provider        = aws.us_east_1
  certificate_arn = aws_acm_certificate.api.arn
  validation_record_fqdns = [
    for dvo in aws_acm_certificate.api.domain_validation_options :
    trimsuffix(dvo.resource_record_name, ".")
  ]

  depends_on = [cloudflare_record.acm_validation]
}

resource "aws_acm_certificate_validation" "images" {
  provider        = aws.us_east_1
  certificate_arn = aws_acm_certificate.images.arn
  validation_record_fqdns = [
    for dvo in aws_acm_certificate.images.domain_validation_options :
    trimsuffix(dvo.resource_record_name, ".")
  ]

  depends_on = [cloudflare_record.acm_validation]
}

# ─── Outputs ─────────────────────────────────────────────────────────────────

output "cloudflare_zone_id" {
  description = "Cloudflare zone ID for wander.gallery. Other modules use this to manage CNAME records pointing at CloudFront."
  value       = data.cloudflare_zone.this.id
}

output "web_cert_arn" {
  description = "ACM cert ARN covering the apex (and www). us-east-1 — fronted by CloudFront."
  value       = aws_acm_certificate_validation.web.certificate_arn
}

output "api_cert_arn" {
  description = "ACM cert ARN for the API subdomain. us-east-1 — fronted by CloudFront."
  value       = aws_acm_certificate_validation.api.certificate_arn
}

output "images_cert_arn" {
  description = "ACM cert ARN for the images subdomain. us-east-1 — CloudFront only accepts certs from there."
  value       = aws_acm_certificate_validation.images.certificate_arn
}
