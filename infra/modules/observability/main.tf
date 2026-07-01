/**
 * CloudWatch alarms + SNS notifications.
 *
 * One topic, one email subscriber, multiple alarms attached. v1
 * goal: alert on the failure modes we can't see from CloudWatch's
 * default dashboards.
 *
 * Alarms wired here:
 *   1. Each Lambda (api, web, jobs) — high error rate
 *   2. Jobs DLQ — any messages (means something has retried-and-failed)
 *   3. CloudFront api distribution — high 5xx rate
 *   4. AWS Budgets at 50% actual — early-warning above the existing
 *      80%/100% notifications on the root budget. Lives here vs.
 *      on the budget resource so all comms route through one SNS topic.
 *
 * Naming: every alarm is `<name_prefix>-<surface>-<metric>` so they
 * sort cleanly in the CloudWatch console.
 *
 * Thresholds are first-principles guesses — tune as we learn what
 * baseline looks like under real traffic.
 */

variable "name_prefix" {
  description = "Resource name prefix (e.g. ml-art-prod)."
  type        = string
}

variable "alert_email" {
  description = "Email subscriber for all SNS-routed alarms. Same address as the AWS Budgets one; we want a single inbox for ops."
  type        = string
}

variable "api_lambda_name" {
  description = "Function name for the api-search Lambda."
  type        = string
}

variable "web_lambda_name" {
  description = "Function name for the web (OpenNext) Lambda."
  type        = string
}

variable "jobs_lambda_name" {
  description = "Function name for the jobs Lambda."
  type        = string
}

variable "jobs_dlq_name" {
  description = "Name (not ARN) of the jobs DLQ — CloudWatch metric dimension wants the queue name."
  type        = string
}

variable "api_cloudfront_distribution_id" {
  description = "Distribution ID for the api CloudFront distribution."
  type        = string
}

variable "web_cloudfront_distribution_id" {
  description = "Distribution ID for the web CloudFront distribution."
  type        = string
}

# ─── SNS topic + email subscription ─────────────────────────────────────────
# One topic for all alarm comms. Email subscriber confirms via a one-time
# AWS link the first time apply runs — check the inbox associated with
# var.alert_email and click "Confirm subscription". Until confirmed, the
# subscription is `PendingConfirmation` and no alerts fire.

resource "aws_sns_topic" "alerts" {
  name = "${var.name_prefix}-alerts"
}

resource "aws_sns_topic_subscription" "alerts_email" {
  topic_arn = aws_sns_topic.alerts.arn
  protocol  = "email"
  endpoint  = var.alert_email
}

# ─── Lambda error-rate alarms ───────────────────────────────────────────────
# Each Lambda gets the same shape: "Errors >= 5 in any 5-minute window."
# `Errors` is the AWS-managed metric; we use SUM so a burst counts even
# if the warm container survives. 5 is a guess; tune once we see noise.

locals {
  lambda_names = {
    api  = var.api_lambda_name
    web  = var.web_lambda_name
    jobs = var.jobs_lambda_name
  }
}

resource "aws_cloudwatch_metric_alarm" "lambda_errors" {
  for_each = local.lambda_names

  alarm_name          = "${var.name_prefix}-${each.key}-errors"
  alarm_description   = "Lambda function ${each.value} surfaced >= 5 errors in 5 minutes."
  comparison_operator = "GreaterThanOrEqualToThreshold"
  evaluation_periods  = 1
  metric_name         = "Errors"
  namespace           = "AWS/Lambda"
  period              = 300
  statistic           = "Sum"
  threshold           = 5
  treat_missing_data  = "notBreaching"

  dimensions = {
    FunctionName = each.value
  }

  alarm_actions = [aws_sns_topic.alerts.arn]
  ok_actions    = [aws_sns_topic.alerts.arn]
}

# ─── Jobs DLQ depth ─────────────────────────────────────────────────────────
# Any message landing in the DLQ means a job retried `max_receive_count`
# times and gave up. Even one is worth investigating, hence threshold = 1.

resource "aws_cloudwatch_metric_alarm" "jobs_dlq_depth" {
  alarm_name          = "${var.name_prefix}-jobs-dlq-not-empty"
  alarm_description   = "Jobs DLQ has at least one message — a job exhausted its retries."
  comparison_operator = "GreaterThanOrEqualToThreshold"
  evaluation_periods  = 1
  metric_name         = "ApproximateNumberOfMessagesVisible"
  namespace           = "AWS/SQS"
  period              = 60
  statistic           = "Maximum"
  threshold           = 1
  treat_missing_data  = "notBreaching"

  dimensions = {
    QueueName = var.jobs_dlq_name
  }

  alarm_actions = [aws_sns_topic.alerts.arn]
  ok_actions    = [aws_sns_topic.alerts.arn]
}

