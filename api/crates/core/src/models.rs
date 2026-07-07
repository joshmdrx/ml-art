//! Shared domain types. Serialized over the wire to the Next.js client.
//!
//! Types intentionally use snake_case JSON (consistent with the spec).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtworkSummary {
    pub id: Uuid,
    pub title: Option<String>,
    /// Stable artist id. Lets surfaces that show the grid feed a
    /// secondary view (e.g. the search map) without re-querying the
    /// artwork set — the consumer just collects the distinct
    /// `artist_id`s and asks the map endpoint for those.
    pub artist_id: Uuid,
    pub artist_name: String,
    pub artist_slug: String,
    pub primary_image_url: Option<String>,
    pub price_cents: Option<i64>,
    pub currency: String,
    pub availability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paginated<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FacetCounts {
    pub medium: serde_json::Value,
    pub price: serde_json::Value,
    pub orientation: serde_json::Value,
    pub availability: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    #[default]
    Relevance,
    Newest,
    PriceAsc,
    PriceDesc,
    Nearest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistSummary {
    pub id: Uuid,
    pub slug: String,
    pub display_name: String,
    pub location: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub representative_image_urls: Vec<String>,
}

/// Full artist profile as returned by `GET /v1/artists/:slug`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistFull {
    pub id: Uuid,
    pub slug: String,
    pub display_name: String,
    pub bio: Option<String>,
    pub artist_statement: Option<String>,
    pub location: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub website_url: Option<String>,
    pub socials: serde_json::Value,
    pub commissioning_preferences: Option<serde_json::Value>,
    pub representative_image_urls: Vec<String>,
    /// T-083 — admin preview lifts the public `status='active'`
    /// filter so admins can review pending / paused / rejected
    /// artists inline. Always `active` for non-admin callers (they
    /// can't see other statuses at all). Web layer keys the
    /// "Admin view" banner on this + the caller's is_admin flag.
    pub status: String,
    /// T-085 — "individual" (default) or "gallery". Drives the
    /// "Gallery" chip on the profile header + a copy tweak in
    /// onboarding. No downstream routing fork.
    pub entity_type: String,
}

/// Composite response for `/v1/artists/:slug`: profile + first page of artworks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistDetail {
    pub artist: ArtistFull,
    pub artworks: Paginated<ArtworkSummary>,
    /// Geocoded, non-deleted locations for this artist — only included
    /// when the geocode has completed (lat/lng present). Empty list when
    /// the artist has no public locations; the web client falls back to
    /// the artist's `based in` city pill in that case. See T-038.
    #[serde(default)]
    pub locations: Vec<ArtistLocation>,
    /// T-052 — true when the request carried valid auth AND that user
    /// follows this artist. False for signed-out requests; false for
    /// signed-in requests where no follow row exists. Lets the
    /// `<FollowButton>` render the right state without a second
    /// roundtrip.
    #[serde(default)]
    pub is_following: bool,
    /// T-052 — number of followers this artist has, regardless of who
    /// is asking. Surfaces on the artist page; matches the studio
    /// dashboard count.
    #[serde(default)]
    pub follower_count: i32,
}

/// A physical place where an artist's work can be seen — gallery the
/// artist is represented at, or studio open by appointment. See
/// `db/migrations/0011_artist_locations.sql` and `decisions.md` 2026-05-28.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistLocation {
    pub id: Uuid,
    /// `gallery` | `studio`. Shows / events are out of scope for v1.
    pub kind: String,
    pub name: String,
    pub address: String,
    pub city: Option<String>,
    pub country: Option<String>,
    /// Present iff geocoding has succeeded. Public surfaces only render
    /// rows where both are set.
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub website_url: Option<String>,
    pub display_order: i32,
    /// Last time the geocode job ran for this row, success or failure.
    /// `None` means "never attempted yet" — the studio UI surfaces this
    /// as "Locating…".
    pub geocoded_at: Option<DateTime<Utc>>,
}

