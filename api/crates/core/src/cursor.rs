//! Opaque page cursor for `Paginated<T>` responses.
//!
//! v1 stores only an offset (`{"o": N}`). The JSON shape is
//! deliberately forward-compatible: a future migration to true
//! keyset pagination can ship a `{"k": [score, id]}` variant
//! without changing the API surface, because the cursor is
//! opaque to clients. Server reads it, that's it.
//!
//! Why offset for v1: real keyset on the hybrid search path would
//! require wrapping the RRF-scoring CTE in an outer SELECT just to
//! filter on the computed `rrf_score` — substantial SQL surgery
//! for a ~few-thousand-row corpus where offset works fine. T-037
//! captures the trade-off.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

/// Hard ceiling on offset values we'll honour. Beyond this we
/// `400` — protects against deep-paginate-and-DoS attempts, and
/// matches the candidate-pool ceiling of `api-search`'s hybrid
/// query (200). Generous headroom to avoid 400ing legitimate users
/// of non-hybrid endpoints.
pub const MAX_CURSOR_OFFSET: i64 = 1000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PageCursor {
    /// Offset from the start of the result set.
    #[serde(rename = "o")]
    pub offset: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorError {
    /// Couldn't base64-decode or JSON-parse the input.
    Malformed,
    /// Decoded successfully but the offset is negative or past the cap.
    OutOfRange,
}

impl PageCursor {
    pub fn from_offset(offset: i64) -> Self {
        Self { offset }
    }

    pub fn encode(&self) -> String {
        // serialization can't fail for a struct with a single i64.
        let json = serde_json::to_vec(self).expect("PageCursor serializes");
        URL_SAFE_NO_PAD.encode(json)
    }

    pub fn decode(s: &str) -> Result<Self, CursorError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(s.as_bytes())
            .map_err(|_| CursorError::Malformed)?;
        let c: Self = serde_json::from_slice(&bytes).map_err(|_| CursorError::Malformed)?;
        if c.offset < 0 || c.offset > MAX_CURSOR_OFFSET {
            return Err(CursorError::OutOfRange);
        }
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for offset in [0, 1, 24, 100, MAX_CURSOR_OFFSET] {
            let c = PageCursor::from_offset(offset);
            let enc = c.encode();
            assert_eq!(PageCursor::decode(&enc).unwrap(), c, "offset={offset}");
        }
    }

    #[test]
    fn rejects_malformed_base64() {
        // `!` isn't in the URL-safe base64 alphabet.
        assert_eq!(
            PageCursor::decode("not-a-cursor!"),
            Err(CursorError::Malformed),
        );
    }

    #[test]
    fn rejects_garbage_json() {
        let bad = URL_SAFE_NO_PAD.encode(b"not json");
        assert_eq!(PageCursor::decode(&bad), Err(CursorError::Malformed));
    }

    #[test]
    fn rejects_negative_offset() {
        let enc = PageCursor::from_offset(-1).encode();
        assert_eq!(PageCursor::decode(&enc), Err(CursorError::OutOfRange));
    }

    #[test]
    fn rejects_offset_past_cap() {
        let enc = PageCursor::from_offset(MAX_CURSOR_OFFSET + 1).encode();
        assert_eq!(PageCursor::decode(&enc), Err(CursorError::OutOfRange));
    }

    #[test]
    fn opaque_to_clients() {
        // Encoded cursor should not contain the literal offset value
        // in a recognizable form — clients shouldn't reverse-engineer
        // the schema and depend on it.
        let enc = PageCursor::from_offset(42).encode();
        assert!(!enc.contains("42"));
        assert!(!enc.contains("offset"));
    }
}
