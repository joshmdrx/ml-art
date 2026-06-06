//! `/v1/search/map/cities` — T-042.
//!
//! Returns top N cities by venue count, with a bbox per city so the
//! map can `fitBounds` to it precisely. Used by `/search?map=1` as a
//! "where do I start?" affordance: the cold-start view shows a strip
//! of city pills like "London (12) · Berlin (8) · Lisbon (3)"; click
//! one and the map zooms there.
//!
//! Accepts the same `q` + `medium` filters as `/v1/search/map` so the
//! pivots reflect the active search: a city only appears if at least
//! one artist there has an artwork matching the filter. Without this
//! the strip lies — it'd say "Basingstoke (1)" for `?q=blue` even
//! when the Basingstoke artist has no "blue" works, so clicking the
//! pill would zoom to an empty map.

use crate::AppState;
use axum::{extract::State, Json};
use ml_art_core::error::ApiError;
use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, FromRow};
use std::sync::Arc;
use uuid::Uuid;

/// Default + max for `?limit=`. We don't expect more than a few dozen
/// cities at v0 scale; the cap keeps the response small + cheap.
const DEFAULT_LIMIT: i64 = 12;
const MAX_LIMIT: i64 = 100;

#[derive(Debug, Deserialize)]
pub struct CitiesParams {
    /// How many cities to return (default 12, max 100).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Full-text search query — must match an artwork's `search_tsv`
    /// on at least one of the artist's published works for the city
    /// to count.
    #[serde(default)]
    pub q: Option<String>,
    /// Restrict to artists with at least one artwork of this medium.
    #[serde(default)]
    pub medium: Option<String>,
    /// Comma-separated UUID list — same shape + semantics as
    /// `/v1/search/map?artist_ids=`. When set, the strip aggregates
    /// only cities containing those artists (so the strip matches
    /// the map pins beneath it when both are fed the same id set).
    #[serde(default)]
    pub artist_ids: Option<String>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct CityPivot {
    pub city: String,
    pub country: Option<String>,
    pub count: i64,
    /// Centroid of all locations in this city. Lets the client jump
    /// to a useful viewport even when bbox degenerates to a point.
    pub center_lat: f64,
    pub center_lng: f64,
    /// Tight bbox of every location in this city. Equals the centroid
    /// when there's exactly one row — the client uses fitBounds with
    /// a minimum-zoom hint so a single pin still gets a sensible
    /// viewport.
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

pub async fn handle(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<CitiesParams>,
) -> Result<Json<Vec<CityPivot>>, ApiError> {
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let q = params
        .q
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let medium = params
        .medium
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let artist_ids = parse_artist_ids(params.artist_ids.as_deref())?;

    // Build the SQL incrementally so the artwork-matching EXISTS only
    // appears when there's an actual filter. Mirrors the shape used
    // by `search_map::handle` for the same join — see comment there
    // re: why EXISTS over JOIN (avoids duplicate-per-artwork rows).
    //
    // Param binding order (stable, in case we change clauses later):
    //   $1 = limit (always)
    //   $2 = artist_ids (when set)
    //   then $N for q, then $N for medium
    let mut sql = String::from(
        "SELECT
            al.city                       AS city,
            al.country                    AS country,
            count(*)::bigint              AS count,
            avg(al.lat)::double precision AS center_lat,
            avg(al.lng)::double precision AS center_lng,
            min(al.lng)::double precision AS west,
            min(al.lat)::double precision AS south,
            max(al.lng)::double precision AS east,
            max(al.lat)::double precision AS north
        FROM artist_locations al
        JOIN artists ar ON ar.id = al.artist_id
        WHERE al.deleted_at IS NULL
          AND al.lat IS NOT NULL
          AND al.lng IS NOT NULL
          AND al.city IS NOT NULL
          AND ar.deleted_at IS NULL
          AND ar.status = 'active'",
    );

    let mut idx = 1usize; // $1 is limit
    if artist_ids.is_some() {
        idx += 1;
        sql.push_str(&format!(" AND ar.id = ANY(${idx})"));
    }

    if q.is_some() || medium.is_some() {
        sql.push_str(
            " AND EXISTS (
                SELECT 1 FROM artworks aw
                WHERE aw.artist_id = ar.id
                  AND aw.deleted_at IS NULL
                  AND aw.status = 'published'",
        );
        if q.is_some() {
            idx += 1;
            sql.push_str(&format!(
                " AND aw.search_tsv @@ plainto_tsquery('english', ${idx})"
            ));
        }
        if medium.is_some() {
            idx += 1;
            sql.push_str(&format!(" AND aw.medium = ${idx}"));
        }
        sql.push(')');
    }

    sql.push_str(" GROUP BY al.city, al.country ORDER BY count DESC, al.city ASC LIMIT $1");

    let mut qb = sqlx::query_as::<_, CityPivot>(AssertSqlSafe(sql)).bind(limit);
    if let Some(ids) = artist_ids {
        qb = qb.bind(ids);
    }
    if let Some(q) = q {
        qb = qb.bind(q);
    }
    if let Some(m) = medium {
        qb = qb.bind(m);
    }
    let rows: Vec<CityPivot> = qb.fetch_all(&state.pool).await?;
    Ok(Json(rows))
}

/// Parse a `?artist_ids=uuid1,uuid2` value. Mirrors the helper in
/// `search_map` but inlined to avoid making that one `pub` for the
/// sake of a single caller — duplication is cheap here.
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
    if ids.len() > 500 {
        ids.truncate(500);
    }
    Ok(if ids.is_empty() { None } else { Some(ids) })
}
