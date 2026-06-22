/**
 * Cloudflare Email Routing DNS for the inbound-reply subdomain (T-054).
 *
 * Inbound flow:
 *
 *   inquirer replies to  r-<inquiry_id>-<hmac>@reply.wander.gallery
 *     │  MX (these records) → Cloudflare Email Routing
 *     ▼
 *   Cloudflare Email Worker (infra/email-worker/)
 *     │  fetch() POST  https://api.wander.gallery/v1/webhooks/email/inbound
 *     ▼
 *   api-search `webhooks::inbound_email`
 *
 * Why a dedicated `reply.` subdomain (not the apex): the apex/`send`
 * subdomain already carry Resend's outbound auth (resend.tf). Email
 * Routing wants to own ALL inbound MX for whatever name it's enabled on
 * — scoping it to `reply.` keeps `*@wander.gallery` free of a
 * catch-all and isolates blast radius to reply traffic.
 *
 * ── IMPORTANT: priorities are zone-assigned ──────────────────────────
 * Cloudflare generates the MX *priority* values per-zone when Email
 * Routing is enabled; they're effectively random and may participate in
 * Cloudflare's own DNS-verification check. The values below are a
 * plausible starting point only. Enabling Email Routing for the
 * subdomain is a dashboard/API step (it can't be fully expressed here —
 * see infra/POST_DEPLOY.md); after enabling, read the exact targets +
 * priorities Cloudflare shows and reconcile them here. `allow_overwrite`
 * lets TF adopt the auto-provisioned records instead of fighting them.
 *
 * SPF is stable across zones: Email Routing always authorizes
 * `_spf.mx.cloudflare.net`.
 */

# MX records for `reply.wander.gallery` are managed by Cloudflare Email
# Routing itself, not by Terraform. When Email Routing is enabled for a
# subdomain (a dashboard/API step — see POST_DEPLOY.md), Cloudflare
# auto-provisions three MX rows pointing at `route{1,2,3}.mx.cloudflare.net`
# with zone-assigned priorities, and the API refuses to create/modify
# them: `This zone is managed by Email Routing. Disable Email Routing to
# add/modify MX records. (890190)`.
#
# So we deliberately do NOT declare them here. Inspect the live values in
# the Cloudflare dashboard (or via `cloudflare_dns_record` data source if
# we ever want to assert them in tests). The SPF below is NOT locked by
# Email Routing and is safe to manage in TF.

resource "cloudflare_record" "email_routing_spf" {
  zone_id = data.cloudflare_zone.this.id
  name    = "reply"
  type    = "TXT"
  content = "v=spf1 include:_spf.mx.cloudflare.net ~all"
  ttl     = 1
  comment = "Email Routing SPF for reply.<domain> (modules/dns/email_routing.tf)"

  allow_overwrite = true
}
