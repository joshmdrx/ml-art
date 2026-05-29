//! `/v1/search/map/cities` — T-042.
//!
//! Returns top N cities by venue count, with a bbox per city so the
//! map can `fitBounds` to it precisely. Used by `/search?map=1` as a
//! "where do I start?" affordance: the cold-start view shows a strip
//! of city pills like "London (12) · Berlin (8) · Lisbon (3)"; click
//! one and the map zooms there.
//!
//! Cheap query: groups by `artist_locations.city` filtering out
//! pre-geocode rows and inactive artists. No bbox / search-context
//! params here — this is intentionally a static "what's on offer"
//! pivot, not a dynamic filter on the active search.

use crate::AppState;
use axum::{extract::State, Json};
use ml_art_core::error::ApiError;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;

/// Default + max for `?limit=`. We don't expect more than a few dozen
/// cities at v0 scale; the cap keeps the response small + cheap.
const DEFAULT_LIMIT: i64 = 12;
const MAX_LIMIT: i64 = 100;

#[derive(Debug, Deserialize)]
pub struct CitiesParams {
    /// How many cities to return (default 12, max 100).
    #[serde(default)]
    pub limit: Option<i64>,
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

    let rows: Vec<CityPivot> = sqlx::query_as(
        r#"
        SELECT
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
          AND ar.status = 'active'
        GROUP BY al.city, al.country
        ORDER BY count DESC, al.city ASC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows))
}
