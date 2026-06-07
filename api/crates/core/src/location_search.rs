//! Shared helpers for the `?location=` filter that both `/v1/search`
//! and `/v1/search/map` accept.
//!
//! The naive "substring on city" we shipped first failed quietly when
//! users typed common things — "uk", "germany", "GB" — because we
//! store country as ISO alpha-2 (`'GB'`, `'DE'`) and city as the
//! local name. This module turns the user's term into:
//!
//!   - a normalized substring (`"uk"` → `"%uk%"`) for matching
//!     against the city column and the artist's free-text "based in"
//!     field
//!   - a set of ISO alpha-2 country codes (`"uk"` → `{"GB", "UK"}`)
//!     so the country column can be matched exactly
//!
//! Callers OR these together in SQL. The synonym table is finite +
//! hand-curated — covers the everyday "what an English-speaking user
//! types" cases. Genuine i18n (CJK names, transliterations, regional
//! groupings like "EU") is out of scope for v1.

/// Result of normalizing a user `?location=` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationTerms {
    /// `LIKE`-ready pattern (already lowercased + `%`-wrapped + with
    /// `%`/`_` escaped). None when the input was empty/whitespace.
    pub pattern: String,
    /// ISO alpha-2 codes (uppercased) to match exactly against
    /// country columns. Always includes the user's term if it's
    /// 2-3 chars (so `"GB"` works directly); plus any synonym
    /// expansion (`"uk"` → adds `"GB"`).
    pub iso_codes: Vec<String>,
}

impl LocationTerms {
    /// Build from raw input. Returns `None` when the term is empty
    /// after trim so the SQL builder can short-circuit.
    pub fn from_query(input: &str) -> Option<Self> {
        let raw = input.trim();
        if raw.is_empty() {
            return None;
        }
        let lower = raw.to_lowercase();

        // Escape LIKE wildcards in the user input so `_` and `%` can't
        // accidentally widen the match.
        let escaped = lower
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("%{escaped}%");

        // Build ISO candidate set: the user's input uppercased
        // (handles `GB`, `US`, `DE` direct) + any synonym mapping
        // (handles `UK`, `USA`, `Germany`).
        let mut iso_codes: Vec<String> = Vec::new();
        let upper = raw.to_uppercase();
        // Only add as an ISO candidate if it could plausibly be one.
        // Restricting to 2-3 chars avoids matching "Berlin" as an ISO
        // code (and PG comparing against a fixed-width column).
        if upper.len() == 2 || upper.len() == 3 {
            iso_codes.push(upper);
        }
        if let Some(code) = country_synonym(&lower) {
            if !iso_codes.iter().any(|c| c == code) {
                iso_codes.push(code.to_string());
            }
        }

        Some(Self { pattern, iso_codes })
    }
}

