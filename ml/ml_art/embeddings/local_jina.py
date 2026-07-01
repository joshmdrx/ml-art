"""Local jina-clip-v2 embedder via Hugging Face transformers.

Runs the same model that Jina's API exposes (1024-dim multimodal), but locally.
Useful for spikes / batch eval where API calls would be slow or expensive.

For production request-path embedding the HTTP client (`jina.py`) is preferred:
PyTorch in a Rust Lambda is not a good time.

Install: `uv pip install -e ".[local]"` to add torch + transformers + einops + timm.
First call downloads ~2GB of weights to the HF cache (~/.cache/huggingface).

Performance notes:
- Image decoding (PIL) runs in a ThreadPoolExecutor — image-decode parallelism
  matters more than people expect for large corpora. Releases the GIL during
  the actual decode.
- Model forward pass is batched (default batch_size=32). MPS / CUDA handles
  batched matmul efficiently. The text path uses a smaller batch since text
  inputs are tiny.
- We do NOT pipeline decode-vs-forward in this implementation. For a 2k-image
  spike that's ~13s of parallel decode + 6s of model = ~19s total instead of
  a fully-pipelined ~7s. The added complexity isn't worth it at this scale.
"""

from __future__ import annotations

import io
from concurrent.futures import ThreadPoolExecutor

import numpy as np
from numpy.typing import NDArray
from PIL import Image
from tqdm import tqdm

_MODEL_ID = "jinaai/jina-clip-v2"
_DIM = 1024


class LocalJinaClipEmbedder:
    """Multimodal embedder using jina-clip-v2 loaded locally."""

    def __init__(
        self,
        *,
        device: str | None = None,
        image_batch_size: int = 32,
        text_batch_size: int = 64,
        decode_workers: int = 8,
        show_progress: bool = True,
    ) -> None:
        # Lazy-import to keep the base package importable without torch installed.
        import torch
        from transformers import AutoModel

        if device is None:
            if torch.cuda.is_available():
                device = "cuda"
            elif getattr(torch.backends, "mps", None) and torch.backends.mps.is_available():
                device = "mps"
            else:
                device = "cpu"

        self._torch = torch
        self._device = device
        self._image_batch_size = image_batch_size
        self._text_batch_size = text_batch_size
        self._decode_workers = decode_workers
        self._show_progress = show_progress

        # trust_remote_code=True: jina-clip-v2 ships custom modeling code.
        self._model = AutoModel.from_pretrained(_MODEL_ID, trust_remote_code=True)
        self._model = self._model.to(device).eval()

    @property
    def model_name(self) -> str:
        return _MODEL_ID

    @property
    def model_version(self) -> str:
        # Unified with the Rust HTTP path as of migration 0009.
        # See TODO(T-024) — model_version unification.
        return "v2"

    @property
    def dimension(self) -> int:
        return _DIM

    def embed_texts(self, texts: list[str]) -> NDArray[np.float32]:
        if not texts:
            return np.zeros((0, _DIM), dtype=np.float32)
        out: list[NDArray[np.float32]] = []
        n_batches = (len(texts) + self._text_batch_size - 1) // self._text_batch_size
        iterator = range(0, len(texts), self._text_batch_size)
        if self._show_progress and n_batches > 1:
            iterator = tqdm(iterator, total=n_batches, desc="text embed", unit="batch")
        with self._torch.no_grad():
            for i in iterator:
                batch = texts[i : i + self._text_batch_size]
                vec = self._model.encode_text(batch)
                out.append(self._to_numpy(vec))
        return np.concatenate(out, axis=0)

    def embed_images(self, images: list[bytes]) -> NDArray[np.float32]:
        if not images:
            return np.zeros((0, _DIM), dtype=np.float32)

        # 1) Parallel PIL decode. Releases GIL during JPEG decode.
        pil_images = self._decode_parallel(images)

        # 2) Batched model forward.
        out: list[NDArray[np.float32]] = []
        n_batches = (len(pil_images) + self._image_batch_size - 1) // self._image_batch_size
        iterator = range(0, len(pil_images), self._image_batch_size)
        if self._show_progress:
            iterator = tqdm(iterator, total=n_batches, desc="image embed", unit="batch")
        with self._torch.no_grad():
            for i in iterator:
                batch = pil_images[i : i + self._image_batch_size]
                vec = self._model.encode_image(batch)
                out.append(self._to_numpy(vec))
        return np.concatenate(out, axis=0)

    # Internals

    def _decode_parallel(self, image_bytes: list[bytes]) -> list[Image.Image]:
        """JPEG → PIL.Image (RGB) in parallel. Returns in input order."""
        def _decode_one(b: bytes) -> Image.Image:
            return Image.open(io.BytesIO(b)).convert("RGB")

        with ThreadPoolExecutor(max_workers=self._decode_workers) as ex:
            mapped = ex.map(_decode_one, image_bytes)
            if self._show_progress:
                mapped = tqdm(
                    mapped,
                    total=len(image_bytes),
                    desc="decoding",
                    unit="img",
                )
            return list(mapped)

    def _to_numpy(self, vec) -> NDArray[np.float32]:
        if hasattr(vec, "detach"):
            vec = vec.detach().cpu().numpy()
        arr = np.asarray(vec, dtype=np.float32)
        if arr.ndim == 1:
            arr = arr[None, :]
        return arr
