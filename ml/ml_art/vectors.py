"""Numpy vector utilities. Pure functions, no I/O.

Conventions:
- Single vector: shape `(d,)`, float32.
- Batch: shape `(n, d)`, float32.
- All functions accept either and return the same shape where meaningful.
"""

from __future__ import annotations

import numpy as np
from numpy.typing import NDArray

Vec = NDArray[np.float32]


def normalize(v: Vec, eps: float = 1e-12) -> Vec:
    """L2-normalize a vector or batch of vectors along the last axis."""
    v = np.asarray(v, dtype=np.float32)
    norm = np.linalg.norm(v, axis=-1, keepdims=True)
    return v / np.maximum(norm, eps)


def cosine(a: Vec, b: Vec) -> float | Vec:
    """Cosine similarity.

    - `a` shape `(d,)`, `b` shape `(d,)`     -> float
    - `a` shape `(d,)`, `b` shape `(n, d)`   -> `(n,)`
    - `a` shape `(n, d)`, `b` shape `(d,)`   -> `(n,)`
    - `a` shape `(n, d)`, `b` shape `(m, d)` -> `(n, m)`
    """
    a = normalize(np.asarray(a, dtype=np.float32))
    b = normalize(np.asarray(b, dtype=np.float32))
    if a.ndim == 1 and b.ndim == 1:
        return float(np.dot(a, b))
    if a.ndim == 1:
        return b @ a
    if b.ndim == 1:
        return a @ b
    return a @ b.T


def top_k(
    query: Vec,
    corpus: Vec,
    k: int,
    exclude: set[int] | None = None,
) -> list[tuple[int, float]]:
    """Return the top-k indices and cosine similarities from `corpus` for a single query.

    `corpus` shape `(n, d)`, `query` shape `(d,)`.
    `exclude` is an optional set of corpus indices to skip.
    Returns `[(index, similarity), ...]` length `<= k`, sorted descending by similarity.
    """
    if corpus.ndim != 2:
        raise ValueError(f"corpus must be 2D, got shape {corpus.shape}")
    sims = cosine(query, corpus)  # (n,)
    sims = np.asarray(sims, dtype=np.float32)
    if exclude:
        mask = np.ones(len(sims), dtype=bool)
        for i in exclude:
            if 0 <= i < len(sims):
                mask[i] = False
        sims = np.where(mask, sims, -np.inf)
    n = len(sims)
    k = min(k, n)
    # argpartition is O(n); then sort just the top-k slice.
    idx_unsorted = np.argpartition(-sims, k - 1)[:k]
    idx_sorted = idx_unsorted[np.argsort(-sims[idx_unsorted])]
    return [(int(i), float(sims[i])) for i in idx_sorted if sims[i] != -np.inf]


def mean_vector(vs: Vec) -> Vec:
    """Mean of a batch `(n, d)` along axis 0, returns `(d,)`."""
    vs = np.asarray(vs, dtype=np.float32)
    if vs.ndim != 2:
        raise ValueError(f"expected 2D, got shape {vs.shape}")
    return vs.mean(axis=0).astype(np.float32)
