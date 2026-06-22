//! Shared input validators for handler-level checks the schema can't
//! express. Each validator is a small pure function returning a
//! normalised value or a string error (which handlers map onto
//! `ApiError::BadRequest`).
//!
//! Validators live in `core` rather than `api-search` so jobs handlers,
//! seed scripts, and any future binary can share them — there's exactly
//! one source of truth for what a valid `artworks.dimensions` row
//! looks like.

use serde_json::{json, Value};

/// Centimetre ceiling. 5000cm = 50m — larger than any panel painting
/// in history. Generous enough that we never accidentally cap a real
/// artwork; mean enough that fat-fingered "5" turns into a useful
/// error instead of `{"width": 50000000, …}` poisoning the corpus.
const MAX_DIMENSION_CM: i64 = 5000;

/// T-070 — validate + normalise the `artworks.dimensions` JSONB.
///
/// **Shape:**
/// ```json
/// {"unit": "cm", "width": 80, "height": 60, "depth": 4}
/// ```
///
/// - `width` and `height` are **required** when the object is present
///   (we never want half-filled dimensions polluting the corpus — it's
///   all-or-nothing). They must be positive integers ≤ `MAX_DIMENSION_CM`.
/// - `depth` is **optional**. When present it follows the same range.
/// - `unit`: only `"cm"` is accepted in v1. Defaults to `"cm"` when
///   omitted, and is normalised onto the output so downstream readers
///   never have to handle the absent case.
/// - **No extra keys.** A closed schema means the data layer can grow
///   confidently — adding e.g. a `unit_was: "in"` tracker later is a
///   deliberate change, not a silent stowaway from some client.
///
/// Callers (studio create + patch) should call this ONLY when the
/// caller has signalled they want to set dimensions. T-070's product
/// decision is that artists can publish without dimensions (drafts and
/// published rows alike), so an absent or NULL value never reaches this
/// validator — see `decisions.md` 2026-06-22.
pub fn dimensions_v1(input: &Value) -> Result<Value, String> {
    let obj = input
        .as_object()
        .ok_or_else(|| "dimensions: must be a JSON object".to_string())?;

    // Closed schema — reject unknown keys early so a typo or schema
    // drift can't sneak through.
    for key in obj.keys() {
        match key.as_str() {
            "width" | "height" | "depth" | "unit" => {}
            other => return Err(format!("dimensions.{other}: unknown field")),
        }
    }

    let unit = match obj.get("unit") {
        None | Some(Value::Null) => "cm",
        Some(Value::String(s)) if s == "cm" => "cm",
        Some(Value::String(s)) => {
            return Err(format!(
                "dimensions.unit: only \"cm\" is accepted (got {s:?})"
            ))
        }
        Some(_) => return Err("dimensions.unit: must be a string".to_string()),
    };

    let width = required_dimension(obj, "width")?;
    let height = required_dimension(obj, "height")?;
    let depth = optional_dimension(obj, "depth")?;

    let mut out = json!({
        "unit": unit,
        "width": width,
        "height": height,
    });
    if let Some(d) = depth {
        out["depth"] = json!(d);
    }
    Ok(out)
}

fn required_dimension(obj: &serde_json::Map<String, Value>, key: &str) -> Result<i64, String> {
    match obj.get(key) {
        None | Some(Value::Null) => {
            Err(format!("dimensions.{key}: required when dimensions is set"))
        }
        Some(v) => extract_positive_int(v, key),
    }
}

fn optional_dimension(
    obj: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<i64>, String> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => extract_positive_int(v, key).map(Some),
    }
}

