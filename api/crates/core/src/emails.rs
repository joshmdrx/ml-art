//! T-032 — Resend HTTP client + email templates.
//!
//! Mirrors the degrades-gracefully pattern used by `Embedder`,
//! `GeocodingClient`, `ObjectStore`, `JobsBackend`:
//!
//! - `EmailClient::from_env()` — Real Resend client when
//!   `RESEND_API_KEY` is set.
//! - `EmailClient::disabled()` — Returns `Ok(())` without sending;
//!   logs at info. Used in local dev without a paid key.
//! - `EmailClient::for_tests()` — In-memory capture so integration
//!   tests can assert on subject/recipient/body.
//!
//! The handlers (see `crate::jobs::handle`) load the inquiry +
//! related rows, render via the templates in this file, then call
//! `EmailClient::send`. No HTTP server lives in this module — it's
//! pure client + template code.

use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const RESEND_URL: &str = "https://api.resend.com/emails";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct EmailClient {
    inner: Arc<Inner>,
}

enum Inner {
    Real {
        api_key: String,
        from: String,
        http: reqwest::Client,
    },
    Disabled {
        from: String,
    },
    Test {
        from: String,
        sent: Mutex<Vec<SentEmail>>,
    },
}

/// Captured by the test backend so assertions can look at what would
/// have been sent. Not part of the Real / Disabled API surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentEmail {
    pub to: String,
    pub from: String,
    pub subject: String,
    pub body_html: String,
    pub reply_to: Option<String>,
    /// Extra headers — `List-Unsubscribe` + `List-Unsubscribe-Post` go
    /// here for notification kinds. Empty for transactional sends.
    /// `Vec<(name, value)>` rather than `HashMap` so test assertions
    /// can match positionally and order is stable.
    pub headers: Vec<(String, String)>,
}

impl EmailClient {
    /// Production constructor. Reads `RESEND_API_KEY` + `RESEND_FROM_EMAIL`;
    /// falls back to `Disabled` when either is unset.
    pub fn from_env() -> Self {
        let from = std::env::var("RESEND_FROM_EMAIL").unwrap_or_default();
        match std::env::var("RESEND_API_KEY") {
            Ok(key) if !key.trim().is_empty() && !from.trim().is_empty() => Self::real(key, from),
            _ => Self::disabled(from),
        }
    }

    pub fn real(api_key: String, from: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .expect("reqwest client");
        Self {
            inner: Arc::new(Inner::Real {
                api_key,
                from,
                http,
            }),
        }
    }

    /// Constructor used in local dev when `RESEND_API_KEY` is unset.
    /// `send()` returns Ok and logs at info — same semantics the
    /// `GeocodingClient::Disabled` path uses.
    pub fn disabled(from: String) -> Self {
        Self {
            inner: Arc::new(Inner::Disabled { from }),
        }
    }

    /// Test backend. Use a recognisable from-address so the captured
    /// list is easy to assert on.
    pub fn for_tests() -> Self {
        Self {
            inner: Arc::new(Inner::Test {
                from: "test@example.com".to_string(),
                sent: Mutex::new(Vec::new()),
            }),
        }
    }

    /// `true` when this client will actually call Resend.
    pub fn enabled(&self) -> bool {
        matches!(*self.inner, Inner::Real { .. })
    }

    /// Configured sender address. Useful for templates that want to
    /// reference the platform's email (e.g. in the footer).
    pub fn from(&self) -> &str {
        match &*self.inner {
            Inner::Real { from, .. } | Inner::Disabled { from } | Inner::Test { from, .. } => from,
        }
    }

    /// Send an email. `reply_to` is the address the recipient's reply
    /// goes to — used for inquiry-delivered emails so the artist
    /// can hit reply and land in the inquirer's inbox.
    pub async fn send(
        &self,
        to: &str,
        subject: &str,
        body_html: &str,
        reply_to: Option<&str>,
    ) -> Result<(), EmailError> {
        self.send_with_headers(to, subject, body_html, reply_to, &[])
            .await
    }

