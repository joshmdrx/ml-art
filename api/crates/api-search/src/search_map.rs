//! `/v1/search/map` — T-038 G5.
//!
//! Returns map pins (`artist_locations` rows) matching the given filters,
//! one row per location. The web client uses this for `/search?map=1`:
//!   - Mapbox GL JS clusters client-side based on the rows we hand back
//!   - Bbox is the moving window; the client re-fetches when the user
//!     pans/zooms past a threshold
//!
//! Why a separate endpoint (not a `?map=1` flag on `/v1/search`):
//! `/v1/search` ranks artworks via a hybrid keyword + vector fusion. Map
//! mode wants venues, not artworks, and "rank by relevance" doesn't
//! translate (a gallery either is in the bbox or it isn't). Two
//! endpoints with overlapping filter parsing is less complex than one
//! endpoint with two divergent codepaths.
//!
//! Scope for v1 — keyword + medium + bbox. Vector search, modifiers,
//! and price filters are intentionally not surfaced here: they apply to
//! artworks (a thing the artist *makes*), but a viewer using the map
//! wants venues (a place they can *go*). Filtering venues by "is this
//! artist's median artwork price in [a, b]?" doesn't help anyone find
//! a gallery to visit on Saturday.

use crate::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use ml_art_core::{error::ApiError, images::url_for_s3_key};
use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, FromRow};
use std::sync::Arc;
use uuid::Uuid;

/// Hard cap on pins per response. Mapbox GL clusters smoothly into the
/// low thousands; we cap at 500 to keep payloads small (<100KB) and
/// give us room to add server-side clustering later if needed.
const MAP_PIN_LIMIT: i64 = 500;

