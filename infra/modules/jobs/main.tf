/**
 * Background-jobs runtime: SQS queue + jobs-lambda + DLQ + IAM.
 *
 * Shape mirrors `core::jobs::JobsBackend::Postgres` in the local
 * stack — same `JobEvent` enum on the wire, same dispatch code path
 * (the difference is one driver pulls from `jobs` table polling,
 * the other gets handed an `SQSEvent` by Lambda). See
 * decisions.md 2026-05-29 — "Jobs queue: Postgres local, SQS+Lambda prod"
 * for the architectural rationale.
 *
 * Lambda is event-source-mapped to the queue with a small batch size.
 * Concurrency cap protects downstream services (Resend, Rekognition)
 * from a thundering herd if the queue ever backs up.
 */

variable "name_prefix" {
  description = "Resource name prefix."
  type        = string
}

variable "lambda_memory_mb" {
  description = "Lambda memory size."
  type        = number
}

variable "lambda_timeout_s" {
  description = "Lambda per-event timeout."
  type        = number
}

variable "queue_visibility_timeout_s" {
  description = "SQS visibility timeout. Must be >= 6x lambda_timeout_s per AWS recommendation."
  type        = number
}

variable "max_receive_count" {
  description = "Retries before a message lands in the DLQ."
  type        = number
}

variable "uploads_bucket_arn" {
  description = "S3 bucket ARN — needed for the Rekognition / moderation handlers' IAM grants."
  type        = string
}

variable "artworks_bucket_arn" {
  description = "S3 bucket ARN — needed by Rekognition for content reads."
  type        = string
}

variable "config_parameter_path" {
  description = "SSM path prefix this lambda reads its config from."
  type        = string
}

# ─── SQS queues ──────────────────────────────────────────────────────────────

resource "aws_sqs_queue" "dlq" {
  name                      = "${var.name_prefix}-jobs-dlq"
  message_retention_seconds = 1209600 # 14 days — max — gives us time to inspect failed jobs
}

resource "aws_sqs_queue" "main" {
  name                       = "${var.name_prefix}-jobs"
  visibility_timeout_seconds = var.queue_visibility_timeout_s
  message_retention_seconds  = 345600 # 4 days

  redrive_policy = jsonencode({
    deadLetterTargetArn = aws_sqs_queue.dlq.arn
    maxReceiveCount     = var.max_receive_count
  })
}

# ─── Placeholder Lambda payload ──────────────────────────────────────────────
# A trivial Node 20 function that just logs the incoming event. CI
# replaces the code with the real jobs-lambda artifact via
# `aws lambda update-function-code`. The lifecycle{} on the function
# below tells TF to ignore code drift on subsequent applies.

data "archive_file" "placeholder" {
  type        = "zip"
  output_path = "${path.module}/.terraform/placeholder.zip"

  source {
    filename = "index.mjs"
    content  = <<-EOT
      // Placeholder jobs lambda. Replaced by CI on first deploy.
      export const handler = async (event) => {
        console.log("jobs-lambda placeholder; event:", JSON.stringify(event));
        return { statusCode: 200, body: "placeholder" };
      };
    EOT
  }
}

# ─── Log group ───────────────────────────────────────────────────────────────
# Create the log group up-front (instead of letting Lambda lazy-create
# it) so we can set retention. Default retention is "never expire"
# which adds up over time.

