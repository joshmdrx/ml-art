"""Demo data seed for local development.

Ingests the WikiArt corpus into Postgres + S3-compatible storage:

- Groups images by WikiArt style (parsed from filename: ``wikiart-<style>-NNN.jpg``).
- Creates one synthetic artist per style (clearly labeled as demo content).
- For each image: uploads the JPEG to S3 (``artworks/`` bucket), inserts an
  ``artworks`` row, an ``artwork_images`` row, and an ``artwork_embeddings`` row
  using the cached jina-clip-v2 embeddings from the spike.
- Creates 6 hand-curated themed neighborhoods bridging multiple styles.

All rows are flagged ``is_demo = true``. Production deployments filter these
out; staging and local dev show them.

Idempotency: pass ``--reset`` to delete all ``is_demo = true`` rows before
inserting. Without ``--reset`` the script refuses to run if demo content
already exists, to avoid duplicates.

Run::

    uv pip install -e ".[local,data,seed]"
    docker compose -f ../docker-compose.dev.yml up -d
    sqlx migrate run --source ../db/migrations \\
        --database-url postgres://ml_art:dev@localhost:5432/ml_art_dev
    uv run python -m ml_art.seed \\
        --data-dir spikes/2026-05-modifier-deltas/data/wikiart \\
        --reset
"""

from __future__ import annotations

import argparse
import hashlib
import io
import os
import re
import sys
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

import boto3
import numpy as np
import psycopg
from botocore.client import Config as BotoConfig
from psycopg.types.json import Jsonb
from tqdm import tqdm

from ml_art.config import get_config
from ml_art.corpus import CorpusItem, load_corpus
from ml_art.embeddings.cache import CachedEmbedder
from ml_art.vectors import normalize


# ─────────────────────────────────────────────────────────────────────────────
# Style → synthetic artist mapping
# ─────────────────────────────────────────────────────────────────────────────

# Filename pattern from datasets/wikiart.py: wikiart-<style_slug>-NNN.jpg
_FILENAME_RE = re.compile(r"^wikiart-(.+?)-(\d+)\.jpg$")


@dataclass(frozen=True)
class SyntheticArtist:
    slug: str
    display_name: str
    bio: str
    city: str | None
    country: str | None  # ISO 3166-1 alpha-2
    lat: float | None
    lng: float | None


# A handful of cities, deterministically distributed across styles so the
# location filter has something to filter on.
_CITIES = [
    ("London", "GB", 51.5074, -0.1278),
    ("Berlin", "DE", 52.5200, 13.4050),
    ("Paris", "FR", 48.8566, 2.3522),
    ("New York", "US", 40.7128, -74.0060),
    ("Mexico City", "MX", 19.4326, -99.1332),
    ("Tokyo", "JP", 35.6762, 139.6503),
    ("Lisbon", "PT", 38.7223, -9.1393),
    ("Cape Town", "ZA", -33.9249, 18.4241),
    ("São Paulo", "BR", -23.5505, -46.6333),
    ("Melbourne", "AU", -37.8136, 144.9631),
]


def synthetic_artist_for_style(style: str) -> SyntheticArtist:
    """Build a deterministic SyntheticArtist from a WikiArt style label."""
    pretty = style.replace("_", " ").title()
    slug = "demo-" + style.lower().replace("_", "-")
    display_name = f"{pretty} Studio (Demo)"
    bio = (
        f"A curated demo collection in the {pretty} style, used to showcase the "
        f"platform's search and discovery features. Not a real artist."
    )
    # Deterministic city assignment by style hash.
    h = int(hashlib.sha256(style.encode()).hexdigest(), 16)
    city, country, lat, lng = _CITIES[h % len(_CITIES)]
    return SyntheticArtist(
        slug=slug,
        display_name=display_name,
        bio=bio,
        city=city,
        country=country,
        lat=lat,
        lng=lng,
    )


# ─────────────────────────────────────────────────────────────────────────────
# Themed neighborhoods (hand-curated, kind='curated')
# ─────────────────────────────────────────────────────────────────────────────

