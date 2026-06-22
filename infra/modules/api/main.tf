/**
 * api-search Rust binary on Lambda + CloudFront-fronted Function URL
 * with WAF in front.
 *
 * Why CloudFront in front of the Function URL (instead of pointing
 * Route53 straight at the URL):
 *   - Custom domain (`api.<domain>`) — Function URLs only give you
 *     an AWS-owned hostname.
 *   - WAF attachment point (T-034). WAF doesn't support Function
 *     URLs directly; it sits in front of CloudFront.
 *   - HTTP/3, edge POPs, and gzip happen "for free" via CloudFront's
 *     default behaviour.
 *
 * Lambda runtime: cargo-lambda → `provided.al2023` with ARM64.
 * The deploy artefact path is parameterised via a separate variable
 * so CI can swap in a build-output S3 key.
 */

variable "name_prefix" {
  description = "Resource name prefix."
  type        = string
}

variable "api_domain" {
  description = "FQDN the API serves at (e.g. api.wander.gallery)."
  type        = string
}

variable "lambda_memory_mb" {
  description = "Lambda memory size."
  type        = number
}

variable "lambda_timeout_s" {
  description = "Lambda per-request timeout. Counterpart limits live at the CloudFront origin timeout."
  type        = number
}

variable "lambda_architecture" {
  description = "arm64 or x86_64."
  type        = string
}

variable "waf_rate_limit_per_5min" {
  description = "Per-IP rate-based rule threshold (T-034)."
  type        = number
}

variable "cloudflare_zone_id" {
  description = "Cloudflare zone ID — for the CNAME record pointing at the CloudFront distribution."
  type        = string
}

variable "acm_cert_arn" {
  description = "us-east-1 ACM cert covering `api_domain` (CloudFront requires us-east-1)."
  type        = string
}

variable "jobs_queue_arn" {
  description = "SQS queue ARN — for the api lambda's sqs:SendMessage policy."
  type        = string
}

variable "jobs_queue_url" {
  description = "SQS queue URL — passed to the lambda as JOBS_QUEUE_URL env var."
  type        = string
}

variable "uploads_bucket_arn" {
  description = "S3 bucket ARN — api lambda PUTs visual-search uploads here. Used for the IAM policy."
  type        = string
}

variable "uploads_bucket_name" {
  description = "S3 bucket NAME — passed to the lambda as S3_UPLOADS_BUCKET. Without it the code defaults to the literal string \"uploads\", which doesn't exist in prod and surfaces as a generic `s3 put: service error` 500."
  type        = string
}

variable "artworks_bucket_arn" {
  description = "S3 bucket — api lambda reads metadata + occasionally PUTs studio uploads."
  type        = string
}

variable "config_parameter_path" {
  description = "SSM path prefix the lambda reads on cold start."
  type        = string
}

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

# ─── Placeholder Lambda payload ──────────────────────────────────────────────

data "archive_file" "placeholder" {
  type        = "zip"
  output_path = "${path.module}/.terraform/placeholder.zip"

  source {
    filename = "index.mjs"
    content  = <<-EOT
      // Placeholder api-search lambda. Replaced by CI on first deploy.
      export const handler = async (event) => {
        console.log("api-search placeholder; event:", JSON.stringify(event));
        return {
          statusCode: 200,
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ status: "placeholder", note: "infra is up; real api code not deployed yet" }),
        };
      };
    EOT
  }
}

# ─── Log group ───────────────────────────────────────────────────────────────