/// Map common English country names + alternative codes to ISO
/// alpha-2. Returns `None` when the input doesn't match the table —
/// the caller still has the substring path to fall back on.
///
/// Curated short list — extend when a user search misses. Not meant
/// to be exhaustive; the i18n version of this is a separate slice.
fn country_synonym(lower: &str) -> Option<&'static str> {
    match lower {
        // UK + Ireland
        "uk" | "united kingdom" | "great britain" | "britain" => Some("GB"),
        "england" | "scotland" | "wales" => Some("GB"),
        "northern ireland" | "ni" => Some("GB"),
        "eire" | "ireland" => Some("IE"),
        // North America
        "usa" | "united states" | "united states of america" | "america" => Some("US"),
        "canada" => Some("CA"),
        "mexico" => Some("MX"),
        // Western Europe
        "germany" | "deutschland" => Some("DE"),
        "france" => Some("FR"),
        "italy" | "italia" => Some("IT"),
        "spain" | "españa" | "espana" => Some("ES"),
        "portugal" => Some("PT"),
        "netherlands" | "holland" => Some("NL"),
        "belgium" => Some("BE"),
        "switzerland" => Some("CH"),
        "austria" => Some("AT"),
        // Nordics
        "sweden" => Some("SE"),
        "norway" => Some("NO"),
        "denmark" => Some("DK"),
        "finland" => Some("FI"),
        "iceland" => Some("IS"),
        // Asia-Pacific
        "japan" => Some("JP"),
        "china" => Some("CN"),
        "korea" | "south korea" => Some("KR"),
        "india" => Some("IN"),
        "australia" => Some("AU"),
        "new zealand" | "nz" => Some("NZ"),
        // South America
        "brazil" | "brasil" => Some("BR"),
        "argentina" => Some("AR"),
        "chile" => Some("CL"),
        "colombia" => Some("CO"),
        // Middle East + Africa
        "uae" | "united arab emirates" => Some("AE"),
        "israel" => Some("IL"),
        "turkey" | "türkiye" | "turkiye" => Some("TR"),
        "south africa" | "sa" => Some("ZA"),
        "egypt" => Some("EG"),
        // Eastern Europe
        "poland" => Some("PL"),
        "czechia" | "czech republic" => Some("CZ"),
        "russia" => Some("RU"),
        "ukraine" => Some("UA"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_none() {
        assert!(LocationTerms::from_query("").is_none());
        assert!(LocationTerms::from_query("   ").is_none());
    }

    #[test]
    fn substring_pattern_is_lowercased_and_wrapped() {
        let t = LocationTerms::from_query("LONDON").unwrap();
        assert_eq!(t.pattern, "%london%");
    }

    #[test]
    fn wildcards_are_escaped() {
        let t = LocationTerms::from_query("100% pure").unwrap();
        assert!(t.pattern.contains("100\\%"));
        let t2 = LocationTerms::from_query("a_b").unwrap();
        assert!(t2.pattern.contains("a\\_b"));
    }

    #[test]
    fn two_char_input_is_treated_as_iso_candidate() {
        let t = LocationTerms::from_query("gb").unwrap();
        assert!(t.iso_codes.contains(&"GB".to_string()));
    }

    #[test]
    fn three_char_input_is_treated_as_iso_candidate() {
        let t = LocationTerms::from_query("nyc").unwrap();
        assert!(t.iso_codes.contains(&"NYC".to_string()));
        // Even though NYC isn't a real country code, harmless — won't
        // match anything in the country column.
    }

    #[test]
    fn long_input_is_not_iso_candidate_unless_synonym() {
        let t = LocationTerms::from_query("Berlin").unwrap();
        // "Berlin" is 6 chars, not 2/3, so no raw ISO candidate.
        assert!(!t.iso_codes.iter().any(|c| c == "BERLIN"));
    }

    #[test]
    fn synonym_uk_expands_to_gb() {
        let t = LocationTerms::from_query("uk").unwrap();
        assert!(t.iso_codes.contains(&"GB".to_string()));
        // Original "UK" also kept (in case anyone stored UK literally).
        assert!(t.iso_codes.contains(&"UK".to_string()));
    }

    #[test]
    fn synonym_germany_expands_to_de() {
        let t = LocationTerms::from_query("Germany").unwrap();
        assert_eq!(t.iso_codes, vec!["DE".to_string()]);
    }

    #[test]
    fn synonym_usa_expands_to_us() {
        let t = LocationTerms::from_query("USA").unwrap();
        // "USA" itself + "US" from synonym table.
        assert!(t.iso_codes.contains(&"USA".to_string()));
        assert!(t.iso_codes.contains(&"US".to_string()));
    }

    #[test]
    fn iso_codes_deduplicate() {
        // "gb" → upper "GB", synonym is None for raw "gb" → so only "GB".
        // Actually "gb" doesn't hit synonym table. Let's test "GB":
        let t = LocationTerms::from_query("GB").unwrap();
        assert_eq!(t.iso_codes, vec!["GB".to_string()]);
    }
}
