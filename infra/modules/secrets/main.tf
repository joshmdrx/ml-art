/**
 * SSM Parameter Store tree for runtime config + 3rd-party API keys.
 *
 * Path convention: /${name_prefix}/<key>
 *   e.g. /ml-art-prod/database_url
 *        /ml-art-prod/jina_api_key
 *        /ml-art-prod/clerk_secret_key
 *        /ml-art-prod/resend_api_key
 *        /ml-art-prod/mapbox_token
 *
 * Why SSM (not Secrets Manager): free tier covers our usage,
 * versioning + IAM granularity are the same, and SSM is the
 * AWS default for "config that's secret" at small scale.
 *
 * Values are populated OUT-OF-BAND (this module creates the parameter
 * containers, not the values — committing real secrets to TF state
 * defeats the purpose). After the first apply, set values with:
 *
 *   aws ssm put-parameter --name /ml-art-prod/jina_api_key \
 *     --value "actually-the-key" --type SecureString --overwrite
 *
 * The api + jobs lambdas read these by path on cold start; their
 * IAM policies (defined in those modules) grant ssm:GetParametersByPath
 * over this prefix only.
 */

variable "name_prefix" {
  description = "Resource name prefix (project + environment, e.g. ml-art-prod). Used as the SSM parameter path root."
  type        = string
}

locals {
  # Single root keeps IAM policies clean — `ssm:GetParametersByPath`
  # on this prefix covers everything below it.
  parameter_path_prefix = "/${var.name_prefix}/"

  # The list of expected parameter keys. Creating them here as
  # placeholders means `terraform apply` fails fast if a new key is
  # added but not provisioned — otherwise the Lambda silently boots
  # with a missing config and 500s.
  parameter_keys = [
    "database_url",               # Neon postgres connection string
    "jina_api_key",               # text-embedding API
    "clerk_secret_key",           # JWT verification on the API side
    "clerk_issuer",               # ditto — public but read at boot anyway
    "clerk_jwks_url",             # ditto
    "resend_api_key",             # inquiry + reply email send
    "resend_from_email",          # "From" address
    "mapbox_token",               # forward geocoding
    "web_base_url",               # for email link interpolation
    "image_base_url",             # `https://images.<domain>` — env-specific
    "uploads_public_url_prefix",  # same host; key-prefix routes to uploads bucket
  ]
}

# One SecureString parameter per key. Created with a placeholder
# value; the operator overwrites each with the real secret using
# `aws ssm put-parameter --overwrite` after the first apply.
#
# `lifecycle.ignore_changes = [value]` is the magic that makes this
# safe: subsequent `terraform apply` runs will NOT revert the
# operator-set value back to "placeholder — set out-of-band".
#
# `tier = "Standard"` keeps us on the free tier (Advanced is $0.05
# per param per month; we don't need the 8KB ceiling).
resource "aws_ssm_parameter" "config" {
  for_each = toset(local.parameter_keys)

  name        = "${local.parameter_path_prefix}${each.key}"
  description = "Runtime config — populated out-of-band. See modules/secrets/main.tf for the full list."
  type        = "SecureString"
  tier        = "Standard"
  value       = "placeholder — set out-of-band via `aws ssm put-parameter --overwrite`"

  lifecycle {
    ignore_changes = [value]
  }
}

output "parameter_path_prefix" {
  description = "SSM path prefix for IAM policies + Lambda env hints."
  value       = local.parameter_path_prefix
}
