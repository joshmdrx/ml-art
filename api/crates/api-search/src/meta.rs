//! Cheap metadata endpoints — things the web client renders without
//! needing a paid call. Kept in one place so we don't sprinkle "list
//! all valid X" handlers across feature modules.

use axum::Json;
use ml_art_core::{error::ApiError, modifiers};
use serde::Serialize;

#[derive(Serialize)]
pub struct ModifierInfo {
    /// URL token — passed back in `?modifiers=…`.
    pub name: &'static str,
    /// Human label for the button. Title-cased + spaces.
    pub label: String,
}

/// `GET /v1/modifiers` — the registered visual-search modifiers. Used
/// by the search page to render the button row. Static for v0 (no
/// per-user customization).
pub async fn list_modifiers() -> Result<Json<Vec<ModifierInfo>>, ApiError> {
    let items = modifiers::all_names()
        .into_iter()
        .map(|name| ModifierInfo {
            name,
            label: humanize(name),
        })
        .collect();
    Ok(Json(items))
}

/// `"more_minimal"` → `"More minimal"`. Display-only.
fn humanize(name: &str) -> String {
    let words: Vec<String> = name.split('_').map(str::to_string).collect();
    let mut out = String::new();
    for (i, w) in words.iter().enumerate() {
        if i == 0 {
            // Capitalize first character of the first word.
            let mut chars = w.chars();
            if let Some(c) = chars.next() {
                out.push_str(&c.to_uppercase().to_string());
                out.push_str(chars.as_str());
            }
        } else {
            out.push(' ');
            out.push_str(w);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_basic_cases() {
        assert_eq!(humanize("moodier"), "Moodier");
        assert_eq!(humanize("more_minimal"), "More minimal");
        assert_eq!(humanize("more_textured"), "More textured");
    }
}
