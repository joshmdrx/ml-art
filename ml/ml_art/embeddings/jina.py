"""Jina embeddings client (jina-clip-v2 multimodal).

API: POST https://api.jina.ai/v1/embeddings
Docs: https://jina.ai/embeddings/

Accepts a mixed batch of text and image inputs. We expose separate `embed_texts`
and `embed_images` for clarity; the underlying API call is the same shape.
"""

from __future__ import annotations

import base64
import time
from typing import Any

import numpy as np
import requests
from numpy.typing import NDArray

from ml_art.embeddings.base import Embedder

_ENDPOINT = "https://api.jina.ai/v1/embeddings"
_BATCH_SIZE = 32
_TIMEOUT_S = 60
_MAX_RETRIES = 3


class JinaEmbedder:
    """Multimodal embedder using `jina-clip-v2` (1024-dim)."""

    def __init__(
        self,
        api_key: str,
        model: str = "jina-clip-v2",
        dimension: int = 1024,
    ) -> None:
        self._api_key = api_key
        self._model = model
        self._dimension = dimension

    @property
    def model_name(self) -> str:
        return self._model

    @property
    def model_version(self) -> str:
        # Unified with the local PyTorch path as of migration 0009.
        # See TODO(T-024) — model_version unification.
        return "v2"

    @property
    def dimension(self) -> int:
        return self._dimension

    def embed_texts(self, texts: list[str]) -> NDArray[np.float32]:
        if not texts:
            return np.zeros((0, self._dimension), dtype=np.float32)
        return self._embed_in_batches([{"text": t} for t in texts])

    def embed_images(self, images: list[bytes]) -> NDArray[np.float32]:
        if not images:
            return np.zeros((0, self._dimension), dtype=np.float32)
        payload = [{"image": base64.b64encode(b).decode("ascii")} for b in images]
        return self._embed_in_batches(payload)

    # Internals

    def _embed_in_batches(self, items: list[dict[str, Any]]) -> NDArray[np.float32]:
        out: list[NDArray[np.float32]] = []
        for i in range(0, len(items), _BATCH_SIZE):
            chunk = items[i : i + _BATCH_SIZE]
            out.append(self._embed_one_batch(chunk))
        return np.concatenate(out, axis=0)

    def _embed_one_batch(self, batch: list[dict[str, Any]]) -> NDArray[np.float32]:
        body = {
            "model": self._model,
            "input": batch,
            "embedding_type": "float",
        }
        headers = {
            "Authorization": f"Bearer {self._api_key}",
            "Content-Type": "application/json",
            "Accept": "application/json",
        }

        for attempt in range(_MAX_RETRIES):
            try:
                resp = requests.post(
                    _ENDPOINT, json=body, headers=headers, timeout=_TIMEOUT_S
                )
                if resp.status_code == 429 or resp.status_code >= 500:
                    self._sleep_backoff(attempt)
                    continue
                resp.raise_for_status()
                data = resp.json()
                vecs = [np.asarray(d["embedding"], dtype=np.float32) for d in data["data"]]
                arr = np.stack(vecs, axis=0)
                if arr.shape[1] != self._dimension:
                    raise RuntimeError(
                        f"Jina returned dim {arr.shape[1]}, expected {self._dimension}"
                    )
                return arr
            except requests.RequestException as e:
                if attempt == _MAX_RETRIES - 1:
                    raise
                self._sleep_backoff(attempt)
                _ = e  # swallow, retry
        raise RuntimeError("unreachable")

    @staticmethod
    def _sleep_backoff(attempt: int) -> None:
        time.sleep(2**attempt)