# NOTE: style keys here must match the slug form produced by datasets/wikiart.py,
# i.e. lowercase with underscores preserved (e.g. "color_field_painting").
_NEIGHBORHOODS: list[tuple[str, str, str, list[str]]] = [
    (
        "the-impressionists",
        "The Impressionists",
        "Painters chasing light, atmosphere, and the impression of a moment.",
        ["impressionism", "post_impressionism", "pointillism"],
    ),
    (
        "fields-of-color",
        "Fields of Color",
        "Reductive, contemplative, color-as-subject. Rothko's heirs.",
        ["color_field_painting", "minimalism"],
    ),
    (
        "geometric-fractures",
        "Geometric Fractures",
        "Cubism and its descendants — splintering form into facets.",
        ["cubism", "synthetic_cubism", "analytical_cubism"],
    ),
    (
        "expressionist-souls",
        "Expressionist Souls",
        "Inner worlds painted outward — emotion before description.",
        ["expressionism", "abstract_expressionism", "fauvism", "action_painting"],
    ),
    (
        "old-masters",
        "Old Masters",
        "Renaissance and Baroque — the long apprenticeship of European painting.",
        ["high_renaissance", "baroque", "early_renaissance", "northern_renaissance", "mannerism_late_renaissance"],
    ),
    (
        "pop-and-after",
        "Pop and After",
        "Bold, graphic, of-the-world — Pop Art and the realist responses that followed.",
        ["pop_art", "contemporary_realism", "new_realism"],
    ),
]


