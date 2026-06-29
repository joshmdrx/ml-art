//! T-058 — `/v1/studio/series/*`. Authenticated artist's own series.
//!
//! Endpoints:
//!   - `GET    /v1/studio/series`                  — list
//!   - `POST   /v1/studio/series`                  — create
//!   - `GET    /v1/studio/series/:id`              — detail
//!   - `PATCH  /v1/studio/series/:id`              — update title/statement/cover/slug
//!   - `DELETE /v1/studio/series/:id`              — soft-delete
//!   - `PUT    /v1/studio/series/:id/artworks`     — set membership (multi-select)
//!
//! Ownership is enforced in SQL: every query joins through
//! `series.artist_id = $current_artist_id`. A 404 leaks no info about
//! other artists' series; same pattern as the artwork handlers.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use ml_art_core::{error::ApiError, images::url_for_s3_key, models::SeriesStudio};
use serde::Deserialize;
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AuthedUser;
use crate::studio::current_artist_id;
use crate::AppState;

const MAX_TITLE_LEN: usize = 200;
const MAX_SLUG_LEN: usize = 100;
const MAX_STATEMENT_LEN: usize = 500;
/// Cap on a single PUT membership replacement. Generous (real artists
/// rarely have more than ~50 published works); guards against
/// pathological clients sending tens of thousands of ids.
const MAX_MEMBERSHIP_IDS: usize = 500;

// ─────────────────────────────────────────────────────────────────────────────
// Shared row + load helper
// ─────────────────────────────────────────────────────────────────────────────

