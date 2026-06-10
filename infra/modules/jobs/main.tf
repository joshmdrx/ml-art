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

  # Placeholder runtime + handler. When the real Rust artifact lands,
  # we'll flip runtime → "provided.al2023" + handler → "bootstrap"
  # in a follow-up commit (one-time change; CI deploys after that point
  # only swap code, not config).
  runtime     = "nodejs20.x"
  handler     = "index.handler"
  memory_size = var.lambda_memory_mb
  timeout     = var.lambda_timeout_s

  filename         = data.archive_file.placeholder.output_path
  source_code_hash = data.archive_file.placeholder.output_base64sha256

  environment {
    variables = {
      CONFIG_PARAMETER_PATH = var.config_parameter_path
      RUST_LOG              = "info"
    }
  }

  depends_on = [aws_cloudwatch_log_group.jobs]

  # CI replaces the code on every deploy. Without this, every `terraform
  # apply` would revert whatever CI shipped back to the placeholder.
  lifecycle {
    ignore_changes = [
      filename,
      source_code_hash,
      environment, # CI may set extra env vars; let it
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

output "lambda_function_name" {
  description = "Jobs Lambda function name — CI uses this for `aws lambda update-function-code` on deploy."
  value       = aws_lambda_function.jobs.function_name
}