#[derive(Debug, Deserialize)]
pub struct MapSearchParams {
    /// Free-text query — applied to artwork title + description via
    /// the existing `artworks_search` tsvector. A pin shows if *any*
    /// of its artist's published artworks match.
    #[serde(default)]
    pub q: Option<String>,
    /// Medium filter — pin shows if any of its artist's published
    /// artworks have this medium. Pre-filter happens in SQL so we
    /// don't drag artworks across the join unnecessarily.
    #[serde(default)]
    pub medium: Option<String>,
    /// Bbox as "west,south,east,north" (lng,lat,lng,lat). Mirrors
    /// Mapbox's `bounds.toArray().flat()` ordering. When omitted, the
    /// client gets every matching pin (capped at `MAP_PIN_LIMIT`),
    /// which is fine on first page-load before the map has settled.
    #[serde(default)]
    pub bbox: Option<String>,
    /// Soft text filter on `artist_locations.city` (case-insensitive
    /// substring) — lets `/search?location=berlin&map=1` work.
    #[serde(default)]
    pub location: Option<String>,
    /// Pin down to a single artist by slug (T-041). Used by the
    /// "See on map" CTA on `/artists/[slug]` — gives a viewer one
    /// click from "I like this artist" to "where can I see their
    /// work." Composes with the other filters: `?artist=alice&q=cobalt`
    /// still requires alice to have an artwork matching cobalt.
    #[serde(default)]
    pub artist: Option<String>,
    /// Comma-separated list of artist UUIDs. When set, the map shows
    /// venues for *exactly these artists* and ignores `q`/`medium`
    /// (which the upstream caller — typically the /search page — has
    /// already applied to produce this id set). This is the path the
    /// web app takes when "Where to see them" is toggled while a
    /// search is active: the grid already ran the hybrid retrieval,
    /// the map just renders pins for the artists it found. Keeps the
    /// two surfaces in sync without re-running the embed.
    ///
    /// Compatible with `q`/`medium` for the rare API caller that
    /// wants to intersect — we don't strip them; they just compose.
    /// Max ~500 ids per request (well under URL length caps).
    #[serde(default)]
    pub artist_ids: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MapPin {
    pub location_id: Uuid,
    pub lat: f64,
    pub lng: f64,
    pub name: String,
    pub kind: String, // "gallery" | "studio"
    pub city: Option<String>,
    pub country: Option<String>,
    pub artist: MapPinArtist,
}

#[derive(Debug, Serialize)]
pub struct MapPinArtist {
    pub slug: String,
    pub display_name: String,
    /// One representative thumbnail (the artist's most-recent published
    /// artwork's primary image). Optional — artists with no images
    /// still get pinned, just without a thumb in the popover.
    pub primary_image_url: Option<String>,
}

pub async fn handle(
    State(state): State<Arc<AppState>>,
    Query(params): Query<MapSearchParams>,
) -> Result<Json<Vec<MapPin>>, ApiError> {
    let bbox = parse_bbox(params.bbox.as_deref())?;

    // We compose SQL piece-by-piece because the filter set is a sparse
    // intersection — same approach as `/v1/search`. Parameters are
    // bound positionally as we add WHERE clauses, keeping the prepared
    // statement cache happy.
    let mut sql = String::from(
        r#"
        SELECT DISTINCT ON (al.id)
            al.id            AS location_id,
            al.lat           AS lat,
            al.lng           AS lng,
            al.name          AS name,
            al.kind          AS kind,
            al.city          AS city,
            al.country       AS country,
            ar.slug          AS artist_slug,
            ar.display_name  AS artist_display_name,
            ai.s3_key        AS primary_s3_key
        FROM artist_locations al
        JOIN artists ar ON ar.id = al.artist_id
        LEFT JOIN LATERAL (
            SELECT s3_key
            FROM artworks aw
            JOIN artwork_images aii
              ON aii.artwork_id = aw.id
             AND aii.is_primary
             AND aii.moderation_status = 'approved'
            WHERE aw.artist_id = ar.id
              AND aw.deleted_at IS NULL
              AND aw.status = 'published'
            ORDER BY aw.published_at DESC NULLS LAST
            LIMIT 1
        ) ai ON true
        WHERE al.deleted_at IS NULL
          AND al.lat IS NOT NULL
          AND al.lng IS NOT NULL
          AND ar.deleted_at IS NULL
          AND ar.status = 'active'
        "#,
    );

    // Bind positionally — index tracks the next `$N` to use.
    let mut binds: Vec<BoundParam> = Vec::new();
    let mut next: u32 = 0;

    let push = |sql: &mut String, binds: &mut Vec<BoundParam>, next: &mut u32, b: BoundParam| {
        *next += 1;
        sql.push_str(&format!(" ${} ", next));
        binds.push(b);
    };

    if let Some(bbox) = bbox.as_ref() {
        sql.push_str(" AND al.lng BETWEEN");
        push(&mut sql, &mut binds, &mut next, BoundParam::F64(bbox.west));
        sql.push_str(" AND");
        push(&mut sql, &mut binds, &mut next, BoundParam::F64(bbox.east));
        sql.push_str(" AND al.lat BETWEEN");
        push(&mut sql, &mut binds, &mut next, BoundParam::F64(bbox.south));
        sql.push_str(" AND");
        push(&mut sql, &mut binds, &mut next, BoundParam::F64(bbox.north));
    }

    if let Some(loc) = location_filter(params.location.as_deref()) {
        sql.push_str(" AND lower(al.city) LIKE");
        push(&mut sql, &mut binds, &mut next, BoundParam::Text(loc));
    }

    // Artist filter (T-041) — exact-match on slug. Uses the existing
    // `artists_slug_idx` index, so cost is constant regardless of
    // corpus size.
    if let Some(slug) = params
        .artist
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        sql.push_str(" AND ar.slug =");
        push(
            &mut sql,
            &mut binds,
            &mut next,
            BoundParam::Text(slug.to_string()),
        );
    }

    // Artist-id set (the "map = view of grid result" path). When the
    // /search page is in map mode it forwards the artist ids it just
    // pulled out of the grid response — that means the map shows
    // exactly the artists that matched the user's search, hybrid
    // ranking and all, without re-running the embedding.
    if let Some(ids) = parse_artist_ids(params.artist_ids.as_deref())? {
        sql.push_str(" AND ar.id = ANY(");
        push(&mut sql, &mut binds, &mut next, BoundParam::UuidArr(ids));
        sql.push(')');
    }

