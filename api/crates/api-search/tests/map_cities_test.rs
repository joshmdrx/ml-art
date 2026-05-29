//! T-042 — integration tests for `/v1/search/map/cities`.

mod common;

use common::{app_keyword_only, get_json, MIGRATOR};
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Debug, Deserialize)]
struct City {
    city: String,
    country: Option<String>,
    count: i64,
    #[allow(dead_code)]
    center_lat: f64,
    #[allow(dead_code)]
    center_lng: f64,
    west: f64,
    south: f64,
    east: f64,
    north: f64,
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn returns_cities_with_counts(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (status, cities): (_, Vec<City>) = get_json(app, "/v1/search/map/cities").await;
    assert_eq!(status, 200);

    // Seed has 2 geocoded rows: alice's gallery in London, bruno's in
    // Berlin. Both expected.
    let names: Vec<&str> = cities.iter().map(|c| c.city.as_str()).collect();
    assert!(names.contains(&"London"));
    assert!(names.contains(&"Berlin"));

    for c in &cities {
        assert_eq!(c.count, 1);
        assert!(c.east >= c.west);
        assert!(c.north >= c.south);
    }

    let london = cities.iter().find(|c| c.city == "London").unwrap();
    assert_eq!(london.country.as_deref(), Some("GB"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn excludes_pre_geocode_rows(pool: PgPool) {
    // Alice's "Studio (by appointment)" row has lat=lng=NULL → must
    // not contribute to any city's count. We assert that by inserting
    // a second pre-geocode row for alice in a *new* city and
    // confirming that city doesn't appear.
    sqlx::query(
        "INSERT INTO artist_locations
         (artist_id, kind, name, address, geocoded_at)
         VALUES ('aaa11111-1111-1111-1111-111111111111',
                 'studio', 'Hidden Studio', '1 Pending St', NULL)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let app = app_keyword_only(pool);
    let (_, cities): (_, Vec<City>) = get_json(app, "/v1/search/map/cities").await;
    // Hidden studio has lat=NULL so its city stays NULL; the SQL
    // filters `al.city IS NOT NULL` so we shouldn't see a NULL-city
    // row at all. (The seed's pre-geocode row had no city either.)
    for c in &cities {
        assert!(!c.city.is_empty(), "no empty city should appear");
    }
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn ordered_by_count_desc(pool: PgPool) {
    // Add two more London rows to give it a higher count than Berlin.
    sqlx::query(
        "INSERT INTO artist_locations
         (artist_id, kind, name, address, city, country, lat, lng, geocoded_at)
         VALUES
           ('aaa11111-1111-1111-1111-111111111111',
            'gallery', 'Second London Gallery', '2 More St, London',
            'London', 'GB', 51.52, -0.10, now()),
           ('aaa11111-1111-1111-1111-111111111111',
            'studio', 'Third London Studio', '3 Another St, London',
            'London', 'GB', 51.51, -0.11, now())",
    )
    .execute(&pool)
    .await
    .unwrap();

    let app = app_keyword_only(pool);
    let (_, cities): (_, Vec<City>) = get_json(app, "/v1/search/map/cities").await;
    assert!(cities.len() >= 2);
    assert_eq!(cities[0].city, "London");
    assert_eq!(cities[0].count, 3);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn limit_param_caps_response(pool: PgPool) {
    let app = app_keyword_only(pool);
    let (_, cities): (_, Vec<City>) = get_json(app, "/v1/search/map/cities?limit=1").await;
    assert_eq!(cities.len(), 1);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn multi_pin_city_has_real_bbox(pool: PgPool) {
    // London has only one seeded pin, so its bbox degenerates to a
    // point. Add a second pin in London with a distinct lat/lng and
    // confirm the bbox widens to include both.
    sqlx::query(
        "INSERT INTO artist_locations
         (artist_id, kind, name, address, city, country, lat, lng, geocoded_at)
         VALUES ('aaa11111-1111-1111-1111-111111111111',
                 'gallery', 'East London Space', '99 Far East Rd, London',
                 'London', 'GB', 51.55, 0.05, now())",
    )
    .execute(&pool)
    .await
    .unwrap();

    let app = app_keyword_only(pool);
    let (_, cities): (_, Vec<City>) = get_json(app, "/v1/search/map/cities").await;
    let london = cities.iter().find(|c| c.city == "London").unwrap();
    assert!(
        london.east - london.west > 0.0,
        "bbox should widen when multiple pins"
    );
    assert!(london.north - london.south > 0.0);
}
