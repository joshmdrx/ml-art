//! T-083 — admin surface (`/v1/admin/*`).
//!
//! All endpoints under here require `AdminUser` (see
//! `crate::extractors::AdminUser`). Non-admins get a 403, never a 404,
//! because the surface URL is itself public knowledge by now — the
//! "hide the admin surface" job belongs to the web layer (`/admin/*`
//! routes return 404 for non-admin browsers).
//!
//! Every mutating endpoint writes one `admin_audit_log` row via
//! `ml_art_core::admin::audit::record` before applying the change. The
//! audit insert and the mutation are *not* in a single transaction —
//! intentionally — so a failed mutation still leaves an audit row
//! reflecting the admin's intent.

pub mod artists;
