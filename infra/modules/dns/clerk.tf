/**
 * Clerk (auth) DNS records.
 *
 * Clerk hosts the frontend API + account portal + transactional email
 * for our `wander.gallery` instance. Five CNAMEs in our Cloudflare
 * zone wire it all up:
 *
 *   accounts        → accounts.clerk.services                        (account portal: sign-in / sign-up UI)
 *   clerk           → frontend-api.clerk.services                    (JWT issuer + JWKS — the api-search lambda fetches from here)
 *   clk._domainkey  → dkim1.kohxudqsmzte.clerk.services              (DKIM key 1 for outbound email)
 *   clk2._domainkey → dkim2.kohxudqsmzte.clerk.services              (DKIM key 2 — Clerk rotates)
 *   clkmail         → mail.kohxudqsmzte.clerk.services               (Mail-From subdomain for SPF alignment)
 *
 * `proxied = false` on every record is critical:
 *   - DKIM / Mail-From break if proxied (Cloudflare would terminate
 *     TLS + add headers, busting the DKIM signature).
 *   - `clerk.` + `accounts.` need to reach Clerk directly so they
 *     can serve their own TLS + handle the auth flows.
 *
 * After applying, click "Verify" on the Clerk dashboard's DNS page.
 * Propagation through Cloudflare is usually <1 min.
 *
 * Values are tenant-specific (the `kohxudqsmzte` substring is our
 * Clerk instance id). If you migrate to a different Clerk
 * application, regenerate these from that dashboard's DNS page.
 */

locals {
  clerk_records = {
    "accounts"        = "accounts.clerk.services"
    "clerk"           = "frontend-api.clerk.services"
    "clk._domainkey"  = "dkim1.kohxudqsmzte.clerk.services"
    "clk2._domainkey" = "dkim2.kohxudqsmzte.clerk.services"
    "clkmail"         = "mail.kohxudqsmzte.clerk.services"
  }
}

resource "cloudflare_record" "clerk" {
  for_each = local.clerk_records

  zone_id = data.cloudflare_zone.this.id
  name    = each.key
  type    = "CNAME"
  content = each.value
  ttl     = 1 # Auto — Cloudflare picks a short TTL by default
  proxied = false
  comment = "Clerk auth — ${each.key} (modules/dns/clerk.tf)"
}
