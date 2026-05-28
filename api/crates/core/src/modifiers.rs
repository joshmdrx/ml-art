//! Modifier vectors — "moodier", "warmer", "more_minimal", etc. — and
//! the math to apply them to a search anchor.
//!
//! T-010 Phase C. The technique was validated by the
//! `2026-05-modifier-deltas` spike on a 2000-image WikiArt corpus
//! (`ml/spikes/2026-05-modifier-deltas/FINDINGS.md`). Verdict was
//! **ship at α=0.8**.
//!
//! ## How a delta is computed
//!
//! Each modifier is defined by contrastive text pairs (4 "positive",
//! 4 "negative" descriptions). The delta is:
//!
//! ```text
//!   delta = normalize( mean(embed(positives)) - mean(embed(negatives)) )
//! ```
//!
//! To shift an anchor along the modifier direction, normalize again:
//!
//! ```text
//!   shifted = normalize(anchor + α · delta)
//! ```
//!
//! ## Caching
//!
//! The 8 modifier texts per modifier go through `Embedder::embed_text`,
//! which is backed by the `query_embedding_cache` Postgres table. First
//! request per modifier per process hits Jina once for each of the 8
//! texts; subsequent requests are cache reads (~1-5ms each).
//!
//! An in-process delta cache (so a delta-using request isn't paying 8
//! DB roundtrips) is a follow-up if request latency shows up as a
//! problem. The DB cache is the right durable layer either way.
//!
//! ## Composing modifiers
//!
//! Multiple modifiers in one request sum their deltas before
//! normalizing:
//!
//! ```text
//!   shifted = normalize(anchor + α · (δ_moodier + δ_warmer))
//! ```
//!
//! The spike didn't test compositions exhaustively. Worth re-spiking
//! when product surfaces it as a problem.

use crate::embedder::Embedder;
use pgvector::Vector;

pub const DEFAULT_ALPHA: f32 = 0.8;

#[derive(Debug, Clone, Copy)]
pub struct Modifier {
    pub name: &'static str,
    pub positive: &'static [&'static str],
    pub negative: &'static [&'static str],
}

/// The five modifier directions the spike validated. Verbatim from
/// `ml/spikes/2026-05-modifier-deltas/modifiers.py` — changing these
/// requires re-running the spike to confirm the directions still
/// behave (the contrastive pairs *are* the modifier definition).
pub const MODIFIERS: &[Modifier] = &[
    Modifier {
        name: "moodier",
        positive: &[
            "a moody atmospheric painting with dark tones",
            "a brooding melancholic artwork in shadow",
            "a somber overcast landscape painting",
            "a dark contemplative piece, low-key lighting",
        ],
        negative: &[
            "a bright cheerful painting with vivid colors",
            "an uplifting joyful artwork in sunlight",
            "a vibrant happy scene, high-key lighting",
            "a light and airy artwork, optimistic mood",
        ],
    },
    Modifier {
        name: "warmer",
        positive: &[
            "an artwork with warm tones, red orange and yellow",
            "a painting dominated by amber, ochre, and crimson",
            "a sunlit warm-palette artwork",
            "an artwork with warm earthy reds and golds",
        ],
        negative: &[
            "an artwork with cool tones, blue and teal",
            "a painting dominated by navy, cyan, and slate",
            "a cold-palette winter artwork",
            "an artwork with cool blues and greens",
        ],
    },
    Modifier {
        name: "more_minimal",
        positive: &[
            "a minimalist artwork with empty negative space",
            "a sparse composition with a single simple form",
            "a clean reductive piece, few elements",
            "an artwork with quiet emptiness and restraint",
        ],
        negative: &[
            "a maximalist artwork densely packed with detail",
            "a busy composition with many elements",
            "an ornate baroque piece, intricate and crowded",
            "an artwork full of pattern, texture, and visual noise",
        ],
    },
    Modifier {
        name: "more_textured",
        positive: &[
            "a heavily textured painting with thick impasto",
            "an artwork with rough tactile surface",
            "a painting where you can see brushwork and material",
            "an artwork with grainy, gritty, palpable texture",
        ],
        negative: &[
            "a smooth flat artwork with no visible texture",
            "a digital crisp clean illustration",
            "an artwork with even uniform surface",
            "a glossy flat-finish painting",
        ],
    },
    Modifier {
        name: "more_graphic",
        positive: &[
            "a graphic artwork with bold flat shapes",
            "a poster-like illustration with strong outlines",
            "a stylized graphic composition",
            "an artwork with clean vector-like forms",
        ],
        negative: &[
            "a painterly artwork with soft brushwork",
            "a naturalistic painting with blended tones",
            "a representational artwork with realistic shading",
            "a loose expressive oil painting",
        ],
    },
];

/// Look up a modifier by URL token. Token comparison is exact —
/// callers should pre-normalize (lowercase, trim).
pub fn find(name: &str) -> Option<&'static Modifier> {
    MODIFIERS.iter().find(|m| m.name == name)
}

/// All registered modifier names, in declaration order. Used by the
/// web client to render the button row.
pub fn all_names() -> Vec<&'static str> {
    MODIFIERS.iter().map(|m| m.name).collect()
}

