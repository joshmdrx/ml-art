//! HTTP middleware shared across binaries.

pub mod rate_limit;

pub use rate_limit::{
    inquiry_limit, search_limit, RateLimitPolicy, RateLimiters, EVENTS_DEFAULT_PER_MIN,
    INQUIRY_DEFAULT_PER_HOUR, SEARCH_DEFAULT_PER_MIN, UPLOADS_DEFAULT_PER_HOUR,
};
