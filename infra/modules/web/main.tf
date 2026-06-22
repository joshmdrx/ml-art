/**
 * Next.js web app, deployed all-AWS via OpenNext.
 *
 * Shape:
 *
 *   CloudFront (web_domain = apex)
 *     ├── /_next/static/*   → S3 (web_assets_bucket)  ─ immutable, long TTL
 *     ├── /public/*         → S3 (web_assets_bucket)  ─ medium TTL
 *     ├── /_next/image*     → image-optim Lambda      ─ TODO Phase 2
 *     └── /*                → server Lambda (Function URL origin)
 *
 * OpenNext (`open-next` npm package) is run during the CI build
 * step against the Next.js project; it produces:
 *   - `.open-next/server-function/`  → zipped into the server lambda
 *   - `.open-next/assets/`           → synced to the S3 assets bucket
 *   - `.open-next/cache/`            → ISR cache (S3 + DynamoDB for tag
 *                                      revalidation; TODO Phase 2)
 *
 * Why CloudFront in front of the Function URL (vs Route53 → URL):
 *   - Custom domain on the apex needs CloudFront — Function URLs only
 *     give you an AWS-owned hostname.
 *   - Static assets get served from S3 without invoking Lambda (the
 *     whole point of OpenNext over pure SSR-on-Lambda).
 *   - WAF attachment point if we want one on the web tier later.
 *
 * Lambda runtime: Node.js 20 ARM64. OpenNext bundles plain JS so
 * there's no cross-compile pain — ARM64 is purely a price/perf win.
 */

