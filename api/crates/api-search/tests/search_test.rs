// Per-file allow: `Deserialize`-only fields trigger `dead_code` under
// `-D warnings`, but we keep them to document the JSON contract the API
// returns. See `decisions.md` 2026-05-27 — Pre-commit hooks.
#![allow(dead_code)]

mod common;

use common::{app_keyword_only, get_json, get_status, MIGRATOR};
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Deserialize, Debug)]
struct Summary {
    title: Option<String>,
    artist_slug: String,
    primary_image_url: Option<String>,
    currency: String,
    availability: String,
}

#[derive(Deserialize, Debug)]
struct Page {
    items: Vec<Summary>,
    next_cursor: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// No-query path: shape, ordering, draft exclusion
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_no_query_returns_published_artworks_newest_first(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, page): (_, Page) = get_json(app, "/v1/search?limit=10").await;
    assert_eq!(status, 200);
    // 5 published artworks; draft "Hidden Sketch" must not appear.
    assert_eq!(page.items.len(), 5);
    let titles: Vec<_> = page.items.iter().filter_map(|i| i.title.clone()).collect();
    assert!(!titles.iter().any(|t| t == "Hidden Sketch"));
    // Newest-first by default — "Linocut Study" (1 day ago) comes before "Blue Morning" (5 days ago)
    assert_eq!(page.items[0].title.as_deref(), Some("Linocut Study"));
    assert_eq!(
        page.items.last().unwrap().title.as_deref(),
        Some("Blue Morning")
    );
    assert!(page.next_cursor.is_none());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_respects_limit(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?limit=2").await;
    assert_eq!(page.items.len(), 2);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_returns_primary_image_url(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?limit=5").await;
    for item in &page.items {
        let url = item
            .primary_image_url
            .as_ref()
            .expect("each fixture artwork has a primary image");
        assert!(
            url.contains("test/"),
            "expected test fixture s3_key in URL, got {url}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Keyword search (embedder disabled)
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_keyword_matches_title(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?q=blue&limit=10").await;
    // "Blue Morning" should match; nothing else has "blue" in tsvector.
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].title.as_deref(), Some("Blue Morning"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_keyword_matches_medium(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?q=sculpture&limit=10").await;
    // Both Stone Forms are sculptures.
    assert_eq!(page.items.len(), 2);
    for item in &page.items {
        assert_eq!(item.artist_slug, "bruno-test");
    }
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_keyword_no_match_is_empty(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?q=ocelot&limit=10").await;
    assert!(page.items.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// Filters
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_medium_filter(pool: PgPool) {
    // T-073 — `?medium=` now filters on `medium_category` (canonical
    // taxonomy code) rather than the free-text `medium` field. Tag
    // Linocut Study with the canonical `print` code; everything else
    // stays NULL → silently excluded.
    sqlx::query("UPDATE artworks SET medium_category='print' WHERE title='Linocut Study'")
        .execute(&pool)
        .await
        .unwrap();

    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?medium=print&limit=10").await;
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].title.as_deref(), Some("Linocut Study"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_medium_filter_multi_value(pool: PgPool) {
    // Comma-separated multi-value. Painting + Print should both surface.
    sqlx::query("UPDATE artworks SET medium_category='print' WHERE title='Linocut Study'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE artworks SET medium_category='painting' WHERE title='Blue Morning'")
        .execute(&pool)
        .await
        .unwrap();

    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?medium=painting,print&limit=10").await;
    let titles: Vec<_> = page.items.iter().filter_map(|i| i.title.clone()).collect();
    assert!(titles.contains(&"Blue Morning".to_string()));
    assert!(titles.contains(&"Linocut Study".to_string()));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_medium_filter_unknown_value_passes_through(pool: PgPool) {
    // Bookmarked URL with a since-renamed category surfaces a clean
    // unfiltered result rather than 400-ing or returning empty.
    sqlx::query("UPDATE artworks SET medium_category='print' WHERE title='Linocut Study'")
        .execute(&pool)
        .await
        .unwrap();

    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?medium=nft,bogus&limit=10").await;
    // Both tokens are dropped → no filter clause → all published rows.
    assert!(page.items.len() > 1, "all unknown → no filter applied");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_price_range_filter(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?price_min=100000&price_max=300000").await;
    // Blue Morning (100k), Crimson Field (250k) — Stone Form II (500k) excluded.
    let titles: Vec<_> = page.items.iter().filter_map(|i| i.title.clone()).collect();
    assert!(titles.contains(&"Blue Morning".to_string()));
    assert!(titles.contains(&"Crimson Field".to_string()));
    assert!(!titles.contains(&"Stone Form II".to_string()));
}

// ── T-070 size-band filter ───────────────────────────────────────────

/// Set known dimensions on three seed artworks for the size tests.
/// `s` = Blue Morning (30×30), `m` = Crimson Field (80×60),
/// `l` = Stone Form I (150×60). Leaves Stone Form II + Linocut Study
/// undimensioned so we can also verify the silent-exclusion behaviour.
async fn seed_size_dimensions(pool: &PgPool) {
    sqlx::query(
        r#"UPDATE artworks
           SET dimensions = jsonb_build_object('unit', 'cm', 'width', $1::int, 'height', $2::int)
           WHERE title = $3"#,
    )
    .bind(30_i32)
    .bind(30_i32)
    .bind("Blue Morning")
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        r#"UPDATE artworks
           SET dimensions = jsonb_build_object('unit', 'cm', 'width', $1::int, 'height', $2::int)
           WHERE title = $3"#,
    )
    .bind(80_i32)
    .bind(60_i32)
    .bind("Crimson Field")
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        r#"UPDATE artworks
           SET dimensions = jsonb_build_object('unit', 'cm', 'width', $1::int, 'height', $2::int)
           WHERE title = $3"#,
    )
    .bind(150_i32)
    .bind(60_i32)
    .bind("Stone Form I")
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_size_band_small(pool: PgPool) {
    seed_size_dimensions(&pool).await;
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?size=s&limit=10").await;
    let titles: Vec<_> = page.items.iter().filter_map(|i| i.title.clone()).collect();
    assert_eq!(titles, vec!["Blue Morning".to_string()]);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_size_band_medium(pool: PgPool) {
    seed_size_dimensions(&pool).await;
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?size=m&limit=10").await;
    let titles: Vec<_> = page.items.iter().filter_map(|i| i.title.clone()).collect();
    assert_eq!(titles, vec!["Crimson Field".to_string()]);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_size_band_large(pool: PgPool) {
    seed_size_dimensions(&pool).await;
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?size=l&limit=10").await;
    let titles: Vec<_> = page.items.iter().filter_map(|i| i.title.clone()).collect();
    assert_eq!(titles, vec!["Stone Form I".to_string()]);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_size_excludes_undimensioned_works(pool: PgPool) {
    // Don't seed any dimensions; the filter should return zero rows
    // rather than silently fall back to "everything".
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?size=m&limit=10").await;
    assert!(
        page.items.is_empty(),
        "non-dimensioned works should not pass any size band; got {:?}",
        page.items
            .iter()
            .filter_map(|i| i.title.clone())
            .collect::<Vec<_>>()
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_size_unknown_band_falls_through(pool: PgPool) {
    seed_size_dimensions(&pool).await;
    let app = app_keyword_only(pool);
    let (_, with_bogus): (_, Page) = get_json(app.clone(), "/v1/search?size=xl&limit=20").await;
    let (_, no_filter): (_, Page) = get_json(app, "/v1/search?limit=20").await;
    // `size=xl` is not a band we recognise — clause is skipped, so the
    // result set matches a no-filter query. Defensive: bookmarked URLs
    // shouldn't 400 if we ever rename bands.
    assert_eq!(with_bogus.items.len(), no_filter.items.len());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_availability_filter(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?availability=sold").await;
    // Only Stone Form II is sold.
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].title.as_deref(), Some("Stone Form II"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Geographic filters
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_location_filter_string_match(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?location=berlin").await;
    // Bruno's two sculptures; Alice (London) and Carmen (no location) excluded.
    for item in &page.items {
        assert_eq!(item.artist_slug, "bruno-test");
    }
    assert_eq!(page.items.len(), 2);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_location_matches_iso_country_code(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?location=DE").await;
    // `DE` is the ISO code for Bruno's "Berlin, DE" location.
    assert!(!page.items.is_empty());
    assert!(page.items.iter().all(|i| i.artist_slug == "bruno-test"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_location_matches_country_synonym(pool: PgPool) {
    // `uk` → synonym → GB → matches Alice's London.
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?location=uk").await;
    assert!(page.items.iter().all(|i| i.artist_slug == "alice-test"));
    assert!(!page.items.is_empty(), "uk should match Alice via GB");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_location_full_country_name(pool: PgPool) {
    // `Germany` → DE → Bruno.
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?location=Germany").await;
    assert!(page.items.iter().all(|i| i.artist_slug == "bruno-test"));
    assert!(!page.items.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_near_me_haversine(pool: PgPool) {
    let app = app_keyword_only(pool);
    // Center on Berlin, 50km radius — Bruno only.
    let (_, page): (_, Page) = get_json(
        app,
        "/v1/search?near_lat=52.52&near_lng=13.405&near_radius_km=50",
    )
    .await;
    for item in &page.items {
        assert_eq!(item.artist_slug, "bruno-test");
    }
    assert!(!page.items.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_nearest_sort_requires_coords(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, _) = get_status(app, "/v1/search?sort=nearest").await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_near_lat_without_lng_is_400(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, _) = get_status(app, "/v1/search?near_lat=51.5").await;
    assert_eq!(status, 400);
}

// ─────────────────────────────────────────────────────────────────────────────
// Currency / metadata preserved
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_preserves_currency(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?q=stone").await;
    for item in &page.items {
        assert_eq!(item.currency, "EUR");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Cursor pagination (T-037). Seed has 5 published artworks; paging
// limit=2 walks them as 2 → 2 → 1 across three pages.
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_paginates_via_opaque_cursor(pool: PgPool) {
    let app = app_keyword_only(pool);

    // Page 1 — no cursor yet.
    let (status, p1): (_, Page) = get_json(app.clone(), "/v1/search?limit=2").await;
    assert_eq!(status, 200);
    assert_eq!(p1.items.len(), 2);
    let c1 = p1
        .next_cursor
        .expect("expected a next_cursor on a non-final page");

    // Page 2 — feed cursor back in.
    let (status, p2): (_, Page) =
        get_json(app.clone(), &format!("/v1/search?limit=2&cursor={c1}")).await;
    assert_eq!(status, 200);
    assert_eq!(p2.items.len(), 2);
    let c2 = p2.next_cursor.expect("expected a next_cursor on page 2");

    // No duplicates across page boundaries. Dedup by *title* not
    // artist_slug — the seed has multiple artworks per artist
    // (Bruno's two Stone Forms straddle the page boundary), which
    // is legitimate and shouldn't fail the dedup check.
    let p1_titles: Vec<_> = p1.items.iter().filter_map(|i| i.title.clone()).collect();
    let p2_titles: Vec<_> = p2.items.iter().filter_map(|i| i.title.clone()).collect();
    for t in &p2_titles {
        assert!(!p1_titles.contains(t), "page 2 leaked a page-1 item: {t}");
    }

    // Page 3 — last page, single remaining item, no further cursor.
    let (status, p3): (_, Page) = get_json(app, &format!("/v1/search?limit=2&cursor={c2}")).await;
    assert_eq!(status, 200);
    assert_eq!(p3.items.len(), 1);
    assert!(p3.next_cursor.is_none());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_first_page_has_no_cursor_when_all_fit(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, page): (_, Page) = get_json(app, "/v1/search?limit=10").await;
    // 5 items fit under limit=10, so the server shouldn't fabricate a cursor.
    assert_eq!(page.items.len(), 5);
    assert!(page.next_cursor.is_none());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_rejects_malformed_cursor(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, _) = get_status(app, "/v1/search?cursor=not-a-real-cursor").await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_cursor_threads_through_filters(pool: PgPool) {
    // A cursor obtained while a filter was active must still apply
    // the filter on the follow-up request (the client passes the
    // same params alongside the cursor). Tag two artworks with the
    // canonical `painting` category, then page through them at
    // limit=1 and assert we got both distinct titles + no third page.
    sqlx::query(
        "UPDATE artworks SET medium_category='painting'
         WHERE title IN ('Blue Morning', 'Crimson Field')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let app = app_keyword_only(pool);
    let (_, p1): (_, Page) = get_json(app.clone(), "/v1/search?medium=painting&limit=1").await;
    assert_eq!(p1.items.len(), 1);
    let c1 = p1.next_cursor.expect("painting filter has > 1 match");

    let (_, p2): (_, Page) = get_json(
        app,
        &format!("/v1/search?medium=painting&limit=1&cursor={c1}"),
    )
    .await;
    assert_eq!(p2.items.len(), 1);
    // Different titles → cursor advanced past the first match.
    assert_ne!(p1.items[0].title, p2.items[0].title);
    // No third page — confirms the filter is still applied on page 2
    // (without it, the cursor offset would land in non-painting rows).
    assert!(p2.next_cursor.is_none());
}
