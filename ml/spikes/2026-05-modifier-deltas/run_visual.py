"""Visual-only spike runner — no LLM judge required.

Embeds the corpus locally, computes modifier deltas, picks anchor images,
and writes side-by-side comparison PNGs for: baseline / text-fusion / delta.

Output: spikes/2026-05-modifier-deltas/results/<modifier>/<anchor_id>.png

You eyeball the PNGs and decide whether deltas produce the intended shift.
Faster, cheaper, and more honest than an LLM judge for a first pass.

Run: `uv run python spikes/2026-05-modifier-deltas/run_visual.py`
"""

from __future__ import annotations

import argparse
import random
import sys
from pathlib import Path

import matplotlib

matplotlib.use("Agg")  # non-interactive
import matplotlib.pyplot as plt
import numpy as np
from PIL import Image
from tqdm import tqdm

SPIKE_DIR = Path(__file__).parent
sys.path.insert(0, str(SPIKE_DIR))

from ml_art.config import get_config
from ml_art.corpus import CorpusItem, load_corpus
from ml_art.embeddings.cache import CachedEmbedder
from ml_art.embeddings.local_jina import LocalJinaClipEmbedder
from ml_art.vectors import normalize, top_k

# Spike-local — flat imports because the directory name starts with a digit.
from deltas import apply_delta, compute_all_deltas  # type: ignore  # noqa: E402
from modifiers import MODIFIERS, all_names  # type: ignore  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--alpha", type=float, default=0.2)
    parser.add_argument("--k", type=int, default=5)
    parser.add_argument("--anchors", type=int, default=10)
    parser.add_argument("--max-corpus", type=int, default=2500)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--data-dir", type=Path, default=None)
    parser.add_argument("--results-dir", type=Path, default=None)
    args = parser.parse_args()

    random.seed(args.seed)
    np.random.seed(args.seed)

    cfg = get_config()
    data_dir = args.data_dir or (SPIKE_DIR / "data" / "images")
    results_dir = args.results_dir or (SPIKE_DIR / "results")
    results_dir.mkdir(parents=True, exist_ok=True)

    # 1. Load corpus
    items = load_corpus(data_dir, max_items=args.max_corpus)
    if not items:
        sys.exit(f"no images in {data_dir} — run fetch_met.py first")
    print(f"corpus: {len(items)} images")

    # 2. Embed corpus (cached)
    embedder = CachedEmbedder(LocalJinaClipEmbedder(), cfg.cache_dir)
    print(f"embedder: {embedder.model_name} dim={embedder.dimension}")

    image_bytes = [it.read_bytes() for it in tqdm(items, desc="reading")]
    corpus_vecs = embedder.embed_images(image_bytes)
    corpus_vecs = normalize(corpus_vecs)

    # 3. Compute deltas
    deltas = compute_all_deltas(embedder)

    # 4. Pick anchors
    n_anchors = min(args.anchors, len(items))
    anchor_indices = random.sample(range(len(items)), k=n_anchors)
    print(f"anchors: {n_anchors}")

    # 5. For each (modifier, anchor), build three result sets, render PNG
    for modifier_name in tqdm(all_names(), desc="modifiers"):
        out_subdir = results_dir / modifier_name
        out_subdir.mkdir(parents=True, exist_ok=True)

        # Precompute the text-fusion query for this modifier
        pos = embedder.embed_texts(list(MODIFIERS[modifier_name].positive))
        text_q = normalize(pos.mean(axis=0))

        for anchor_idx in anchor_indices:
            anchor = items[anchor_idx]
            img_q = corpus_vecs[anchor_idx]
            delta_q = apply_delta(img_q, deltas[modifier_name], args.alpha)

            baseline = _top_k_paths(img_q, corpus_vecs, items, args.k, exclude={anchor_idx})
            text_fusion = _text_fusion_paths(
                img_q, text_q, corpus_vecs, items, args.k, exclude={anchor_idx}
            )
            delta_set = _top_k_paths(delta_q, corpus_vecs, items, args.k, exclude={anchor_idx})

            out_path = out_subdir / f"{anchor.id}.png"
            _render(
                title=f"modifier: {modifier_name}  alpha={args.alpha}  anchor={anchor.id}",
                query_path=anchor.path,
                rows=[
                    ("baseline (image only)", baseline),
                    ("text-fusion (RRF)", text_fusion),
                    (f"delta (+{args.alpha})", delta_set),
                ],
                out_path=out_path,
            )

    print(f"\nwrote PNGs to {results_dir}")
    print("open them to eyeball whether the delta row genuinely shifts toward the modifier.")


def _top_k_paths(
    query, corpus_vecs, items: list[CorpusItem], k: int, exclude: set[int]
) -> list[Path]:
    ranked = top_k(query, corpus_vecs, k=k, exclude=exclude)
    return [items[i].path for i, _ in ranked]


def _text_fusion_paths(
    img_q, text_q, corpus_vecs, items: list[CorpusItem], k: int, exclude: set[int]
) -> list[Path]:
    img_top = top_k(img_q, corpus_vecs, k=50, exclude=exclude)
    txt_top = top_k(text_q, corpus_vecs, k=50, exclude=exclude)
    ranks: dict[int, float] = {}
    for rank, (i, _) in enumerate(img_top):
        ranks[i] = ranks.get(i, 0.0) + 1.0 / (60 + rank)
    for rank, (i, _) in enumerate(txt_top):
        ranks[i] = ranks.get(i, 0.0) + 1.0 / (60 + rank)
    fused = sorted(ranks.items(), key=lambda kv: -kv[1])[:k]
    return [items[i].path for i, _ in fused]


def _render(
    *,
    title: str,
    query_path: Path,
    rows: list[tuple[str, list[Path]]],
    out_path: Path,
) -> None:
    cols = max(len(paths) for _, paths in rows) + 1  # +1 for query column
    n_rows = len(rows)
    fig, axes = plt.subplots(
        n_rows,
        cols,
        figsize=(2.2 * cols, 2.2 * n_rows),
        squeeze=False,
    )
    fig.suptitle(title, fontsize=10)
    for r, (label, paths) in enumerate(rows):
        # First column: query thumbnail with label
        ax_q = axes[r][0]
        with Image.open(query_path) as im:
            ax_q.imshow(im)
        ax_q.set_ylabel(label, fontsize=9)
        ax_q.set_xticks([])
        ax_q.set_yticks([])
        if r == 0:
            ax_q.set_title("query", fontsize=8)

        for c in range(1, cols):
            ax = axes[r][c]
            j = c - 1
            if j < len(paths):
                with Image.open(paths[j]) as im:
                    ax.imshow(im)
            ax.axis("off")

    plt.tight_layout()
    plt.savefig(out_path, dpi=110, bbox_inches="tight")
    plt.close(fig)


if __name__ == "__main__":
    main()
