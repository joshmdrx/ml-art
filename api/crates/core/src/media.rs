//! Canonical medium taxonomy (T-073).
//!
//! The v1 list pinned by `db/migrations/0021_artwork_medium_category.sql`'s
//! CHECK constraint. Single source of truth — the SQL constraint, the
//! validator, the studio dropdown's options, the FilterBar multi-select,
//! and any future ML clustering (T-057, T-061) all reference the same
//! `CATEGORIES` array.
//!
//! Adding a category later: extend `CATEGORIES`, ship the additive
//! `ALTER TABLE ... DROP CONSTRAINT ... ADD CONSTRAINT ...` migration,
//! and update the seed importer's `STYLE_TO_CATEGORY` map. Removing a
//! category requires backfilling every row that uses it — deliberately
//! structural to discourage taxonomy churn.
//!
//! Codes are snake_case (storage form). The display layer titlecases
//! and substitutes for `mixed_media` (→ "Mixed media").

/// V1 medium categories. Order is meaningful — used by the studio
/// dropdown + filter pills, so it's tuned to roughly match expected
/// platform-volume frequency (commonest first) for at-a-glance scan.
pub const CATEGORIES: &[&str] = &[
    "painting",
    "drawing",
    "photography",
    "print",
    "sculpture",
    "mixed_media",
    "collage",
    "textile",
    "ceramic",
    "digital",
    "other",
];

/// Whether `code` is one of the canonical category strings. Used by
/// the input validator + by the seed importer's style-mapping check
/// to fail fast if someone introduces a typo.
pub fn is_valid_category(code: &str) -> bool {
    CATEGORIES.contains(&code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_categories_accepted() {
        for &c in CATEGORIES {
            assert!(is_valid_category(c), "{c} should be valid");
        }
    }

    #[test]
    fn unknown_categories_rejected() {
        for bad in ["", "PAINTING", "Mixed Media", "nft", "video", "sculpture "] {
            assert!(!is_valid_category(bad), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn category_count_matches_migration_constraint() {
        // Forcing function: if we add a category to the array but
        // forget to extend the SQL CHECK, this stays in sync with
        // the migration's hand-count. Update both together.
        assert_eq!(CATEGORIES.len(), 11);
    }
}
