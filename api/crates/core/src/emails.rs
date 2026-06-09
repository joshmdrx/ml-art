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
        match &*self.inner {
            Inner::Disabled { from } => {
                tracing::info!(
                    %to,
                    %from,
                    subject,
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
                });
                Ok(())
            }
            Inner::Real {
                api_key,
                from,
                http,
            } => send_via_resend(http, api_key, from, to, subject, body_html, reply_to).await,
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

async fn send_via_resend(
    http: &reqwest::Client,
    api_key: &str,
    from: &str,
    to: &str,
    subject: &str,
    body_html: &str,
    reply_to: Option<&str>,
) -> Result<(), EmailError> {
    #[derive(Serialize)]
    struct Body<'a> {
        from: &'a str,
        to: Vec<&'a str>,
        subject: &'a str,
        html: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        reply_to: Option<Vec<&'a str>>,
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