# ─── CloudFront 5xx rate (api only) ─────────────────────────────────────────
# 5xx rate on the api distribution — a high value usually means the
# Lambda is failing requests faster than the Lambda-error alarm above
# can catch (Lambda-Errors counts handler crashes; CloudFront-5xx
# counts everything including timeouts + APIG errors).
#
# `5xxErrorRate` is a percentage, evaluated over 5-minute windows.
# Threshold 5 = 5% (one in 20 requests). Generous; tune down later.
#
# CloudFront metrics live in us-east-1 regardless of distribution region
# — the metric is global. No special provider needed because we ARE
# us-east-1 here.

resource "aws_cloudwatch_metric_alarm" "api_cloudfront_5xx" {
  alarm_name          = "${var.name_prefix}-api-cloudfront-5xx"
  alarm_description   = "Api CloudFront distribution 5xx rate above 5% over 5 minutes."
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 1
  metric_name         = "5xxErrorRate"
  namespace           = "AWS/CloudFront"
  period              = 300
  statistic           = "Average"
  threshold           = 5
  treat_missing_data  = "notBreaching"

  dimensions = {
    DistributionId = var.api_cloudfront_distribution_id
    Region         = "Global"
  }

  alarm_actions = [aws_sns_topic.alerts.arn]
  ok_actions    = [aws_sns_topic.alerts.arn]
}

# T-084.2 — mirror of the api alarm above, on the web distribution.
# Same 5% threshold — the web tier serves a lot more variety (SSR
# render errors, static asset 404s from stale CDN caches, etc) so
# this is a genuine safety net that Lambda-Errors alone doesn't
# catch (a 502 from APIG doesn't count as a Lambda Error).
resource "aws_cloudwatch_metric_alarm" "web_cloudfront_5xx" {
  alarm_name          = "${var.name_prefix}-web-cloudfront-5xx"
  alarm_description   = "Web CloudFront distribution 5xx rate above 5% over 5 minutes."
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 1
  metric_name         = "5xxErrorRate"
  namespace           = "AWS/CloudFront"
  period              = 300
  statistic           = "Average"
  threshold           = 5
  treat_missing_data  = "notBreaching"

  dimensions = {
    DistributionId = var.web_cloudfront_distribution_id
    Region         = "Global"
  }

  alarm_actions = [aws_sns_topic.alerts.arn]
  ok_actions    = [aws_sns_topic.alerts.arn]
}

# ─── Neon connection failures ───────────────────────────────────────────────
# T-084.2 — CloudWatch Logs metric filter over the api Lambda logs
# looking for the two sqlx error patterns that indicate a database
# problem the Rust code has to surface but the platform-level Lambda
# alarms miss (they'd count as a handler crash IF a request happened
# during the outage; we want to know even during idle periods so we
# don't miss quiet degradations).
#
# Pattern grammar: `?A ?B` is CloudWatch's "match A OR B, unquoted"
# syntax. `"pool timed out"` is sqlx's display for
# `Error::PoolTimedOut`; `"error connecting to database"` covers the
# Io variant during Neon cold-start.

resource "aws_cloudwatch_log_metric_filter" "neon_pool_errors" {
  name           = "${var.name_prefix}-neon-pool-errors"
  log_group_name = "/aws/lambda/${var.api_lambda_name}"
  pattern        = "?\"pool timed out\" ?\"error connecting to database\""

  metric_transformation {
    name          = "NeonPoolErrors"
    namespace     = "MlArt/Db"
    value         = "1"
    default_value = "0"
    unit          = "Count"
  }
}

resource "aws_cloudwatch_metric_alarm" "neon_pool_errors" {
  alarm_name          = "${var.name_prefix}-neon-pool-errors"
  alarm_description   = "Api Lambda logged >= 3 Neon pool / connection errors in 5 minutes."
  comparison_operator = "GreaterThanOrEqualToThreshold"
  evaluation_periods  = 1
  metric_name         = "NeonPoolErrors"
  namespace           = "MlArt/Db"
  period              = 300
  statistic           = "Sum"
  threshold           = 3
  treat_missing_data  = "notBreaching"

  alarm_actions = [aws_sns_topic.alerts.arn]
  ok_actions    = [aws_sns_topic.alerts.arn]
}