    /// Notification-flavoured send. Wraps `send` with `List-Unsubscribe` +
    /// `List-Unsubscribe-Post` headers so Gmail/Outlook honour the
    /// `unsubscribe_url` for one-click (RFC 8058) AND show their
    /// built-in unsubscribe UI prominently — both improve our sender
    /// reputation. The URL also belongs in the footer copy of the
    /// HTML body so a recipient using a non-honouring client still has
    /// the option.
    pub async fn send_notification(
        &self,
        to: &str,
        subject: &str,
        body_html: &str,
        unsubscribe_url: &str,
    ) -> Result<(), EmailError> {
        let headers = vec![
            (
                "List-Unsubscribe".to_string(),
                format!("<{unsubscribe_url}>"),
            ),
            (
                "List-Unsubscribe-Post".to_string(),
                "List-Unsubscribe=One-Click".to_string(),
            ),
        ];
        self.send_with_headers(to, subject, body_html, None, &headers)
            .await
    }

    async fn send_with_headers(
        &self,
        to: &str,
        subject: &str,
        body_html: &str,
        reply_to: Option<&str>,
        headers: &[(String, String)],
    ) -> Result<(), EmailError> {
        match &*self.inner {
            Inner::Disabled { from } => {
                tracing::info!(
                    %to,
                    %from,
                    subject,
                    headers = headers.len(),
                    "email send (disabled — no RESEND_API_KEY)"
                );
                Ok(())
            }
            Inner::Test { from, sent } => {
                sent.lock().unwrap().push(SentEmail {
                    to: to.to_string(),
                    from: from.to_string(),
                    subject: subject.to_string(),
                    body_html: body_html.to_string(),
                    reply_to: reply_to.map(str::to_string),
                    headers: headers.to_vec(),
                });
                Ok(())
            }
            Inner::Real {
                api_key,
                from,
                http,
            } => {
                send_via_resend(
                    http, api_key, from, to, subject, body_html, reply_to, headers,
                )
                .await
            }
        }
    }