resource "aws_cloudwatch_log_group" "api" {
  name              = "/aws/lambda/${var.name_prefix}-api"
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

resource "aws_iam_role" "api_lambda" {
  name               = "${var.name_prefix}-api-lambda"
  assume_role_policy = data.aws_iam_policy_document.lambda_assume.json
}

data "aws_iam_policy_document" "api_lambda" {
  statement {
    sid       = "Logs"
    actions   = ["logs:CreateLogStream", "logs:PutLogEvents"]
    resources = ["${aws_cloudwatch_log_group.api.arn}:*"]
  }

  statement {
    sid       = "SqsSend"
    actions   = ["sqs:SendMessage", "sqs:GetQueueAttributes"]
    resources = [var.jobs_queue_arn]
  }

  statement {
    sid     = "SsmRead"
    actions = ["ssm:GetParametersByPath", "ssm:GetParameter", "ssm:GetParameters"]
    resources = [
      "arn:aws:ssm:*:*:parameter${trimsuffix(var.config_parameter_path, "/")}",
      "arn:aws:ssm:*:*:parameter${var.config_parameter_path}*",
    ]
  }

  # api-search reads metadata from artworks bucket + writes/reads
  # visual-search uploads.
  statement {
    sid       = "S3Read"
    actions   = ["s3:GetObject", "s3:HeadObject"]
    resources = ["${var.artworks_bucket_arn}/*", "${var.uploads_bucket_arn}/*"]
  }

  statement {
    sid       = "S3Write"
    actions   = ["s3:PutObject"]
    resources = ["${var.uploads_bucket_arn}/*", "${var.artworks_bucket_arn}/*"]
  }
}

resource "aws_iam_role_policy" "api_lambda" {
  name   = "${var.name_prefix}-api-lambda"
  role   = aws_iam_role.api_lambda.id
  policy = data.aws_iam_policy_document.api_lambda.json
}

# ─── Lambda + Function URL ───────────────────────────────────────────────────

resource "aws_lambda_function" "api" {
  function_name = "${var.name_prefix}-api"
  role          = aws_iam_role.api_lambda.arn

  # Rust binary built by cargo-lambda. `provided.al2023` is the
  # bring-your-own-binary runtime; the handler name `bootstrap` is the
  # required filename for the executable inside the zip.
  runtime       = "provided.al2023"
  handler       = "bootstrap"
  architectures = [var.lambda_architecture]
  memory_size   = var.lambda_memory_mb
  timeout       = var.lambda_timeout_s

  filename         = data.archive_file.placeholder.output_path
  source_code_hash = data.archive_file.placeholder.output_base64sha256

  environment {
    variables = {
      CONFIG_PARAMETER_PATH = var.config_parameter_path
      JOBS_QUEUE_URL        = var.jobs_queue_url
      RUST_LOG              = "info"
      # Flips Config::load's prod sanity checks ON: ANON_COOKIE_SECRET
      # must not be the dev placeholder, Clerk vars must be set, etc.
      ML_ART_ENV = "prod"
      # Public/static config — was previously fetched per cold start
      # from SSM as SecureString (one KMS Decrypt each). These aren't
      # secrets so they live in TF-managed Lambda env now; bootstrap_ssm
      # only loads actual credentials. See modules/secrets/main.tf.
      CLERK_ISSUER              = "https://clerk.wander.gallery"
      CLERK_JWKS_URL            = "https://clerk.wander.gallery/.well-known/jwks.json"
      WEB_BASE_URL              = "https://wander.gallery"
      IMAGE_BASE_URL            = "https://images.wander.gallery"
      UPLOADS_PUBLIC_URL_PREFIX = "https://images.wander.gallery"
      RESEND_FROM_EMAIL         = "info@wander.gallery"
      # T-054 — subdomain the tokenised inquiry Reply-To addresses live
      # under (`r-<id>-<hmac>@reply.wander.gallery`). Cloudflare Email
      # Routing MX for this subdomain forwards to the inbound Worker.
      # Static, non-secret → TF env. INBOUND_EMAIL_SECRET (the webhook
      # auth secret) is a SecureString in SSM, layered on by bootstrap_ssm.
      REPLY_EMAIL_DOMAIN = "reply.wander.gallery"
      # Bucket name (not ARN) — the AWS SDK's put_object().bucket(name)
      # needs the name. Without this, Config::load defaults to the
      # literal string "uploads" which doesn't exist in prod.
      S3_UPLOADS_BUCKET = var.uploads_bucket_name
    }
  }

  depends_on = [aws_cloudwatch_log_group.api]

  lifecycle {
    # `environment` is intentionally NOT ignored — TF owns the static
    # Lambda config env (ML_ART_ENV, CONFIG_PARAMETER_PATH, etc.).
    # Runtime secrets are layered on top by core::config::bootstrap_ssm
    # at cold start (read from SSM, set as process env). That keeps
    # secrets out of TF state without losing declarative env ownership.
    ignore_changes = [
      filename,
      source_code_hash,
    ]
  }
}

# ─── API Gateway HTTP API (v2) ───────────────────────────────────────────────
# Sits between CloudFront and Lambda. The Function URL alternative was
# blocked by this AWS account's new-account restriction (Function URLs
# silently 403 on both NONE and AWS_IAM modes); APIG is the well-trodden
# fallback and the topology the user originally expected anyway.
#
# Shape:
#   CloudFront (WAF here) ──→ APIG HTTP API ──→ Lambda
#
# We use HTTP API (v2) not REST API (v1) — leaner, cheaper, $1/M
# requests, and we don't need v1's request validators / VTL transforms.

resource "aws_apigatewayv2_api" "api" {
  name          = "${var.name_prefix}-api"
  protocol_type = "HTTP"
  description   = "HTTP API in front of the api-search Lambda. CloudFront origin attaches here."
}

# AWS_PROXY integration: APIG passes the entire request envelope to
# Lambda; Lambda returns { statusCode, headers, body } — exactly what
# the placeholder (and OpenNext, and lambda_http for Rust) emit.
resource "aws_apigatewayv2_integration" "api" {
  api_id                 = aws_apigatewayv2_api.api.id
  integration_type       = "AWS_PROXY"
  integration_uri        = aws_lambda_function.api.invoke_arn
  integration_method     = "POST" # AWS_PROXY always uses POST internally
  payload_format_version = "2.0"
  timeout_milliseconds   = 29000 # APIG hard cap is 30s; leave 1s slack
}

# Catch-all route — the Rust axum app does its own internal routing.
resource "aws_apigatewayv2_route" "api_default" {
  api_id    = aws_apigatewayv2_api.api.id
  route_key = "$default"
  target    = "integrations/${aws_apigatewayv2_integration.api.id}"
}

# Default stage with auto-deploy. APIG v2 simplifies the v1 "create
# deployment, point stage at it" dance — just flip auto_deploy = true.
resource "aws_apigatewayv2_stage" "api" {
  api_id      = aws_apigatewayv2_api.api.id
  name        = "$default"
  auto_deploy = true
}

# Grant APIG permission to invoke the lambda. Without this, every
# request hits Lambda's resource policy + gets denied. source_arn
# pins it to *this* API only (prevents another API in the account
# being able to invoke our function).
resource "aws_lambda_permission" "apigw_invoke" {
  statement_id  = "AllowAPIGatewayInvoke"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.api.function_name
  principal     = "apigateway.amazonaws.com"
  source_arn    = "${aws_apigatewayv2_api.api.execution_arn}/*/*"
}

# ─── WAF — T-034 ─────────────────────────────────────────────────────────────
# Two rules:
#   1. Rate-based per-IP — drops abusive bursts.
#   2. AWS managed common rules — SQLi / XSS / known-bad bots.
#
# CLOUDFRONT-scoped ACLs MUST live in us-east-1.

resource "aws_wafv2_web_acl" "api" {
  provider = aws.us_east_1

  name        = "${var.name_prefix}-api"
  description = "Rate limit + AWS common rules in front of the api CloudFront distribution."
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
      metric_name                = "${var.name_prefix}-api-rate-limit"
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

        # `/v1/uploads/image` is multipart binary — easily >8KB and
        # carries the same Adobe XMP / random-binary false-positive
        # patterns that block image uploads on the web tier. Mirror the
        # web ACL's override set (see modules/web/main.tf for the full
        # rationale + the WAF log entry that motivated CrossSiteScripting).
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
      metric_name                = "${var.name_prefix}-api-common-rules"
      sampled_requests_enabled   = true
    }
  }

  visibility_config {
    cloudwatch_metrics_enabled = true
    metric_name                = "${var.name_prefix}-api"
    sampled_requests_enabled   = true
  }
}

