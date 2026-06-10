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

resource "aws_cloudfront_distribution" "web" {
  enabled         = true
  is_ipv6_enabled = true
  comment         = "${var.name_prefix} web"
  aliases         = [var.web_domain]
  price_class     = "PriceClass_100"

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
