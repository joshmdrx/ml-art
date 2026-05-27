"""Embedder protocol.

Any multimodal embedding provider implements this. Used by the cache layer
and by all spike / eval code so we can swap providers without touching callers.
"""

from __future__ import annotations

from typing import Protocol, runtime_checkable

import numpy as np
from numpy.typing import NDArray


@runtime_checkable
class Embedder(Protocol):
    """Returns float32 arrays of shape `(n, dimension)`."""

    @property
    def model_name(self) -> str: ...

    @property
    def model_version(self) -> str: ...

    @property
    def dimension(self) -> int: ...

    def embed_texts(self, texts: list[str]) -> NDArray[np.float32]: ...

    def embed_images(self, images: list[bytes]) -> NDArray[np.float32]: ...
