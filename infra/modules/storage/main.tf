/**
 * S3 buckets for image storage + CloudFront in front for delivery.
 *
 * Two buckets:
 *   - artworks_bucket  — WikiArt seed + studio-uploaded artwork
 *     images. Heavy reads, write-once. Long Cache-Control.
 *   - uploads_bucket   — visual-search uploads + (T-012 Phase 1)
 *     artist artwork-image uploads, same `uploads/<uuid>.<ext>`
 *     keys. Lifecycle expires only objects tagged `state=pending`;
 *     the api retags attached artwork images `state=attached`
 *     (T-089, see core::object_store).
 *
 * CloudFront fronts both via separate origins on a single
 * distribution → single `images.<domain>` hostname → cheaper +
 * simpler. The distribution is signed with the us-east-1 cert
 * from the dns module.
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

variable "name_prefix" {
  description = "Resource name prefix."
  type        = string
}

variable "artworks_bucket" {
  description = "Globally-unique S3 bucket name for artwork images."
  type        = string
}

variable "uploads_bucket" {
  description = "Globally-unique S3 bucket name for upload anchors + studio uploads."
  type        = string
}

variable "images_domain" {
  description = "FQDN the CloudFront distribution serves at."
  type        = string
}

variable "cloudflare_zone_id" {
  description = "Cloudflare zone ID — needed for the CNAME record pointing at the CloudFront distribution. (Cloudflare flattens CNAMEs at the apex; for a subdomain like images.<domain> it's a plain CNAME.)"
  type        = string
}

variable "acm_cert_arn" {
  description = "us-east-1 ACM cert covering `images_domain`."
  type        = string
}

# ─── Buckets ─────────────────────────────────────────────────────────────────

resource "aws_s3_bucket" "artworks" {
  bucket = var.artworks_bucket
}

resource "aws_s3_bucket" "uploads" {
  bucket = var.uploads_bucket
}

# Server-side encryption with the default S3-managed key. Free, no key
# rotation to think about. Upgrade to KMS only if compliance demands it.
resource "aws_s3_bucket_server_side_encryption_configuration" "artworks" {
  bucket = aws_s3_bucket.artworks.id
  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "uploads" {
  bucket = aws_s3_bucket.uploads.id
  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

# Versioning on artworks only — write-once content, but if we ever
# stomp on an image during a re-seed we want the previous bytes back.
# Uploads are short-lived and constantly rotate; versioning would just
# bloat storage cost.
resource "aws_s3_bucket_versioning" "artworks" {
  bucket = aws_s3_bucket.artworks.id
  versioning_configuration {
    status = "Enabled"
  }
}

# Lifecycle on uploads — T-089. The bucket mixes throwaway visual-search
# uploads with permanent artist artwork images (T-012 Phase 1), so the
# rule must NOT match every object (the original `filter {}` deleted
# artists' images 90 days after upload). The api PUTs every object
# tagged `state=pending` and retags it `state=attached` when an
# artwork_images row references it (core::object_store::Retention).
# Only pending objects expire; untagged objects (pre-T-089 uploads that
# the backfill hasn't touched) are kept — failing safe.
#
# There is no app-level `uploads.expires_at` cleanup job; this rule is
# the only thing that deletes objects in this bucket.
resource "aws_s3_bucket_lifecycle_configuration" "uploads" {
  bucket = aws_s3_bucket.uploads.id

  rule {
    id     = "expire-pending-uploads"
    status = "Enabled"

    filter {
      tag {
        key   = "state"
        value = "pending"
      }
    }

    expiration {
      days = 90
    }
  }
}

# Both buckets are private — CloudFront reads via OAC, nothing else.
resource "aws_s3_bucket_public_access_block" "artworks" {
  bucket                  = aws_s3_bucket.artworks.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_public_access_block" "uploads" {
  bucket                  = aws_s3_bucket.uploads.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

# ─── CloudFront → S3 (images.<domain>) ──────────────────────────────────────
# One distribution, two origins, path-based routing:
#   /artworks/*  → artworks bucket
#   /uploads/*   → uploads bucket
# The Rust app already constructs paths under those prefixes (see
# core::storage), so no rewrites needed.

resource "aws_cloudfront_origin_access_control" "images" {
  name                              = "${var.name_prefix}-images-oac"
  description                       = "OAC for the images distribution → S3 buckets."
  origin_access_control_origin_type = "s3"
  signing_behavior                  = "always"
  signing_protocol                  = "sigv4"
}

resource "aws_cloudfront_distribution" "images" {
  enabled         = true
  is_ipv6_enabled = true
  comment         = "${var.name_prefix} image CDN"
  aliases         = [var.images_domain]
  price_class     = "PriceClass_100" # NA + EU edges only — cheapest tier

  origin {
    domain_name              = aws_s3_bucket.artworks.bucket_regional_domain_name
    origin_id                = "s3-artworks"
    origin_access_control_id = aws_cloudfront_origin_access_control.images.id
  }

  origin {
    domain_name              = aws_s3_bucket.uploads.bucket_regional_domain_name
    origin_id                = "s3-uploads"
    origin_access_control_id = aws_cloudfront_origin_access_control.images.id
  }

  # Default behaviour goes to the artworks bucket. Most reads are
  # against /artworks/<id>/<variant>.<ext>; uploads is a smaller surface.
  default_cache_behavior {
    target_origin_id       = "s3-artworks"
    viewer_protocol_policy = "redirect-to-https"
    allowed_methods        = ["GET", "HEAD"]
    cached_methods         = ["GET", "HEAD"]
    compress               = true

    # AWS-managed "CachingOptimized" policy id — long TTLs + gzip.
    # See: https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/using-managed-cache-policies.html
    cache_policy_id = "658327ea-f89d-4fab-a63d-7e88639e58f6"
  }

  ordered_cache_behavior {
    path_pattern           = "/uploads/*"
    target_origin_id       = "s3-uploads"
    viewer_protocol_policy = "redirect-to-https"
    allowed_methods        = ["GET", "HEAD"]
    cached_methods         = ["GET", "HEAD"]
    compress               = true
    cache_policy_id        = "658327ea-f89d-4fab-a63d-7e88639e58f6"
  }

  viewer_certificate {
    acm_certificate_arn      = var.acm_cert_arn
    ssl_support_method       = "sni-only"
    minimum_protocol_version = "TLSv1.2_2021"
  }

  restrictions {
    geo_restriction {
      restriction_type = "none"
    }
  }
}

# Bucket policies — allow CloudFront (via OAC) to GetObject. The
# condition keys CloudFront's AWS:SourceArn to *this* distribution
# so other distros / accounts can't sneak in.

data "aws_iam_policy_document" "artworks_cloudfront_read" {
  statement {
    actions   = ["s3:GetObject"]
    resources = ["${aws_s3_bucket.artworks.arn}/*"]
    principals {
      type        = "Service"
      identifiers = ["cloudfront.amazonaws.com"]
    }
    condition {
      test     = "StringEquals"
      variable = "AWS:SourceArn"
      values   = [aws_cloudfront_distribution.images.arn]
    }
  }
}

data "aws_iam_policy_document" "uploads_cloudfront_read" {
  statement {
    actions   = ["s3:GetObject"]
    resources = ["${aws_s3_bucket.uploads.arn}/*"]
    principals {
      type        = "Service"
      identifiers = ["cloudfront.amazonaws.com"]
    }
    condition {
      test     = "StringEquals"
      variable = "AWS:SourceArn"
      values   = [aws_cloudfront_distribution.images.arn]
    }
  }
}

resource "aws_s3_bucket_policy" "artworks" {
  bucket = aws_s3_bucket.artworks.id
  policy = data.aws_iam_policy_document.artworks_cloudfront_read.json
}

resource "aws_s3_bucket_policy" "uploads" {
  bucket = aws_s3_bucket.uploads.id
  policy = data.aws_iam_policy_document.uploads_cloudfront_read.json
}

# ─── DNS — images.<domain> → CloudFront ─────────────────────────────────────
# Cloudflare doesn't have ALIAS-style records; we use a plain CNAME
# to the CloudFront-issued hostname. `proxied = false` so requests go
# direct to CloudFront (Cloudflare's CDN sitting in front of CloudFront
# would double-cache + double-bill).

resource "cloudflare_record" "images" {
  zone_id = var.cloudflare_zone_id
  name    = var.images_domain
  type    = "CNAME"
  content = aws_cloudfront_distribution.images.domain_name
  ttl     = 1 # 1 = "Auto" — Cloudflare picks
  proxied = false
  comment = "images.<domain> → CloudFront images distribution (modules/storage/)"
}

# ─── Outputs ─────────────────────────────────────────────────────────────────

output "artworks_bucket_arn" {
  description = "ARN of the artworks bucket — needed by api + jobs lambdas' IAM policies."
  value       = aws_s3_bucket.artworks.arn
}

output "uploads_bucket_arn" {
  description = "ARN of the uploads bucket — needed by api + jobs lambdas."
  value       = aws_s3_bucket.uploads.arn
}

output "uploads_bucket_name" {
  description = "Name of the uploads bucket — set as S3_UPLOADS_BUCKET on api + jobs lambdas. Without this they default to the literal string \"uploads\" and S3 put fails."
  value       = aws_s3_bucket.uploads.bucket
}

output "cloudfront_distribution_id" {
  description = "Distribution ID — handy for cache invalidations during cutover."
  value       = aws_cloudfront_distribution.images.id
}
