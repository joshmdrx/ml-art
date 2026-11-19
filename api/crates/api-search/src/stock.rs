//! T-090 — how many copies of an artwork can still be sold.
//!
//! Capacity is `COALESCE(edition_size, 1)`: a NULL edition size is a
//! one-off original. Copies sold = live orders (`paid` / `shipped` /
//! `delivered` / `disputed`) + `editions_sold_offline` (sales the artist
//! made elsewhere, recorded in the studio). A `pending` order whose
//! `hold_expires_at` is still in the future reserves one copy while its
//! buyer is on Stripe Checkout.
//!
//! Every path that can change those counts — checkout, the payment and
//! refund webhooks, the studio editor — first takes a row lock on the
//! artwork via [`lock`], so the count and the write that depends on it
//! are atomic (two buyers can't both reserve the last copy).
//!
//! `artworks.availability` stays the public source of truth. For a work
//! that's on sale (`available` / `sold`) [`sync_availability`] derives
//! `sold` (= sold out) vs `available` from the counts; `inquire` and
//! `not_for_sale` are the artist's choice and never touched.

use sqlx::PgConnection;
use uuid::Uuid;

/// How long the buyer's Stripe Checkout Session stays open. Stripe's
/// minimum is 30 minutes from creation; one more minute keeps us clear of
/// it when the request takes a moment to reach Stripe.
pub const CHECKOUT_WINDOW: chrono::Duration = chrono::Duration::minutes(31);

/// Extra time a hold outlives its Checkout Session, so a payment made in
/// the session's last second still owns its copy while the
/// `checkout.session.completed` webhook is in flight.
pub const HOLD_GRACE: chrono::Duration = chrono::Duration::minutes(10);

#[derive(Debug, Clone)]
pub struct Stock {
    pub edition_size: Option<i32>,
    pub editions_sold_offline: i32,
    pub availability: String,
    pub status: String,
    pub deleted: bool,
    /// Live orders, excluding the order passed to [`lock`] (if any).
    pub sold_online: i64,
    /// Unexpired holds by *other* buyers (and not the excluded order).
    pub held_by_others: i64,
}

impl Stock {
    pub fn capacity(&self) -> i64 {
        i64::from(self.edition_size.unwrap_or(1))
    }

    pub fn sold(&self) -> i64 {
        self.sold_online + i64::from(self.editions_sold_offline)
    }

    /// Copies not yet sold (holds not subtracted).
    pub fn remaining(&self) -> i64 {
        (self.capacity() - self.sold()).max(0)
    }

    /// Copies a new buyer could reserve right now.
    pub fn free_now(&self) -> i64 {
        (self.remaining() - self.held_by_others).max(0)
    }

    /// Can a payment that just completed be honoured? The work must still
    /// be published and on sale, and have a copy left once every *other*
    /// live order is counted. (The paying order's own hold is what reserved
    /// that copy, so holds aren't subtracted here.)
    pub fn can_fulfil(&self) -> bool {
        !self.deleted
            && self.status == "published"
            && self.availability == "available"
            && self.remaining() > 0
    }
}

#[derive(sqlx::FromRow)]
struct LockedRow {
    edition_size: Option<i32>,
    editions_sold_offline: i32,
    availability: String,
    status: String,
    deleted: bool,
}

