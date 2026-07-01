//! T-084.1 — `/v1/admin/stats`. A read-only aggregate view over the
//! platform's core tables + the `events` behavioural stream. Feeds the
//! `/admin/stats` web page.
//!
//! Everything is one round-trip: a single endpoint returns headline
//! counts, a weekly funnel over the last 4 weeks, and recent admin
//! activity. Each SQL statement is small + indexed; the whole
//! response should land in ~200ms even at 10× current volume.

use crate::extractors::AdminUser;
use crate::AppState;
use axum::{extract::State, Json};
use chrono::{DateTime, Utc};
use ml_art_core::error::ApiError;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub counts: Counts,
    pub weekly_funnel: Vec<WeeklyFunnel>,
    pub admin_activity: AdminActivity,
}

#[derive(Debug, Serialize)]
pub struct Counts {
    pub users: TimeWindow,
    pub artists_active: TimeWindow,
    pub artworks_published: TimeWindow,
    pub inquiries_delivered: TimeWindow,
}

#[derive(Debug, Serialize)]
pub struct TimeWindow {
    pub total: i64,
    pub last_7d: i64,
    pub last_30d: i64,
}

#[derive(Debug, Serialize)]
pub struct WeeklyFunnel {
    /// ISO date of the week's Monday.
    pub week: chrono::NaiveDate,
    pub searches: i64,
    pub views: i64,
    pub inquiries_started: i64,
    pub inquiries_submitted: i64,
}

#[derive(Debug, Serialize)]
pub struct AdminActivity {
    pub mutations_last_7d: i64,
    pub last_mutation_at: Option<DateTime<Utc>>,
}

pub async fn handle(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> Result<Json<StatsResponse>, ApiError> {
    let pool = &state.pool;

    // Headline counts. Each `SELECT count(*) FILTER (WHERE …)` runs
    // over a small table with a status/date filter; indexes on
    // `created_at`, `published_at`, `delivered_at` make these cheap.
    let (users_total, users_7d, users_30d): (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            count(*) FILTER (WHERE true),
            count(*) FILTER (WHERE created_at > now() - interval '7 days'),
            count(*) FILTER (WHERE created_at > now() - interval '30 days')
        FROM users
        "#,
    )
    .fetch_one(pool)
    .await?;

    let (artists_total, artists_7d, artists_30d): (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            count(*) FILTER (WHERE true),
            count(*) FILTER (WHERE created_at > now() - interval '7 days'),
            count(*) FILTER (WHERE created_at > now() - interval '30 days')
        FROM artists
        WHERE deleted_at IS NULL AND status = 'active'
        "#,
    )
    .fetch_one(pool)
    .await?;

    let (artworks_total, artworks_7d, artworks_30d): (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            count(*) FILTER (WHERE true),
            count(*) FILTER (WHERE published_at > now() - interval '7 days'),
            count(*) FILTER (WHERE published_at > now() - interval '30 days')
        FROM artworks
        WHERE deleted_at IS NULL AND status = 'published'
        "#,
    )
    .fetch_one(pool)
    .await?;

    let (inquiries_total, inquiries_7d, inquiries_30d): (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            count(*) FILTER (WHERE true),
            count(*) FILTER (WHERE delivered_at > now() - interval '7 days'),
            count(*) FILTER (WHERE delivered_at > now() - interval '30 days')
        FROM inquiries
        WHERE delivered_at IS NOT NULL
        "#,
    )
    .fetch_one(pool)
    .await?;

    // Weekly funnel — 4 weeks of search / view / inquiry activity.
    // The `date_trunc('week', ...)` groups Monday-to-Sunday; empty
    // weeks are represented by zero-rows via the generate_series
    // spine so the frontend can just render whatever it gets.
    let funnel_rows: Vec<(chrono::NaiveDate, i64, i64, i64, i64)> = sqlx::query_as(
        r#"
        WITH weeks AS (
            SELECT (date_trunc('week', gs)::date) AS week
            FROM generate_series(
                date_trunc('week', now() - interval '3 weeks'),
                date_trunc('week', now()),
                interval '1 week'
            ) gs
        ),
        agg AS (
            SELECT
                date_trunc('week', occurred_at)::date AS week,
                count(*) FILTER (WHERE event_name = 'search_executed') AS searches,
                count(*) FILTER (WHERE event_name = 'artwork_viewed') AS views,
                count(*) FILTER (WHERE event_name = 'inquiry_started') AS started,
                count(*) FILTER (WHERE event_name = 'inquiry_submitted') AS submitted
            FROM events
            WHERE occurred_at > now() - interval '4 weeks'
            GROUP BY 1
        )
        SELECT
            w.week,
            COALESCE(a.searches, 0)::bigint  AS searches,
            COALESCE(a.views, 0)::bigint     AS views,
            COALESCE(a.started, 0)::bigint   AS started,
            COALESCE(a.submitted, 0)::bigint AS submitted
        FROM weeks w
        LEFT JOIN agg a ON a.week = w.week
        ORDER BY w.week
        "#,
    )
    .fetch_all(pool)
    .await?;

    let weekly_funnel = funnel_rows
        .into_iter()
        .map(|(week, searches, views, started, submitted)| WeeklyFunnel {
            week,
            searches,
            views,
            inquiries_started: started,
            inquiries_submitted: submitted,
        })
        .collect();

    // Admin activity — how many mutations in the last 7 days + the
    // most recent one's timestamp. Useful for the "is anything
    // happening" glance.
    let (mutations_last_7d, last_mutation_at): (i64, Option<DateTime<Utc>>) = sqlx::query_as(
        r#"
        SELECT
            count(*) FILTER (WHERE created_at > now() - interval '7 days')::bigint,
            max(created_at)
        FROM admin_audit_log
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(Json(StatsResponse {
        counts: Counts {
            users: TimeWindow {
                total: users_total,
                last_7d: users_7d,
                last_30d: users_30d,
            },
            artists_active: TimeWindow {
                total: artists_total,
                last_7d: artists_7d,
                last_30d: artists_30d,
            },
            artworks_published: TimeWindow {
                total: artworks_total,
                last_7d: artworks_7d,
                last_30d: artworks_30d,
            },
            inquiries_delivered: TimeWindow {
                total: inquiries_total,
                last_7d: inquiries_7d,
                last_30d: inquiries_30d,
            },
        },
        weekly_funnel,
        admin_activity: AdminActivity {
            mutations_last_7d,
            last_mutation_at,
        },
    }))
}
