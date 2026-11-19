#!/usr/bin/env bash
# One-time Stripe wiring for the prod API (M-03).
#
# Creates the two webhook endpoints the marketplace needs, both pointing at
# https://api.wander.gallery/v1/webhooks/stripe:
#   - platform endpoint: checkout.session.completed / .expired (T-090
#     releases a buyer's hold on a copy), charge.updated (late-
#     arriving Stripe fee), charge.refunded,
#     charge.dispute.created
#   - Connect endpoint (connect=true): account.updated — events on
#     connected accounts are ONLY delivered to Connect endpoints, and this is
#     what flips an artist to sellable after onboarding.
# Stores the Stripe secret key and both signing secrets in SSM
# (STRIPE_WEBHOOK_SECRET = "whsec_platform,whsec_connect"), then bounces the
# api Lambda so its next cold start loads them.
#
# Run this yourself: it creates persistent Stripe config and moves secrets
# into prod. No secret is ever printed.
#
#   scripts/stripe-webhooks-setup.sh                 # key from api/.env
#   STRIPE_KEY_SOURCE=path/to/.env scripts/stripe-webhooks-setup.sh
#
# Re-running creates *additional* endpoints — delete stale ones in the
# Stripe dashboard (Developers → Webhooks) first.

set -euo pipefail

PROFILE="${AWS_PROFILE_NAME:-ml-art}"
REGION="us-east-1"
SSM_PREFIX="/ml-art-prod"
URL="https://api.wander.gallery/v1/webhooks/stripe"
KEY_SOURCE="${STRIPE_KEY_SOURCE:-api/.env}"

command -v stripe >/dev/null || { echo "✘ stripe CLI not installed (brew install stripe/stripe-cli/stripe)"; exit 1; }

KEY="$(grep -E '^STRIPE_SECRET_KEY=' "$KEY_SOURCE" | cut -d= -f2- | tr -d '"'"'")"
case "$KEY" in
  sk_test_*) MODE=test ;;
  sk_live_*)
    MODE=live
    read -r -p "This is a LIVE key — real money. Continue? [y/N] " ans
    [ "$ans" = "y" ] || { echo "aborted"; exit 1; }
    ;;
  *) echo "✘ no sk_test_/sk_live_ STRIPE_SECRET_KEY found in $KEY_SOURCE"; exit 1 ;;
esac
export STRIPE_API_KEY="$KEY"

# Create an endpoint and print only its signing secret (captured, not echoed).
create_endpoint() {
  stripe webhook_endpoints create --url "$URL" -d "description=wander prod ($MODE)" "$@" \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["secret"])'
}

echo "▶ creating platform webhook endpoint ($MODE)"
PLATFORM_SECRET="$(create_endpoint \
  -d "enabled_events[]=checkout.session.completed" \
  -d "enabled_events[]=checkout.session.expired" \
  -d "enabled_events[]=charge.updated" \
  -d "enabled_events[]=charge.refunded" \
  -d "enabled_events[]=charge.dispute.created")"

echo "▶ creating Connect webhook endpoint ($MODE)"
CONNECT_SECRET="$(create_endpoint -d "connect=true" -d "enabled_events[]=account.updated")"

case "$PLATFORM_SECRET$CONNECT_SECRET" in
  whsec_*whsec_*) ;;
  *) echo "✘ did not get two whsec_ secrets back from Stripe"; exit 1 ;;
esac

echo "▶ writing secrets to SSM ($SSM_PREFIX)"
aws ssm put-parameter --profile "$PROFILE" --region "$REGION" \
  --name "$SSM_PREFIX/stripe_secret_key" --type SecureString --overwrite \
  --value "$KEY" >/dev/null
aws ssm put-parameter --profile "$PROFILE" --region "$REGION" \
  --name "$SSM_PREFIX/stripe_webhook_secret" --type SecureString --overwrite \
  --value "$PLATFORM_SECRET,$CONNECT_SECRET" >/dev/null

echo "▶ bouncing api Lambda so it reloads SSM on next cold start"
aws lambda update-function-configuration --profile "$PROFILE" --region "$REGION" \
  --function-name ml-art-prod-api \
  --description "stripe ($MODE) wired $(date -u +%Y-%m-%dT%H:%M:%SZ)" >/dev/null

echo "✔ Stripe ($MODE) wired: 2 webhook endpoints, key + secrets in SSM, api bounced."