#[derive(FromRow)]
struct SeriesRow {
    id: Uuid,
    slug: String,
    title: String,
    statement: Option<String>,
    cover_artwork_id: Option<Uuid>,
    cover_s3_key: Option<String>,
    artwork_count: i64,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

impl SeriesRow {
    fn into_studio(self) -> SeriesStudio {
        SeriesStudio {
            id: self.id,
            slug: self.slug,
            title: self.title,
            statement: self.statement,
            cover_artwork_id: self.cover_artwork_id,
            cover_image_url: self.cover_s3_key.as_deref().map(url_for_s3_key),
            artwork_count: i32::try_from(self.artwork_count).unwrap_or(0),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Pull a series's studio view, scoped to the current artist. The
/// cover image url is resolved here so callers don't issue N+1 fetches.
/// `cover_s3_key` falls back to the first member artwork's primary
/// image when `cover_artwork_id` is unset.
const STUDIO_ROW_SQL: &str = r#"
SELECT
    s.id,
    s.slug,
    s.title,
    s.statement,
    s.cover_artwork_id,
    COALESCE(cover_ai.s3_key, first_ai.s3_key) AS cover_s3_key,
    (SELECT COUNT(*)::bigint FROM artworks a
     WHERE a.series_id = s.id
       AND a.deleted_at IS NULL) AS artwork_count,
    s.created_at,
    s.updated_at
FROM series s
LEFT JOIN artwork_images cover_ai
       ON cover_ai.artwork_id = s.cover_artwork_id
      AND cover_ai.is_primary
      AND cover_ai.moderation_status = 'approved'
LEFT JOIN LATERAL (
    SELECT ai.s3_key
    FROM artworks a
    JOIN artwork_images ai
           ON ai.artwork_id = a.id
          AND ai.is_primary
          AND ai.moderation_status = 'approved'
    WHERE a.series_id = s.id
      AND a.deleted_at IS NULL
      AND a.status = 'published'
    ORDER BY a.published_at DESC NULLS LAST, a.created_at DESC
    LIMIT 1
) first_ai ON TRUE
WHERE s.artist_id = $1
  AND s.deleted_at IS NULL
"#;

// ─────────────────────────────────────────────────────────────────────────────
// Validation
// ─────────────────────────────────────────────────────────────────────────────

fn validate_title(title: &str) -> Result<String, ApiError> {
    let trimmed = title.trim().to_string();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("title: required".into()));
    }
    if trimmed.chars().count() > MAX_TITLE_LEN {
        return Err(ApiError::BadRequest(format!(
            "title: exceeds {MAX_TITLE_LEN} characters"
        )));
    }
    Ok(trimmed)
}

fn validate_statement(statement: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(s) = statement else {
        return Ok(None);
    };
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > MAX_STATEMENT_LEN {
        return Err(ApiError::BadRequest(format!(
            "statement: exceeds {MAX_STATEMENT_LEN} characters"
        )));
    }
    Ok(Some(trimmed.to_string()))
}

/// Kebab-case slug derived from title. Lowercased, non-ASCII chars
/// dropped (no NFKD fold — adds a dep for marginal value; an artist
/// who wants "café-visions" instead of "caf-visions" can edit the
/// slug after create). Non-alphanumeric collapsed to dashes,
/// leading/trailing dashes trimmed.
///
/// Empty result → "series" fallback so the NOT NULL slug holds. The
/// per-artist unique index surfaces collisions as 409 at insert
/// time; future iteration could auto-suffix (-2, -3) but v1 lets
/// the artist re-title.
fn slugify(input: &str) -> String {
    let lower = input.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut prev_dash = true;
    for c in lower.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "series".to_string()
    } else if trimmed.chars().count() > MAX_SLUG_LEN {
        trimmed.chars().take(MAX_SLUG_LEN).collect()
    } else {
        trimmed.to_string()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/studio/series — list
// ─────────────────────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct StudioSeriesList {
    pub items: Vec<SeriesStudio>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<StudioSeriesList>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;
    let sql = format!("{STUDIO_ROW_SQL} ORDER BY s.created_at DESC");
    let rows: Vec<SeriesRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(artist_id)
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(StudioSeriesList {
        items: rows.into_iter().map(SeriesRow::into_studio).collect(),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/studio/series — create
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateSeries {
    pub title: String,
    #[serde(default)]
    pub statement: Option<String>,
    /// Optional cover. If set, must be an artwork owned by the caller.
    #[serde(default)]
    pub cover_artwork_id: Option<Uuid>,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Json(body): Json<CreateSeries>,
) -> Result<(StatusCode, Json<SeriesStudio>), ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;
    let title = validate_title(&body.title)?;
    let statement = validate_statement(body.statement.as_deref())?;
    let slug = slugify(&title);
    validate_cover(&state.pool, artist_id, body.cover_artwork_id).await?;

    let id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO series (artist_id, slug, title, statement, cover_artwork_id)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
        "#,
    )
    .bind(artist_id)
    .bind(&slug)
    .bind(&title)
    .bind(statement.as_deref())
    .bind(body.cover_artwork_id)
    .fetch_one(&state.pool)
    .await
    .map_err(map_slug_conflict)?;

    let row = load_studio_row(&state.pool, artist_id, id).await?;
    Ok((StatusCode::CREATED, Json(row.into_studio())))
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/studio/series/:id — detail
// ─────────────────────────────────────────────────────────────────────────────

pub async fn detail(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path(id): Path<Uuid>,
) -> Result<Json<SeriesStudio>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;
    let row = load_studio_row(&state.pool, artist_id, id).await?;
    Ok(Json(row.into_studio()))
}

// ─────────────────────────────────────────────────────────────────────────────
// PATCH /v1/studio/series/:id — update
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PatchSeries {
    /// When present, replaces the title AND re-derives the slug.
    /// If a slug collision results, the request fails 409.
    #[serde(default)]
    pub title: Option<String>,
    /// Three-valued: absent → leave alone; `null` → clear; otherwise
    /// → replace. Uses the same double-option helper as the artist
    /// settings patch (T-072).
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::deserialize_double_option"
    )]
    pub statement: Option<Option<String>>,
    /// Same three-valued semantics for the cover picker.
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::deserialize_double_option"
    )]
    pub cover_artwork_id: Option<Option<Uuid>>,
}