# ─── CloudFront ──────────────────────────────────────────────────────────────
# Fronts the API Gateway HTTP API so:
#   - Custom domain (api.<domain>) works.
#   - WAF can attach.
#   - We get HTTP/3 + IPv6 + edge POPs.
#
# APIG's invoke URL is `https://<api-id>.execute-api.<region>.amazonaws.com`.
# We parse the hostname out of the full URL the same way we used to for
# Function URLs.

resource "aws_cloudfront_distribution" "api" {
  enabled         = true
  is_ipv6_enabled = true
  comment         = "${var.name_prefix} api"
  aliases         = [var.api_domain]
  price_class     = "PriceClass_100"
  web_acl_id      = aws_wafv2_web_acl.api.arn

  origin {
    domain_name = replace(aws_apigatewayv2_api.api.api_endpoint, "https://", "")
    origin_id   = "apigw-api"

    custom_origin_config {
      http_port              = 80
      https_port             = 443
      origin_protocol_policy = "https-only"
      origin_ssl_protocols   = ["TLSv1.2"]
    }
  }

  default_cache_behavior {
    target_origin_id       = "apigw-api"
    viewer_protocol_policy = "redirect-to-https"
    allowed_methods        = ["GET", "HEAD", "OPTIONS", "PUT", "POST", "PATCH", "DELETE"]
    cached_methods         = ["GET", "HEAD"]
    compress               = true

    # AWS-managed "CachingDisabled" — the API responds dynamically.
    cache_policy_id = "4135ea2d-6df8-44a3-9df3-4b5a84be39ad"

    # AWS-managed "AllViewerExceptHostHeader" — APIG doesn't sign so
    # forwarding Authorization is safe (no collision like with OAC).
    origin_request_policy_id = "b689b0a8-53d0-40ab-baf2-68738e2966ac"
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

# ─── DNS — api.<domain> → CloudFront ────────────────────────────────────────
# Cloudflare CNAME pointing at the CloudFront-issued hostname.
# `proxied = false` because we don't want Cloudflare's CDN in front of
# CloudFront (would double-bill, double-cache, and complicate the
# WAF story).

resource "cloudflare_record" "api" {
  zone_id = var.cloudflare_zone_id
  name    = var.api_domain
  type    = "CNAME"
  content = aws_cloudfront_distribution.api.domain_name
  ttl     = 1 # Auto
  proxied = false
  comment = "api.<domain> → api CloudFront distribution (modules/api/)"
}

# ─── Outputs ─────────────────────────────────────────────────────────────────

output "apigateway_endpoint" {
  description = "APIG HTTP API endpoint (AWS-owned hostname). Useful for direct-poking the API before DNS lands."
  value       = aws_apigatewayv2_api.api.api_endpoint
}

output "cloudfront_distribution_id" {
  description = "CloudFront distribution ID — handy for cache invalidations."
  value       = aws_cloudfront_distribution.api.id
}

output "lambda_function_name" {
  description = "API Lambda function name — CI uses this for `aws lambda update-function-code` on deploy."
  value       = aws_lambda_function.api.function_name
}
