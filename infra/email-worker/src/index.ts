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
    const email = await PostalMime.parse(message.raw);

    const body = stripQuotedHistory(email.text ?? "");
    // Replay-dedup key. Prefer the inbound Message-ID header; fall back
    // to a synthesised id so the webhook's NOT NULL guard is always
    // satisfied (a missing header shouldn't make the message un-stored).
    const messageId =
      message.headers.get("message-id") ??
      email.messageId ??
      `cf-${crypto.randomUUID()}@reply.wander.gallery`;

    const payload: InboundPayload = {
      to: message.to,
      from: message.from,
      message: body,
      message_id: messageId,
    };

    const res = await fetch(env.REPLY_WEBHOOK_URL, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-inbound-secret": env.INBOUND_SECRET,
      },
      body: JSON.stringify(payload),
    });

    if (!res.ok) {
      // Throwing rejects the message so Email Routing surfaces the
      // failure (and retries within its bounded policy) rather than
      // silently dropping a real reply. A 400 (bad/forged token) will
      // keep failing — acceptable: such mail isn't a legitimate reply.
      const detail = await res.text().catch(() => "");
      throw new Error(`inbound webhook ${res.status}: ${detail.slice(0, 200)}`);
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