pub async fn patch(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchSeries>,
) -> Result<Json<SeriesStudio>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    // Confirm the series exists + belongs to caller before any writes.
    let _existing = load_studio_row(&state.pool, artist_id, id).await?;

    let mut tx = state.pool.begin().await?;

    if let Some(title_raw) = body.title.as_deref() {
        let title = validate_title(title_raw)?;
        let slug = slugify(&title);
        sqlx::query(
            r#"UPDATE series SET title = $1, slug = $2, updated_at = now()
               WHERE id = $3 AND artist_id = $4"#,
        )
        .bind(&title)
        .bind(&slug)
        .bind(id)
        .bind(artist_id)
        .execute(&mut *tx)
        .await
        .map_err(map_slug_conflict)?;
    }

    if let Some(opt) = body.statement.as_ref() {
        let validated = validate_statement(opt.as_deref())?;
        sqlx::query(
            r#"UPDATE series SET statement = $1, updated_at = now()
               WHERE id = $2 AND artist_id = $3"#,
        )
        .bind(validated.as_deref())
        .bind(id)
        .bind(artist_id)
        .execute(&mut *tx)
        .await?;
    }

    if let Some(opt) = body.cover_artwork_id.as_ref() {
        if let Some(cover_id) = opt {
            // Validate inside the tx so a concurrent delete can't race.
            let ok: Option<(Uuid,)> = sqlx::query_as(
                "SELECT id FROM artworks WHERE id = $1 AND artist_id = $2 AND deleted_at IS NULL",
            )
            .bind(cover_id)
            .bind(artist_id)
            .fetch_optional(&mut *tx)
            .await?;
            if ok.is_none() {
                return Err(ApiError::BadRequest(
                    "cover_artwork_id: not an artwork owned by you".into(),
                ));
            }
        }
        sqlx::query(
            r#"UPDATE series SET cover_artwork_id = $1, updated_at = now()
               WHERE id = $2 AND artist_id = $3"#,
        )
        .bind(*opt)
        .bind(id)
        .bind(artist_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    let row = load_studio_row(&state.pool, artist_id, id).await?;
    Ok(Json(row.into_studio()))
}

// ─────────────────────────────────────────────────────────────────────────────
// DELETE /v1/studio/series/:id — soft-delete
// ─────────────────────────────────────────────────────────────────────────────

pub async fn delete(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;
    // Soft-delete the series. The FK `ON DELETE SET NULL` on
    // artworks.series_id doesn't fire on soft-delete — we explicitly
    // clear membership in the same transaction so the artwork rows
    // stop pointing at a (logically) gone series.
    let mut tx = state.pool.begin().await?;
    let affected = sqlx::query(
        r#"UPDATE series SET deleted_at = now()
           WHERE id = $1 AND artist_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(id)
    .bind(artist_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(ApiError::NotFound);
    }
    sqlx::query(
        r#"UPDATE artworks SET series_id = NULL, updated_at = now()
           WHERE series_id = $1 AND artist_id = $2"#,
    )
    .bind(id)
    .bind(artist_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────────
// PUT /v1/studio/series/:id/artworks — bulk set membership
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SetMembership {
    /// The new full set of artworks for this series. Anything in this
    /// list AND owned by the caller → gets `series_id = $series`.
    /// Anything currently in the series but NOT in this list → its
    /// `series_id` clears to NULL. Atomic — one transaction, full
    /// replace semantics.
    pub artwork_ids: Vec<Uuid>,
}

#[derive(serde::Serialize)]
pub struct MembershipAck {
    pub added: u64,
    pub removed: u64,
    pub artwork_count: i32,
}

pub async fn set_artworks(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path(series_id): Path<Uuid>,
    Json(body): Json<SetMembership>,
) -> Result<Json<MembershipAck>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;
    let _existing = load_studio_row(&state.pool, artist_id, series_id).await?;

    if body.artwork_ids.len() > MAX_MEMBERSHIP_IDS {
        return Err(ApiError::BadRequest(format!(
            "artwork_ids: exceeds {MAX_MEMBERSHIP_IDS} ids per request"
        )));
    }
    // De-dup on the way in — caller can pass duplicates without harm,
    // but our UPDATE counts would otherwise overcount.
    let mut ids = body.artwork_ids.clone();
    ids.sort();
    ids.dedup();

    let mut tx = state.pool.begin().await?;

    // 1. Add: set series_id = $series for the listed artworks the
    //    caller owns. Returns the count actually flipped (excludes
    //    ones that already belong to the series).
    let added = sqlx::query(
        r#"
        UPDATE artworks
           SET series_id = $1, updated_at = now()
         WHERE artist_id = $2
           AND id = ANY($3)
           AND deleted_at IS NULL
           AND (series_id IS DISTINCT FROM $1)
        "#,
    )
    .bind(series_id)
    .bind(artist_id)
    .bind(&ids)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    // 2. Remove: clear series_id for artworks currently in this series
    //    but NOT in the new list.
    let removed = sqlx::query(
        r#"
        UPDATE artworks
           SET series_id = NULL, updated_at = now()
         WHERE artist_id = $1
           AND series_id = $2
           AND NOT (id = ANY($3))
        "#,
    )
    .bind(artist_id)
    .bind(series_id)
    .bind(&ids)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    let count: (i64,) = sqlx::query_as(
        r#"SELECT COUNT(*)::bigint FROM artworks
           WHERE series_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(series_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(MembershipAck {
        added,
        removed,
        artwork_count: i32::try_from(count.0).unwrap_or(0),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

async fn load_studio_row(
    pool: &ml_art_core::db::Pool,
    artist_id: Uuid,
    id: Uuid,
) -> Result<SeriesRow, ApiError> {
    let sql = format!("{STUDIO_ROW_SQL} AND s.id = $2");
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(artist_id)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound)
}

async fn validate_cover(
    pool: &ml_art_core::db::Pool,
    artist_id: Uuid,
    cover: Option<Uuid>,
) -> Result<(), ApiError> {
    let Some(cover_id) = cover else {
        return Ok(());
    };
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM artworks WHERE id = $1 AND artist_id = $2 AND deleted_at IS NULL",
    )
    .bind(cover_id)
    .bind(artist_id)
    .fetch_optional(pool)
    .await?;
    if row.is_none() {
        return Err(ApiError::BadRequest(
            "cover_artwork_id: not an artwork owned by you".into(),
        ));
    }
    Ok(())
}

/// Map the `series_artist_slug_unique_idx` violation to a 409. Any
/// other DB error passes through unchanged.
fn map_slug_conflict(e: sqlx::Error) -> ApiError {
    if let Some(db_err) = e.as_database_error() {
        if db_err.code().as_deref() == Some("23505")
            && db_err
                .constraint()
                .is_some_and(|c| c.contains("series_artist_slug_unique"))
        {
            return ApiError::Conflict(
                "a series with that title already exists — try a different title".into(),
            );
        }
    }
    ApiError::Internal(anyhow::anyhow!("series write: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basics() {
        assert_eq!(slugify("Quiet Mornings"), "quiet-mornings");
        assert_eq!(slugify("  Lit From Within  "), "lit-from-within");
        // Non-ASCII chars drop (no NFKD fold in v1).
        assert_eq!(slugify("Café Visions"), "caf-visions");
        assert_eq!(slugify("Already-Hyphenated"), "already-hyphenated");
        assert_eq!(slugify("Double  Spaces"), "double-spaces");
        // Pathological → 'series' fallback so the NOT NULL slug holds.
        assert_eq!(slugify(""), "series");
        assert_eq!(slugify("!!!"), "series");
        assert_eq!(slugify("   "), "series");
    }

    #[test]
    fn slugify_truncates_long_input() {
        let long = "a".repeat(150);
        assert_eq!(slugify(&long).chars().count(), MAX_SLUG_LEN);
    }
}