/// Compute the delta vector for one modifier. Embeds each of the 8
/// contrastive texts (via `Embedder::embed_text`, so misses go through
/// the DB-backed query cache), then `normalize(mean(pos) - mean(neg))`.
///
/// Returns `None` if the embedder is disabled — caller's choice
/// whether that's a hard error (the search handler treats it as one
/// when `modifiers=…` is on the request).
pub async fn compute_delta(
    modifier: &Modifier,
    embedder: &Embedder,
) -> anyhow::Result<Option<Vector>> {
    if !embedder.enabled() {
        return Ok(None);
    }
    let pos_vecs = embed_all(modifier.positive, embedder).await?;
    let neg_vecs = embed_all(modifier.negative, embedder).await?;

    // Mean each side, subtract, normalize.
    let pos_mean = mean(&pos_vecs);
    let neg_mean = mean(&neg_vecs);
    let direction = sub(&pos_mean, &neg_mean);
    Ok(Some(Vector::from(normalize(&direction))))
}

/// `normalize(anchor + α · Σ deltas)`. Empty `deltas` is a no-op
/// (returns the anchor unchanged), making the same code path work
/// whether or not modifiers are present.
pub fn apply_deltas(anchor: &Vector, deltas: &[Vector], alpha: f32) -> Vector {
    if deltas.is_empty() {
        return anchor.clone();
    }
    let mut acc: Vec<f32> = anchor.as_slice().to_vec();
    for d in deltas {
        let d_slice = d.as_slice();
        debug_assert_eq!(acc.len(), d_slice.len(), "anchor + delta dim mismatch");
        for (a, b) in acc.iter_mut().zip(d_slice.iter()) {
            *a += alpha * *b;
        }
    }
    Vector::from(normalize(&acc))
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal: vector math
// ─────────────────────────────────────────────────────────────────────────────

async fn embed_all(texts: &[&str], embedder: &Embedder) -> anyhow::Result<Vec<Vec<f32>>> {
    let mut out = Vec::with_capacity(texts.len());
    for t in texts {
        match embedder.embed_text(t).await? {
            Some(v) => out.push(v.as_slice().to_vec()),
            None => anyhow::bail!("embedder returned None for modifier text"),
        }
    }
    Ok(out)
}

fn mean(vecs: &[Vec<f32>]) -> Vec<f32> {
    debug_assert!(!vecs.is_empty(), "mean of empty slice");
    let dim = vecs[0].len();
    let mut out = vec![0.0_f32; dim];
    for v in vecs {
        debug_assert_eq!(v.len(), dim, "ragged vector dims");
        for (i, x) in v.iter().enumerate() {
            out[i] += *x;
        }
    }
    let n = vecs.len() as f32;
    for x in out.iter_mut() {
        *x /= n;
    }
    out
}

fn sub(a: &[f32], b: &[f32]) -> Vec<f32> {
    debug_assert_eq!(a.len(), b.len(), "sub dim mismatch");
    a.iter().zip(b.iter()).map(|(x, y)| x - y).collect()
}

fn normalize(v: &[f32]) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    // Defensive: zero vector → zero vector (not NaN). Only happens when
    // pos == neg (e.g. fixed-vector test embedder); the resulting
    // delta is a no-op which is acceptable behavior.
    if norm == 0.0 {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_returns_known_modifiers() {
        assert!(find("moodier").is_some());
        assert!(find("warmer").is_some());
        assert!(find("nonsense").is_none());
    }

    #[test]
    fn all_names_returns_five() {
        assert_eq!(all_names().len(), 5);
    }

    #[test]
    fn apply_deltas_zero_shift_returns_anchor() {
        // Empty deltas = no shift; the anchor passes through.
        let anchor = Vector::from(vec![1.0_f32, 0.0, 0.0, 0.0]);
        let out = apply_deltas(&anchor, &[], DEFAULT_ALPHA);
        assert_eq!(out.as_slice(), anchor.as_slice());
    }

    #[test]
    fn apply_deltas_shifts_along_direction() {
        // Anchor = (1, 0); delta = (0, 1); alpha = 1.0
        // → (1, 1) → normalized to (1/√2, 1/√2).
        let anchor = Vector::from(vec![1.0_f32, 0.0]);
        let delta = Vector::from(vec![0.0_f32, 1.0]);
        let out = apply_deltas(&anchor, std::slice::from_ref(&delta), 1.0);
        let s = out.as_slice();
        let expected = std::f32::consts::FRAC_1_SQRT_2;
        assert!((s[0] - expected).abs() < 1e-5, "got {}", s[0]);
        assert!((s[1] - expected).abs() < 1e-5, "got {}", s[1]);
    }

    #[test]
    fn apply_deltas_composes_multiple() {
        // Two deltas sum; e.g. anchor=(1,0,0), δ₁=(0,1,0), δ₂=(0,0,1),
        // alpha=1.0 → (1,1,1) → normalized to 1/√3 each component.
        let anchor = Vector::from(vec![1.0_f32, 0.0, 0.0]);
        let d1 = Vector::from(vec![0.0_f32, 1.0, 0.0]);
        let d2 = Vector::from(vec![0.0_f32, 0.0, 1.0]);
        let out = apply_deltas(&anchor, &[d1, d2], 1.0);
        let expected = 1.0_f32 / 3.0_f32.sqrt();
        for x in out.as_slice() {
            assert!((x - expected).abs() < 1e-5, "got {x}");
        }
    }

    #[test]
    fn normalize_handles_zero_vector() {
        let z = vec![0.0_f32, 0.0, 0.0];
        assert_eq!(normalize(&z), vec![0.0_f32, 0.0, 0.0]);
    }

    #[test]
    fn mean_centers_two_opposing_vectors() {
        let v1 = vec![1.0_f32, 0.0, 0.0];
        let v2 = vec![-1.0_f32, 0.0, 0.0];
        assert_eq!(mean(&[v1, v2]), vec![0.0_f32, 0.0, 0.0]);
    }
}
