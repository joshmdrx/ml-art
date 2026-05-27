"""Embedding providers and cache."""

from ml_art.embeddings.base import Embedder
from ml_art.embeddings.cache import CachedEmbedder

# JinaEmbedder (HTTP) and LocalJinaClipEmbedder are imported lazily by callers
# so that this package imports cleanly without their optional deps.

__all__ = ["Embedder", "CachedEmbedder"]
