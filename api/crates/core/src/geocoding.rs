//! T-038 G2 — Mapbox forward geocoding.
//!
//! Turns a street address into `(lat, lng, city, country)` so a new
//! `artist_locations` row becomes a real map pin. Mirrors the
//! degrades-gracefully pattern used elsewhere in this crate:
//!
//! - `MAPBOX_TOKEN` set: real Mapbox v6 forward-geocoding calls.
//! - `MAPBOX_TOKEN` unset: `Disabled` variant; `geocode_address()`
//!   returns `Ok(None)` and the row keeps its NULL lat/lng (hidden from
//!   public surfaces until a real geocode lands).
//! - `for_tests()`: in-memory canned responses, no network.
//!
//! Why this isn't an Inngest job (yet): Inngest isn't wired up in the
//! codebase. To unblock G3 (studio CRUD) without standing up an
//! orchestrator, the studio handlers call `trigger_background_geocode`
//! which `tokio::spawn`s the work on the same process. Crashes lose
//! the in-flight task — but the row is preserved with `geocoded_at IS
//! NULL`, the `artist_locations_geocode_pending_idx` partial index
//! finds it on next pass, and a periodic re-scan (or eventual Inngest
//! `artist_location.geocode` function) can pick it back up. See
//! `decisions.md` 2026-05-28 — Geography promoted to v1.

use crate::db::Pool;
use chrono::Utc;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};
use uuid::Uuid;

const MAPBOX_FORWARD_URL: &str = "https://api.mapbox.com/search/geocode/v6/forward";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Result of geocoding an address.
///
/// `lng` and `lat` are required for the row to become publicly visible
/// (see `api-search::artist`). `city` and `country` are best-effort
/// metadata from Mapbox's `context` block; either may be `None` if
/// Mapbox couldn't classify the address that finely.
#[derive(Debug, Clone, PartialEq)]
pub struct Geocoded {
    pub lng: f64,
    pub lat: f64,
    pub city: Option<String>,
    pub country: Option<String>,
}

/// Geocoding client. Three variants:
///
/// - `Real` — production path; HTTP calls to Mapbox v6
/// - `Disabled` — `MAPBOX_TOKEN` absent; every call returns `Ok(None)`
/// - `Test` — canned `(address → result)` map, no network
#[derive(Clone)]
pub struct GeocodingClient {
    inner: Arc<Inner>,
}

enum Inner {
    Real {
        token: String,
        http: reqwest::Client,
    },
    Disabled,
    Test {
        canned: Vec<(String, Option<Geocoded>)>,
    },
}

impl GeocodingClient {
    /// Production constructor. Reads `MAPBOX_TOKEN`; falls back to
    /// `Disabled` when unset so local dev + tests don't blow up.
    pub fn from_env() -> Self {
        match std::env::var("MAPBOX_TOKEN") {
            Ok(token) if !token.trim().is_empty() => Self::real(token),
            _ => Self::disabled(),
        }
    }

    pub fn real(token: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .expect("reqwest client");
        Self {
            inner: Arc::new(Inner::Real { token, http }),
        }
    }

    pub fn disabled() -> Self {
        Self {
            inner: Arc::new(Inner::Disabled),
        }
    }

    /// Test constructor — `canned` is matched exactly against the
    /// passed address. Match returns the canned `Option<Geocoded>`;
    /// non-match returns `Ok(None)` (mirroring `Disabled`).
    pub fn for_tests(canned: Vec<(String, Option<Geocoded>)>) -> Self {
        Self {
            inner: Arc::new(Inner::Test { canned }),
        }
    }

    /// `true` when this client will actually call Mapbox. Handlers can
    /// surface a UI hint ("Geocoding is disabled in this environment")
    /// when it's `false`.
    pub fn enabled(&self) -> bool {
        matches!(*self.inner, Inner::Real { .. })
    }

    /// Forward-geocode an address.
    ///
    /// - `Ok(Some(g))` — Mapbox returned at least one feature; first one
    ///   wins. We don't expose alternatives; the artist re-edits the
    ///   address if the pin is wrong.
    /// - `Ok(None)` — Mapbox returned zero features, OR the token is
    ///   unset (Disabled variant). Caller writes `geocoded_at = now()`
    ///   in both cases so we don't infinitely retry.
    /// - `Err(_)` — network / parse error. Caller does NOT update
    ///   `geocoded_at`; the row stays in the pending queue.
    pub async fn geocode_address(&self, address: &str) -> Result<Option<Geocoded>, GeocodeError> {
        match &*self.inner {
            Inner::Disabled => Ok(None),
            Inner::Test { canned } => Ok(canned
                .iter()
                .find(|(a, _)| a == address)
                .and_then(|(_, g)| g.clone())),
            Inner::Real { token, http } => mapbox_forward(http, token, address).await,
        }
    }
}

