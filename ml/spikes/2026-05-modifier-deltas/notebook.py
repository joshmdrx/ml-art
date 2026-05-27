# ruff: noqa: E402
"""Spike notebook — modifier delta vectors.

Run in VSCode interactive (open this file, "Run Cell" on each `# %%` cell),
or convert to .ipynb via `jupytext --to ipynb notebook.py`.

Question: does naive vector arithmetic on a multimodal embedding produce the
semantic shift implied by a modifier label, well enough to ship the UX?

We compare three approaches per anchor image + modifier:
  A. Baseline: top-k on image embedding only.
  B. Text fusion (RRF): top-k on image emb fused with top-k on modifier text emb.
  C. Delta: top-k on (image_emb + alpha * delta_modifier).

Winner is decided by Claude as side-by-side judge.
"""

# %% imports + config
from __future__ import annotations

import json
import random
from pathlib import Path

import anthropic
import numpy as np
from tqdm import tqdm

from ml_art.config import get_config, require
from ml_art.corpus import load_corpus
from ml_art.embeddings.cache import CachedEmbedder
from ml_art.vectors import normalize, top_k
from ml_art.viz import show_comparison

# Choose one. Default: LOCAL (free, runs on CPU/MPS, ~1min for 500 images).
# Flip to HTTP API by importing JinaEmbedder and setting USE_LOCAL = False.
USE_LOCAL = True

# spike-local (flat imports — directory name starts with a digit so it's not a package)
import sys
sys.path.insert(0, str(Path(__file__).parent))
from deltas import compute_all_deltas, apply_delta  # type: ignore  # noqa: E402
from eval import judge_pair  # type: ignore  # noqa: E402
from modifiers import MODIFIERS, all_names  # type: ignore  # noqa: E402

cfg = get_config()
SPIKE_DIR = Path(__file__).parent
DATA_DIR = SPIKE_DIR / "data" / "images"
RESULTS_DIR = SPIKE_DIR / "results"
RESULTS_DIR.mkdir(parents=True, exist_ok=True)

random.seed(0)
np.random.seed(0)

# %% load corpus
items = load_corpus(DATA_DIR, max_items=2000)
print(f"loaded {len(items)} unique images from {DATA_DIR}")

# %% set up embedder
if USE_LOCAL:
    from ml_art.embeddings.local_jina import LocalJinaClipEmbedder

    inner = LocalJinaClipEmbedder()
else:
    from ml_art.embeddings.jina import JinaEmbedder

    inner = JinaEmbedder(api_key=require(cfg.jina_api_key, "JINA_API_KEY"))

embedder = CachedEmbedder(inner=inner, cache_dir=cfg.cache_dir)
print(f"using {embedder.model_name} ({embedder.model_version}) dim={embedder.dimension}")

# %% embed corpus
image_bytes = [it.read_bytes() for it in tqdm(items, desc="reading")]
corpus_vecs = embedder.embed_images(image_bytes)
corpus_vecs = normalize(corpus_vecs)
print(f"corpus embedding shape: {corpus_vecs.shape}")

# %% compute delta vectors
deltas = compute_all_deltas(embedder)
for name, d in deltas.items():
    print(f"  {name}: ||delta|| (post-normalize) = {np.linalg.norm(d.vector):.3f}")

# %% pick anchor images
N_ANCHORS = 20
anchor_indices = random.sample(range(len(items)), k=min(N_ANCHORS, len(items)))
anchors = [items[i] for i in anchor_indices]
print(f"picked {len(anchors)} anchor images")

# %% define result-set builders

K = 5
ALPHA = 0.2  # tune this; try 0.05, 0.1, 0.2, 0.4 in separate runs


def baseline_results(anchor_idx: int) -> list[int]:
    """Approach A: top-k on image embedding alone."""
    q = corpus_vecs[anchor_idx]
    return [i for i, _ in top_k(q, corpus_vecs, k=K + 1, exclude={anchor_idx})][:K]


def delta_results(anchor_idx: int, modifier_name: str, alpha: float = ALPHA) -> list[int]:
    """Approach C: image embedding + alpha * delta."""
    q = apply_delta(corpus_vecs[anchor_idx], deltas[modifier_name], alpha)
    return [i for i, _ in top_k(q, corpus_vecs, k=K + 1, exclude={anchor_idx})][:K]


def text_fusion_results(anchor_idx: int, modifier_name: str) -> list[int]:
    """Approach B: RRF of image-emb top-k and modifier-text-emb top-k."""
    img_q = corpus_vecs[anchor_idx]
    # Use the modifier's positive prompts averaged as the text query.
    pos = embedder.embed_texts(list(MODIFIERS[modifier_name].positive))
    text_q = normalize(pos.mean(axis=0))

    img_top = top_k(img_q, corpus_vecs, k=50, exclude={anchor_idx})
    txt_top = top_k(text_q, corpus_vecs, k=50, exclude={anchor_idx})

    ranks: dict[int, float] = {}
    for rank, (i, _) in enumerate(img_top):
        ranks[i] = ranks.get(i, 0.0) + 1.0 / (60 + rank)
    for rank, (i, _) in enumerate(txt_top):
        ranks[i] = ranks.get(i, 0.0) + 1.0 / (60 + rank)
    fused = sorted(ranks.items(), key=lambda kv: -kv[1])
    return [i for i, _ in fused[:K]]


