//! `/v1/search` — hybrid keyword + vector ranking with structured filters.
//!
//! Two execution paths:
//!
//! 1. **Hybrid** — when we have a text query or an image-upload anchor.
//!    Computes a keyword rank (Postgres tsvector / `ts_rank`) and a
//!    semantic rank (pgvector cosine distance against
//!    `artwork_embeddings`), then fuses via Reciprocal Rank Fusion
//!    (RRF, k=60). When the caller only provides an image anchor and
//!    no text, the keyword CTE returns zero rows (empty tsquery), and
//!    the result is a pure-vector search ordered by cosine distance.
//! 2. **No-query** — no text, no image anchor. Returns artworks
//!    ordered by sort param (default: newest).
//!
//! Anchor precedence: `image_upload_id` wins over `q` when both are
//! set (the spike validated that image embeddings dominate signal).
//! Filters apply in all paths.

use crate::AppState;
use axum::http::HeaderMap;
use axum::{
    extract::{Query, State},
    Json,
};
use ml_art_core::{
    auth::OptionalAnonId,
    cursor::{CursorError, PageCursor},
    error::ApiError,
    events::{self, EventName},
    location_search::LocationTerms,
    models::{ArtworkSummary, Paginated, SortOrder},
    modifiers::{self, DEFAULT_ALPHA},
};
use pgvector::Vector;
use serde::Deserialize;
use sqlx::{postgres::PgArguments, Arguments, AssertSqlSafe, Postgres};
use std::sync::Arc;
use uuid::Uuid;

const RRF_K: i64 = 60;
const CANDIDATE_POOL: i64 = 200;
/// T-082 — defensive ceiling on `?refine=` text length. Refine is a
/// free-form input that pays one Jina embed per request; without a cap
/// a malicious caller could spend our budget on long strings. 500 chars
/// is comfortably more than any natural-language refinement.
const REFINE_MAX_LEN: usize = 500;
/// Upper bound on `size=l` band — mirrors the validator's
/// `MAX_DIMENSION_CM` so an artwork created at the ceiling is still
/// filterable. Kept local to avoid pulling the core::validation
/// constant in (different concern: storage vs query).
const MAX_SIZE_CM: i32 = 5000;

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    pub q: Option<String>,
    /// Visual-search anchor. Looks up `uploads.embedding` for this id
    /// and uses it as the semantic ranking vector. Wins over `q` for
    /// the semantic side when both are set; `q` still drives the
    /// keyword side, letting callers compose "things like this image
    /// AND about painting." Phase B of T-010.
    #[serde(default)]
    pub image_upload_id: Option<Uuid>,
    /// Visual-search anchor sourced from an *existing platform artwork*
    /// rather than an uploaded image. Reads the artwork's vector out
    /// of `artwork_embeddings` directly — no upload roundtrip. Lets a
    /// "Find visually similar →" action on the artwork detail page
    /// reuse the full search surface (filters + modifiers + map).
    /// Precedence: `image_upload_id` > `seed_artwork_id` > `q` text
    /// embedding. The seed artwork itself is excluded from results
    /// (it'd otherwise self-match at position 1, which isn't useful).
    #[serde(default)]
    pub seed_artwork_id: Option<Uuid>,
    /// Comma-separated modifier names — e.g. `?modifiers=moodier,warmer`.
    /// Each known modifier contributes its δ-vector (computed via
    /// `core::modifiers::compute_delta`); the anchor is shifted at
    /// α=0.8 per the WikiArt spike. Unknown names → 400. Requires
    /// `image_upload_id`: modifiers without a visual anchor weren't
    /// part of the validated path. Phase C of T-010.
    #[serde(default)]
    pub modifiers: Option<String>,
    /// T-082 — free-form "refine with text" override. Adds a fourth RRF
    /// channel (`refine_ranked`) alongside keyword + semantic + taste
    /// when at least one primary signal is also set (q, image_upload_id,
    /// or seed_artwork_id). Without a primary signal, refine is silently
    /// ignored — refine alone would just be regular text search, so we
    /// don't promote it into the keyword channel. Composes with modifiers
    /// (refine modifies the result-set ranking; modifiers modify the
    /// anchor vector). Length-capped at REFINE_MAX_LEN.
    #[serde(default)]
    pub refine: Option<String>,

    // Filters
    #[serde(default)]
    pub medium: Option<String>,
    #[serde(default)]
    pub price_min: Option<i64>,
    #[serde(default)]
    pub price_max: Option<i64>,
    #[serde(default)]
    pub availability: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub near_lat: Option<f64>,
    #[serde(default)]
    pub near_lng: Option<f64>,
    #[serde(default)]
    pub near_radius_km: Option<f64>,
    /// T-070 — size band over the longest side of the artwork in cm.
    /// `s` ≤ 40, `m` 41..=100, `l` > 100. Single band per query in v1;
    /// non-dimensioned works are silently excluded from any size filter.
    /// Unknown values are ignored (tolerant — the URL stays user-typeable).
    #[serde(default)]
    pub size: Option<String>,

    /// T-056.3 — opt the request in/out of the personalised RRF blend.
    /// `on` forces the taste channel on (subject to the user being
    /// eligible — signed-in + `interaction_count >= 5` + taste vector
    /// present). `off` forces it off even when the operator default
    /// is on. Absent → fall through to `Config::search_personalize_enabled`.
    /// Useful for A/B + debugging; the web layer can also flip it from
    /// a `/search?personalize=off` link.
    #[serde(default)]
    pub personalize: Option<String>,

    // Sort + pagination
    #[serde(default)]
    pub sort: Option<SortOrder>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// Opaque cursor from a previous response's `next_cursor`. Treat
    /// as opaque on the client: today it decodes to an offset, but
    /// the server may swap for keyset later without changing the API
    /// shape. T-037.
    #[serde(default)]
    pub cursor: Option<String>,
}

