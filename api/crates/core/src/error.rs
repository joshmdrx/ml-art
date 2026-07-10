//! API error type. Renders as RFC 7807 problem+json.

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("not found")]
    NotFound,

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden")]
    Forbidden,

    /// State conflict — duplicate slug, version mismatch, etc.
    /// Renders as 409. T-058 series CRUD uses this for per-artist
    /// duplicate-slug attempts.
    #[error("conflict: {0}")]
    Conflict(String),

    /// Carries the soonest the caller may retry, in seconds. Surfaced via
    /// the `Retry-After` response header so clients can back off without
    /// guessing. Set by the rate-limit middleware in `core::middleware`.
    #[error("rate limited; retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    /// A required external dependency isn't configured/available, so the
    /// endpoint can't serve. Renders as 503. The marketplace endpoints
    /// return this when `STRIPE_SECRET_KEY` is unset (M-01) — dev
    /// instances without Stripe credentials answer 503 at the entry
    /// rather than 500-ing deeper in.
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden => StatusCode::FORBIDDEN,
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            ApiError::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            ApiError::Internal(_) | ApiError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn title(&self) -> &'static str {
        match self {
            ApiError::BadRequest(_) => "Bad Request",
            ApiError::Unauthorized => "Unauthorized",
            ApiError::Forbidden => "Forbidden",
            ApiError::NotFound => "Not Found",
            ApiError::Conflict(_) => "Conflict",
            ApiError::RateLimited { .. } => "Too Many Requests",
            ApiError::ServiceUnavailable(_) => "Service Unavailable",
            ApiError::Internal(_) | ApiError::Database(_) => "Internal Server Error",
        }
    }
}

#[derive(Serialize)]
struct ProblemJson<'a> {
    #[serde(rename = "type")]
    typ: &'a str,
    title: &'a str,
    status: u16,
    detail: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        // Don't leak internal error details to clients in non-dev environments.
        let detail = match &self {
            ApiError::Internal(_) | ApiError::Database(_) => {
                tracing::error!(error = ?self, "internal error");
                "internal server error".to_string()
            }
            _ => self.to_string(),
        };

        let status = self.status();
        let body = ProblemJson {
            typ: "about:blank",
            title: self.title(),
            status: status.as_u16(),
            detail,
        };

        let mut resp = (status, Json(body)).into_response();
        resp.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/problem+json"),
        );
        // RFC 7231 §7.1.3: include Retry-After on 429 so clients can back
        // off without polling. Value is integer seconds.
        if let ApiError::RateLimited { retry_after_secs } = &self {
            if let Ok(v) = axum::http::HeaderValue::from_str(&retry_after_secs.to_string()) {
                resp.headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, v);
            }
        }
        resp
    }
}
