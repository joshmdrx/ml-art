//! T-080 — FX rates + canonical-GBP price maintenance.
//!
//! The platform's price filter operates in GBP (initial focus is UK
//! artists). Artists set prices in their own currency; we persist a
//! `price_gbp_cents` column on `artworks` so search filters can use
//! a single, indexed comparison without per-query JOIN+convert math.
//!
//! Two operations live here:
//!
//! 1. [`refresh_rates`] — calls the Frankfurter API (ECB data, no API
//!    key) for the latest rates against GBP, upserts each row into
//!    `fx_rates`, then bulk-recomputes `artworks.price_gbp_cents`
//!    against the new rates.
//! 2. [`compute_price_gbp_cents`] — point lookup used by studio write
//!    handlers when they insert or update a single artwork. Reads the
//!    current rate from the same `fx_rates` table.
//!
//! Cron not live yet — invoke via `jobs-worker --enqueue` until real
//! artists onboard. The seed rates in migration `0023_fx_rates.sql`
//! give us a usable day-1 baseline.

use crate::db::Pool;
use serde::Deserialize;

/// The free Frankfurter endpoint. ECB-sourced rates, GBP base.
const FRANKFURTER_URL: &str = "https://api.frankfurter.app/latest";

/// Currencies we ask Frankfurter for. Mirrors the seed in
/// `0023_fx_rates.sql`. Extending the list is a code change + a
/// migration that seeds the row — the API itself will return whatever
/// you ask for from its supported set.
const TRACKED_CODES: &[&str] = &["USD", "EUR", "CAD", "AUD", "JPY"];

#[derive(Debug, Deserialize)]
struct FrankfurterResponse {
    /// `{ "USD": 1.27, ... }` where 1 GBP = the named amount in that
    /// currency. We invert to store "GBP per 1 unit of code".
    rates: std::collections::HashMap<String, f64>,
}

/// Result of a refresh run for telemetry / the caller's log line.
#[derive(Debug)]
pub struct RefreshResult {
    pub rates_updated: usize,
    pub artworks_repriced: u64,
}

/// Pull the latest rates from Frankfurter, upsert into `fx_rates`,
/// recompute `artworks.price_gbp_cents` across the corpus.
///
/// GBP itself stays pinned at `1.0` (the API doesn't return it when
/// it's the base; we maintain it via the migration seed and never
/// touch it here).
pub async fn refresh_rates(pool: &Pool) -> anyhow::Result<RefreshResult> {
    let symbols = TRACKED_CODES.join(",");
    let url = format!("{FRANKFURTER_URL}?base=GBP&symbols={symbols}");
    let resp: FrankfurterResponse = reqwest::get(&url).await?.error_for_status()?.json().await?;

    let mut tx = pool.begin().await?;
    let mut rates_updated = 0usize;
    for code in TRACKED_CODES {
        let Some(per_gbp) = resp.rates.get(*code) else {
            tracing::warn!(code = %code, "frankfurter response missing currency; leaving stale rate in place");
            continue;
        };
        if *per_gbp <= 0.0 || !per_gbp.is_finite() {
            tracing::warn!(code = %code, value = per_gbp, "non-positive / non-finite rate; skipping");
            continue;
        }
        // 1 GBP = `per_gbp` units of `code` → 1 unit of `code` = `1/per_gbp` GBP.
        let rate_to_gbp = 1.0 / *per_gbp;
        sqlx::query(
            r#"
            INSERT INTO fx_rates (code, rate_to_gbp, fetched_at)
            VALUES ($1, $2, now())
            ON CONFLICT (code) DO UPDATE SET
                rate_to_gbp = EXCLUDED.rate_to_gbp,
                fetched_at  = EXCLUDED.fetched_at
            "#,
        )
        .bind(code)
        .bind(rate_to_gbp)
        .execute(&mut *tx)
        .await?;
        rates_updated += 1;
    }

    // Recompute price_gbp_cents from the freshly-upserted rates.
    // LEFT JOIN so artworks priced in a currency we don't track stay
    // at NULL rather than getting stale values from a previous run.
    let result = sqlx::query(
        r#"
        UPDATE artworks a
        SET price_gbp_cents = CASE
            WHEN a.price_cents IS NULL OR fx.rate_to_gbp IS NULL THEN NULL
            ELSE ROUND(a.price_cents * fx.rate_to_gbp)::bigint
          END
        FROM (SELECT code, rate_to_gbp FROM fx_rates) fx
        WHERE fx.code = a.currency
        "#,
    )
    .execute(&mut *tx)
    .await?;
    let artworks_repriced = result.rows_affected();

    tx.commit().await?;

    tracing::info!(rates_updated, artworks_repriced, "fx_rates refreshed");
    Ok(RefreshResult {
        rates_updated,
        artworks_repriced,
    })
}

/// Convert a `(price_cents, currency)` pair to GBP minor units using
/// the current `fx_rates`. Returns `None` when `price_cents` is None
/// (POA / inquire-only) OR the currency isn't tracked. Studio write
/// handlers call this so newly-created / patched artworks have a
/// correct GBP value without waiting for the next FX-refresh sweep.
pub async fn compute_price_gbp_cents(
    pool: &Pool,
    price_cents: Option<i64>,
    currency: &str,
) -> sqlx::Result<Option<i64>> {
    let Some(price) = price_cents else {
        return Ok(None);
    };
    let row: Option<(f64,)> =
        sqlx::query_as("SELECT rate_to_gbp::float8 FROM fx_rates WHERE code = $1")
            .bind(currency)
            .fetch_optional(pool)
            .await?;
    let Some((rate,)) = row else { return Ok(None) };
    Ok(Some((price as f64 * rate).round() as i64))
}
