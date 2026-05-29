//! Force-geocode a specific `artist_locations` row.
//!
//! Operational tool — not part of the request path. Loads config from
//! the same `.env` the api uses, builds a `GeocodingClient` from
//! `MAPBOX_TOKEN`, and calls `geocode_and_update` synchronously on the
//! given row id.
//!
//! Usage:
//!   cargo run -p api-search --bin regeocode -- <location-uuid>
//!
//! Exits non-zero on bad input or DB failure. Prints the row before
//! and after so it's easy to eyeball what changed.

use std::process::ExitCode;

use ml_art_core::{
    config::Config,
    geocoding::{geocode_and_update, GeocodingClient},
};
use sqlx::Row;
use uuid::Uuid;

#[tokio::main]
async fn main() -> ExitCode {
    ml_art_core::telemetry::init();

    let id = match std::env::args().nth(1) {
        Some(s) => match Uuid::parse_str(&s) {
            Ok(u) => u,
            Err(e) => {
                eprintln!("error: invalid uuid: {e}");
                return ExitCode::from(2);
            }
        },
        None => {
            eprintln!("usage: regeocode <location-uuid>");
            return ExitCode::from(2);
        }
    };

    let cfg = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: config load failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let pool = match ml_art_core::db::make_pool(&cfg.database_url).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: db pool failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let client = GeocodingClient::from_env();
    if !client.enabled() {
        eprintln!(
            "warning: MAPBOX_TOKEN is unset — the call will succeed but \
             leave lat/lng NULL. Set MAPBOX_TOKEN in api/.env first."
        );
    }

    println!("─── before ─────────────────────────────────────────────");
    if let Err(e) = print_row(&pool, id).await {
        eprintln!("error: row lookup failed: {e}");
        return ExitCode::FAILURE;
    }

    // Clear geocoded_at so the helper writes fresh values rather than
    // skipping or stamping stale-already.
    if let Err(e) = sqlx::query(
        "UPDATE artist_locations SET geocoded_at = NULL, lat = NULL, lng = NULL, \
         city = NULL, country = NULL WHERE id = $1",
    )
    .bind(id)
    .execute(&pool)
    .await
    {
        eprintln!("error: reset row failed: {e}");
        return ExitCode::FAILURE;
    }

    if let Err(e) = geocode_and_update(&client, &pool, id).await {
        eprintln!("error: geocode failed: {e}");
        return ExitCode::FAILURE;
    }

    println!("─── after ──────────────────────────────────────────────");
    if let Err(e) = print_row(&pool, id).await {
        eprintln!("error: post-row lookup failed: {e}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

async fn print_row(pool: &ml_art_core::db::Pool, id: Uuid) -> sqlx::Result<()> {
    let row = sqlx::query(
        "SELECT kind, name, address, city, country, lat, lng, geocoded_at \
         FROM artist_locations WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    match row {
        None => {
            println!("(row not found)");
        }
        Some(r) => {
            let kind: String = r.try_get("kind")?;
            let name: String = r.try_get("name")?;
            let address: String = r.try_get("address")?;
            let city: Option<String> = r.try_get("city")?;
            let country: Option<String> = r.try_get("country")?;
            let lat: Option<f64> = r.try_get("lat")?;
            let lng: Option<f64> = r.try_get("lng")?;
            let geocoded_at: Option<chrono::DateTime<chrono::Utc>> = r.try_get("geocoded_at")?;
            println!("  kind:        {kind}");
            println!("  name:        {name}");
            println!("  address:     {address}");
            println!("  city:        {city:?}");
            println!("  country:     {country:?}");
            println!("  lat,lng:     {lat:?}, {lng:?}");
            println!("  geocoded_at: {geocoded_at:?}");
        }
    }
    Ok(())
}