# ─────────────────────────────────────────────────────────────────────────────
# Main
# ─────────────────────────────────────────────────────────────────────────────


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument(
        "--data-dir",
        type=Path,
        default=None,
        help="Directory of WikiArt JPEGs (required unless --locations-only)",
    )
    parser.add_argument("--reset", action="store_true", help="Delete existing demo rows first")
    parser.add_argument("--limit", type=int, default=None, help="Cap artworks ingested (debug)")
    parser.add_argument(
        "--locations-only",
        action="store_true",
        help=(
            "Skip corpus + embedding. Just insert one artist_locations "
            "row per existing demo artist that lacks one. Use after an "
            "older seed run to backfill the search map."
        ),
    )
    parser.add_argument(
        "--database-url",
        # Default matches the compose stack (Makefile + docker-compose
        # both bind Postgres to host port 5433 to avoid colliding with
        # a system Postgres on 5432). DATABASE_URL env wins.
        default=os.environ.get("DATABASE_URL", "postgres://ml_art:dev@localhost:5433/ml_art_dev"),
    )
    args = parser.parse_args()

    cfg = get_config()

    # Postgres. autocommit=True so `with conn.transaction()` issues real
    # BEGIN/COMMIT — with autocommit=False, conn.transaction() only opens
    # a savepoint inside an implicit outer transaction that never commits.
    conn = psycopg.connect(args.database_url, autocommit=True)

    # Backfill-only path: skip corpus/embedder/S3 entirely. Walks
    # existing `demo-*` artists and adds a location for any that
    # lack one. Safe to re-run.
    if args.locations_only:
        try:
            with conn.transaction():
                _ensure_locations_backfill(conn)
        finally:
            conn.close()
        print("done.")
        return

    if args.data_dir is None:
        sys.exit("--data-dir is required unless --locations-only is set")

    # S3 / MinIO
    s3 = boto3.client(
        "s3",
        endpoint_url=os.environ.get("S3_ENDPOINT", "http://localhost:9000"),
        aws_access_key_id=os.environ.get("AWS_ACCESS_KEY_ID", "dev"),
        aws_secret_access_key=os.environ.get("AWS_SECRET_ACCESS_KEY", "devpassword"),
        region_name=os.environ.get("AWS_REGION", "us-east-1"),
        config=BotoConfig(signature_version="s3v4", s3={"addressing_style": "path"}),
    )
    bucket = os.environ.get("S3_BUCKET_ARTWORKS", "artworks")

    if args.reset:
        _reset_demo(conn)

    if _has_demo_content(conn):
        sys.exit(
            "demo content already present — pass --reset to wipe and re-seed, "
            "or drop the existing rows manually."
        )

    items = load_corpus(args.data_dir)
    if not items:
        sys.exit(f"no images in {args.data_dir} — run datasets.wikiart first")

    grouped = _group_by_style(items)
    if args.limit:
        # Trim per style proportionally so we keep variety.
        per = max(1, args.limit // len(grouped))
        grouped = {k: v[:per] for k, v in grouped.items()}
        items = [it for vs in grouped.values() for it in vs]

    print(f"styles: {len(grouped)}, artworks: {len(items)}")

    # Load embedder (uses cached embeddings from the spike).
    from ml_art.embeddings.local_jina import LocalJinaClipEmbedder

    embedder = CachedEmbedder(LocalJinaClipEmbedder(show_progress=True), cfg.cache_dir)

    # Embed everything via the cache — fast if the spike has been run.
    image_bytes = [it.read_bytes() for it in items]
    embeddings = embedder.embed_images(image_bytes)
    embeddings = normalize(embeddings)

    # Map item.sha256 → embedding vector for lookup during insert.
    sha_to_vec = {item.sha256: embeddings[i] for i, item in enumerate(items)}

    try:
        with conn.transaction():
            artists_by_style = _ensure_artists(conn, grouped.keys())
            _ensure_artworks(conn, s3, bucket, grouped, artists_by_style, sha_to_vec, embedder)
            _ensure_neighborhoods(conn, grouped)
            _ensure_locations(conn, artists_by_style)
    finally:
        conn.close()
    print("done.")


# ─────────────────────────────────────────────────────────────────────────────
# Per-step helpers
# ─────────────────────────────────────────────────────────────────────────────


def _has_demo_content(conn: psycopg.Connection) -> bool:
    with conn.cursor() as cur:
        cur.execute("SELECT 1 FROM artworks WHERE is_demo = true LIMIT 1")
        return cur.fetchone() is not None


def _reset_demo(conn: psycopg.Connection) -> None:
    """Wipe all is_demo=true rows. Uses conn.transaction(), not `with conn:`,
    because `with conn:` *closes* the connection in psycopg3."""
    print("resetting demo content...")
    with conn.transaction():
        with conn.cursor() as cur:
            # Order matters: children before parents (no soft delete here — wipe).
            cur.execute(
                "DELETE FROM neighborhood_artworks WHERE artwork_id IN "
                "(SELECT id FROM artworks WHERE is_demo = true)"
            )
            cur.execute(
                "DELETE FROM neighborhoods WHERE slug = ANY(%s)",
                ([n[0] for n in _NEIGHBORHOODS],),
            )
            cur.execute(
                "DELETE FROM artwork_embeddings WHERE artwork_id IN "
                "(SELECT id FROM artworks WHERE is_demo = true)"
            )
            cur.execute(
                "DELETE FROM artwork_images WHERE artwork_id IN "
                "(SELECT id FROM artworks WHERE is_demo = true)"
            )
            cur.execute("DELETE FROM artworks WHERE is_demo = true")
            cur.execute(
                "DELETE FROM artist_locations "
                "WHERE artist_id IN (SELECT id FROM artists WHERE slug LIKE 'demo-%%')"
            )
            cur.execute("DELETE FROM artists WHERE slug LIKE 'demo-%%'")


def _group_by_style(items: list[CorpusItem]) -> dict[str, list[CorpusItem]]:
    grouped: dict[str, list[CorpusItem]] = defaultdict(list)
    for it in items:
        m = _FILENAME_RE.match(it.path.name)
        if not m:
            continue
        style = m.group(1)
        # Restore the original WikiArt style label form (underscores from the slug).
        # Our wikiart loader slugified lowercase + underscores already; capitalize
        # words for display when used elsewhere. Keep the slug form as the key.
        grouped[style].append(it)
    return dict(grouped)


def _ensure_artists(
    conn: psycopg.Connection,
    styles: Iterable[str],
) -> dict[str, str]:
    """Insert (or fetch) a synthetic artist per style. Returns slug→id."""
    out: dict[str, str] = {}
    with conn.cursor() as cur:
        for style in styles:
            a = synthetic_artist_for_style(style)
            cur.execute(
                """
                INSERT INTO artists (
                    slug, display_name, bio,
                    location, city, country, lat, lng, geocoded_at,
                    inquiry_preferences, status
                )
                VALUES (
                    %s, %s, %s,
                    %s, %s, %s, %s, %s, now(),
                    %s, 'active'
                )
                ON CONFLICT (slug) DO UPDATE SET display_name = EXCLUDED.display_name
                RETURNING id;
                """,
                (
                    a.slug,
                    a.display_name,
                    a.bio,
                    f"{a.city}, {a.country}" if a.city else None,
                    a.city,
                    a.country,
                    a.lat,
                    a.lng,
                    Jsonb({"type": "platform"}),
                ),
            )
            row = cur.fetchone()
            assert row is not None
            out[style] = row[0]
    print(f"artists: {len(out)} ensured")
    return out


def _ensure_artworks(
    conn: psycopg.Connection,
    s3,
    bucket: str,
    grouped: dict[str, list[CorpusItem]],
    artists_by_style: dict[str, str],
    sha_to_vec: dict[str, np.ndarray],
    embedder: CachedEmbedder,
) -> None:
    """Upload, insert artwork + image + embedding rows for each item."""
    model_name = embedder.model_name
    model_version = embedder.model_version

    total = sum(len(v) for v in grouped.values())
    pbar = tqdm(total=total, desc="ingesting", unit="art")

    with conn.cursor() as cur:
        for style, items in grouped.items():
            artist_id = artists_by_style[style]
            pretty = style.replace("_", " ").title()
            for item in items:
                s3_key = f"demo/{style}/{item.sha256[:16]}.jpg"
                # Upload (idempotent on key)
                with item.path.open("rb") as f:
                    s3.put_object(
                        Bucket=bucket,
                        Key=s3_key,
                        Body=f,
                        ContentType="image/jpeg",
                        CacheControl="public, max-age=31536000, immutable",
                    )

                # Insert artwork
                cur.execute(
                    """
                    INSERT INTO artworks (
                        artist_id, title, description, medium, status,
                        is_demo, published_at
                    )
                    VALUES (%s, %s, %s, %s, 'published', true, now())
                    RETURNING id;
                    """,
                    (
                        artist_id,
                        f"Untitled ({pretty})",
                        f"Demo work in the {pretty} style, sourced from the public WikiArt dataset.",
                        pretty,
                    ),
                )
                row = cur.fetchone()
                assert row is not None
                artwork_id = row[0]

                # Insert primary image row
                cur.execute(
                    """
                    INSERT INTO artwork_images (
                        artwork_id, s3_key, width, height,
                        is_primary, display_order, moderation_status
                    )
                    VALUES (%s, %s, %s, %s, true, 0, 'approved');
                    """,
                    (artwork_id, s3_key, item.width, item.height),
                )

                # Insert embedding
                vec = sha_to_vec.get(item.sha256)
                if vec is None:
                    # Should not happen — embed_images was called over the full set.
                    continue
                cur.execute(
                    """
                    INSERT INTO artwork_embeddings (
                        artwork_id, model_name, model_version, embedding
                    )
                    VALUES (%s, %s, %s, %s);
                    """,
                    (
                        artwork_id,
                        model_name,
                        model_version,
                        _vec_to_pgvector(vec),
                    ),
                )

                pbar.update(1)
    pbar.close()


def _ensure_neighborhoods(
    conn: psycopg.Connection,
    grouped: dict[str, list[CorpusItem]],
) -> None:
    """Create the 6 themed neighborhoods over the seeded artworks."""
    with conn.cursor() as cur:
        for i, (slug, name, description, styles) in enumerate(_NEIGHBORHOODS):
            # Collect artwork IDs for the styles in this neighborhood.
            style_keys = [s for s in styles if s in grouped]
            if not style_keys:
                continue
            cur.execute(
                """
                SELECT a.id
                FROM artworks a
                JOIN artists ar ON ar.id = a.artist_id
                WHERE a.is_demo = true
                  AND ar.slug = ANY(%s)
                """,
                ([f"demo-{s.lower().replace('_', '-')}" for s in style_keys],),
            )
            artwork_ids = [r[0] for r in cur.fetchall()]
            if not artwork_ids:
                continue

            reps = artwork_ids[:3]  # first three as representative thumbs

            cur.execute(
                """
                INSERT INTO neighborhoods (
                    slug, name, description, kind,
                    representative_artwork_ids, artwork_count,
                    display_order, is_featured, computed_at
                )
                VALUES (%s, %s, %s, 'curated', %s, %s, %s, true, now())
                ON CONFLICT (slug) DO UPDATE SET
                    name = EXCLUDED.name,
                    description = EXCLUDED.description,
                    representative_artwork_ids = EXCLUDED.representative_artwork_ids,
                    artwork_count = EXCLUDED.artwork_count
                RETURNING id;
                """,
                (slug, name, description, reps, len(artwork_ids), i),
            )
            row = cur.fetchone()
            assert row is not None
            nb_id = row[0]

            # Wipe any existing memberships and re-insert.
            cur.execute(
                "DELETE FROM neighborhood_artworks WHERE neighborhood_id = %s",
                (nb_id,),
            )
            cur.executemany(
                "INSERT INTO neighborhood_artworks (neighborhood_id, artwork_id) "
                "VALUES (%s, %s)",
                [(nb_id, aid) for aid in artwork_ids],
            )
    print(f"neighborhoods: {len(_NEIGHBORHOODS)} ensured")


def _ensure_locations_backfill(conn: psycopg.Connection) -> None:
    """Run `_ensure_locations` against whatever demo artists already
    exist in the DB. Used by the `--locations-only` CLI path to
    upgrade an older seed without re-embedding the entire corpus.

    Reconstructs the `style → artist_id` map from the artist's
    `demo-<style-with-hyphens>` slug.
    """
    artists_by_style: dict[str, str] = {}
    with conn.cursor() as cur:
        cur.execute(
            "SELECT slug, id FROM artists "
            "WHERE slug LIKE 'demo-%%' AND deleted_at IS NULL"
        )
        for slug, artist_id in cur.fetchall():
            # synthetic_artist_for_style builds the slug as
            # "demo-" + style.lower().replace("_", "-"). Invert.
            style = slug.removeprefix("demo-").replace("-", "_")
            artists_by_style[style] = str(artist_id)
    if not artists_by_style:
        print("no demo artists found — nothing to backfill")
        return
    print(f"backfilling locations for {len(artists_by_style)} demo artists")
    _ensure_locations(conn, artists_by_style)


def _ensure_locations(
    conn: psycopg.Connection,
    artists_by_style: dict[str, str],
) -> None:
    """Insert one `artist_locations` gallery per demo artist, so the
    search-map view has pins to render. Idempotent: an artist that
    already has at least one location is skipped, so re-running the
    seed without ``--reset`` doesn't create duplicates.

    Each demo artist's `artists.lat/lng/city/country` (assigned by
    ``synthetic_artist_for_style``) already pins them to a deterministic
    city. We mirror that location into ``artist_locations`` as a
    "gallery showing" so the public map endpoints see it.
    """
    inserted = 0
    with conn.cursor() as cur:
        for style, artist_id in artists_by_style.items():
            # Skip when this artist already has a location (idempotency).
            cur.execute(
                "SELECT 1 FROM artist_locations "
                "WHERE artist_id = %s AND deleted_at IS NULL LIMIT 1",
                (artist_id,),
            )
            if cur.fetchone() is not None:
                continue

            a = synthetic_artist_for_style(style)
            if a.lat is None or a.lng is None or a.city is None:
                continue  # artist has no anchor city, nothing to pin

            pretty = style.replace("_", " ").title()
            cur.execute(
                """
                INSERT INTO artist_locations (
                    artist_id, kind, name, address,
                    city, country, lat, lng, geocoded_at
                )
                VALUES (
                    %s, 'gallery', %s, %s,
                    %s, %s, %s, %s, now()
                )
                """,
                (
                    artist_id,
                    f"{pretty} Gallery (Demo)",
                    # Address is the "based in" string — good enough for
                    # demo content; the real geocoded lat/lng comes from
                    # synthetic_artist_for_style's curated city list.
                    f"{a.city}, {a.country}",
                    a.city,
                    a.country,
                    a.lat,
                    a.lng,
                ),
            )
            inserted += 1
    print(f"artist_locations: {inserted} new (existing rows preserved)")


def _vec_to_pgvector(v: np.ndarray) -> str:
    """Format a 1D numpy float vector for the pgvector text input form."""
    return "[" + ",".join(f"{float(x):.6f}" for x in v) + "]"


if __name__ == "__main__":
    main()