    /// Test-only: drain the captured list. Panics on non-test variants.
    pub fn captured(&self) -> Vec<SentEmail> {
        match &*self.inner {
            Inner::Test { sent, .. } => sent.lock().unwrap().clone(),
            _ => panic!("captured() only valid on for_tests() backend"),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn send_via_resend(
    http: &reqwest::Client,
    api_key: &str,
    from: &str,
    to: &str,
    subject: &str,
    body_html: &str,
    reply_to: Option<&str>,
    headers: &[(String, String)],
) -> Result<(), EmailError> {
    #[derive(Serialize)]
    struct Body<'a> {
        from: &'a str,
        to: Vec<&'a str>,
        subject: &'a str,
        html: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        reply_to: Option<Vec<&'a str>>,
        #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        headers: std::collections::BTreeMap<&'a str, &'a str>,
    }

    let resp = http
        .post(RESEND_URL)
        .bearer_auth(api_key)
        .json(&Body {
            from,
            to: vec![to],
            subject,
            html: body_html,
            reply_to: reply_to.map(|r| vec![r]),
            headers: headers
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect(),
        })
        .send()
        .await
        .map_err(|e| EmailError::Http(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(EmailError::Status {
            status: status.as_u16(),
            body,
        });
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum EmailError {
    #[error("resend HTTP error: {0}")]
    Http(String),
    #[error("resend returned {status}: {body}")]
    Status { status: u16, body: String },
}

// ─────────────────────────────────────────────────────────────────────────────
// Email templates
// ─────────────────────────────────────────────────────────────────────────────
//
// Inline HTML strings with hand-escaped interpolation. We don't pull
// in a template engine — these two templates are short enough that
// the strings are clearer than askama / handlebars / etc would be.
//
// Every user-supplied field goes through `escape_html`. The only
// values inserted raw are URLs (the verification link + the artwork
// detail link), which we build server-side from trusted input.

pub mod templates {
    use super::escape_html;

    /// Email sent to the inquirer asking them to confirm their email
    /// before the inquiry is delivered to the artist. Returned as
    /// `(subject, body_html)`.
    pub fn verification(
        verify_url: &str,
        inquirer_name: &str,
        artwork_title: Option<&str>,
        artist_display_name: &str,
    ) -> (String, String) {
        let title = artwork_title.unwrap_or("an artwork");
        let subject = format!("Confirm your inquiry about {title}");
        let body = format!(
            r#"<div style="font-family: -apple-system, system-ui, sans-serif; max-width: 560px; margin: 0 auto;">
  <p>Hi {name},</p>
  <p>Thanks for your interest in <strong>{title}</strong> by {artist}. To deliver your message, we just need to confirm your email.</p>
  <p style="margin: 24px 0;">
    <a href="{url}" style="display: inline-block; padding: 10px 18px; background: #111; color: #fff; text-decoration: none;">
      Confirm + send my inquiry
    </a>
  </p>
  <p style="font-size: 13px; color: #666;">If you didn't send this inquiry, you can safely ignore this email — nothing was delivered to the artist.</p>
</div>"#,
            name = escape_html(inquirer_name),
            title = escape_html(title),
            artist = escape_html(artist_display_name),
            url = verify_url, // trusted; built server-side
        );
        (subject, body)
    }

    /// Email sent to the artist when an inquiry is delivered. The
    /// inquirer's email goes in `reply_to` so the artist can just
    /// hit reply — they don't need to copy the address out of the
    /// body. Returns `(subject, body_html)`.
    #[allow(clippy::too_many_arguments)]
    pub fn delivered_to_artist(
        artwork_url: &str,
        artwork_title: Option<&str>,
        artwork_image_url: Option<&str>,
        inquirer_name: &str,
        inquirer_email: &str,
        message: &str,
        budget: Option<&str>,
    ) -> (String, String) {
        let title = artwork_title.unwrap_or("your artwork");
        let subject = format!("New inquiry about {title}");
        let thumb = match artwork_image_url {
            Some(u) => format!(
                r#"<img src="{u}" alt="" style="width: 100%; max-width: 360px; display: block; margin-bottom: 16px;" />"#,
                u = u
            ),
            None => String::new(),
        };
        let budget_line = match budget {
            Some(b) if !b.trim().is_empty() => format!(
                r#"<p style="font-size: 14px; color: #444;"><strong>Budget:</strong> {b}</p>"#,
                b = escape_html(b)
            ),
            _ => String::new(),
        };
        let body = format!(
            r#"<div style="font-family: -apple-system, system-ui, sans-serif; max-width: 560px; margin: 0 auto;">
  {thumb}
  <p style="font-size: 14px; color: #444; margin: 0 0 4px;">New inquiry about <a href="{url}" style="color: #111;">{title}</a></p>
  <p style="font-size: 16px; margin: 16px 0 4px;"><strong>{name}</strong> &lt;<a href="mailto:{email}">{email}</a>&gt;</p>
  {budget_line}
  <blockquote style="border-left: 3px solid #ddd; padding-left: 12px; color: #333; margin: 16px 0;">{msg}</blockquote>
  <p style="font-size: 13px; color: #666;">Hit reply to respond — your reply goes straight to the buyer.</p>
</div>"#,
            url = artwork_url,
            title = escape_html(title),
            name = escape_html(inquirer_name),
            email = escape_html(inquirer_email),
            budget_line = budget_line,
            msg = escape_html(message).replace('\n', "<br />"),
            thumb = thumb,
        );
        (subject, body)
    }

    /// Email sent to the inquirer when an artist replies from the
    /// studio inbox. Same look as `delivered_to_artist` but with
    /// the message flowing the other way. `reply_to` should be set
    /// to the artist's address so a further reply lands in their
    /// inbox; if a reply ever comes back via a future inbound-email
    /// webhook we'd thread it onto the same `inquiry_replies` row.
    /// Returns `(subject, body_html)`. T-011 Phase 4b.
    pub fn artist_reply(
        artwork_url: &str,
        artwork_title: Option<&str>,
        artist_display_name: &str,
        inquirer_name: &str,
        message: &str,
    ) -> (String, String) {
        let title = artwork_title.unwrap_or("your inquiry");
        let subject = format!("Reply from {artist_display_name} about {title}");
        let body = format!(
            r#"<div style="font-family: -apple-system, system-ui, sans-serif; max-width: 560px; margin: 0 auto;">
  <p>Hi {name},</p>
  <p style="font-size: 14px; color: #444; margin: 0 0 4px;">
    <strong>{artist}</strong> replied to your inquiry about
    <a href="{url}" style="color: #111;">{title}</a>:
  </p>
  <blockquote style="border-left: 3px solid #ddd; padding-left: 12px; color: #333; margin: 16px 0;">{msg}</blockquote>
  <p style="font-size: 13px; color: #666;">Hit reply to keep the conversation going — your reply goes straight back to {artist}.</p>
</div>"#,
            url = artwork_url,
            title = escape_html(title),
            artist = escape_html(artist_display_name),
            name = escape_html(inquirer_name),
            msg = escape_html(message).replace('\n', "<br />"),
        );
        (subject, body)
    }

    /// T-054 — email sent to the artist when the *inquirer* replies to a
    /// thread (their reply arrived via the inbound-email webhook). The
    /// mirror of `artist_reply`: same look, message flowing inquirer →
    /// artist. `reply_to` is set by the handler to the inquirer's real
    /// address so the artist can respond from the studio inbox or by
    /// hitting reply. Returns `(subject, body_html)`.
    pub fn inquirer_reply_forward(
        artwork_url: &str,
        artwork_title: Option<&str>,
        inquirer_name: &str,
        message: &str,
    ) -> (String, String) {
        let title = artwork_title.unwrap_or("your artwork");
        let subject = format!("New reply from {inquirer_name} about {title}");
        let body = format!(
            r#"<div style="font-family: -apple-system, system-ui, sans-serif; max-width: 560px; margin: 0 auto;">
  <p style="font-size: 14px; color: #444; margin: 0 0 4px;">
    <strong>{name}</strong> replied to their inquiry about
    <a href="{url}" style="color: #111;">{title}</a>:
  </p>
  <blockquote style="border-left: 3px solid #ddd; padding-left: 12px; color: #333; margin: 16px 0;">{msg}</blockquote>
  <p style="font-size: 13px; color: #666;">Reply from your studio inbox, or hit reply to respond to {name} directly.</p>
</div>"#,
            url = artwork_url,
            title = escape_html(title),
            name = escape_html(inquirer_name),
            msg = escape_html(message).replace('\n', "<br />"),
        );
        (subject, body)
    }

    /// T-052b — daily digest of new works from the artists this user
    /// follows. `groups` is already ordered the way it'll appear in
    /// the email (most-recently-published artist first, works within
    /// each artist also recency-first).
    ///
    /// `unsubscribe_url` is included in the body as a visible link
    /// AND fed into `EmailClient::send_notification` for the
    /// `List-Unsubscribe` headers — the body link is a fallback for
    /// mail clients that don't honour the header.
    pub fn new_works_digest(
        groups: &[DigestArtistGroup<'_>],
        manage_prefs_url: &str,
        unsubscribe_url: &str,
    ) -> (String, String) {
        let total_works: usize = groups.iter().map(|g| g.works.len()).sum();
        let subject = if groups.len() == 1 {
            let g = &groups[0];
            if g.works.len() == 1 {
                format!("1 new work from {}", g.artist_display_name)
            } else {
                format!("{} new works from {}", g.works.len(), g.artist_display_name)
            }
        } else {
            format!("{total_works} new works from artists you follow")
        };

        let mut sections = String::new();
        for g in groups {
            let artist_label = escape_html(g.artist_display_name);
            sections.push_str(&format!(
                r#"<div style="margin: 28px 0 8px;"><a href="{artist_url}" style="font-weight: 600; color: #1A1A1A; text-decoration: none;">{artist}</a></div>"#,
                artist_url = g.artist_url, // trusted; built server-side
                artist = artist_label,
            ));
            for w in &g.works {
                let title = escape_html(w.title.unwrap_or("Untitled"));
                let img_html = match w.image_url {
                    Some(url) => format!(
                        r#"<img src="{url}" alt="" width="120" height="120" style="display:block;border:0;outline:none;text-decoration:none;width:120px;height:120px;object-fit:cover;background:#1A1A1A;" />"#,
                        url = url,
                    ),
                    None => String::from(
                        r#"<div style="width:120px;height:120px;background:#1A1A1A;"></div>"#,
                    ),
                };
                sections.push_str(&format!(
                    r#"<a href="{url}" style="display:block;margin:10px 0;text-decoration:none;color:#1A1A1A;">
  <table cellpadding="0" cellspacing="0" border="0" style="border-collapse:collapse;">
    <tr>
      <td style="padding-right:14px;vertical-align:top;">{img}</td>
      <td style="vertical-align:top;padding-top:2px;font-family:-apple-system,system-ui,sans-serif;">
        <div style="font-style:italic;font-size:16px;color:#1A1A1A;">{title}</div>
      </td>
    </tr>
  </table>
</a>"#,
                    url = w.url,
                    img = img_html,
                    title = title,
                ));
            }
        }

        let body = format!(
            r#"<div style="font-family: -apple-system, system-ui, sans-serif; max-width: 560px; margin: 0 auto; color: #1A1A1A;">
  <p style="font-size: 15px; line-height: 1.5;">New work from artists you follow on Wander.</p>
  {sections}
  <hr style="border: none; border-top: 1px solid #E5E5E3; margin: 36px 0 20px;" />
  <p style="font-size: 12px; color: #6B6B6B; line-height: 1.6;">
    You're getting this because you follow these artists on Wander.
    <a href="{manage}" style="color: #6B6B6B; text-decoration: underline;">Manage email preferences</a>
    · <a href="{unsub}" style="color: #6B6B6B; text-decoration: underline;">Unsubscribe from new-work digests</a>
  </p>
</div>"#,
            sections = sections,
            manage = manage_prefs_url, // trusted
            unsub = unsubscribe_url,   // trusted (HMAC-signed token)
        );
        (subject, body)
    }

    /// One artist's worth of new artworks for the digest template.
    /// Lifetime parameter lets handlers pass borrowed strings without
    /// cloning every row.
    #[derive(Debug, Clone)]
    pub struct DigestArtistGroup<'a> {
        pub artist_display_name: &'a str,
        pub artist_url: &'a str,
        pub works: Vec<DigestWork<'a>>,
    }

    #[derive(Debug, Clone)]
    pub struct DigestWork<'a> {
        pub title: Option<&'a str>,
        pub url: &'a str,
        pub image_url: Option<&'a str>,
    }
}

/// HTML-escape user-supplied text. Same five-substitution table the
/// other templates in this crate use.
pub(crate) fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_client_no_ops() {
        let c = EmailClient::disabled("noreply@example.com".to_string());
        assert!(!c.enabled());
        c.send("to@example.com", "hi", "<p>hi</p>", None)
            .await
            .unwrap();
        // Nothing observable — no panic, ok.
    }

    #[tokio::test]
    async fn for_tests_captures_outgoing() {
        let c = EmailClient::for_tests();
        c.send(
            "artist@example.com",
            "New inquiry",
            "<p>Hello</p>",
            Some("buyer@example.com"),
        )
        .await
        .unwrap();
        let sent = c.captured();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "artist@example.com");
        assert_eq!(sent[0].subject, "New inquiry");
        assert_eq!(sent[0].reply_to.as_deref(), Some("buyer@example.com"));
    }

    #[test]
    fn verification_template_includes_link() {
        let (subject, body) = templates::verification(
            "https://ml-art.example/inquiries/verify/abc",
            "Jane Doe",
            Some("Blue Morning"),
            "Alice Test",
        );
        assert!(subject.contains("Blue Morning"));
        assert!(body.contains("https://ml-art.example/inquiries/verify/abc"));
        assert!(body.contains("Jane Doe"));
        assert!(body.contains("Alice Test"));
    }

    #[test]
    fn delivered_template_escapes_user_input() {
        let (_, body) = templates::delivered_to_artist(
            "https://ml-art.example/artworks/123",
            Some("Blue Morning"),
            None,
            "Jane <script>alert(1)</script>",
            "buyer@example.com",
            "I'd like to <buy> this",
            Some("£500-1k"),
        );
        // Script tag must not survive intact — we escape `<` and `>`.
        assert!(!body.contains("<script>"));
        assert!(body.contains("&lt;script&gt;"));
        // Budget shows up.
        assert!(body.contains("£500-1k"));
        // Buyer email appears as a reply mailto.
        assert!(body.contains("mailto:buyer@example.com"));
    }

    #[test]
    fn delivered_template_omits_thumb_when_no_image() {
        let (_, body) = templates::delivered_to_artist(
            "https://ml-art.example/artworks/123",
            Some("Untitled"),
            None,
            "Jane",
            "j@example.com",
            "hi",
            None,
        );
        assert!(!body.contains("<img"));
    }

    #[test]
    fn delivered_template_message_newlines_become_breaks() {
        let (_, body) = templates::delivered_to_artist(
            "https://ml-art.example/artworks/123",
            None,
            None,
            "Jane",
            "j@example.com",
            "line 1\nline 2",
            None,
        );
        assert!(body.contains("line 1<br />line 2"));
    }
}