    // Keyword / medium filters require the artist to have at least one
    // matching artwork. We express that with an EXISTS subquery so the
    // join stays a per-location row and we don't double-count.
    let has_q = params
        .q
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let has_medium = params
        .medium
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if has_q || has_medium {
        sql.push_str(
            " AND EXISTS (
                SELECT 1 FROM artworks aw
                WHERE aw.artist_id = ar.id
                  AND aw.deleted_at IS NULL
                  AND aw.status = 'published'",
        );
        if let Some(q) = params.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            sql.push_str(" AND aw.search_tsv @@ plainto_tsquery('english',");
            push(
                &mut sql,
                &mut binds,
                &mut next,
                BoundParam::Text(q.to_string()),
            );
            sql.push(')');
        }
        if let Some(m) = params
            .medium
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            sql.push_str(" AND aw.medium =");
            push(
                &mut sql,
                &mut binds,
                &mut next,
                BoundParam::Text(m.to_string()),
            );
        }
        sql.push(')');
    }

    sql.push_str(" ORDER BY al.id, al.display_order ASC LIMIT ");
    next += 1;
    sql.push_str(&format!("${next}"));
    binds.push(BoundParam::I64(MAP_PIN_LIMIT));

    // Build the sqlx query, applying binds in order. `AssertSqlSafe`
    // is the audited-by-hand escape from sqlx's `'static str` bound:
    // the SQL is hand-assembled from string literals + positional
    // bind markers — no user input ever lands in the string itself.
    let mut q = sqlx::query_as::<_, MapPinRow>(AssertSqlSafe(sql));
    for b in binds {
        q = match b {
            BoundParam::F64(v) => q.bind(v),
            BoundParam::Text(v) => q.bind(v),
            BoundParam::I64(v) => q.bind(v),
            BoundParam::UuidArr(v) => q.bind(v),
        };
    }
    let rows: Vec<MapPinRow> = q.fetch_all(&state.pool).await?;

    Ok(Json(rows.into_iter().map(MapPinRow::into_dto).collect()))
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct Bbox {
    west: f64,
    south: f64,
    east: f64,
    north: f64,
}

/// Parse a "west,south,east,north" bbox. Returns `Ok(None)` when the
/// caller omits the param. Returns a validation error for malformed
/// values, out-of-range lat/lng, or an inverted bbox.
fn parse_bbox(input: Option<&str>) -> Result<Option<Bbox>, ApiError> {
    let Some(s) = input else {
        return Ok(None);
    };
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 4 {
        return Err(ApiError::BadRequest(
            "bbox must be 'west,south,east,north' (4 comma-separated floats)".into(),
        ));
    }
    let mut nums = [0f64; 4];
    for (i, p) in parts.iter().enumerate() {
        nums[i] = p.trim().parse::<f64>().map_err(|_| {
            ApiError::BadRequest(format!("bbox component {i} is not a number: {p:?}"))
        })?;
    }
    let [west, south, east, north] = nums;
    if !(-180.0..=180.0).contains(&west)
        || !(-180.0..=180.0).contains(&east)
        || !(-90.0..=90.0).contains(&south)
        || !(-90.0..=90.0).contains(&north)
    {
        return Err(ApiError::BadRequest(
            "bbox lng must be in [-180, 180] and lat in [-90, 90]".into(),
        ));
    }
    if west >= east || south >= north {
        return Err(ApiError::BadRequest(
            "bbox must have west < east and south < north".into(),
        ));
    }
    Ok(Some(Bbox {
        west,
        south,
        east,
        north,
    }))
}

/// Build the `LIKE` filter for the location column. Returns the
/// pattern (lowercased + wildcard-wrapped) or `None` when the input is
/// empty / whitespace.
fn location_filter(input: Option<&str>) -> Option<String> {
    let v = input?.trim();
    if v.is_empty() {
        return None;
    }
    // Escape `%` and `_` in user input so they can't accidentally
    // widen the match. (Postgres doesn't ESCAPE by default; we wrap
    // with `%…%`.)
    let escaped = v
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    Some(format!("%{}%", escaped.to_lowercase()))
}

