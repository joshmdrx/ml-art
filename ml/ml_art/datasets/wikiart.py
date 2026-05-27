"""WikiArt dataset loader — stratified sample for spikes and demo seeds.

Pulls a varied subset of paintings from a WikiArt-style Hugging Face dataset,
saves each as a JPEG with a deterministic filename.

The canonical dataset (`huggan/wikiart`) is gated and requires:
  huggingface-cli login

If it can't load (gated, network issue, dataset moved), the loader walks a
small fallback list of known-equivalent datasets until one succeeds. The
schema across these is consistent: `image` (PIL) + `style` (int or str).
"""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path

from tqdm import tqdm

# In preference order. First one that loads wins.
_DATASET_CANDIDATES = [
    "huggan/wikiart",
    "Artificio/WikiArt",
    "jlbaker361/wikiart-balanced",
]


@dataclass
class FetchResult:
    dataset_name: str
    out_dir: Path
    saved: int
    style_counts: dict[str, int]


def fetch_stratified(
    out_dir: Path,
    *,
    per_style: int = 75,
    max_total: int | None = None,
    image_size_max: int = 1024,
) -> FetchResult:
    """Stream the dataset, save `per_style` examples per style label to `out_dir`.

    Stops early once `max_total` images are saved (if set). Skips images that
    fail to decode or are smaller than 64px on the short edge.
    """
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    ds_iter, dataset_name = _open_dataset()
    style_field, style_decoder = _resolve_style_field(ds_iter)

    saved_per_style: Counter[str] = Counter()
    saved_total = 0

    pbar = tqdm(desc=f"sampling from {dataset_name}", unit="img")
    for example in ds_iter:
        if max_total and saved_total >= max_total:
            break

        style_value = example.get(style_field)
        if style_value is None:
            continue
        style_label = style_decoder(style_value)

        if saved_per_style[style_label] >= per_style:
            continue

        img = example.get("image")
        if img is None or not hasattr(img, "save"):
            continue
        if min(img.size) < 64:
            continue

        # Resize down if very large; we don't need full-res for embedding.
        if max(img.size) > image_size_max:
            ratio = image_size_max / max(img.size)
            new_size = (int(img.size[0] * ratio), int(img.size[1] * ratio))
            img = img.resize(new_size)

        fname = f"wikiart-{_slug(style_label)}-{saved_per_style[style_label]:03d}.jpg"
        out_path = out_dir / fname
        try:
            img.convert("RGB").save(out_path, "JPEG", quality=88)
        except Exception:
            continue

        saved_per_style[style_label] += 1
        saved_total += 1
        pbar.update(1)
        pbar.set_postfix(styles=len(saved_per_style))

    pbar.close()

    return FetchResult(
        dataset_name=dataset_name,
        out_dir=out_dir,
        saved=saved_total,
        style_counts=dict(saved_per_style),
    )


def _open_dataset():
    """Try each candidate dataset, return (iterator, name) for the first that works."""
    from datasets import load_dataset

    last_err: Exception | None = None
    for name in _DATASET_CANDIDATES:
        try:
            ds = load_dataset(name, split="train", streaming=True)
            return ds, name
        except Exception as e:
            last_err = e
            continue
    raise RuntimeError(
        f"No WikiArt dataset could be loaded. Last error: {last_err}\n"
        f"Tried: {_DATASET_CANDIDATES}\n"
        f"For `huggan/wikiart`, run `huggingface-cli login` first."
    )


def _resolve_style_field(streaming_ds):
    """Pick a style/label column from whatever schema the dataset exposes."""
    info = getattr(streaming_ds, "info", None)
    features = getattr(info, "features", None) if info else None

    candidates = ("style", "label", "genre", "artist")
    if features:
        for c in candidates:
            if c in features:
                f = features[c]
                if hasattr(f, "names"):  # ClassLabel: map int -> str
                    names = f.names
                    return c, (lambda v, names=names: names[v] if 0 <= v < len(names) else str(v))
                return c, str
    # Fallback: peek at one example
    sample = next(iter(streaming_ds))
    for c in candidates:
        if c in sample:
            return c, str
    raise RuntimeError(f"No style/label field found. Sample keys: {list(sample.keys())}")


def _slug(s: str) -> str:
    return "".join(ch if ch.isalnum() or ch == "-" else "_" for ch in s.lower())


def _cli() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, required=True, help="Output directory")
    parser.add_argument("--per-style", type=int, default=75)
    parser.add_argument("--max-total", type=int, default=2000)
    args = parser.parse_args()

    result = fetch_stratified(
        args.out,
        per_style=args.per_style,
        max_total=args.max_total,
    )
    print(f"\nFetched {result.saved} images from {result.dataset_name} → {result.out_dir}")
    print("Style breakdown:")
    for style, n in sorted(result.style_counts.items(), key=lambda kv: -kv[1]):
        print(f"  {style:<30} {n}")


if __name__ == "__main__":
    _cli()
