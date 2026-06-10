/**
 * Terraform + provider version pins, and the remote state backend.
 *
 * State is stored in S3 + DynamoDB locking. The bucket + table need
 * to exist BEFORE `terraform init` will work — see infra/README.md
 * for the one-time bootstrap commands.
 *
 * Bucket name is hardcoded here (not a variable) because the backend
 * block doesn't accept variables — chicken-and-egg with state. If you
 * fork this for a different project, edit it once here.
 */

terraform {
  required_version = ">= 1.5.0"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.70"
    }
    random = {
      source  = "hashicorp/random"
      version = "~> 3.6"
    }
    # `archive` zips the placeholder lambda payloads at plan time so
    # we never commit pre-built zip files. CI replaces the code via
    # `aws lambda update-function-code` after the infra is up.
    archive = {
      source  = "hashicorp/archive"
      version = "~> 2.4"
    }
    # `cloudflare` because the domain is registered with Cloudflare
    # Registrar, which mandates Cloudflare's nameservers — so DNS
    # records live in Cloudflare rather than Route53. ACM certs still
    # live in AWS (us-east-1); CloudFront still consumes them; only
    # the DNS records change provider.
    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "~> 4.40"
    }
  }

  backend "s3" {
    bucket         = "ml-art-tfstate"
    key            = "infra/terraform.tfstate"
    region         = "us-east-1"
    dynamodb_table = "ml-art-tfstate-lock"
    encrypt        = true
  }
}

provider "aws" {
  region = var.aws_region

  default_tags {
    tags = {
      Project     = var.project_name
      Environment = var.environment
      ManagedBy   = "terraform"
    }
  }
}

# Some AWS resources (CloudFront, ACM certs for CloudFront) must live
# in us-east-1 regardless of the primary region. A second provider
# aliased to that region covers them without forcing the whole stack
# into us-east-1.
provider "aws" {
  alias  = "us_east_1"
  region = "us-east-1"

  default_tags {
    tags = {
      Project     = var.project_name
      Environment = var.environment
      ManagedBy   = "terraform"
    }
  }
}

# Cloudflare provider — auth via the CLOUDFLARE_API_TOKEN env var
# (NOT a tfvar). The token needs Zone:Read + DNS:Edit on
# wander.gallery only. Token is regenerated periodically per the
# usual hygiene; if you rotate it, just export the new one — no TF
# change needed.
provider "cloudflare" {}
