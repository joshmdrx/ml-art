//! Parser for the `?artist_ids=uuid1,uuid2,…` query param that
//! both `/v1/search/map` and `/v1/search/map/cities` accept.
//!
//! Centralised here so the two endpoints stay in sync: validation
//! rules, the dedupe + cap behaviour, and the error message a
//! caller sees on a malformed UUID are identical regardless of
//! which endpoint they hit.

use crate::error::ApiError;
use uuid::Uuid;

/// Cap on the number of ids honoured per request. Matches
/// `MAP_PIN_LIMIT` server-side — asking for more pins than the
/// endpoint will ever return is pointless. Also keeps the URL
/// length + the SQL `IN`-list reasonable.
pub const MAX_ARTIST_IDS: usize = 500;

/// Parse `?artist_ids=uuid1,uuid2,…` into a sorted, deduplicated,
/// capped vector. Returns:
///   - `Ok(None)` when the input is absent / empty / whitespace
///   - `Ok(Some(vec))` on success
///   - `Err(BadRequest)` on the first non-UUID token (we 400 rather
///     than silently drop ids — the caller almost certainly has a bug)
pub fn parse_artist_ids(raw: Option<&str>) -> Result<Option<Vec<Uuid>>, ApiError> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let mut ids: Vec<Uuid> = Vec::new();
    for tok in raw.split(',') {
        let tok = tok.trim();
        if tok.is_empty() {
            continue;
        }
        let parsed = Uuid::parse_str(tok)
            .map_err(|_| ApiError::BadRequest(format!("artist_ids: invalid uuid '{tok}'")))?;
        ids.push(parsed);
    }
    ids.sort();
    ids.dedup();
    if ids.len() > MAX_ARTIST_IDS {
        ids.truncate(MAX_ARTIST_IDS);
    }
    Ok(if ids.is_empty() { None } else { Some(ids) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_inputs_yield_none() {
        assert!(parse_artist_ids(None).unwrap().is_none());
        assert!(parse_artist_ids(Some("")).unwrap().is_none());
        assert!(parse_artist_ids(Some("   ")).unwrap().is_none());
        assert!(parse_artist_ids(Some(",,")).unwrap().is_none());
    }

    #[test]
    fn parses_and_dedupes() {
        let id = Uuid::new_v4();
        let raw = format!("{id},{id}");
        let out = parse_artist_ids(Some(&raw)).unwrap().unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], id);
    }

    #[test]
    fn invalid_uuid_is_bad_request() {
        let err = parse_artist_ids(Some("not-a-uuid")).unwrap_err();
        // ApiError::BadRequest carries a message — we just need to
        // verify it didn't silently return Ok(None).
        assert!(matches!(err, ApiError::BadRequest(_)));
    }

    #[test]
    fn caps_at_max() {
        let raw: String = (0..MAX_ARTIST_IDS + 50)
            .map(|_| Uuid::new_v4().to_string())
            .collect::<Vec<_>>()
            .join(",");
        let out = parse_artist_ids(Some(&raw)).unwrap().unwrap();
        assert_eq!(out.len(), MAX_ARTIST_IDS);
    }
}