/// Full artwork as returned by `GET /v1/artworks/:id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtworkFull {
    pub id: Uuid,
    pub title: Option<String>,
    pub description: Option<String>,
    pub year_created: Option<i32>,
    /// T-073 — free-text materials description ("Oil on linen").
    pub medium: Option<String>,
    /// T-073 — canonical taxonomy code from `core::media::CATEGORIES`.
    /// Nullable until backfilled for older rows.
    pub medium_category: Option<String>,
    pub dimensions: Option<serde_json::Value>,
    pub price_cents: Option<i64>,
    pub currency: String,
    pub availability: String,
    pub external_url: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub artist: ArtworkArtist,
    pub images: Vec<ArtworkImage>,
}

/// Embedded artist on `ArtworkFull` — enough to render the credit line and
/// link to the artist's portfolio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtworkArtist {
    pub id: Uuid,
    pub slug: String,
    pub display_name: String,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtworkImage {
    pub id: Uuid,
    pub url: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub is_primary: bool,
    pub display_order: i32,
}

/// A user's saved collection of artworks, as returned in lists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSummary {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_public: bool,
    pub share_id: Option<String>,
    /// Up to 4 image URLs from the most-recently-added artworks. Used as
    /// the asymmetric "mosaic" cover on `/collections`.
    pub cover_image_urls: Vec<String>,
    pub artwork_count: i32,
    pub updated_at: DateTime<Utc>,
    /// True iff the request supplied `?artwork_id=<id>` AND that artwork
    /// is currently in this collection. Always `false` on plain list
    /// calls — the Save modal opts in via the query param so it can
    /// render check-state correctly without a second roundtrip per row.
    #[serde(default)]
    pub contains_artwork: bool,
}

/// Response for `POST /v1/artworks/:id/inquiries`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InquiryAck {
    pub id: Uuid,
    /// `delivered` — sent immediately (signed-in users; their Clerk-verified
    /// email skips the verification step).
    /// `pending_verification` — anonymous; check your inbox for a confirm link.
    pub status: String,
}

/// Full collection view — metadata + paginated artworks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionDetail {
    pub collection: CollectionSummary,
    pub artworks: Paginated<ArtworkSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Neighborhood {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    /// "curated" | "semantic" | "geographic" (see `db/migrations/0005_neighborhoods.sql`)
    pub kind: String,
    pub representative_image_urls: Vec<String>,
    pub artwork_count: i32,
    pub is_featured: bool,
}

/// Composite response for `/v1/neighborhoods/:slug`: header + first page of artworks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeighborhoodDetail {
    pub neighborhood: Neighborhood,
    pub artworks: Paginated<ArtworkSummary>,
}

/// T-058 — artist-curated grouping of their own works. Public read shape;
/// what the artist page renders when `?view=series` is on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesSummary {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub statement: Option<String>,
    /// Resolved from the series's `cover_artwork_id` → primary image
    /// URL. Falls back to the first member artwork's primary image
    /// when `cover_artwork_id` is null. `None` when the series has no
    /// artworks (only possible in studio views — empty series are
    /// hidden from public reads).
    pub cover_image_url: Option<String>,
    pub artwork_count: i32,
}

/// Composite response for `/v1/artists/:slug/series/:series-slug`:
/// header + first page of member artworks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesDetail {
    pub series: SeriesSummary,
    pub artist: ArtistSummary,
    pub artworks: Paginated<ArtworkSummary>,
}

/// Studio-side view of a series — includes editable fields the public
/// shape hides. Used by `/v1/studio/series/*`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesStudio {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub statement: Option<String>,
    pub cover_artwork_id: Option<Uuid>,
    pub cover_image_url: Option<String>,
    pub artwork_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[allow(dead_code)]
fn _ts_check() -> Option<DateTime<Utc>> {
    None
}