terraform {
  required_providers {
    aws = {
      source                = "hashicorp/aws"
      configuration_aliases = [aws.us_east_1]
    }
    archive = {
      source = "hashicorp/archive"
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

variable "web_domain" {
  description = "FQDN the web app serves at (apex)."
  type        = string
}

variable "web_assets_bucket" {
  description = "S3 bucket name for OpenNext static output."
  type        = string
}

variable "lambda_memory_mb" {
  description = "Server-Lambda memory."
  type        = number
}

variable "lambda_timeout_s" {
  description = "Server-Lambda per-request timeout."
  type        = number
}

variable "lambda_architecture" {
  description = "arm64 or x86_64."
  type        = string
}

variable "cloudflare_zone_id" {
  description = "Cloudflare zone ID — for the apex CNAME record (CNAME-flattened by Cloudflare into A/AAAA at resolve time)."
  type        = string
}

variable "acm_cert_arn" {
  description = "us-east-1 ACM cert covering the apex (and www, via SAN)."
  type        = string
}

variable "api_url" {
  description = "Full URL of the API — wired into the server Lambda as NEXT_PUBLIC_API_BASE_URL (and the build-time equivalent for OpenNext)."
  type        = string
}

variable "images_cdn_url" {
  description = "Full URL of the image CDN — wired in for srcset rewrites on rendered pages."
  type        = string
}

variable "config_parameter_path" {
  description = "SSM path prefix the server lambda reads on cold start (Clerk keys, Mapbox token, etc.)."
  type        = string
}

variable "waf_rate_limit_per_5min" {
  description = "Per-IP requests/5min ceiling. Same as the api WAF — both surfaces are write paths (web has /sign-up, /v1/upload via the SSR proxy)."
  type        = number
}

# ─── Placeholder Lambda payload ──────────────────────────────────────────────

data "archive_file" "placeholder" {
  type        = "zip"
  output_path = "${path.module}/.terraform/placeholder.zip"

  source {
    filename = "index.mjs"
    content  = <<-EOT
      // Placeholder web-server lambda. Replaced by CI's OpenNext build.
      export const handler = async (event) => {
        return {
          statusCode: 200,
          headers: { "content-type": "text/html; charset=utf-8" },
          body: "<!doctype html><html><body><h1>ml-art</h1><p>Infra is up. The Next.js app hasn't been deployed yet.</p></body></html>",
        };
      };
    EOT
  }
}

# ─── S3 assets bucket ────────────────────────────────────────────────────────

resource "aws_s3_bucket" "web_assets" {
  bucket = var.web_assets_bucket
}

resource "aws_s3_bucket_server_side_encryption_configuration" "web_assets" {
  bucket = aws_s3_bucket.web_assets.id
  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_s3_bucket_public_access_block" "web_assets" {
  bucket                  = aws_s3_bucket.web_assets.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

# ─── Log group ───────────────────────────────────────────────────────────────

resource "aws_cloudwatch_log_group" "web" {
  name              = "/aws/lambda/${var.name_prefix}-web"
  retention_in_days = 14
}

# ─── IAM ─────────────────────────────────────────────────────────────────────

data "aws_iam_policy_document" "lambda_assume" {
  statement {
    actions = ["sts:AssumeRole"]
    principals {
      type        = "Service"
      identifiers = ["lambda.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "web_lambda" {
  name               = "${var.name_prefix}-web-lambda"
  assume_role_policy = data.aws_iam_policy_document.lambda_assume.json
}

data "aws_iam_policy_document" "web_lambda" {
  statement {
    sid       = "Logs"
    actions   = ["logs:CreateLogStream", "logs:PutLogEvents"]
    resources = ["${aws_cloudwatch_log_group.web.arn}:*"]
  }

  statement {
    sid     = "SsmRead"
    actions = ["ssm:GetParametersByPath", "ssm:GetParameter", "ssm:GetParameters"]
    resources = [
      "arn:aws:ssm:*:*:parameter${trimsuffix(var.config_parameter_path, "/")}",
      "arn:aws:ssm:*:*:parameter${var.config_parameter_path}*",
    ]
  }
}

resource "aws_iam_role_policy" "web_lambda" {
  name   = "${var.name_prefix}-web-lambda"
  role   = aws_iam_role.web_lambda.id
  policy = data.aws_iam_policy_document.web_lambda.json
}

# ─── Server Lambda + Function URL ────────────────────────────────────────────

resource "aws_lambda_function" "web_server" {
  function_name = "${var.name_prefix}-web"
  role          = aws_iam_role.web_lambda.arn

  runtime       = "nodejs20.x"
  handler       = "index.handler"
  architectures = [var.lambda_architecture]
  memory_size   = var.lambda_memory_mb
  timeout       = var.lambda_timeout_s

  filename         = data.archive_file.placeholder.output_path
  source_code_hash = data.archive_file.placeholder.output_base64sha256

  environment {
    variables = {
      CONFIG_PARAMETER_PATH    = var.config_parameter_path
      NEXT_PUBLIC_API_BASE_URL = var.api_url
      IMAGES_CDN_URL           = var.images_cdn_url
    }
  }

  depends_on = [aws_cloudwatch_log_group.web]

  lifecycle {
    ignore_changes = [
      filename,
      source_code_hash,
      environment,
    ]
  }
}

# ─── API Gateway HTTP API (v2) ───────────────────────────────────────────────
# Sits between CloudFront and Lambda. See modules/api/ for the full
# rationale; same shape here (CloudFront → APIG → Lambda).

resource "aws_apigatewayv2_api" "web" {
  name          = "${var.name_prefix}-web"
  protocol_type = "HTTP"
  description   = "HTTP API in front of the web (OpenNext) Lambda. CloudFront's default origin attaches here."
}

resource "aws_apigatewayv2_integration" "web" {
  api_id                 = aws_apigatewayv2_api.web.id
  integration_type       = "AWS_PROXY"
  integration_uri        = aws_lambda_function.web_server.invoke_arn
  integration_method     = "POST"
  payload_format_version = "2.0"
  timeout_milliseconds   = 29000

  # Pin the Lambda's view of `Host` to the canonical public hostname.
  #
  # Without this, the Lambda receives `Host: <apigw-invoke>.execute-api.…`
  # because CloudFront's `AllViewerExceptHostHeader` origin policy strips
  # the original viewer Host (required for SNI compatibility with the
  # API Gateway certificate). Clerk's middleware then constructs all
  # absolute URLs — including the `redirect_url` in its session-handshake
  # 307 — from that wrong host, and Clerk's Frontend API rejects with
  # "redirect_url is invalid" because the API Gateway URL isn't an
  # allowed origin.
  #
  # API Gateway parameter mapping lets us rewrite headers at the
  # integration → Lambda boundary. Overwriting Host here means every
  # downstream consumer (clerkMiddleware, headers().get('host'),
  # request-derived URL helpers) sees `wander.gallery` natively without
  # any middleware-layer reconstruction (which hung the Lambda at 10s
  # when we tried).
  request_parameters = {
    "overwrite:header.Host" = "wander.gallery"
  }
}

resource "aws_apigatewayv2_route" "web_default" {
  api_id    = aws_apigatewayv2_api.web.id
  route_key = "$default"
  target    = "integrations/${aws_apigatewayv2_integration.web.id}"
}

resource "aws_apigatewayv2_stage" "web" {
  api_id      = aws_apigatewayv2_api.web.id
  name        = "$default"
  auto_deploy = true
}

resource "aws_lambda_permission" "apigw_invoke" {
  statement_id  = "AllowAPIGatewayInvoke"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.web_server.function_name
  principal     = "apigateway.amazonaws.com"
  source_arn    = "${aws_apigatewayv2_api.web.execution_arn}/*/*"
}

# ─── CloudFront ──────────────────────────────────────────────────────────────
# Two origins, path-based behaviours:
#   /_next/static/*  → S3 (immutable, long TTL)
#   /public/*        → S3 (medium TTL)
#   default (SSR)    → API Gateway → Lambda

resource "aws_cloudfront_origin_access_control" "web_assets" {
  name                              = "${var.name_prefix}-web-assets-oac"
  description                       = "OAC for the web CloudFront → S3 assets bucket."
  origin_access_control_origin_type = "s3"
  signing_behavior                  = "always"
  signing_protocol                  = "sigv4"
}

# ─── WAF — T-034 (web tier) ─────────────────────────────────────────────────
# Mirrors the api WAF: rate-based per-IP + AWS managed common rules.
# Distinct ACL (not shared with api) so we can tune limits independently
# — web traffic is mostly static-asset GETs cached at CloudFront so it
# rarely hits the rate-limit; the api's mix is more dynamic. Both ACLs
# live in us-east-1 since CLOUDFRONT-scoped WAF is region-pinned there.

resource "aws_wafv2_web_acl" "web" {
  provider = aws.us_east_1

  name        = "${var.name_prefix}-web"
  description = "Rate limit + AWS common rules in front of the web CloudFront distribution."
  scope       = "CLOUDFRONT"

  default_action {
    allow {}
  }

  rule {
    name     = "RateLimitPerIp"
    priority = 1

    action {
      block {}
    }

    statement {
      rate_based_statement {
        limit              = var.waf_rate_limit_per_5min
        aggregate_key_type = "IP"
      }
    }

    visibility_config {
      cloudwatch_metrics_enabled = true
      metric_name                = "${var.name_prefix}-web-rate-limit"
      sampled_requests_enabled   = true
    }
  }

  rule {
    name     = "AWSManagedCommonRules"
    priority = 2

    override_action {
      none {}
    }

    statement {
      managed_rule_group_statement {
        name        = "AWSManagedRulesCommonRuleSet"
        vendor_name = "AWS"

        # All four body-inspection rules below false-positive on
        # multipart image uploads. Server Actions on /studio + /onboarding
        # post PNGs/JPEGs whose embedded metadata routinely looks like
        # an attack pattern to AWS's regex:
        #   - SizeRestrictions_BODY    — body > 8KB (any real image)
        #   - CrossSiteScripting_BODY  — Adobe XMP `xmlns:x="adobe:ns:meta/"`
        #     in PNGs from Photoshop / Lightroom / Affinity etc. — confirmed
        #     blocking real uploads (WAF log 2026-06-22T12:47Z).
        #   - GenericLFI_BODY / GenericRFI_BODY / EC2MetaDataSSRF_BODY —
        #     random binary byte sequences in image data can match these
        #     pre-emptively; demoted together because the cause is identical.
        # All four still COUNT (so we can see them in metrics + sampled
        # requests if abuse patterns emerge); they just don't terminate.
        # Defence-in-depth: app-layer validates the upload mime, dimensions,
        # and pixel content via moderation; that's the real guard for a
        # binary-upload route, not WAF body regex.
        rule_action_override {
          name = "SizeRestrictions_BODY"
          action_to_use {
            count {}
          }
        }
        rule_action_override {
          name = "CrossSiteScripting_BODY"
          action_to_use {
            count {}
          }
        }
        rule_action_override {
          name = "GenericLFI_BODY"
          action_to_use {
            count {}
          }
        }
        rule_action_override {
          name = "GenericRFI_BODY"
          action_to_use {
            count {}
          }
        }
        rule_action_override {
          name = "EC2MetaDataSSRF_BODY"
          action_to_use {
            count {}
          }
        }
      }
    }

    visibility_config {
      cloudwatch_metrics_enabled = true
      metric_name                = "${var.name_prefix}-web-common-rules"
      sampled_requests_enabled   = true
    }
  }

  visibility_config {
    cloudwatch_metrics_enabled = true
    metric_name                = "${var.name_prefix}-web"
    sampled_requests_enabled   = true
  }
}

resource "aws_cloudfront_distribution" "web" {
  enabled         = true
  is_ipv6_enabled = true
  comment         = "${var.name_prefix} web"
  aliases         = [var.web_domain]
  price_class     = "PriceClass_100"
  web_acl_id      = aws_wafv2_web_acl.web.arn

  # S3 origin — static assets.
  origin {
    domain_name              = aws_s3_bucket.web_assets.bucket_regional_domain_name
    origin_id                = "s3-web-assets"
    origin_access_control_id = aws_cloudfront_origin_access_control.web_assets.id
  }

  # APIG origin — SSR. APIG forwards to Lambda; no signing collision.
  origin {
    domain_name = replace(aws_apigatewayv2_api.web.api_endpoint, "https://", "")
    origin_id   = "apigw-web"

    custom_origin_config {
      http_port              = 80
      https_port             = 443
      origin_protocol_policy = "https-only"
      origin_ssl_protocols   = ["TLSv1.2"]
    }
  }

  # Default behaviour: SSR (no cache).
  default_cache_behavior {
    target_origin_id       = "apigw-web"
    viewer_protocol_policy = "redirect-to-https"
    allowed_methods        = ["GET", "HEAD", "OPTIONS", "PUT", "POST", "PATCH", "DELETE"]
    cached_methods         = ["GET", "HEAD"]
    compress               = true

    # CachingDisabled + AllViewerExceptHostHeader (managed). APIG doesn't
    # sign, so no Authorization-collision workaround needed.
    cache_policy_id          = "4135ea2d-6df8-44a3-9df3-4b5a84be39ad"
    origin_request_policy_id = "b689b0a8-53d0-40ab-baf2-68738e2966ac"
  }

  # /_next/static/* — hashed filenames, safe to cache forever.
  ordered_cache_behavior {
    path_pattern           = "/_next/static/*"
    target_origin_id       = "s3-web-assets"
    viewer_protocol_policy = "redirect-to-https"
    allowed_methods        = ["GET", "HEAD"]
    cached_methods         = ["GET", "HEAD"]
    compress               = true
    cache_policy_id        = "658327ea-f89d-4fab-a63d-7e88639e58f6" # CachingOptimized
  }

  # /public/* — not hashed; honour the file's Cache-Control header.
  # (OpenNext sets reasonable defaults; CloudFront's CachingOptimized
  # respects origin Cache-Control via min/default/max TTLs.)
  ordered_cache_behavior {
    path_pattern           = "/public/*"
    target_origin_id       = "s3-web-assets"
    viewer_protocol_policy = "redirect-to-https"
    allowed_methods        = ["GET", "HEAD"]
    cached_methods         = ["GET", "HEAD"]
    compress               = true
    cache_policy_id        = "658327ea-f89d-4fab-a63d-7e88639e58f6"
  }

  # Root-level static assets. Next.js serves files from `web/public/`
  # at the URL root (e.g. `public/favicon.ico` → `/favicon.ico`), and
  # OpenNext copies them to the root of the assets bucket. Without
  # these path patterns, every static file at the root falls through
  # to the Lambda — and Next.js's metadata routes (icon/og/etc.) end
  # up serving empty bytes in OpenNext's runtime.
  #
  # One behaviour per extension because CloudFront patterns don't
  # support brace-expansion. Add more (`*.webp`, `*.txt`, …) as we
  # need them.
  dynamic "ordered_cache_behavior" {
    for_each = toset(["*.ico", "*.svg", "*.png", "*.jpg", "*.webp", "*.txt", "*.xml", "*.json", "*.webmanifest"])
    content {
      path_pattern           = ordered_cache_behavior.value
      target_origin_id       = "s3-web-assets"
      viewer_protocol_policy = "redirect-to-https"
      allowed_methods        = ["GET", "HEAD"]
      cached_methods         = ["GET", "HEAD"]
      compress               = true
      cache_policy_id        = "658327ea-f89d-4fab-a63d-7e88639e58f6" # CachingOptimized
    }
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

# S3 bucket policy — let only THIS CloudFront distribution read.
data "aws_iam_policy_document" "web_assets_cloudfront_read" {
  statement {
    actions   = ["s3:GetObject"]
    resources = ["${aws_s3_bucket.web_assets.arn}/*"]
    principals {
      type        = "Service"
      identifiers = ["cloudfront.amazonaws.com"]
    }
    condition {
      test     = "StringEquals"
      variable = "AWS:SourceArn"
      values   = [aws_cloudfront_distribution.web.arn]
    }
  }
}

resource "aws_s3_bucket_policy" "web_assets" {
  bucket = aws_s3_bucket.web_assets.id
  policy = data.aws_iam_policy_document.web_assets_cloudfront_read.json
}

# ─── DNS — apex → CloudFront ────────────────────────────────────────────────
# Cloudflare's CNAME flattening makes this safe at the apex — the
# zone APIs return A/AAAA records to resolvers even though we wrote
# a CNAME. Without CNAME flattening this wouldn't be RFC-legal; with
# it, it Just Works.
#
# `proxied = false` because we don't want Cloudflare's CDN in front
# of CloudFront — that would double-cache, double-bill, and break
# the SSR path which depends on CloudFront's cache-disabled default.

resource "cloudflare_record" "web_apex" {
  zone_id = var.cloudflare_zone_id
  name    = "@" # apex
  type    = "CNAME"
  content = aws_cloudfront_distribution.web.domain_name
  ttl     = 1 # Auto
  proxied = false
  comment = "apex → web CloudFront distribution (modules/web/)"
}

# ─── Outputs ─────────────────────────────────────────────────────────────────

output "cloudfront_distribution_id" {
  description = "CloudFront distribution ID — for cache invalidations on deploy."
  value       = aws_cloudfront_distribution.web.id
}

output "server_lambda_name" {
  description = "Server Lambda function name — CI publishes new code to this name."
  value       = aws_lambda_function.web_server.function_name
}

output "assets_bucket_name" {
  description = "S3 bucket holding OpenNext static output. CI syncs `.open-next/assets/` here."
  value       = aws_s3_bucket.web_assets.id
}

output "apigateway_endpoint" {
  description = "APIG HTTP API endpoint (AWS-owned hostname). Useful for direct-poking the SSR fn before DNS lands."
  value       = aws_apigatewayv2_api.web.api_endpoint
}