/// Lock the artwork row (`FOR UPDATE`) and count its copies. Must run in a
/// transaction; the lock is held until it commits. `exclude_buyer` leaves
/// that buyer's own holds out of `held_by_others`; `exclude_order` leaves
/// one order out of both counts (the order being decided on). Returns
/// `None` if the artwork doesn't exist.
pub async fn lock(
    conn: &mut PgConnection,
    artwork_id: Uuid,
    exclude_buyer: Option<Uuid>,
    exclude_order: Option<Uuid>,
) -> Result<Option<Stock>, sqlx::Error> {
    let Some(row): Option<LockedRow> = sqlx::query_as(
        r#"
        SELECT edition_size, editions_sold_offline, availability, status,
               deleted_at IS NOT NULL AS deleted
        FROM artworks
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(artwork_id)
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };

    // Live statuses — paid / shipped / delivered / disputed — are the ones
    // that consume a copy until refunded. Keep in step with
    // `sync_availability` below.
    let (sold_online, held_by_others): (i64, i64) = sqlx::query_as(
        r#"
        SELECT
            count(*) FILTER (WHERE status IN ('paid', 'shipped', 'delivered', 'disputed')),
            count(*) FILTER (
                WHERE status = 'pending'
                  AND hold_expires_at > now()
                  AND buyer_user_id IS DISTINCT FROM $2
            )
        FROM orders
        WHERE artwork_id = $1
          AND id IS DISTINCT FROM $3
        "#,
    )
    .bind(artwork_id)
    .bind(exclude_buyer)
    .bind(exclude_order)
    .fetch_one(&mut *conn)
    .await?;

    Ok(Some(Stock {
        edition_size: row.edition_size,
        editions_sold_offline: row.editions_sold_offline,
        availability: row.availability,
        status: row.status,
        deleted: row.deleted,
        sold_online,
        held_by_others,
    }))
}

/// Re-derive `availability` for a work that's on sale: `sold` once every
/// copy is sold, back to `available` when one frees up (a refund, a bigger
/// edition, fewer offline sales). Leaves `inquire` / `not_for_sale` alone.
/// Call inside the transaction holding [`lock`].
pub async fn sync_availability(
    conn: &mut PgConnection,
    artwork_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE artworks a
        SET availability = s.derived, updated_at = now()
        FROM (
            SELECT CASE
                WHEN (SELECT count(*) FROM orders o
                      WHERE o.artwork_id = $1
                        AND o.status IN ('paid', 'shipped', 'delivered', 'disputed'))
                     + w.editions_sold_offline
                     >= COALESCE(w.edition_size, 1)
                THEN 'sold' ELSE 'available'
            END AS derived
            FROM artworks w WHERE w.id = $1
        ) s
        WHERE a.id = $1
          AND a.availability IN ('available', 'sold')
          AND a.availability <> s.derived
        "#,
    )
    .bind(artwork_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock(edition: Option<i32>, offline: i32, online: i64, held: i64) -> Stock {
        Stock {
            edition_size: edition,
            editions_sold_offline: offline,
            availability: "available".into(),
            status: "published".into(),
            deleted: false,
            sold_online: online,
            held_by_others: held,
        }
    }

    #[test]
    fn original_is_an_edition_of_one() {
        let s = stock(None, 0, 0, 0);
        assert_eq!((s.capacity(), s.remaining(), s.free_now()), (1, 1, 1));
        assert!(s.can_fulfil());
        let sold = stock(None, 0, 1, 0);
        assert_eq!(sold.remaining(), 0);
        assert!(!sold.can_fulfil());
    }

    #[test]
    fn edition_counts_online_offline_and_holds() {
        let s = stock(Some(10), 3, 5, 1);
        assert_eq!(s.sold(), 8);
        assert_eq!(s.remaining(), 2);
        assert_eq!(s.free_now(), 1, "one of the two left is on hold");
        assert!(s.can_fulfil(), "holds don't block a payment that completed");
    }

    #[test]
    fn last_copy_held_by_someone_else_is_not_free() {
        let s = stock(Some(5), 0, 4, 1);
        assert_eq!(s.remaining(), 1);
        assert_eq!(s.free_now(), 0);
    }

    #[test]
    fn withdrawn_or_unpublished_work_cannot_be_fulfilled() {
        let mut s = stock(Some(5), 0, 0, 0);
        s.availability = "not_for_sale".into();
        assert!(!s.can_fulfil());
        let mut s = stock(None, 0, 0, 0);
        s.status = "archived".into();
        assert!(!s.can_fulfil());
        let mut s = stock(None, 0, 0, 0);
        s.deleted = true;
        assert!(!s.can_fulfil());
    }

    #[test]
    fn counts_never_go_negative() {
        // An artist could over-record offline sales relative to a later
        // edition shrink; the DB CHECK stops it, but the maths still clamps.
        let s = stock(Some(2), 2, 1, 3);
        assert_eq!((s.remaining(), s.free_now()), (0, 0));
    }
}
