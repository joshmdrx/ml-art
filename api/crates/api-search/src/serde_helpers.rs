//! Serde helpers shared across PATCH endpoints.
//!
//! See `decisions.md` for why explicit-null-to-clear matters here: every
//! PATCH endpoint on the studio surface uses `Option<Option<T>>` to
//! distinguish "field absent" (leave column alone) from "field
//! explicitly null" (clear column) from "field present" (update).
//! Serde's default deserialize collapses `null` → `None` instead of
//! `Some(None)`, which silently breaks the "clear" path. This module
//! is where we override that.

use serde::{Deserialize, Deserializer};

/// Deserialize a JSON field into `Option<Option<T>>` so the three
/// patch states stay distinct:
///
/// | JSON shape            | Rust value         | SQL outcome           |
/// |-----------------------|--------------------|-----------------------|
/// | field absent          | `None`             | column left alone     |
/// | field explicit `null` | `Some(None)`       | column SET TO NULL    |
/// | field with a value    | `Some(Some(v))`    | column SET TO v       |
///
/// Pair with `#[serde(default)]` on the field so the "absent" case
/// produces `None`:
///
/// ```ignore
/// #[serde(default, deserialize_with = "deserialize_double_option")]
/// pub field: Option<Option<String>>,
/// ```
///
/// Then on the handler side, `body.field.is_some()` is true for both
/// explicit-null and explicit-value (i.e. "the caller touched it"), and
/// `body.field.flatten()` is `Some(v)` for explicit-value, `None`
/// otherwise — which lines up with the
/// `CASE WHEN $is_some::boolean THEN $value ELSE col END` SQL idiom
/// used across the studio patch handlers.
pub(crate) fn deserialize_double_option<'de, T, D>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}
