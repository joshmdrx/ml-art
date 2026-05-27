"""Compute modifier delta vectors.

A delta is `mean(embed(positive_texts)) - mean(embed(negative_texts))`, optionally
L2-normalized. To shift an image embedding "moodier", add `alpha * delta_moodier`.

Spike-local. If the spike succeeds, the cleaned-up version lives in
`ml_art/recommendations.py` (or similar).
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
from numpy.typing import NDArray

from ml_art.embeddings.base import Embedder
from ml_art.vectors import mean_vector, normalize

from modifiers import MODIFIERS, Modifier  # type: ignore  # spike-local flat import


@dataclass(frozen=True)
class Delta:
    name: str
    vector: NDArray[np.float32]  # shape (d,), L2-normalized


def compute_delta(modifier: Modifier, embedder: Embedder) -> Delta:
    pos = embedder.embed_texts(list(modifier.positive))
    neg = embedder.embed_texts(list(modifier.negative))
    direction = mean_vector(pos) - mean_vector(neg)
    return Delta(name=modifier.name, vector=normalize(direction))


def compute_all_deltas(embedder: Embedder) -> dict[str, Delta]:
    return {name: compute_delta(m, embedder) for name, m in MODIFIERS.items()}


def apply_delta(
    image_vec: NDArray[np.float32],
    delta: Delta,
    alpha: float,
    *,
    renormalize: bool = True,
) -> NDArray[np.float32]:
    """Shift `image_vec` along `delta` by `alpha`. Returns same shape."""
    out = image_vec.astype(np.float32) + alpha * delta.vector
    return normalize(out) if renormalize else out
