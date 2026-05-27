"""Disk-backed embedding cache.

Wraps any `Embedder`. Keys are SHA256 of `(model_name, model_version, kind, input_bytes)`.
Layout on disk: `<cache_dir>/<model_name>/<model_version>/<kind>/<sha>.npy`.

The cache is intentionally simple — no eviction, no manifest. Embedding API
calls are the slow + expensive part; disk space is cheap.
"""

from __future__ import annotations

import hashlib
from pathlib import Path

import numpy as np
from numpy.typing import NDArray

from ml_art.embeddings.base import Embedder


def _sha256(*parts: bytes) -> str:
    h = hashlib.sha256()
    for p in parts:
        h.update(p)
        h.update(b"\x00")
    return h.hexdigest()


class CachedEmbedder:
    """Wraps an Embedder with a content-addressed on-disk cache."""

    def __init__(self, inner: Embedder, cache_dir: Path) -> None:
        self._inner = inner
        self._root = (
            Path(cache_dir) / inner.model_name / inner.model_version
        )
        self._root.mkdir(parents=True, exist_ok=True)

    # Embedder protocol passthroughs

    @property
    def model_name(self) -> str:
        return self._inner.model_name

    @property
    def model_version(self) -> str:
        return self._inner.model_version

    @property
    def dimension(self) -> int:
        return self._inner.dimension

    # Public API

    def embed_texts(self, texts: list[str]) -> NDArray[np.float32]:
        return self._embed_with_cache(
            inputs=[t.encode("utf-8") for t in texts],
            kind="text",
            backend=lambda missing_indices: self._inner.embed_texts(
                [texts[i] for i in missing_indices]
            ),
        )

    def embed_images(self, images: list[bytes]) -> NDArray[np.float32]:
        return self._embed_with_cache(
            inputs=images,
            kind="image",
            backend=lambda missing_indices: self._inner.embed_images(
                [images[i] for i in missing_indices]
            ),
        )

    # Internals

    def _path_for(self, kind: str, sha: str) -> Path:
        d = self._root / kind
        d.mkdir(parents=True, exist_ok=True)
        return d / f"{sha}.npy"

    def _embed_with_cache(
        self,
        inputs: list[bytes],
        kind: str,
        backend,
    ) -> NDArray[np.float32]:
        n = len(inputs)
        if n == 0:
            return np.zeros((0, self.dimension), dtype=np.float32)

        # SHA per input, scoped to model identity.
        model_tag = f"{self.model_name}:{self.model_version}:{kind}".encode("utf-8")
        shas = [_sha256(model_tag, b) for b in inputs]
        paths = [self._path_for(kind, s) for s in shas]

        out = np.empty((n, self.dimension), dtype=np.float32)
        missing_indices: list[int] = []
        for i, p in enumerate(paths):
            if p.exists():
                out[i] = np.load(p)
            else:
                missing_indices.append(i)

        if missing_indices:
            fresh = backend(missing_indices)
            if fresh.shape != (len(missing_indices), self.dimension):
                raise RuntimeError(
                    f"Embedder returned shape {fresh.shape}, "
                    f"expected ({len(missing_indices)}, {self.dimension})"
                )
            for k, i in enumerate(missing_indices):
                vec = fresh[k].astype(np.float32)
                np.save(paths[i], vec)
                out[i] = vec

        return out