/// Accept either an integer (`80`) or a float that's actually a whole
/// number (`80.0`). Reject decimals like `80.5` — we don't store
/// fractional cm; an artist's tape measure resolves to the nearest
/// cm in practice, and accepting decimals would make round-tripping
/// the JSONB into the band-bucket SQL fragile.
fn extract_positive_int(v: &Value, key: &str) -> Result<i64, String> {
    let n = match v {
        Value::Number(n) => n,
        _ => return Err(format!("dimensions.{key}: must be a number")),
    };
    let as_i64 = if let Some(i) = n.as_i64() {
        i
    } else if let Some(f) = n.as_f64() {
        if f.fract() != 0.0 {
            return Err(format!(
                "dimensions.{key}: must be a whole number (got {f})"
            ));
        }
        f as i64
    } else {
        return Err(format!("dimensions.{key}: must be an integer"));
    };
    if as_i64 < 1 {
        return Err(format!("dimensions.{key}: must be ≥ 1"));
    }
    if as_i64 > MAX_DIMENSION_CM {
        return Err(format!(
            "dimensions.{key}: must be ≤ {MAX_DIMENSION_CM} (got {as_i64})"
        ));
    }
    Ok(as_i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_width_height_only() {
        let v = json!({"width": 80, "height": 60});
        assert_eq!(
            dimensions_v1(&v).unwrap(),
            json!({"unit": "cm", "width": 80, "height": 60})
        );
    }

    #[test]
    fn happy_path_with_depth() {
        let v = json!({"width": 80, "height": 60, "depth": 4});
        assert_eq!(
            dimensions_v1(&v).unwrap(),
            json!({"unit": "cm", "width": 80, "height": 60, "depth": 4})
        );
    }

    #[test]
    fn unit_default_is_cm() {
        let v = json!({"width": 80, "height": 60});
        let out = dimensions_v1(&v).unwrap();
        assert_eq!(out["unit"], "cm");
    }

    #[test]
    fn explicit_cm_unit_passes() {
        let v = json!({"unit": "cm", "width": 80, "height": 60});
        assert!(dimensions_v1(&v).is_ok());
    }

    #[test]
    fn non_cm_unit_rejected() {
        let v = json!({"unit": "in", "width": 80, "height": 60});
        assert!(dimensions_v1(&v).unwrap_err().contains("unit"));
    }

    #[test]
    fn whole_float_accepted() {
        let v = json!({"width": 80.0, "height": 60.0});
        let out = dimensions_v1(&v).unwrap();
        assert_eq!(out["width"], 80);
        assert_eq!(out["height"], 60);
    }

    #[test]
    fn fractional_rejected() {
        let v = json!({"width": 80.5, "height": 60});
        let err = dimensions_v1(&v).unwrap_err();
        assert!(err.contains("whole number"), "{err}");
    }

    #[test]
    fn missing_width_rejected() {
        let v = json!({"height": 60});
        assert!(dimensions_v1(&v).unwrap_err().contains("width"));
    }

    #[test]
    fn missing_height_rejected() {
        let v = json!({"width": 80});
        assert!(dimensions_v1(&v).unwrap_err().contains("height"));
    }

    #[test]
    fn null_dimension_value_rejected() {
        let v = json!({"width": null, "height": 60});
        let err = dimensions_v1(&v).unwrap_err();
        assert!(err.contains("width") && err.contains("required"));
    }

    #[test]
    fn zero_rejected() {
        let v = json!({"width": 0, "height": 60});
        assert!(dimensions_v1(&v).unwrap_err().contains("≥ 1"));
    }

    #[test]
    fn negative_rejected() {
        let v = json!({"width": -10, "height": 60});
        assert!(dimensions_v1(&v).unwrap_err().contains("≥ 1"));
    }

    #[test]
    fn overflow_rejected() {
        let v = json!({"width": 50000, "height": 60});
        let err = dimensions_v1(&v).unwrap_err();
        assert!(err.contains("≤ 5000"), "{err}");
    }

    #[test]
    fn ceiling_inclusive() {
        let v = json!({"width": 5000, "height": 60});
        assert!(dimensions_v1(&v).is_ok());
    }

    #[test]
    fn unknown_field_rejected() {
        let v = json!({"width": 80, "height": 60, "wat": 1});
        let err = dimensions_v1(&v).unwrap_err();
        assert!(err.contains("wat") && err.contains("unknown"));
    }

    #[test]
    fn non_object_rejected() {
        assert!(dimensions_v1(&json!([1, 2, 3])).is_err());
        assert!(dimensions_v1(&json!("80x60")).is_err());
        assert!(dimensions_v1(&json!(80)).is_err());
    }

    #[test]
    fn string_dimension_rejected() {
        let v = json!({"width": "80", "height": 60});
        assert!(dimensions_v1(&v).unwrap_err().contains("number"));
    }

    #[test]
    fn depth_validated_when_present() {
        assert!(dimensions_v1(&json!({"width": 1, "height": 1, "depth": 0})).is_err());
        assert!(dimensions_v1(&json!({"width": 1, "height": 1, "depth": 6000})).is_err());
        assert!(dimensions_v1(&json!({"width": 1, "height": 1, "depth": 5})).is_ok());
    }

    #[test]
    fn extra_unit_field_handling_is_strict() {
        // `unit_legacy` would be a silent stowaway under a permissive
        // schema. Closed schema → 400.
        let v = json!({"width": 1, "height": 1, "unit_legacy": "in"});
        assert!(dimensions_v1(&v).unwrap_err().contains("unknown"));
    }
}
