/**
 * Resend (transactional email) DNS records.
 *
 * Four records authenticate `wander.gallery` to Resend, who relays
 * through AWS SES under the hood:
 *
 *   resend._domainkey  → DKIM public key (TXT)        — signs outbound mail
 *   send               → MX to SES feedback (priority 10) — bounce processing
 *   send               → SPF (TXT)                    — authorizes SES to send
 *   _dmarc             → DMARC policy (TXT)          — start permissive (p=none) and
 *                                                       tighten as confidence builds
 *
 * `proxied` is not applicable to TXT/MX records (Cloudflare only
 * proxies A/AAAA/CNAME); these are always DNS-only by nature.
 *
 * After applying, click "Verify DNS records" on the Resend domains
 * page. Cloudflare propagation is <1 min; Resend's check runs on
 * demand.
 *
 * The DKIM value is the public key half of a keypair Resend generated;
 * the private half lives inside Resend and is rotated by their ops.
 * If we ever migrate off Resend, this entire file gets removed +
 * re-issued from the new provider.
 */

resource "cloudflare_record" "resend_dkim" {
  zone_id = data.cloudflare_zone.this.id
  name    = "resend._domainkey"
  type    = "TXT"
  content = "p=MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQC/vnDhBHHIpvXBzjohJGLb8xSNOs5KVjbyFHOZMlfFIKVlN2HlU4SexZJfflMGy+8s43z9qIh8bnHcjjIouavQ78ET8XmIBFZFLxJUdP2DZxwunpVdBcQOgdvQA9d9wnYh5/o4HuytH+bQwtT3oGl/nA4hU9MSaztwJuy6SfZXaQIDAQAB"
  ttl     = 1
  comment = "Resend DKIM (modules/dns/resend.tf)"
}

resource "cloudflare_record" "resend_mx" {
  zone_id  = data.cloudflare_zone.this.id
  name     = "send"
  type     = "MX"
  content  = "feedback-smtp.us-east-1.amazonses.com"
  priority = 10
  ttl      = 1
  comment  = "Resend SES bounce feedback (modules/dns/resend.tf)"
}

resource "cloudflare_record" "resend_spf" {
  zone_id = data.cloudflare_zone.this.id
  name    = "send"
  type    = "TXT"
  content = "v=spf1 include:amazonses.com ~all"
  ttl     = 1
  comment = "Resend SPF — authorizes SES to send for send.<domain> (modules/dns/resend.tf)"
}

resource "cloudflare_record" "resend_dmarc" {
  zone_id = data.cloudflare_zone.this.id
  name    = "_dmarc"
  type    = "TXT"
  content = "v=DMARC1; p=none;"
  ttl     = 1
  comment = "DMARC — monitor-only at v1; tighten to p=quarantine once aligned (modules/dns/resend.tf)"
}