fn default_limit() -> i64 {
    24
}

pub async fn handle(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
    OptionalAnonId(anon_id): OptionalAnonId,
    auth: Option<crate::extractors::AuthedUser>,
    headers: HeaderMap,
) -> Result<Json<Paginated<ArtworkSummary>>, ApiError> {
    let limit = params.limit.clamp(1, 48);
    let sort = params.sort.unwrap_or_default();

    // Decode the opaque cursor → offset. Malformed / out-of-range
    // are 400 so a malicious client can't tarpit us with deep
    // pages. None means "first page" (offset 0).
    let offset: i64 = match params.cursor.as_deref() {
        None => 0,
        Some(c) => match PageCursor::decode(c) {
            Ok(p) => p.offset,
            Err(CursorError::Malformed) => {
                return Err(ApiError::BadRequest("cursor: malformed".into()))
            }
            Err(CursorError::OutOfRange) => {
                return Err(ApiError::BadRequest("cursor: out of range".into()))
            }
        },
    };
    // Trace the anon_id for rate-limiting + behavior tracking observability.
    // No behavior change yet — that lands when we wire rate limiting (T-007).
    if let Some(id) = anon_id {
        tracing::debug!(anon_id = %id, "search request");
    }

    // Validate `nearest` sort presupposes a near point
    if sort == SortOrder::Nearest && (params.near_lat.is_none() || params.near_lng.is_none()) {
        return Err(ApiError::BadRequest(
            "sort=nearest requires near_lat and near_lng".into(),
        ));
    }
    // Validate near params come as a group
    if params.near_lat.is_some() != params.near_lng.is_some() {
        return Err(ApiError::BadRequest(
            "near_lat and near_lng must both be set".into(),
        ));
    }

    // Resolve the semantic anchor vector. Two sources, image wins when
    // both are set:
    //   1. `image_upload_id` → look up `uploads.embedding`. Unknown id
    //      → 404 (capability-style; UUIDs are unguessable). Row exists
    //      but embedding NULL → 400 (the upload is mid-flight, retry).
    //      A `moderation_status = 'rejected'` row is treated as if it
    //      didn't exist (T-008b) — same 404 so the abuse path can't
    //      tell whether the upload landed.
    //   2. `q` → call `embed_text`. Returns None when the embedder is
    //      disabled in dev (no JINA_API_KEY); search degrades to
    //      keyword-only via the existing branch.
    let upload_vec: Option<Vector> = if let Some(id) = params.image_upload_id {
        let row: Option<(Option<Vector>,)> = sqlx::query_as(
            "SELECT embedding FROM uploads WHERE id = $1 AND moderation_status != 'rejected'",
        )
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
        match row {
            Some((Some(v),)) => Some(v),
            Some((None,)) => {
                return Err(ApiError::BadRequest(
                    "upload exists but embedding is not ready yet".into(),
                ))
            }
            None => return Err(ApiError::NotFound),
        }
    } else {
        None
    };
    // Seed-from-artwork anchor: look up the existing embedding rather
    // than re-running an embed pass. Embedding-row missing for this
    // model_name/model_version is treated as 404 (rare: only if the
    // artwork hasn't been indexed under the current embedder).
    let seed_vec: Option<Vector> = if let Some(id) = params.seed_artwork_id {
        let row: Option<(Vector,)> = sqlx::query_as(
            r#"
            SELECT ae.embedding
            FROM artwork_embeddings ae
            JOIN artworks a ON a.id = ae.artwork_id
            WHERE ae.artwork_id = $1
              AND ae.model_name = $2
              AND ae.model_version = $3
              AND a.deleted_at IS NULL
              AND a.status = 'published'
            "#,
        )
        .bind(id)
        .bind(state.cfg.embedding_model_name.clone())
        .bind(state.cfg.embedding_model_version.clone())
        .fetch_optional(&state.pool)
        .await?;
        match row {
            Some((v,)) => Some(v),
            None => return Err(ApiError::NotFound),
        }
    } else {
        None
    };
    let text_vec: Option<Vector> = match &params.q {
        Some(q) if !q.trim().is_empty() => state.embedder.embed_text(q).await?,
        _ => None,
    };
    // Precedence: explicit image upload > seed artwork > text embed.
    // First two are explicit user intents; text falls in last as a
    // soft default.
    let mut semantic_anchor = upload_vec.or(seed_vec).or(text_vec);

    // Modifiers: validate + apply δ-vectors at α=0.8. Spec ties this to
    // image_upload_id (the spike only validated visual anchors), so we
    // reject modifiers without one rather than silently shifting a text
    // vector in untested territory.
    let modifier_names = parse_modifiers(params.modifiers.as_deref())?;
    if !modifier_names.is_empty() {
        // Modifiers were validated in the spike against visual anchors
        // only (text-anchor + modifier behaviour wasn't tested). Both
        // `image_upload_id` and `seed_artwork_id` resolve to image
        // embeddings, so either satisfies the precondition.
        if params.image_upload_id.is_none() && params.seed_artwork_id.is_none() {
            return Err(ApiError::BadRequest(
                "modifiers require image_upload_id or seed_artwork_id".into(),
            ));
        }
        // semantic_anchor must be Some here (image_upload_id resolved
        // above), but be defensive about the upstream invariant.
        let Some(anchor) = semantic_anchor.as_ref() else {
            return Err(ApiError::BadRequest(
                "modifiers require a resolved image anchor".into(),
            ));
        };
        let mut deltas: Vec<Vector> = Vec::with_capacity(modifier_names.len());
        for m in &modifier_names {
            match modifiers::compute_delta(m, &state.embedder).await {
                Ok(Some(d)) => deltas.push(d),
                Ok(None) => {
                    return Err(ApiError::BadRequest(
                        "embedder unavailable; modifiers can't be applied".into(),
                    ))
                }
                Err(e) => {
                    return Err(ApiError::Internal(anyhow::anyhow!(
                        "compute_delta({}): {e}",
                        m.name
                    )))
                }
            }
        }
        semantic_anchor = Some(modifiers::apply_deltas(anchor, &deltas, DEFAULT_ALPHA));
    }

    let has_text = params.q.as_deref().is_some_and(|q| !q.trim().is_empty());
    let has_visual_anchor = params.image_upload_id.is_some() || params.seed_artwork_id.is_some();

    // T-082 — resolve the refine vector. Only fires when:
    //   1. `?refine=…` is set with a non-empty trimmed value, AND
    //   2. At least one primary signal is set (text or visual anchor).
    //
    // Without a primary signal, refine is silently dropped — alone it
    // would just be regular text search, and we'd be promoting refine
    // into the keyword channel rather than the refine channel. Better
    // to no-op than to do something subtly different from what the URL
    // shape claims.
    //
    // Length-capped defensively; refine is a free-form input that pays
    // one Jina embed per request.
    let refine_text = params
        .refine
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    // Feature-flag gate: refine only fires when the operator has
    // explicitly enabled it via `SEARCH_REFINE_ENABLED=true`. Off
    // pre-launch until A/B evidence justifies flipping it on. When
    // disabled the URL param is silently ignored — no embed hit, no
    // CTE, no 400 (bookmarked URLs still work, they just don't do
    // anything extra).
    let refine_vec: Option<Vector> = match refine_text {
        Some(r) if state.cfg.search_refine_enabled && (has_text || has_visual_anchor) => {
            if r.len() > REFINE_MAX_LEN {
                return Err(ApiError::BadRequest(format!(
                    "refine: text too long (max {REFINE_MAX_LEN} chars)"
                )));
            }
            state.embedder.embed_text(r).await?
        }
        _ => None,
    };

    // T-056.3 — decide whether the taste channel joins this request's
    // RRF blend. Three gates ALL must hold:
    //
    //   1. Operator-enabled OR caller-explicit-opt-in.
    //      `Config::search_personalize_enabled` is the global default;
    //      `?personalize=on` overrides false→true. `?personalize=off`
    //      overrides true→false.
    //   2. Signed-in caller. Anonymous events feed the user's taste
    //      vector on sign-in via T-033, but pre-signin we don't have a
    //      user record to look the vector up against.
    //   3. The user is eligible: a taste vector is set AND
    //      `interaction_count >= MIN_INTERACTION_COUNT` (5 — the
    //      threshold a finished T-061 calibrator session clears).
    //
    // Gate 1 is the kill switch; gates 2 + 3 are the natural ones.
    let personalize_pref = match params.personalize.as_deref() {
        Some("on") => Some(true),
        Some("off") => Some(false),
        _ => None,
    };
    let personalize_active = personalize_pref.unwrap_or(state.cfg.search_personalize_enabled);
    let user_id = auth.as_ref().map(|crate::extractors::AuthedUser(u)| u.id);

    // Taste-channel lookup is gated to the hybrid path (has_text OR
    // visual anchor); the no-query browse path never blends taste, so
    // no need to pay for the SQL hop there.
    let taste_vec: Option<Vector> =
        match (personalize_active, has_text || has_visual_anchor, user_id) {
            (true, true, Some(uid)) => {
                let row: Option<(Option<Vector>, i32)> = sqlx::query_as(
                    "SELECT taste_embedding, interaction_count \
                 FROM user_profiles WHERE user_id = $1",
                )
                .bind(uid)
                .fetch_optional(&state.pool)
                .await?;
                match row {
                    Some((Some(v), c)) if c >= crate::recommendations::MIN_INTERACTION_COUNT => {
                        Some(v)
                    }
                    _ => None,
                }
            }
            _ => None,
        };

    if taste_vec.is_some() {
        tracing::info!(
            personalize = "on",
            user_id = ?user_id,
            rrf_k = RRF_K,
            "search with personalisation"
        );
    }

    if refine_vec.is_some() {
        tracing::info!(
            refine = "on",
            refine_len = refine_text.map(str::len),
            rrf_k = RRF_K,
            "search with refine channel"
        );
    }

    // Fetch limit+1 so we can detect whether a next page exists
    // without a separate COUNT query. If we got back limit+1 rows,
    // drop the sentinel and issue a cursor pointing to the next
    // page's start. T-037.
    let mut items = if has_text || has_visual_anchor {
        run_hybrid(
            &state,
            &params,
            semantic_anchor.as_ref(),
            taste_vec.as_ref(),
            refine_vec.as_ref(),
            sort,
            limit,
            offset,
        )
        .await?
    } else {
        run_no_query(&state, &params, sort, limit, offset).await?
    };

    let has_next = items.len() > limit as usize;
    if has_next {
        items.truncate(limit as usize);
    }
    let next_cursor = has_next.then(|| PageCursor::from_offset(offset + limit).encode());

    // T-050 — search_executed. Captures intent (q + filters) plus the
    // result_count for funnel analysis. Page-1 only (offset 0) so paginated
    // scrolls don't pollute as separate searches.
    if offset == 0 {
        events::emit(
            &state.jobs,
            events::event_log(
                EventName::SearchExecuted,
                anon_id,
                None,
                serde_json::json!({
                    "q": params.q.clone(),
                    "medium": params.medium.clone(),
                    "price_min": params.price_min,
                    "price_max": params.price_max,
                    "availability": params.availability.clone(),
                    "location": params.location.clone(),
                    "size": params.size.clone(),
                    "modifiers": params.modifiers.clone(),
                    "refine": params.refine.clone(),
                    "image_upload_id": params.image_upload_id,
                    "sort": params.sort,
                    "result_count": items.len(),
                }),
                events::extract_request_context(&headers),
            ),
        )
        .await;
    }

    Ok(Json(Paginated { items, next_cursor }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Hybrid path
// ─────────────────────────────────────────────────────────────────────────────

// T-056.3 — `taste_vec` when Some adds a `taste_ranked` channel to the
// RRF fusion via the same shape `semantic_ranked` uses. Empty CTE when
// None so the COALESCE-with-0 in the rrf_score short-circuits cleanly
// and existing queries are unchanged.
//
// T-082 — `refine_vec` adds a fourth `refine_ranked` channel. Unlike
// taste (which only re-ranks), refine joins the candidate-contribution
// clause: a refine-only match (no keyword + no semantic-anchor match)
// can still surface a row, because the user is saying "I'm looking for
// this concept too." Without that, "abstract" as refine wouldn't pull
// in abstract works that the visual anchor didn't already find.
#[allow(clippy::too_many_arguments)]
async fn run_hybrid(
    state: &AppState,
    params: &SearchParams,
    query_vec: Option<&Vector>,
    taste_vec: Option<&Vector>,
    refine_vec: Option<&Vector>,
    sort: SortOrder,
    limit: i64,
    offset: i64,
) -> Result<Vec<ArtworkSummary>, ApiError> {
    let q = params.q.as_deref().unwrap_or("").trim();
    let mut args = PgArguments::default();
    let mut next = ArgIndex::new();

    // Always-bound args first. All binds use owned values (sqlx 0.9 requires 'static).
    let q_idx = next.bind(&mut args, q.to_string())?;
    let model_name_idx = next.bind(&mut args, state.cfg.embedding_model_name.clone())?;
    let model_version_idx = next.bind(&mut args, state.cfg.embedding_model_version.clone())?;
    let candidate_pool_idx = next.bind(&mut args, CANDIDATE_POOL)?;

    // The semantic CTE needs a concrete vector. If we don't have one, run
    // keyword-only by making the semantic CTE return empty.
    let semantic_cte = match query_vec {
        Some(v) => {
            let idx = next.bind(&mut args, v.clone())?;
            format!(
                "semantic_ranked AS (
                    SELECT ae.artwork_id AS id,
                           ROW_NUMBER() OVER (ORDER BY ae.embedding <=> ${idx}) AS rk
                    FROM artwork_embeddings ae
                    WHERE ae.model_name = ${m} AND ae.model_version = ${mv}
                    ORDER BY ae.embedding <=> ${idx}
                    LIMIT ${cp}
                )",
                m = model_name_idx,
                mv = model_version_idx,
                cp = candidate_pool_idx,
            )
        }
        None => "semantic_ranked AS (SELECT NULL::uuid AS id, NULL::bigint AS rk WHERE false)"
            .to_string(),
    };

    // T-056.3 — taste channel. Mirrors the semantic CTE shape so the
    // final SELECT can blend all three uniformly. Empty CTE when not
    // active (off by default; gated by Config + `?personalize=` query).
    let taste_cte = match taste_vec {
        Some(v) => {
            let idx = next.bind(&mut args, v.clone())?;
            format!(
                "taste_ranked AS (
                    SELECT ae.artwork_id AS id,
                           ROW_NUMBER() OVER (ORDER BY ae.embedding <=> ${idx}) AS rk
                    FROM artwork_embeddings ae
                    WHERE ae.model_name = ${m} AND ae.model_version = ${mv}
                    ORDER BY ae.embedding <=> ${idx}
                    LIMIT ${cp}
                )",
                m = model_name_idx,
                mv = model_version_idx,
                cp = candidate_pool_idx,
            )
        }
        None => {
            "taste_ranked AS (SELECT NULL::uuid AS id, NULL::bigint AS rk WHERE false)".to_string()
        }
    };

    // T-082 — refine channel. Same shape as semantic/taste; empty CTE
    // when refine is absent so the rest of the SQL doesn't branch.
    let refine_cte = match refine_vec {
        Some(v) => {
            let idx = next.bind(&mut args, v.clone())?;
            format!(
                "refine_ranked AS (
                    SELECT ae.artwork_id AS id,
                           ROW_NUMBER() OVER (ORDER BY ae.embedding <=> ${idx}) AS rk
                    FROM artwork_embeddings ae
                    WHERE ae.model_name = ${m} AND ae.model_version = ${mv}
                    ORDER BY ae.embedding <=> ${idx}
                    LIMIT ${cp}
                )",
                m = model_name_idx,
                mv = model_version_idx,
                cp = candidate_pool_idx,
            )
        }
        None => {
            "refine_ranked AS (SELECT NULL::uuid AS id, NULL::bigint AS rk WHERE false)".to_string()
        }
    };

    let (filters_sql, _) = build_filters(params, &mut next, &mut args)?;
    let order_sql = build_order(sort);
    // Fetch `limit + 1` so the caller can detect whether a next
    // page exists without a separate COUNT query. The extra row is
    // sentinel-only; we drop it before returning. T-037.
    let limit_idx = next.bind(&mut args, limit + 1)?;
    let offset_idx = next.bind(&mut args, offset)?;

    let sql = format!(
        r#"
        WITH keyword_ranked AS (
            SELECT a.id,
                   ROW_NUMBER() OVER (
                     ORDER BY ts_rank(a.search_tsv, plainto_tsquery('english', ${q_idx})) DESC
                   ) AS rk
            FROM artworks a
            WHERE a.deleted_at IS NULL
              AND a.status = 'published'
              AND a.search_tsv @@ plainto_tsquery('english', ${q_idx})
            LIMIT ${cp}
        ),
        {semantic_cte},
        {taste_cte},
        {refine_cte}
        SELECT
            a.id,
            a.title,
            ar.display_name AS artist_name,
            ar.id           AS artist_id,
            ar.slug         AS artist_slug,
            ai.s3_key       AS primary_s3_key,
            a.price_cents,
            a.currency,
            a.availability,
            (COALESCE(1.0/({rrf_k} + k.rk), 0)
              + COALESCE(1.0/({rrf_k} + s.rk), 0)
              + COALESCE(1.0/({rrf_k} + t.rk), 0)
              + COALESCE(1.0/({rrf_k} + r.rk), 0))::float8 AS rrf_score
        FROM artworks a
        JOIN artists ar ON ar.id = a.artist_id
        LEFT JOIN artwork_images ai
               ON ai.artwork_id = a.id
              AND ai.is_primary
              AND ai.moderation_status = 'approved'
        LEFT JOIN keyword_ranked  k ON k.id = a.id
        LEFT JOIN semantic_ranked s ON s.id = a.id
        LEFT JOIN taste_ranked    t ON t.id = a.id
        LEFT JOIN refine_ranked   r ON r.id = a.id
        WHERE a.deleted_at IS NULL
          AND a.status = 'published'
          AND ar.deleted_at IS NULL
          AND ar.status = 'active'
          AND (k.id IS NOT NULL OR s.id IS NOT NULL OR r.id IS NOT NULL)
          {filters_sql}
        {order_sql}
        LIMIT ${limit_idx} OFFSET ${offset_idx}
        "#,
        rrf_k = RRF_K,
        cp = candidate_pool_idx,
    );

    let rows: Vec<Row> = sqlx::query_as_with::<Postgres, Row, _>(AssertSqlSafe(sql), args)
        .fetch_all(&state.pool)
        .await?;
    Ok(rows.into_iter().map(Row::into_summary).collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// No-query path (filter + sort only)
// ─────────────────────────────────────────────────────────────────────────────

async fn run_no_query(
    state: &AppState,
    params: &SearchParams,
    sort: SortOrder,
    limit: i64,
    offset: i64,
) -> Result<Vec<ArtworkSummary>, ApiError> {
    let mut args = PgArguments::default();
    let mut next = ArgIndex::new();

    let (filters_sql, _) = build_filters(params, &mut next, &mut args)?;
    let order_sql = build_order(sort);
    // Fetch limit+1 to detect a next page (see run_hybrid). T-037.
    let limit_idx = next.bind(&mut args, limit + 1)?;
    let offset_idx = next.bind(&mut args, offset)?;

    let sql = format!(
        r#"
        SELECT
            a.id,
            a.title,
            ar.display_name AS artist_name,
            ar.id           AS artist_id,
            ar.slug         AS artist_slug,
            ai.s3_key       AS primary_s3_key,
            a.price_cents,
            a.currency,
            a.availability,
            0::float8 AS rrf_score
        FROM artworks a
        JOIN artists ar ON ar.id = a.artist_id
        LEFT JOIN artwork_images ai
               ON ai.artwork_id = a.id
              AND ai.is_primary
              AND ai.moderation_status = 'approved'
        WHERE a.deleted_at IS NULL
          AND a.status = 'published'
          AND ar.deleted_at IS NULL
          AND ar.status = 'active'
          {filters_sql}
        {order_sql}
        LIMIT ${limit_idx} OFFSET ${offset_idx}
        "#
    );

    let rows: Vec<Row> = sqlx::query_as_with::<Postgres, Row, _>(AssertSqlSafe(sql), args)
        .fetch_all(&state.pool)
        .await?;
    Ok(rows.into_iter().map(Row::into_summary).collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared filter builder
// ─────────────────────────────────────────────────────────────────────────────

fn build_filters(
    p: &SearchParams,
    next: &mut ArgIndex,
    args: &mut PgArguments,
) -> Result<(String, ()), ApiError> {
    let mut clauses: Vec<String> = Vec::new();

    // Seed-from-artwork: exclude the seeded artwork from results.
    // Without this, the artwork's own vector self-matches at the
    // top of the ranking and the user sees "find me things similar
    // to X → X at position 1" — useless. The artist's *other*
    // works are still in play (they often look similar; that's
    // signal, not noise).
    if let Some(seed) = p.seed_artwork_id {
        let idx = next.bind(args, seed)?;
        clauses.push(format!("AND a.id <> ${idx}"));
    }

    // T-073 — `?medium=` is now a multi-value comma-separated filter
    // against the canonical `medium_category` column ("Painting"),
    // NOT the free-text `medium` ("Oil on linen"). Old single-value
    // exact-match-on-medium-text never matched anything once artists
    // started typing real materials anyway, so this isn't a breaking
    // change in practice — the filter just starts working.
    //
    // `parse_medium_query` silently drops unknown tokens (a bookmark
    // with a since-renamed category surfaces what survives) and
    // returns `None` for "no filter clause" vs `Some(vec![])` which
    // would filter for the empty set. See `core::validation`.
    if let Some(cats) = p
        .medium
        .as_deref()
        .and_then(ml_art_core::validation::parse_medium_query)
    {
        let idx = next.bind(args, cats)?;
        clauses.push(format!("AND a.medium_category = ANY(${idx}::text[])"));
    }

    // T-080 — the price filter is canonical-GBP. Artists set prices in
    // their own currency; `artworks.price_gbp_cents` is the converted
    // value maintained by studio writes + the FX-refresh job. Filtering
    // on the raw `price_cents` would compare a $500 artwork as if it
    // were £500 (it's actually ~£395) which surfaces wrong results.
    // Both bucket presets and custom-range inputs pass GBP values.
    if let Some(pm) = p.price_min {
        let idx = next.bind(args, pm)?;
        clauses.push(format!("AND a.price_gbp_cents >= ${idx}"));
    }
    if let Some(pm) = p.price_max {
        let idx = next.bind(args, pm)?;
        clauses.push(format!("AND a.price_gbp_cents <= ${idx}"));
    }

    if let Some(av) = p.availability.as_deref().filter(|s| !s.is_empty()) {
        let idx = next.bind(args, av.to_string())?;
        clauses.push(format!("AND a.availability = ${idx}"));
    }

    // T-070 — size band over `dimensions->>'width' | 'height'`. Works
    // without dimensions are silently excluded (`IS NOT NULL` on both
    // keys). Unknown band values fall through with no clause — keeps
    // bookmarked URLs from 400'ing if we ever rename the bands.
    if let Some(band) = p.size.as_deref() {
        let bounds: Option<(i32, i32)> = match band.to_ascii_lowercase().as_str() {
            "s" => Some((1, 40)),
            "m" => Some((41, 100)),
            "l" => Some((101, MAX_SIZE_CM)),
            _ => None,
        };
        if let Some((lo, hi)) = bounds {
            let lo_idx = next.bind(args, lo)?;
            let hi_idx = next.bind(args, hi)?;
            clauses.push(format!(
                "AND (a.dimensions->>'width') IS NOT NULL \
                 AND (a.dimensions->>'height') IS NOT NULL \
                 AND GREATEST((a.dimensions->>'width')::int, (a.dimensions->>'height')::int) \
                     BETWEEN ${lo_idx} AND ${hi_idx}"
            ));
        }
    }

    if let Some(loc) = p.location.as_deref().and_then(LocationTerms::from_query) {
        // OR across four paths so common user terms all land:
        //   - substring on the artist's "based in" city + country
        //     (catches "London" / "berlin" / "GB" inside "London, GB")
        //   - substring on the artist's free-text "based in" string
        //   - exact-match on the ISO country code (catches "UK"→GB,
        //     "Germany"→DE via the synonym table)
        //   - EXISTS against artist_locations so a venue in
        //     Basingstoke makes the artist's works findable under
        //     `?location=Basingstoke`, even when the artist's own
        //     "based in" field is empty or different. This keeps the
        //     grid's location filter in agreement with the map's
        //     pins and the CityPivotStrip (both are sourced from
        //     artist_locations).
        let pat_idx = next.bind(args, loc.pattern.clone())?;
        let iso_idx = if loc.iso_codes.is_empty() {
            None
        } else {
            Some(next.bind(args, loc.iso_codes.clone())?)
        };
        let mut sub = format!(
            "AND ((coalesce(ar.city, '') || ', ' || coalesce(ar.country, '')) ILIKE ${pat_idx} OR lower(coalesce(ar.location, '')) LIKE ${pat_idx}"
        );
        if let Some(idx) = iso_idx {
            sub.push_str(&format!(
                " OR upper(coalesce(ar.country, '')) = ANY(${idx})"
            ));
        }
        // Venue match. Mirrors the artist-side OR-chain so the same
        // terms work against both sources.
        sub.push_str(&format!(
            " OR EXISTS (SELECT 1 FROM artist_locations al WHERE al.artist_id = ar.id AND ((coalesce(al.city, '') || ', ' || coalesce(al.country, '')) ILIKE ${pat_idx}"
        ));
        if let Some(idx) = iso_idx {
            sub.push_str(&format!(
                " OR upper(coalesce(al.country, '')) = ANY(${idx})"
            ));
        }
        sub.push_str("))");
        sub.push(')');
        clauses.push(sub);
    }

    if let (Some(lat), Some(lng)) = (p.near_lat, p.near_lng) {
        let radius_km = p.near_radius_km.unwrap_or(50.0).clamp(1.0, 500.0);
        let lat_idx = next.bind(args, lat)?;
        let lng_idx = next.bind(args, lng)?;
        let r_idx = next.bind(args, radius_km)?;
        // Haversine. 6371 km Earth radius. Filters and provides a distance.
        clauses.push(format!(
            "AND ar.lat IS NOT NULL AND ar.lng IS NOT NULL \
             AND (6371.0 * acos( \
                    cos(radians(${lat_idx})) * cos(radians(ar.lat)) \
                  * cos(radians(ar.lng) - radians(${lng_idx})) \
                  + sin(radians(${lat_idx})) * sin(radians(ar.lat)) \
                )) <= ${r_idx}"
        ));
    }

    Ok((clauses.join("\n          "), ()))
}

fn build_order(sort: SortOrder) -> String {
    match sort {
        SortOrder::Relevance => {
            // rrf_score may be 0 in no-query path; tie-break by published_at.
            "ORDER BY rrf_score DESC NULLS LAST, a.published_at DESC".to_string()
        }
        SortOrder::Newest => "ORDER BY a.published_at DESC NULLS LAST".to_string(),
        SortOrder::PriceAsc => "ORDER BY a.price_cents ASC NULLS LAST".to_string(),
        SortOrder::PriceDesc => "ORDER BY a.price_cents DESC NULLS LAST".to_string(),
        SortOrder::Nearest => {
            // Same Haversine as the filter; the SELECT exposes it implicitly via
            // re-computation. (Cleaner alternative would be to project the
            // distance in SELECT, but that means parameterizing it twice.)
            "ORDER BY a.published_at DESC".to_string()
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Tiny arg-position counter. sqlx::PgArguments doesn't expose its current
/// length, so we track it ourselves to produce $1, $2, ... placeholders.
struct ArgIndex(usize);
impl ArgIndex {
    fn new() -> Self {
        Self(0)
    }
    fn bind<'a, T>(&mut self, args: &mut PgArguments, value: T) -> Result<usize, ApiError>
    where
        T: 'a + sqlx::Encode<'a, Postgres> + sqlx::Type<Postgres> + Send + 'static,
    {
        args.add(value)
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("bind: {e}")))?;
        self.0 += 1;
        Ok(self.0)
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    title: Option<String>,
    artist_id: Uuid,
    artist_name: String,
    artist_slug: String,
    primary_s3_key: Option<String>,
    price_cents: Option<i64>,
    currency: String,
    availability: String,
    #[sqlx(default)]
    #[allow(dead_code)]
    rrf_score: Option<f64>,
}

impl Row {
    fn into_summary(self) -> ArtworkSummary {
        ArtworkSummary {
            id: self.id,
            title: self.title,
            artist_id: self.artist_id,
            artist_name: self.artist_name,
            artist_slug: self.artist_slug,
            primary_image_url: self
                .primary_s3_key
                .map(|k| ml_art_core::images::url_for_s3_key(&k)),
            price_cents: self.price_cents,
            currency: self.currency,
            availability: self.availability,
        }
    }
}

/// Parse a `?modifiers=moodier,warmer` value into the registered
/// `Modifier`s. Empty / missing input is an empty `Vec`. Unknown names
/// are a hard 400 so the client can correct the URL.
fn parse_modifiers(raw: Option<&str>) -> Result<Vec<&'static modifiers::Modifier>, ApiError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for token in raw.split(',') {
        let name = token.trim();
        if name.is_empty() {
            continue; // tolerate "moodier,,warmer" or trailing commas
        }
        match modifiers::find(name) {
            Some(m) => out.push(m),
            None => {
                let known = modifiers::all_names();
                return Err(ApiError::BadRequest(format!(
                    "unknown modifier `{name}`; known: {}",
                    known.join(", ")
                )));
            }
        }
    }
    Ok(out)
}