# %% sanity check — visualize one pair
sample_anchor_idx = anchor_indices[0]
sample_modifier = "moodier"

a = baseline_results(sample_anchor_idx)
c = delta_results(sample_anchor_idx, sample_modifier)
print(f"baseline indices: {a}")
print(f"delta '{sample_modifier}' indices: {c}")

show_comparison(
    query_path=items[sample_anchor_idx].path,
    set_a=[items[i].path for i in a],
    set_b=[items[i].path for i in c],
    label_a="baseline",
    label_b=f"+ {sample_modifier}",
)

# %% LLM judge: delta vs baseline, across all modifiers and anchors
client = anthropic.Anthropic(api_key=require(cfg.anthropic_api_key, "ANTHROPIC_API_KEY"))

results: list[dict] = []
for modifier_name in tqdm(all_names(), desc="modifiers"):
    for anchor_idx in tqdm(anchor_indices, desc=f"  {modifier_name}", leave=False):
        baseline = baseline_results(anchor_idx)
        delta = delta_results(anchor_idx, modifier_name)
        if set(baseline) == set(delta):
            results.append(
                {
                    "modifier": modifier_name,
                    "anchor": items[anchor_idx].id,
                    "comparison": "delta_vs_baseline",
                    "winner": "tie",
                    "reason": "identical result sets",
                }
            )
            continue
        verdict = judge_pair(
            client=client,
            modifier_name=modifier_name,
            query_image=items[anchor_idx].path,
            set_a=[items[i].path for i in baseline],
            set_b=[items[i].path for i in delta],
        )
        # Set A = baseline, Set B = delta. A delta-win means "B".
        results.append(
            {
                "modifier": modifier_name,
                "anchor": items[anchor_idx].id,
                "comparison": "delta_vs_baseline",
                "winner": {"A": "baseline", "B": "delta", "tie": "tie"}[verdict.winner],
                "reason": verdict.reason,
            }
        )

(RESULTS_DIR / "delta_vs_baseline.json").write_text(json.dumps(results, indent=2))
print(f"wrote {len(results)} judgements to results/delta_vs_baseline.json")

# %% aggregate win rates
from collections import Counter

per_modifier: dict[str, Counter] = {}
for r in results:
    per_modifier.setdefault(r["modifier"], Counter())[r["winner"]] += 1

print(f"\nDelta vs Baseline — win rates (n={N_ANCHORS} per modifier):")
print(f"{'modifier':<16} {'delta':>6} {'tie':>6} {'baseline':>10} {'delta_winrate':>14}")
for m, counter in per_modifier.items():
    d = counter["delta"]
    t = counter["tie"]
    b = counter["baseline"]
    decided = d + b
    wr = (d / decided) if decided else float("nan")
    print(f"{m:<16} {d:>6} {t:>6} {b:>10} {wr:>13.0%}")

# %% next: delta vs text-fusion
results_vs_textfusion: list[dict] = []
for modifier_name in tqdm(all_names(), desc="modifiers"):
    for anchor_idx in tqdm(anchor_indices, desc=f"  {modifier_name}", leave=False):
        delta = delta_results(anchor_idx, modifier_name)
        textf = text_fusion_results(anchor_idx, modifier_name)
        if set(delta) == set(textf):
            results_vs_textfusion.append(
                {
                    "modifier": modifier_name,
                    "anchor": items[anchor_idx].id,
                    "comparison": "delta_vs_textfusion",
                    "winner": "tie",
                    "reason": "identical result sets",
                }
            )
            continue
        verdict = judge_pair(
            client=client,
            modifier_name=modifier_name,
            query_image=items[anchor_idx].path,
            set_a=[items[i].path for i in textf],
            set_b=[items[i].path for i in delta],
        )
        results_vs_textfusion.append(
            {
                "modifier": modifier_name,
                "anchor": items[anchor_idx].id,
                "comparison": "delta_vs_textfusion",
                "winner": {"A": "textfusion", "B": "delta", "tie": "tie"}[verdict.winner],
                "reason": verdict.reason,
            }
        )

(RESULTS_DIR / "delta_vs_textfusion.json").write_text(
    json.dumps(results_vs_textfusion, indent=2)
)

per_modifier_v2: dict[str, Counter] = {}
for r in results_vs_textfusion:
    per_modifier_v2.setdefault(r["modifier"], Counter())[r["winner"]] += 1

print(f"\nDelta vs Text-fusion — win rates (n={N_ANCHORS} per modifier):")
print(f"{'modifier':<16} {'delta':>6} {'tie':>6} {'textfusion':>12} {'delta_winrate':>14}")
for m, counter in per_modifier_v2.items():
    d = counter["delta"]
    t = counter["tie"]
    f = counter["textfusion"]
    decided = d + f
    wr = (d / decided) if decided else float("nan")
    print(f"{m:<16} {d:>6} {t:>6} {f:>12} {wr:>13.0%}")

# %% verdict
# Fill this in with your interpretation after running the cells above.
#
# Per-modifier go/no-go:
#   moodier:       ?
#   warmer:        ?
#   more_minimal:  ?
#   more_textured: ?
#   more_graphic:  ?
#
# Best alpha (if shipping):   ?
# Beats text-fusion overall?: ?
# Surprises:                  ?
#
# Decision: SHIP / SHIP-PARTIAL / SKIP-USE-TEXTFUSION / NEEDS-MORE-WORK