# ─── Cost / runaway-detection alarms (T-015) ────────────────────────────────
# Belt-and-suspenders alongside the 50%/80%/100% AWS Budgets alerts.
# Budgets fire on aggregate account cost; these fire on the two axes
# most likely to cause a runaway: Lambda invocations (recursion / loop
# / misconfigured cron) and CloudFront egress (someone hotlinking our
# CDN, or a bad cache config).
#
# COST.md pre-launch checklist calls for both.

# Per-Lambda daily-invocations alarm. 100k/day is nothing at real
# traffic; at v1 idle, a Lambda burning 100k in a day almost
# certainly means a recursion or retry loop.
resource "aws_cloudwatch_metric_alarm" "lambda_daily_invocations" {
  for_each = local.lambda_names

  alarm_name          = "${var.name_prefix}-${each.key}-daily-invocations"
  alarm_description   = "Lambda ${each.value} invocations > 100k in a day — possible runaway."
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 1
  metric_name         = "Invocations"
  namespace           = "AWS/Lambda"
  period              = 86400
  statistic           = "Sum"
  threshold           = 100000
  treat_missing_data  = "notBreaching"

  dimensions = {
    FunctionName = each.value
  }

  alarm_actions = [aws_sns_topic.alerts.arn]
  ok_actions    = [aws_sns_topic.alerts.arn]
}

# CloudFront daily bytes-downloaded alarm — api distribution.
# CloudFront's free tier is 1 TB/mo egress. COST.md wants a warning
# at 500 GB/mo; divided over 30 days that's ~17 GB/day. Set to 20 GB
# so a single spiky day doesn't fire — only sustained pressure. On
# web the same alarm is desirable but its DistributionId reference
# walks through module.web → module.dns → cloudflare data source,
# which blocks terraform plan without CLOUDFLARE_API_TOKEN. Add the
# web-side version once the CF token wiring is settled.
resource "aws_cloudwatch_metric_alarm" "api_cloudfront_daily_bytes" {
  alarm_name          = "${var.name_prefix}-api-cloudfront-daily-bytes"
  alarm_description   = "API CloudFront distribution transferred > 20 GB in a day (~500 GB/mo pace)."
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 1
  metric_name         = "BytesDownloaded"
  namespace           = "AWS/CloudFront"
  period              = 86400
  statistic           = "Sum"
  threshold           = 21474836480 # 20 GB in bytes
  treat_missing_data  = "notBreaching"

  dimensions = {
    DistributionId = var.api_cloudfront_distribution_id
    Region         = "Global"
  }

  alarm_actions = [aws_sns_topic.alerts.arn]
  ok_actions    = [aws_sns_topic.alerts.arn]
}

# ─── Budget early-warning at 50% actual ─────────────────────────────────────
# The existing aws_budgets_budget.monthly already alerts at 80% actual +
# 100% forecast. Adding a 50% actual is overlap insurance — if the
# stack starts burning hot mid-month, we want to know before we're 80%
# in. AWS Budgets routes its own emails; this alarm uses the SNS topic
# so it lands in the same channel as everything else.
#
# We attach a separate budget here (rather than another notification on
# the root one) because aws_budgets_budget only takes email subscribers,
# not SNS. Threshold 50%, same monthly cap as the root budget — the
# limit_amount has to match else AWS treats them as independent budgets.

resource "aws_budgets_budget" "early_warning" {
  name              = "${var.name_prefix}-early-warning"
  budget_type       = "COST"
  limit_amount      = "20"
  limit_unit        = "USD"
  time_unit         = "MONTHLY"
  time_period_start = "2026-01-01_00:00"

  notification {
    comparison_operator        = "GREATER_THAN"
    threshold                  = 50
    threshold_type             = "PERCENTAGE"
    notification_type          = "ACTUAL"
    subscriber_sns_topic_arns  = [aws_sns_topic.alerts.arn]
    subscriber_email_addresses = []
  }
}

# Bucket policy so AWS Budgets can publish to the SNS topic. Without
# this, the 50% notification fires but goes nowhere.
data "aws_iam_policy_document" "sns_budget_publish" {
  statement {
    actions   = ["SNS:Publish"]
    resources = [aws_sns_topic.alerts.arn]
    principals {
      type        = "Service"
      identifiers = ["budgets.amazonaws.com"]
    }
  }
}

resource "aws_sns_topic_policy" "alerts" {
  arn    = aws_sns_topic.alerts.arn
  policy = data.aws_iam_policy_document.sns_budget_publish.json
}

# ─── Outputs ─────────────────────────────────────────────────────────────────

output "alerts_topic_arn" {
  description = "SNS topic ARN for the alerts channel — attach more alarms here as we add them."
  value       = aws_sns_topic.alerts.arn
}