/// Parse the `?artist_ids=uuid1,uuid2,…` query param. Returns:
///   - `None` when absent / empty (no filter)
///   - `Some(vec)` of parsed UUIDs (deduped + capped at 500)
///   - `Err` if any token isn't a valid UUID (we'd rather 400 than
///     silently drop ids — caller likely sent a bug)
fn parse_artist_ids(raw: Option<&str>) -> Result<Option<Vec<Uuid>>, ApiError> {
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
    // Cap to keep the URL + IN-list reasonable. 500 mirrors
    // MAP_PIN_LIMIT — there's no point asking for more pins than
    // there are artists that could place them.
    if ids.len() > 500 {
        ids.truncate(500);
    }
    if ids.is_empty() {
        Ok(None)
    } else {
        Ok(Some(ids))
    }
}

#[derive(Debug)]
enum BoundParam {
    F64(f64),
    Text(String),
    I64(i64),
    /// Bound as `uuid[]` for `WHERE ar.id = ANY($N)` filters. Used by
    /// the `?artist_ids=` thread-through path.
    UuidArr(Vec<Uuid>),
}

#[derive(FromRow)]
struct MapPinRow {
    location_id: Uuid,
    lat: f64,
    lng: f64,
    name: String,
    kind: String,
    city: Option<String>,
    country: Option<String>,
    artist_slug: String,
    artist_display_name: String,
    primary_s3_key: Option<String>,
}

impl MapPinRow {
    fn into_dto(self) -> MapPin {
        MapPin {
            location_id: self.location_id,
            lat: self.lat,
            lng: self.lng,
            name: self.name,
            kind: self.kind,
            city: self.city,
            country: self.country,
            artist: MapPinArtist {
                slug: self.artist_slug,
                display_name: self.artist_display_name,
                primary_image_url: self.primary_s3_key.map(|k| url_for_s3_key(&k)),
            },
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests for the helpers — full SQL behavior is exercised in
// `tests/search_map_test.rs`.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bbox_happy() {
        let b = parse_bbox(Some("-1,50,1,52")).unwrap().unwrap();
        assert_eq!(b.west, -1.0);
        assert_eq!(b.south, 50.0);
        assert_eq!(b.east, 1.0);
        assert_eq!(b.north, 52.0);
    }

    #[test]
    fn parse_bbox_missing_is_none() {
        assert!(parse_bbox(None).unwrap().is_none());
    }

    #[test]
    fn parse_bbox_wrong_count_rejected() {
        assert!(parse_bbox(Some("-1,50,1")).is_err());
        assert!(parse_bbox(Some("-1,50,1,52,99")).is_err());
    }

    #[test]
    fn parse_bbox_non_numeric_rejected() {
        assert!(parse_bbox(Some("a,b,c,d")).is_err());
    }

    #[test]
    fn parse_bbox_out_of_range_rejected() {
        assert!(parse_bbox(Some("-181,0,0,0")).is_err());
        assert!(parse_bbox(Some("0,-91,1,1")).is_err());
    }

    #[test]
    fn parse_bbox_inverted_rejected() {
        // west >= east
        assert!(parse_bbox(Some("10,0,5,1")).is_err());
        // south >= north
        assert!(parse_bbox(Some("0,10,1,5")).is_err());
    }

    #[test]
    fn location_filter_escapes_wildcards() {
        // Raw `%` from the user must not turn into a wildcard.
        let pat = location_filter(Some("100% pure")).unwrap();
        assert!(pat.starts_with('%') && pat.ends_with('%'));
        assert!(pat.contains("100\\%"));
        // Underscore likewise.
        let pat2 = location_filter(Some("a_b")).unwrap();
        assert!(pat2.contains("a\\_b"));
    }

    #[test]
    fn location_filter_empty_is_none() {
        assert!(location_filter(None).is_none());
        assert!(location_filter(Some("   ")).is_none());
    }
}
