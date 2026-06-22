/**
 * Inbound inquiry-reply Worker (T-054).
 *
 * Triggered by Cloudflare Email Routing for mail to
 * `r-<inquiry_id>-<hmac>@reply.wander.gallery`. It:
 *
 *   1. parses the raw MIME (postal-mime) to a plain-text body;
 *   2. strips quoted history with a top-of-message heuristic so we
 *      persist just the inquirer's new text;
 *   3. POSTs `{to, from, message, message_id}` to the api-search
 *      inbound webhook, authenticated with the shared `X-Inbound-Secret`
 *      header.
 *
 * The webhook owns all trust decisions — it re-verifies the HMAC in the
 * to-address and dedupes on `message_id`. This Worker is a dumb,
 * transport-only shim: parse → forward. Keeping it logic-free is what
 * makes the handler core swappable to a different inbound transport
 * later (the webhook wouldn't change).
 */

import PostalMime from "postal-mime";

export interface Env {
  /** api-search inbound webhook URL (wrangler.toml [vars]). */
  REPLY_WEBHOOK_URL: string;
  /** Shared secret — must equal SSM /ml-art-prod/inbound_email_secret.
   *  Set via `wrangler secret put INBOUND_SECRET`. */
  INBOUND_SECRET: string;
}

interface InboundPayload {
  to: string;
  from: string;
  message: string;
  message_id: string;
}

export default {
  async email(message: ForwardableEmailMessage, env: Env): Promise<void> {
    // Self-diagnosing wrapper. `wrangler tail` in v3 doesn't reliably
    // surface exceptions thrown from the email handler, which made the
    // first round of debugging blind. We now (a) tag every stage so a
    // failure says where, (b) log a structured line via console.log
    // (which DOES reach tail), and (c) ride a diagnostic header on the
    // outbound POST so api-side CloudWatch logs can pick it up even if
    // the local tail is disconnected.
    let stage = "init";
    try {
      stage = "parse";
      const email = await PostalMime.parse(message.raw);

      stage = "extract-body";
      const body = stripQuotedHistory(email.text ?? "");

      stage = "extract-message-id";
      const messageId =
        message.headers.get("message-id") ??
        email.messageId ??
        `cf-${crypto.randomUUID()}@reply.wander.gallery`;

      stage = "post";
      const payload: InboundPayload = {
        to: message.to,
        from: message.from,
        message: body,
        message_id: messageId,
      };

      console.log(
        JSON.stringify({
          ev: "inbound-email-prePost",
          to: message.to,
          from: message.from,
          bodyLen: body.length,
          messageId,
        }),
      );

      const res = await fetch(env.REPLY_WEBHOOK_URL, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "x-inbound-secret": env.INBOUND_SECRET,
          // AWS WAF's CommonRuleSet blocks any HTTP request without a
          // User-Agent (`NoUserAgent_HEADER`). Cloudflare Workers' fetch
          // doesn't add one by default. Identify ourselves explicitly.
          "user-agent": "ml-art-inbound-email-worker/1 (Cloudflare Email Routing)",
        },
        body: JSON.stringify(payload),
      });

      if (!res.ok) {
        const detail = await res.text().catch(() => "");
        throw new Error(
          `webhook ${res.status}: ${detail.slice(0, 300)}`,
        );
      }

      console.log(
        JSON.stringify({ ev: "inbound-email-ok", to: message.to }),
      );
    } catch (err) {
      const msg = err instanceof Error ? (err.stack ?? err.message) : String(err);
      // Loud structured log — tail captures console.error, and even when
      // it doesn't, the CF Workers dashboard "Logs" tab shows it.
      console.error(
        JSON.stringify({
          ev: "inbound-email-fail",
          stage,
          to: message.to,
          from: message.from,
          err: msg.slice(0, 1500),
        }),
      );
      // Re-throw so Email Routing retries within its bounded policy.
      // Persistent failures (forged tokens, 400-class) keep failing —
      // acceptable: such mail isn't a legitimate reply.
      throw err;
    }
  },
};

/**
 * Trim quoted history from a plain-text reply. Mail clients prepend the
 * new text and append the quoted original below a recognisable marker;
 * we cut at the first marker we see. If that leaves nothing (unusual
 * layout), fall back to the full text — the webhook rejects an empty
 * message anyway, so we never make things worse by over-trimming to
 * blank.
 */
function stripQuotedHistory(text: string): string {
  const lines = text.split(/\r?\n/);
  const markers: RegExp[] = [
    /^\s*>/, // quoted line
    /^\s*On .+ wrote:\s*$/, // Gmail / Apple Mail attribution
    /^\s*-----\s*Original Message\s*-----/i, // Outlook
    /^\s*_{5,}\s*$/, // Outlook divider line
    /^\s*From:\s.+/, // forwarded header block
    /^\s*Sent from my /i, // mobile signature ahead of quote
  ];

  let cut = lines.length;
  for (let i = 0; i < lines.length; i++) {
    if (markers.some((re) => re.test(lines[i]))) {
      cut = i;
      break;
    }
  }

  const trimmed = lines.slice(0, cut).join("\n").trim();
  return trimmed.length > 0 ? trimmed : text.trim();
}
