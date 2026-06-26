"""T-057 — unit tests for the algorithmic-neighbourhoods builder.

Slug + label-response parsing are easy to test in isolation.
Clustering against tiny synthetic embeddings proves the HDBSCAN
wiring + centroid arithmetic without needing the full corpus. The
DB-write + Claude-call paths are integration concerns — covered by
the actual prod run, not these unit tests.
"""

from __future__ import annotations

import numpy as np
import pytest

from ml_art.neighborhoods import (
    ArtworkRow,
    Cluster,
    LabelledCluster,
    _parse_label_response,
    cluster_artworks,
    dedupe_slugs,
    slugify,
)

# ──────────────────────────────────────────────────────────────────────
# slugify
# ──────────────────────────────────────────────────────────────────────


@pytest.mark.parametrize(
    "name,expected",
    [
        ("Quiet Mornings", "quiet-mornings"),
        ("Saturated Geometry", "saturated-geometry"),
        ("Café Visions", "cafe-visions"),  # accent stripped
        ("Soft, Figurative.", "soft-figurative"),
        ("  Lit From Within  ", "lit-from-within"),
        ("Already-Hyphenated", "already-hyphenated"),
        ("Double  Spaces", "double-spaces"),
    ],
)
def test_slugify_happy_paths(name: str, expected: str) -> None:
    assert slugify(name) == expected


def test_slugify_empty_falls_back_to_cluster() -> None:
    # The `slug` column is NOT NULL — pathological input must still
    # produce a valid slug rather than the empty string.
    assert slugify("") == "cluster"
    assert slugify("!!!") == "cluster"
    assert slugify("   ") == "cluster"


# ──────────────────────────────────────────────────────────────────────
# dedupe_slugs
# ──────────────────────────────────────────────────────────────────────


def _lc(name: str, slug: str, size: int = 1) -> LabelledCluster:
    """Minimal LabelledCluster for dedup tests — the cluster's content
    doesn't matter, only the slug."""

    return LabelledCluster(
        cluster=Cluster(id=0, artworks=[], centroid=np.zeros(3)),
        name=name,
        description="",
        slug=slug,
    )


def test_dedupe_slugs_first_occurrence_keeps_bare_slug() -> None:
    out = dedupe_slugs(
        [
            _lc("A", "moody"),
            _lc("B", "moody"),
            _lc("C", "warm"),
            _lc("D", "moody"),
        ]
    )
    assert [x.slug for x in out] == ["moody", "moody-2", "warm", "moody-3"]


def test_dedupe_slugs_stable_when_unique() -> None:
    out = dedupe_slugs([_lc("A", "alpha"), _lc("B", "beta")])
    assert [x.slug for x in out] == ["alpha", "beta"]


# ──────────────────────────────────────────────────────────────────────
# _parse_label_response
# ──────────────────────────────────────────────────────────────────────


def test_parse_label_response_clean_json() -> None:
    raw = '{"name": "Quiet Mornings", "description": "Soft light, hushed colours."}'
    assert _parse_label_response(raw, cluster_id=0) == (
        "Quiet Mornings",
        "Soft light, hushed colours.",
    )


def test_parse_label_response_with_prose_around_it() -> None:
    # Claude sometimes adds "Here's the JSON:" despite instructions.
    raw = 'Here you go:\n{"name": "X", "description": "Y"}\nLet me know if you want changes.'
    assert _parse_label_response(raw, cluster_id=5) == ("X", "Y")


def test_parse_label_response_malformed_falls_back() -> None:
    name, desc = _parse_label_response("totally not json", cluster_id=7)
    assert name == "Cluster 7"
    assert desc  # non-empty fallback description


def test_parse_label_response_missing_fields_falls_back_per_field() -> None:
    raw = '{"name": "Got A Name"}'
    name, desc = _parse_label_response(raw, cluster_id=2)
    assert name == "Got A Name"
    assert desc != ""  # default description picked


def test_parse_label_response_empty_fields_treated_as_missing() -> None:
    raw = '{"name": "", "description": "   "}'
    name, desc = _parse_label_response(raw, cluster_id=11)
    assert name == "Cluster 11"
    assert desc  # non-empty fallback


# ──────────────────────────────────────────────────────────────────────
# cluster_artworks
# ──────────────────────────────────────────────────────────────────────


def _row(idx: int, embedding: list[float]) -> ArtworkRow:
    return ArtworkRow(
        id=f"00000000-0000-0000-0000-{idx:012d}",
        title=f"art-{idx}",
        image_url=f"https://test.example/{idx}.png",
        embedding=embedding,
    )


def test_cluster_artworks_separates_two_dense_groups() -> None:
    # Construct two tight Gaussian blobs in 4d, far apart. HDBSCAN
    # should recover them as two clusters and tag the outliers as noise.
    rng = np.random.default_rng(seed=42)
    centre_a = np.array([0.0, 0.0, 0.0, 0.0])
    centre_b = np.array([10.0, 10.0, 10.0, 10.0])
    blob_a = [centre_a + rng.normal(scale=0.1, size=4) for _ in range(40)]
    blob_b = [centre_b + rng.normal(scale=0.1, size=4) for _ in range(40)]

    artworks = [_row(i, list(emb)) for i, emb in enumerate(blob_a + blob_b)]
    clusters = cluster_artworks(artworks, min_cluster_size=10)

    assert len(clusters) == 2, "two well-separated blobs → exactly two clusters"
    # Each cluster gets the right ~40 members (a few may be flagged as
    # noise even in clean data; just check it's the bulk).
    sizes = sorted(c.size for c in clusters)
    assert all(s >= 30 for s in sizes), f"clusters too small: {sizes}"


def test_most_central_orders_by_distance() -> None:
    # `most_central` is the centroid-distance sort used to pick label
    # examples + representatives. Build a Cluster directly — going via
    # `cluster_artworks` requires enough density for HDBSCAN to find
    # the cluster at all, which is overkill for what we're testing.
    artworks = [_row(i, [float(i), 0.0, 0.0]) for i in range(5)]
    centroid = np.array([2.0, 0.0, 0.0])  # mean of the line
    cluster = Cluster(id=0, artworks=artworks, centroid=centroid)
    top3 = cluster.most_central(3)
    # idx 2 is on the centroid; idx 1 and 3 are equidistant at d=1.
    # np.argsort is stable so the lower idx wins on ties.
    ids = [a.id for a in top3]
    assert ids[0].endswith("000000000002")
    assert ids[1].endswith("000000000001")
    assert ids[2].endswith("000000000003")


def test_cluster_artworks_drops_noise() -> None:
    # One tight cluster + one obvious outlier. The outlier is the noise
    # row with HDBSCAN label = -1 and shouldn't appear in any Cluster.
    rng = np.random.default_rng(seed=7)
    blob = [list(rng.normal(scale=0.1, size=4)) for _ in range(30)]
    outlier = [99.0, 99.0, 99.0, 99.0]
    artworks = [_row(i, e) for i, e in enumerate(blob + [outlier])]
    clusters = cluster_artworks(artworks, min_cluster_size=10)
    all_ids = {a.id for c in clusters for a in c.artworks}
    outlier_id = artworks[-1].id
    assert outlier_id not in all_ids