async fn mapbox_forward(
    http: &reqwest::Client,
    token: &str,
    address: &str,
) -> Result<Option<Geocoded>, GeocodeError> {
    let resp = http
        .get(MAPBOX_FORWARD_URL)
        .query(&[
            ("q", address),
            ("access_token", token),
            ("limit", "1"),
            // We don't restrict types — galleries / studios can be inside
            // POIs, addresses, places. Mapbox ranks by relevance and we
            // accept the top hit.
        ])
        .send()
        .await
        .map_err(|e| GeocodeError::Http(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(GeocodeError::Status {
            status: status.as_u16(),
            body,
        });
    }

    let parsed: MapboxForwardResponse = resp
        .json()
        .await
        .map_err(|e| GeocodeError::Parse(e.to_string()))?;

    Ok(parsed.into_first_geocoded())
}

#[derive(Debug, thiserror::Error)]
pub enum GeocodeError {
    #[error("mapbox HTTP error: {0}")]
    Http(String),
    #[error("mapbox returned {status}: {body}")]
    Status { status: u16, body: String },
    #[error("mapbox response parse error: {0}")]
    Parse(String),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

// ─────────────────────────────────────────────────────────────────────────────
// Mapbox v6 response shape
// https://docs.mapbox.com/api/search/geocoding-v6/
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct MapboxForwardResponse {
    #[serde(default)]
    features: Vec<MapboxFeature>,
}

#[derive(Debug, Deserialize)]
struct MapboxFeature {
    geometry: MapboxGeometry,
    #[serde(default)]
    properties: MapboxProperties,
}

#[derive(Debug, Deserialize)]
struct MapboxGeometry {
    /// `[lng, lat]` — GeoJSON ordering, NOT (lat, lng).
    coordinates: [f64; 2],
}

#[derive(Debug, Default, Deserialize)]
struct MapboxProperties {
    #[serde(default)]
    context: MapboxContext,
}

#[derive(Debug, Default, Deserialize)]
struct MapboxContext {
    place: Option<MapboxContextEntry>,
    country: Option<MapboxContextEntry>,
}

#[derive(Debug, Deserialize)]
struct MapboxContextEntry {
    #[serde(default)]
    name: Option<String>,
    /// ISO 3166-1 alpha-2, lowercase, only present on country entries.
    #[serde(default)]
    country_code: Option<String>,
}

impl MapboxForwardResponse {
    fn into_first_geocoded(self) -> Option<Geocoded> {
        let feature = self.features.into_iter().next()?;
        let [lng, lat] = feature.geometry.coordinates;
        let city = feature.properties.context.place.and_then(|p| p.name);
        let country = feature
            .properties
            .context
            .country
            .and_then(|c| c.country_code)
            .map(|c| c.to_uppercase());
        Some(Geocoded {
            lng,
            lat,
            city,
            country,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Background-geocode helper used by studio CRUD handlers
// ─────────────────────────────────────────────────────────────────────────────

/// Spawn a background task that geocodes the given `artist_locations`
/// row and writes the result back to the database. Returns immediately;
/// the caller (typically a studio CRUD handler) should not await.
///
/// On success the row gets `lat`, `lng`, `city`, `country`, and a fresh
/// `geocoded_at`. On `Ok(None)` (no Mapbox match, or token disabled)
/// only `geocoded_at` is set — preventing a retry storm while keeping
/// lat/lng NULL so the row stays hidden from public surfaces. On hard
/// error (network, parse) we log and leave the row untouched; the
/// pending-index will surface it for a future retry.
pub fn trigger_background_geocode(client: GeocodingClient, pool: Pool, location_id: Uuid) {
    tokio::spawn(async move {
        if let Err(e) = geocode_and_update(&client, &pool, location_id).await {
            warn!(
                location_id = %location_id,
                error = %e,
                "background geocode failed",
            );
        }
    });
}

/// Synchronous variant — looks up the address for `location_id`, calls
/// the client, and writes the result back. Exposed so the studio CRUD
/// path can call it directly when waiting on the result is cheap (test
/// environment, disabled client), and so integration tests can drive
/// the full path without racing with `tokio::spawn`.
pub async fn geocode_and_update(
    client: &GeocodingClient,
    pool: &Pool,
    location_id: Uuid,
) -> Result<(), GeocodeError> {
    // Read the address fresh — the row may have been edited between
    // enqueue and run.
    let row: Option<(String,)> = sqlx::query_as(
        r#"SELECT address FROM artist_locations
           WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(location_id)
    .fetch_optional(pool)
    .await?;

    let Some((address,)) = row else {
        debug!(%location_id, "location row gone before geocode ran");
        return Ok(());
    };

    match client.geocode_address(&address).await? {
        Some(g) => {
            sqlx::query(
                r#"UPDATE artist_locations
                   SET lat = $2, lng = $3, city = $4, country = $5,
                       geocoded_at = $6, updated_at = $6
                   WHERE id = $1"#,
            )
            .bind(location_id)
            .bind(g.lat)
            .bind(g.lng)
            .bind(g.city)
            .bind(g.country)
            .bind(Utc::now())
            .execute(pool)
            .await?;
        }
        None => {
            // Mapbox returned nothing, or token is disabled. Stamp
            // geocoded_at so we don't loop forever on the same address.
            sqlx::query(
                r#"UPDATE artist_locations
                   SET geocoded_at = $2, updated_at = $2
                   WHERE id = $1"#,
            )
            .bind(location_id)
            .bind(Utc::now())
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Real shape returned by Mapbox v6 forward-geocoding (sample
    /// trimmed to the fields we care about). Pinning this in a test
    /// catches the day Mapbox renames `context.place.name` etc.
    const SAMPLE_RESPONSE: &str = r#"
    {
      "type": "FeatureCollection",
      "features": [
        {
          "type": "Feature",
          "id": "addr.123",
          "geometry": {
            "type": "Point",
            "coordinates": [-0.0922, 51.5155]
          },
          "properties": {
            "feature_type": "address",
            "full_address": "1 Test St, London EC1A 1AA, United Kingdom",
            "context": {
              "place": { "name": "London", "place_type": ["city"] },
              "country": { "name": "United Kingdom", "country_code": "gb" }
            }
          }
        }
      ]
    }
    "#;

    #[test]
    fn parses_a_real_mapbox_response() {
        let parsed: MapboxForwardResponse = serde_json::from_str(SAMPLE_RESPONSE).unwrap();
        let g = parsed.into_first_geocoded().unwrap();
        assert!((g.lng - -0.0922).abs() < 1e-6);
        assert!((g.lat - 51.5155).abs() < 1e-6);
        assert_eq!(g.city.as_deref(), Some("London"));
        assert_eq!(g.country.as_deref(), Some("GB"));
    }

    #[test]
    fn empty_features_yields_none() {
        let parsed: MapboxForwardResponse =
            serde_json::from_str(r#"{ "type": "FeatureCollection", "features": [] }"#).unwrap();
        assert!(parsed.into_first_geocoded().is_none());
    }

    #[test]
    fn missing_context_still_yields_lat_lng() {
        // Some Mapbox responses (e.g. raw addresses with no
        // higher-order admin matches) come back without place / country
        // context. We still want the pin to land.
        let raw = r#"{
          "type": "FeatureCollection",
          "features": [
            {
              "type": "Feature",
              "geometry": { "type": "Point", "coordinates": [10.0, 20.0] },
              "properties": {}
            }
          ]
        }"#;
        let parsed: MapboxForwardResponse = serde_json::from_str(raw).unwrap();
        let g = parsed.into_first_geocoded().unwrap();
        assert_eq!(g.lng, 10.0);
        assert_eq!(g.lat, 20.0);
        assert!(g.city.is_none());
        assert!(g.country.is_none());
    }

    #[tokio::test]
    async fn disabled_client_returns_none() {
        let c = GeocodingClient::disabled();
        assert!(!c.enabled());
        let g = c.geocode_address("1 Test St").await.unwrap();
        assert!(g.is_none());
    }

    #[tokio::test]
    async fn test_client_returns_canned_response() {
        let canned = vec![(
            "1 Test St, London".to_string(),
            Some(Geocoded {
                lng: -0.0922,
                lat: 51.5155,
                city: Some("London".to_string()),
                country: Some("GB".to_string()),
            }),
        )];
        let c = GeocodingClient::for_tests(canned);
        assert!(!c.enabled()); // test variant is not "real"

        let g = c.geocode_address("1 Test St, London").await.unwrap();
        assert_eq!(g.as_ref().map(|x| x.lat), Some(51.5155));

        // Address not in canned set → None (mirrors Disabled, not error).
        let g2 = c.geocode_address("unknown address").await.unwrap();
        assert!(g2.is_none());
    }
}
