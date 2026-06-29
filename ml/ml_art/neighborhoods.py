"""T-057 — algorithmic neighbourhoods.

Reads image embeddings from `artwork_embeddings`, clusters them with
HDBSCAN, asks a vision LLM to name each cluster, persists as
`kind='semantic'` rows in the `neighborhoods` + `neighborhood_artworks`
tables.

The pipeline is idempotent: each run drops the existing algorithmic
rows and rebuilds. Pre-launch there are no bookmarks to honour; once
real users exist we'll need stable slugs across reruns (Hungarian
matching of old/new centroids), but that complexity is deferred.

Two label providers are wired up — Anthropic (Claude Sonnet 4.6) and
Groq (Llama 4 Scout). Pick with `--provider`. The default is anthropic;
the groq path is the cheap/fast lane for prompt-iteration runs and
backstop labelling.

Usage::

    # Anthropic
    DATABASE_URL=postgres://... ANTHROPIC_API_KEY=sk-ant-... \\
        uv run --extra neighborhoods python -m ml_art.neighborhoods

    # Groq (cheaper, faster)
    DATABASE_URL=postgres://... GROQ_API_KEY=gsk_... \\
        uv run --extra neighborhoods python -m ml_art.neighborhoods \\
            --provider groq

Flags:
    --provider {anthropic,groq}  Which vision LLM to label with.
    --min-cluster-size N         Default 30.
    --sample-size N              Per-cluster images sent to the labeller.
                                 Default 12 for anthropic, 5 for groq
                                 (groq endpoints cap image count).
    --dry-run [--output PATH]    Cluster + label, dump JSON, skip DB.
    --database-url URL           Defaults to $DATABASE_URL.
    --anthropic-key KEY          Defaults to $ANTHROPIC_API_KEY.
    --anthropic-model NAME       Default `claude-sonnet-4-6`.
    --groq-key KEY               Defaults to $GROQ_API_KEY.
    --groq-model NAME            Default `meta-llama/llama-4-scout-17b-16e-instruct`.

See `decisions.md` 2026-06-25 — T-057 design choices for the
clustering + labelling rationale.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import unicodedata
from dataclasses import dataclass
from typing import Protocol

import hdbscan
import numpy as np
import psycopg
import requests
from anthropic import Anthropic
from pgvector.psycopg import register_vector

# Tuned 2026-06-25 against the prod corpus (~2000 artworks). EOM with
# the default min_samples=mcs produced a single 856-artwork mega-bucket
# (Western figurative everything) and dumped 55% as noise. `leaf`
# selection with a low min_samples picks finer-grained clusters from
# the condensed tree, giving ~14 visually coherent neighbourhoods with
# a smooth size decay (90 down to 15). The 74% "noise" rate isn't bad:
# those artworks still appear in search / artist pages — they just
# don't belong to a discrete neighborhood, which is honest signal.
DEFAULT_MIN_CLUSTER_SIZE = 15
DEFAULT_MIN_SAMPLES = 2
DEFAULT_CLUSTER_METHOD = "leaf"
DEFAULT_ANTHROPIC_MODEL = "claude-sonnet-4-6"
DEFAULT_GROQ_MODEL = "meta-llama/llama-4-scout-17b-16e-instruct"
# Per-cluster image counts sent to the labeller. Anthropic happily
# handles 12; Groq's vision endpoints cap at 5 images per request
# (Llama 4 Scout). Both numbers convey a cluster's vibe — Claude
# benefits modestly from more context, Groq just rejects the call.
DEFAULT_ANTHROPIC_SAMPLE = 12
DEFAULT_GROQ_SAMPLE = 5
GROQ_ENDPOINT = "https://api.groq.com/openai/v1/chat/completions"
# Number of artworks stored on `neighborhoods.representative_artwork_ids`
# for the card thumb strip. Existing curated rows ship 3-4; matching.
REPRESENTATIVE_COUNT = 4
# Top-N clusters by size that get `is_featured = true`. The /neighborhoods
# page sorts by display_order; featured ones bubble to the top.
FEATURED_COUNT = 3


@dataclass
class ArtworkRow:
    """One row from the artwork-embedding join. The `embedding` is a
    Python list because pgvector via psycopg returns it that way; we
    convert to np.array in `cluster_artworks`."""

    id: str
    title: str | None
    image_url: str
    embedding: list[float]


@dataclass
class Cluster:
    """A single HDBSCAN cluster. `centroid` is the mean of the
    cluster's embeddings — used to pick the most-central artworks for
    labelling + display."""

    id: int
    artworks: list[ArtworkRow]
    centroid: np.ndarray

    @property
    def size(self) -> int:
        return len(self.artworks)

    def most_central(self, n: int) -> list[ArtworkRow]:
        """Top-n artworks sorted by ascending cosine distance to centroid."""
        # Image embeddings are L2-normalised so this is equivalent to
        # sorting by descending dot-product with the centroid. Just use
        # plain Euclidean distance for clarity.
        embeddings = np.array([a.embedding for a in self.artworks])
        # Centroid wasn't necessarily on the unit sphere after averaging;
        # the distance ordering is what we want, not absolute magnitudes.
        dists = np.linalg.norm(embeddings - self.centroid, axis=1)
        order = np.argsort(dists)
        return [self.artworks[i] for i in order[:n]]


@dataclass
class LabelledCluster:
    """A cluster after Claude has named it. `slug` is derived from
    `name` via `slugify` and guaranteed-unique within the run (the
    persist step appends a numeric suffix on collisions)."""

    cluster: Cluster
    name: str
    description: str
    slug: str


# ──────────────────────────────────────────────────────────────────────
# DB
# ──────────────────────────────────────────────────────────────────────


def fetch_artworks(conn: psycopg.Connection) -> list[ArtworkRow]:
    """Pull every published, non-deleted artwork that has an image
    embedding. The JOIN drops artworks whose primary image is still
    pending moderation."""

    sql = """
        SELECT
            a.id::text,
            a.title,
            ai.s3_key,
            ae.embedding
        FROM artworks a
        JOIN artwork_embeddings ae ON ae.artwork_id = a.id
        JOIN artwork_images ai
               ON ai.artwork_id = a.id
              AND ai.is_primary
              AND ai.moderation_status = 'approved'
        JOIN artists ar ON ar.id = a.artist_id
        WHERE a.deleted_at IS NULL
          AND a.status = 'published'
          AND ar.deleted_at IS NULL
          AND ar.status = 'active'
        ORDER BY a.created_at
    """
    image_prefix = os.environ.get("IMAGE_BASE_URL", "https://images.wander.gallery")

    rows: list[ArtworkRow] = []
    with conn.cursor() as cur:
        cur.execute(sql)
        for art_id, title, s3_key, emb in cur.fetchall():
            # `uploads/`-prefixed keys live in the uploads bucket which
            # is fronted by the same CloudFront in prod. The URL prefix
            # already handles both; the s3_key is appended verbatim. See
            # `core::images::url_for_s3_key` for the canonical resolver.
            url = f"{image_prefix.rstrip('/')}/{s3_key}"
            rows.append(
                ArtworkRow(
                    id=art_id,
                    title=title,
                    image_url=url,
                    # `pgvector` returns either a list[float] or a Vector
                    # object depending on adapter version. `list(emb)`
                    # handles both.
                    embedding=list(emb),
                )
            )
    return rows


def wipe_algorithmic(conn: psycopg.Connection) -> int:
    """DELETE existing `kind='semantic'` neighborhoods. The
    `neighborhood_artworks` rows go with them via ON DELETE CASCADE.
    Returns the count removed for the run summary."""

    with conn.cursor() as cur:
        cur.execute("DELETE FROM neighborhoods WHERE kind = 'semantic' RETURNING id")
        return len(cur.fetchall())


def prune_test_vibes(conn: psycopg.Connection) -> bool:
    """Drop the scrappy seed row left over from the initial /neighborhoods
    skeleton. Idempotent (returns False if already gone)."""

    with conn.cursor() as cur:
        cur.execute("DELETE FROM neighborhoods WHERE slug = 'test-vibes' RETURNING id")
        return bool(cur.fetchall())


def persist(conn: psycopg.Connection, labelled: list[LabelledCluster]) -> None:
    """Write the labelled clusters as `kind='semantic'` rows + their
    join entries. Ordered by descending cluster size so `display_order`
    surfaces the chunkier groups first; the top `FEATURED_COUNT` get
    `is_featured = true`."""

    by_size = sorted(labelled, key=lambda lc: lc.cluster.size, reverse=True)

    with conn.cursor() as cur:
        for idx, lc in enumerate(by_size):
            reps = lc.cluster.most_central(REPRESENTATIVE_COUNT)
            rep_ids = [r.id for r in reps]
            is_featured = idx < FEATURED_COUNT

            # Write the cluster centroid alongside the row — T-061's
            # calibrator filters on `cluster_centroid IS NOT NULL`,
            # and so will any future similarity-based surface. We
            # already have the centroid in memory; the cost of writing
            # it is ~4KB per row.
            cur.execute(
                """
                INSERT INTO neighborhoods (
                    slug, name, description, kind,
                    representative_artwork_ids, artwork_count,
                    is_featured, display_order,
                    cluster_centroid
                ) VALUES (%s, %s, %s, 'semantic',
                          %s::uuid[], %s, %s, %s,
                          %s::vector)
                RETURNING id
                """,
                (
                    lc.slug,
                    lc.name,
                    lc.description,
                    rep_ids,
                    lc.cluster.size,
                    is_featured,
                    idx,
                    # Pass as a literal "[v1,v2,...]" string so psycopg
                    # routes it through the vector cast above without
                    # needing the pgvector psycopg adapter registered.
                    "[" + ",".join(f"{x:.6f}" for x in lc.cluster.centroid) + "]",
                ),
            )
            (neighborhood_id,) = cur.fetchone()

            # Bulk-insert the membership rows. executemany works fine
            # for the ~30-200 row insert per cluster.
            cur.executemany(
                "INSERT INTO neighborhood_artworks (neighborhood_id, artwork_id) VALUES (%s, %s)",
                [(neighborhood_id, a.id) for a in lc.cluster.artworks],
            )


# ──────────────────────────────────────────────────────────────────────
# Clustering
# ──────────────────────────────────────────────────────────────────────


def cluster_artworks(
    artworks: list[ArtworkRow],
    min_cluster_size: int,
    *,
    min_samples: int | None = None,
    cluster_selection_method: str = "eom",
    cluster_selection_epsilon: float = 0.0,
) -> list[Cluster]:
    """Run HDBSCAN. Returns one Cluster per non-noise cluster. Members
    of the noise cluster (label = -1) are dropped.

    Tunables:
    - `cluster_selection_method`: `eom` (default — fewer, broader
      clusters; can over-merge hierarchical data) or `leaf` (more,
      finer-grained clusters from tree leaves).
    - `cluster_selection_epsilon`: distance threshold that merges
      clusters closer than `epsilon`. 0 = off. Useful when leaf is
      too granular but eom too coarse.
    - `min_samples`: noise sensitivity. None defaults to
      `min_cluster_size` (hdbscan's recommendation). Smaller =
      less-aggressive noise classification = more artworks placed."""

    embeddings = np.array([a.embedding for a in artworks])

    clusterer = hdbscan.HDBSCAN(
        # Image embeddings from Jina CLIP are L2-normalised so euclidean
        # ordering matches cosine ordering.
        metric="euclidean",
        min_cluster_size=min_cluster_size,
        min_samples=min_samples,
        cluster_selection_method=cluster_selection_method,
        cluster_selection_epsilon=cluster_selection_epsilon,
    )
    labels = clusterer.fit_predict(embeddings)

    # Group by label, skipping noise (-1).
    grouped: dict[int, list[ArtworkRow]] = {}
    for art, label in zip(artworks, labels, strict=True):
        if label == -1:
            continue
        grouped.setdefault(int(label), []).append(art)

    clusters: list[Cluster] = []
    for cluster_id, members in grouped.items():
        centroid = np.mean(np.array([m.embedding for m in members]), axis=0)
        clusters.append(Cluster(id=cluster_id, artworks=members, centroid=centroid))

    return clusters


def _preview_clusters(clusters: list[Cluster], total_artworks: int) -> str:
    """Compact summary of a clustering — cluster count, noise ratio,
    size distribution. Used by the --preview mode to sweep configs
    without burning labelling calls."""

    if not clusters:
        return f"  → 0 clusters, {total_artworks} noise (100%)"

    sizes = sorted([c.size for c in clusters], reverse=True)
    in_clusters = sum(sizes)
    noise = total_artworks - in_clusters
    noise_pct = noise / total_artworks * 100
    return (
        f"  → {len(clusters)} clusters  •  noise {noise}/{total_artworks} ({noise_pct:.0f}%)\n"
        f"     sizes: {sizes}"
    )


# ──────────────────────────────────────────────────────────────────────
# Labelling
# ──────────────────────────────────────────────────────────────────────


# Prompt is deliberately voice-y — we asked for evocative names and
# the model needs the cue. Functional names ("Painting + Muted") would
# duplicate the medium filter; this prompt keeps it in the discovery
# register.
LABEL_PROMPT = """\
You're looking at {n} artworks that an unsupervised clustering algorithm \
grouped together based on visual similarity. They're not labelled by \
medium or style — they're connected by something the algorithm saw.

Suggest:
1. An evocative 2-4 word neighbourhood name capturing the visual or \
emotional thread. Examples of the right register: "Quiet Mornings", \
"Saturated Geometry", "Soft Figurative", "Lit From Within". Avoid \
generic functional names like "Paintings" or "Oil on Canvas" — those \
are filters, not discovery cues.
2. A single sentence (under 25 words) describing the visual or thematic \
thread. Suitable for a header on a discovery page.

Respond with ONLY a JSON object, no other text:
{{"name": "...", "description": "..."}}
"""


class Labeller(Protocol):
    """A vision-LLM that turns a Cluster into `(name, description)`."""

    name: str  # e.g. "anthropic:claude-sonnet-4-6", used in logs + filenames

    def label(self, cluster: Cluster) -> tuple[str, str]:
        ...


class AnthropicLabeller:
    def __init__(self, api_key: str, model: str, sample_size: int) -> None:
        self.client = Anthropic(api_key=api_key)
        self.model = model
        self.sample_size = sample_size
        self.name = f"anthropic:{model}"

    def label(self, cluster: Cluster) -> tuple[str, str]:
        sample = cluster.most_central(self.sample_size)
        content: list[dict] = [
            {"type": "image", "source": {"type": "url", "url": a.image_url}}
            for a in sample
        ]
        content.append({"type": "text", "text": LABEL_PROMPT.format(n=len(sample))})

        msg = self.client.messages.create(
            model=self.model,
            max_tokens=300,
            messages=[{"role": "user", "content": content}],
        )
        raw = "".join(b.text for b in msg.content if b.type == "text").strip()
        return _parse_label_response(raw, cluster_id=cluster.id)


class GroqLabeller:
    """OpenAI-compatible chat-completions endpoint at Groq, vision via
    `image_url` content parts. Llama 4 Scout supports multi-image up to
    the endpoint's 5-image cap. We use `requests` directly rather than
    pull in the openai SDK for just one POST.

    Groq's free tier rate-limits aggressively, so a small retry loop
    on 429 + 5xx is worth the 10 lines."""

    MAX_ATTEMPTS = 5

    def __init__(self, api_key: str, model: str, sample_size: int) -> None:
        self.api_key = api_key
        self.model = model
        self.sample_size = sample_size
        self.name = f"groq:{model}"

    def label(self, cluster: Cluster) -> tuple[str, str]:
        sample = cluster.most_central(self.sample_size)
        content: list[dict] = [
            {"type": "text", "text": LABEL_PROMPT.format(n=len(sample))}
        ]
        content.extend(
            {"type": "image_url", "image_url": {"url": a.image_url}}
            for a in sample
        )

        last_err: Exception | None = None
        for attempt in range(self.MAX_ATTEMPTS):
            try:
                resp = requests.post(
                    GROQ_ENDPOINT,
                    headers={
                        "Authorization": f"Bearer {self.api_key}",
                        "Content-Type": "application/json",
                    },
                    json={
                        "model": self.model,
                        "max_tokens": 300,
                        "messages": [{"role": "user", "content": content}],
                    },
                    timeout=60,
                )
                if resp.status_code == 429 or resp.status_code >= 500:
                    # Honour Retry-After when Groq sets it; otherwise
                    # exponential backoff. Groq tends to set R-A in seconds.
                    wait = int(resp.headers.get("Retry-After", 2 ** attempt))
                    print(
                        f"  groq {resp.status_code}; retrying in {wait}s (attempt {attempt + 1}/{self.MAX_ATTEMPTS})",
                        file=sys.stderr,
                    )
                    import time
                    time.sleep(wait)
                    continue
                resp.raise_for_status()
                raw = resp.json()["choices"][0]["message"]["content"].strip()
                return _parse_label_response(raw, cluster_id=cluster.id)
            except requests.RequestException as e:
                last_err = e
                import time
                time.sleep(2 ** attempt)

        # Exhausted retries — surface the failure rather than silently
        # falling back so the operator notices and can re-run.
        raise RuntimeError(f"groq labelling failed after {self.MAX_ATTEMPTS} attempts") from last_err


def _parse_label_response(raw: str, *, cluster_id: int) -> tuple[str, str]:
    """Extract `{name, description}` from a Claude response. Tolerant
    of leading/trailing prose around the JSON since the model
    occasionally adds it despite the "JSON only" instruction. Falls
    back to a generic name on malformed input — the run completes."""

    # Find the first `{...}` JSON object in the response.
    match = re.search(r"\{[^{}]*\}", raw, flags=re.DOTALL)
    fallback_name = f"Cluster {cluster_id}"
    fallback_desc = "An algorithmically grouped neighbourhood."
    if not match:
        return fallback_name, fallback_desc

    try:
        parsed = json.loads(match.group(0))
    except json.JSONDecodeError:
        return fallback_name, fallback_desc

    name = parsed.get("name")
    desc = parsed.get("description")
    if not isinstance(name, str) or not name.strip():
        name = fallback_name
    if not isinstance(desc, str) or not desc.strip():
        desc = fallback_desc
    return name.strip(), desc.strip()


# ──────────────────────────────────────────────────────────────────────
# Slugging
# ──────────────────────────────────────────────────────────────────────


def slugify(name: str) -> str:
    """Lowercase + ASCII-fold + non-alnum → '-'. Trims surrounding
    dashes. Empty input → 'cluster' as a defensive fallback (the DB
    `slug` column is NOT NULL)."""

    # Strip combining marks via NFKD decomposition; "Café" → "Cafe".
    ascii_form = unicodedata.normalize("NFKD", name).encode("ascii", "ignore").decode("ascii")
    lower = ascii_form.lower()
    sluggy = re.sub(r"[^a-z0-9]+", "-", lower).strip("-")
    return sluggy or "cluster"


def dedupe_slugs(labelled: list[LabelledCluster]) -> list[LabelledCluster]:
    """Append `-2`, `-3`, … to repeated slugs in the order they appear.
    The first occurrence keeps its bare slug. Stable on input order so
    behaviour doesn't change across reruns of an identical clustering."""

    seen: dict[str, int] = {}
    out: list[LabelledCluster] = []
    for lc in labelled:
        base = lc.slug
        n = seen.get(base, 0) + 1
        seen[base] = n
        final = base if n == 1 else f"{base}-{n}"
        out.append(
            LabelledCluster(
                cluster=lc.cluster,
                name=lc.name,
                description=lc.description,
                slug=final,
            )
        )
    return out


# ──────────────────────────────────────────────────────────────────────
# Main
# ──────────────────────────────────────────────────────────────────────


def _build_labeller(args: argparse.Namespace) -> Labeller:
    """Resolve --provider + per-provider key/model/sample-size into a
    Labeller. Errors out (sys.exit) if the chosen provider's key is missing."""

    if args.provider == "anthropic":
        if not args.anthropic_key:
            sys.exit("✘ --provider anthropic requires ANTHROPIC_API_KEY or --anthropic-key")
        sample = args.sample_size or DEFAULT_ANTHROPIC_SAMPLE
        return AnthropicLabeller(args.anthropic_key, args.anthropic_model, sample)
    if args.provider == "groq":
        if not args.groq_key:
            sys.exit("✘ --provider groq requires GROQ_API_KEY or --groq-key")
        sample = args.sample_size or DEFAULT_GROQ_SAMPLE
        return GroqLabeller(args.groq_key, args.groq_model, sample)
    sys.exit(f"✘ unknown provider {args.provider!r}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Build algorithmic neighbourhoods (T-057).")
    parser.add_argument(
        "--database-url",
        default=os.environ.get("DATABASE_URL"),
        help="Postgres URL. Defaults to $DATABASE_URL.",
    )
    parser.add_argument(
        "--provider",
        choices=["anthropic", "groq"],
        default="anthropic",
        help="Which vision LLM labels the clusters.",
    )
    parser.add_argument(
        "--anthropic-key",
        default=os.environ.get("ANTHROPIC_API_KEY"),
        help="Anthropic API key. Defaults to $ANTHROPIC_API_KEY.",
    )
    parser.add_argument("--anthropic-model", default=DEFAULT_ANTHROPIC_MODEL)
    parser.add_argument(
        "--groq-key",
        default=os.environ.get("GROQ_API_KEY"),
        help="Groq API key. Defaults to $GROQ_API_KEY.",
    )
    parser.add_argument("--groq-model", default=DEFAULT_GROQ_MODEL)
    parser.add_argument(
        "--sample-size",
        type=int,
        default=0,
        help="Per-cluster images sent to the labeller. 0 = provider default.",
    )
    parser.add_argument(
        "--min-cluster-size",
        type=int,
        default=DEFAULT_MIN_CLUSTER_SIZE,
    )
    parser.add_argument(
        "--min-samples",
        type=int,
        default=DEFAULT_MIN_SAMPLES,
        help="HDBSCAN min_samples (noise sensitivity). Lower = less noise.",
    )
    parser.add_argument(
        "--cluster-method",
        choices=["eom", "leaf"],
        default=DEFAULT_CLUSTER_METHOD,
        help="HDBSCAN cluster_selection_method. `leaf` gives more, smaller clusters.",
    )
    parser.add_argument(
        "--cluster-epsilon",
        type=float,
        default=0.0,
        help="HDBSCAN cluster_selection_epsilon. Merges close clusters; 0 = off.",
    )
    parser.add_argument(
        "--preview",
        action="store_true",
        help="Cluster + print stats only. No labelling, no DB. For tuning sweeps.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Skip the DB write; print the labelled clusters.",
    )
    parser.add_argument(
        "--output",
        type=str,
        default=None,
        help=(
            "When --dry-run, write a structured JSON file with labels + "
            "representative image URLs. Useful for bake-off comparisons."
        ),
    )
    parser.add_argument(
        "--prune-test-vibes",
        action="store_true",
        help="Also DELETE the scrappy hand-curated 'test-vibes' row.",
    )
    args = parser.parse_args(argv)

    if not args.database_url:
        parser.error("DATABASE_URL or --database-url required")

    # `--preview` doesn't need an LLM. Skip labeller construction so
    # tuning sweeps don't require an API key.
    labeller = None if args.preview else _build_labeller(args)

    conn = psycopg.connect(args.database_url, autocommit=True)
    # Without the pgvector adapter, the `embedding` column comes back
    # as a string ("[0.1,0.2,…]") and downstream array maths blows up.
    register_vector(conn)
    try:
        artworks = fetch_artworks(conn)
        print(f"▶ {len(artworks)} eligible artworks", file=sys.stderr)
        if len(artworks) < args.min_cluster_size:
            parser.error(
                f"need at least {args.min_cluster_size} artworks; got {len(artworks)}"
            )

        clusters = cluster_artworks(
            artworks,
            args.min_cluster_size,
            min_samples=args.min_samples,
            cluster_selection_method=args.cluster_method,
            cluster_selection_epsilon=args.cluster_epsilon,
        )
        # Sort by size descending — labelling order matches what the
        # final display order will be, so the operator can spot-check
        # the most-visible clusters first as the run streams.
        clusters.sort(key=lambda c: c.size, reverse=True)

        if args.preview:
            cfg = (
                f"min_cluster_size={args.min_cluster_size}, "
                f"min_samples={args.min_samples}, "
                f"method={args.cluster_method}, "
                f"epsilon={args.cluster_epsilon}"
            )
            print(f"▶ HDBSCAN [{cfg}]", file=sys.stderr)
            print(_preview_clusters(clusters, len(artworks)), file=sys.stderr)
            return 0

        assert labeller is not None  # narrowed by the --preview branch above
        print(
            f"▶ HDBSCAN → {len(clusters)} clusters (noise dropped)  •  labeller={labeller.name}",
            file=sys.stderr,
        )

        labelled: list[LabelledCluster] = []
        for c in clusters:
            name, desc = labeller.label(c)
            slug = slugify(name)
            print(f"  cluster {c.id:>3}  n={c.size:>3}  {name!r:<28}  /{slug}", file=sys.stderr)
            labelled.append(
                LabelledCluster(cluster=c, name=name, description=desc, slug=slug)
            )

        labelled = dedupe_slugs(labelled)

        if args.dry_run:
            print("▶ --dry-run: skipping DB write", file=sys.stderr)
            payload = {
                "provider": labeller.name,
                "sample_size": labeller.sample_size,
                "min_cluster_size": args.min_cluster_size,
                "clusters": [
                    {
                        "id": lc.cluster.id,
                        "size": lc.cluster.size,
                        "slug": lc.slug,
                        "name": lc.name,
                        "description": lc.description,
                        "representative_image_urls": [
                            a.image_url
                            for a in lc.cluster.most_central(REPRESENTATIVE_COUNT)
                        ],
                    }
                    for lc in labelled
                ],
            }
            if args.output:
                with open(args.output, "w") as fp:
                    json.dump(payload, fp, indent=2)
                print(f"▶ wrote {args.output}", file=sys.stderr)
            else:
                print(json.dumps(payload, indent=2))
            return 0

        removed = wipe_algorithmic(conn)
        print(f"▶ wiped {removed} existing algorithmic rows", file=sys.stderr)
        if args.prune_test_vibes:
            if prune_test_vibes(conn):
                print("▶ pruned 'test-vibes' curated row", file=sys.stderr)
        persist(conn, labelled)
        print(f"▶ persisted {len(labelled)} algorithmic neighbourhoods", file=sys.stderr)
    finally:
        conn.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