resource "aws_cloudwatch_log_group" "jobs" {
  name              = "/aws/lambda/${var.name_prefix}-jobs"
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

resource "aws_iam_role" "jobs_lambda" {
  name               = "${var.name_prefix}-jobs-lambda"
  assume_role_policy = data.aws_iam_policy_document.lambda_assume.json
}

data "aws_iam_policy_document" "jobs_lambda" {
  # Log writes — explicit rather than via the AWS-managed
  # LambdaBasicExecutionRole so the policy is auditable in one place.
  statement {
    sid       = "Logs"
    actions   = ["logs:CreateLogStream", "logs:PutLogEvents"]
    resources = ["${aws_cloudwatch_log_group.jobs.arn}:*"]
  }

  # SQS — the event source mapping uses these. ChangeMessageVisibility
  # is needed for partial-batch responses.
  statement {
    sid = "SqsConsume"
    actions = [
      "sqs:ReceiveMessage",
      "sqs:DeleteMessage",
      "sqs:GetQueueAttributes",
      "sqs:ChangeMessageVisibility",
    ]
    resources = [aws_sqs_queue.main.arn]
  }

  # T-052b — the kickoff handler enqueues per-user digest jobs back to
  # the same queue, so this Lambda is also a producer.
  statement {
    sid       = "SqsProduce"
    actions   = ["sqs:SendMessage"]
    resources = [aws_sqs_queue.main.arn]
  }

  # SSM — read the runtime config tree on cold start.
  statement {
    sid     = "SsmRead"
    actions = ["ssm:GetParametersByPath", "ssm:GetParameter", "ssm:GetParameters"]
    resources = [
      "arn:aws:ssm:*:*:parameter${trimsuffix(var.config_parameter_path, "/")}",
      "arn:aws:ssm:*:*:parameter${var.config_parameter_path}*",
    ]
  }

  # S3 — moderation reads images, occasional clean-up handlers write.
  statement {
    sid       = "S3Read"
    actions   = ["s3:GetObject", "s3:HeadObject"]
    resources = ["${var.artworks_bucket_arn}/*", "${var.uploads_bucket_arn}/*"]
  }

  statement {
    sid       = "S3Write"
    actions   = ["s3:PutObject", "s3:DeleteObject"]
    resources = ["${var.artworks_bucket_arn}/*", "${var.uploads_bucket_arn}/*"]
  }

  # Rekognition — image moderation handler (T-008).
  statement {
    sid       = "Rekognition"
    actions   = ["rekognition:DetectModerationLabels"]
    resources = ["*"] # Rekognition doesn't support resource-level perms here
  }
}

resource "aws_iam_role_policy" "jobs_lambda" {
  name   = "${var.name_prefix}-jobs-lambda"
  role   = aws_iam_role.jobs_lambda.id
  policy = data.aws_iam_policy_document.jobs_lambda.json
}

# ─── Lambda ──────────────────────────────────────────────────────────────────

resource "aws_lambda_function" "jobs" {
  function_name = "${var.name_prefix}-jobs"
  role          = aws_iam_role.jobs_lambda.arn

  # Rust binary via cargo-lambda. `provided.al2023` is the
  # bring-your-own-binary runtime; the handler name `bootstrap` is the
  # required filename for the executable inside the zip.
  runtime       = "provided.al2023"
  handler       = "bootstrap"
  architectures = ["arm64"] # matches `cargo lambda build --arm64` in deploy-jobs.sh
  memory_size   = var.lambda_memory_mb
  timeout       = var.lambda_timeout_s

  filename         = data.archive_file.placeholder.output_path
  source_code_hash = data.archive_file.placeholder.output_base64sha256

  environment {
    variables = {
      CONFIG_PARAMETER_PATH = var.config_parameter_path
      RUST_LOG              = "info"
      ML_ART_ENV            = "prod"
      # T-052b — the kickoff handler enqueues per-user digest jobs back
      # to the same SQS queue this Lambda consumes. So jobs is both
      # producer + consumer; it needs the queue URL to send.
      JOBS_QUEUE_URL = aws_sqs_queue.main.url
      # Public/static config — see modules/api/main.tf for the same
      # rationale (was SecureString in SSM, now free in TF env).
      CLERK_ISSUER              = "https://clerk.wander.gallery"
      CLERK_JWKS_URL            = "https://clerk.wander.gallery/.well-known/jwks.json"
      WEB_BASE_URL              = "https://wander.gallery"
      IMAGE_BASE_URL            = "https://images.wander.gallery"
      UPLOADS_PUBLIC_URL_PREFIX = "https://images.wander.gallery"
      RESEND_FROM_EMAIL         = "info@wander.gallery"
    }
  }

  depends_on = [aws_cloudwatch_log_group.jobs]

  lifecycle {
    # `environment` is owned by TF — runtime secrets layer on top
    # via core::config::bootstrap_ssm. See modules/api comment.
    ignore_changes = [
      filename,
      source_code_hash,
    ]
  }
}

# ─── Event source mapping ────────────────────────────────────────────────────
# Lambda polls SQS and invokes the function for each batch. Small
# batches + bounded concurrency = the queue can absorb spikes without
# stampeding Resend / Rekognition.

resource "aws_lambda_event_source_mapping" "sqs_to_jobs" {
  event_source_arn = aws_sqs_queue.main.arn
  function_name    = aws_lambda_function.jobs.arn
  batch_size       = 5

  scaling_config {
    maximum_concurrency = 10
  }

  function_response_types = ["ReportBatchItemFailures"]
}

# ─── T-052b: daily new-works digest cron ─────────────────────────────────────
#
# EventBridge fires at 11:00 UTC daily, drops a fixed
# `NotifyFollowersDigestKickoff` JobEvent into the same SQS queue the
# Lambda already consumes. The kickoff handler in core::jobs scans for
# users with new work from followed artists and fans out one
# `NotifyFollowersDigestUser` per qualifying user — same queue, same
# Lambda, no separate infrastructure for the producer.
#
# A separate `aws_sqs_queue_policy` grants events.amazonaws.com the
# SendMessage permission scoped to this specific event rule's ARN.

resource "aws_cloudwatch_event_rule" "new_works_digest_kickoff" {
  name                = "${var.name_prefix}-new-works-digest-kickoff"
  description         = "T-052b — daily kickoff for the new-works digest."
  schedule_expression = "cron(0 11 * * ? *)" # 11:00 UTC daily
}

resource "aws_cloudwatch_event_target" "new_works_digest_kickoff_sqs" {
  rule      = aws_cloudwatch_event_rule.new_works_digest_kickoff.name
  target_id = "jobs-queue"
  arn       = aws_sqs_queue.main.arn
  # Matches the `#[serde(tag = "kind", content = "payload")]` shape on
  # `JobEvent` — the Lambda's SQS handler deserialises this verbatim.
  input = jsonencode({
    kind    = "notify_followers_digest_kickoff"
    payload = {}
  })
}

resource "aws_sqs_queue_policy" "events_send_to_jobs" {
  queue_url = aws_sqs_queue.main.url
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Sid    = "AllowEventBridgeKickoff"
      Effect = "Allow"
      Principal = {
        Service = "events.amazonaws.com"
      }
      Action   = "sqs:SendMessage"
      Resource = aws_sqs_queue.main.arn
      Condition = {
        ArnEquals = {
          "aws:SourceArn" = aws_cloudwatch_event_rule.new_works_digest_kickoff.arn
        }
      }
    }]
  })
}

# ─── Outputs ─────────────────────────────────────────────────────────────────

output "queue_arn" {
  description = "Main SQS queue ARN — the api-search Lambda grants itself sqs:SendMessage on this."
  value       = aws_sqs_queue.main.arn
}

output "queue_url" {
  description = "Main SQS queue URL — the api binary uses this as the JobsBackend::Sqs endpoint."
  value       = aws_sqs_queue.main.url
}

output "dlq_arn" {
  description = "DLQ ARN — alarms wire to this so we know when jobs are failing repeatedly."
  value       = aws_sqs_queue.dlq.arn
}

output "dlq_name" {
  description = "DLQ queue name (the SQS CloudWatch metric dimension wants the name, not ARN)."
  value       = aws_sqs_queue.dlq.name
}

output "lambda_function_name" {
  description = "Jobs Lambda function name — CI uses this for `aws lambda update-function-code` on deploy."
  value       = aws_lambda_function.jobs.function_name
}
