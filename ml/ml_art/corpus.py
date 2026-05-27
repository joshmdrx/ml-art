"""Image corpus loader.

Given a directory of image files, produce a stable list of `CorpusItem`s with
SHA-based IDs. Deduplicates by content hash. Use this to feed an embedder.
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass
from pathlib import Path

from PIL import Image

_EXTS = {".jpg", ".jpeg", ".png", ".webp"}


@dataclass(frozen=True)
class CorpusItem:
    id: str  # first 16 hex chars of SHA256
    sha256: str
    path: Path
    width: int
    height: int

    def read_bytes(self) -> bytes:
        return self.path.read_bytes()


def load_corpus(
    directory: Path,
    *,
    recursive: bool = True,
    max_items: int | None = None,
) -> list[CorpusItem]:
    """Walk `directory`, build a deduped, sorted list of CorpusItems.

    Sort order is stable (by sha) so downstream embedding indices are reproducible.
    """
    directory = Path(directory)
    if not directory.exists():
        raise FileNotFoundError(directory)

    paths = (
        directory.rglob("*") if recursive else directory.iterdir()
    )
    seen: dict[str, CorpusItem] = {}
    for p in paths:
        if not p.is_file() or p.suffix.lower() not in _EXTS:
            continue
        sha = _sha256_file(p)
        if sha in seen:
            continue
        try:
            with Image.open(p) as im:
                w, h = im.size
        except Exception:
            continue
        seen[sha] = CorpusItem(
            id=sha[:16],
            sha256=sha,
            path=p,
            width=w,
            height=h,
        )

    items = sorted(seen.values(), key=lambda c: c.sha256)
    if max_items is not None:
        items = items[:max_items]
    return items


def _sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()
