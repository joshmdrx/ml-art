"""Download a small art corpus from the Met Museum Open Access API.

Public domain, no auth, no rate limit at our scale. Saves images into
`data/images/met-<object_id>.jpg`.

Run: `python -m spikes.2026-05-modifier-deltas.fetch_met --n 500`
Or:  `python spikes/2026-05-modifier-deltas/fetch_met.py --n 500`

API docs: https://metmuseum.github.io/
"""

from __future__ import annotations

import argparse
import random
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

import requests
from tqdm import tqdm

SEARCH_URL = "https://collectionapi.metmuseum.org/public/collection/v1/search"
OBJECT_URL = "https://collectionapi.metmuseum.org/public/collection/v1/objects/{id}"

# Diverse queries to get a varied corpus, including some abstract/modern works.
# Only ~140 PD results per query, so we hit several to build the pool.
QUERIES = [
    "painting", "abstract", "landscape", "portrait", "still life",
    "modern art", "contemporary", "impressionist", "watercolor", "drawing",
    "print", "color", "figure", "minimalist", "geometric", "expressionist",
    "japanese", "european", "american", "photograph", "ink", "oil",
    "garden", "interior", "city", "river", "mountain", "tree", "flower",
    "animal", "bird", "horse", "cat", "ocean", "night", "morning",
    "geometric", "pattern", "monochrome", "moody", "bright", "dark",
    "century", "renaissance", "baroque", "study", "sketch", "composition",
]


def collect_object_ids(target: int) -> list[int]:
    seen: set[int] = set()
    for q in QUERIES:
        if len(seen) >= target * 3:
            break
        try:
            resp = requests.get(
                SEARCH_URL,
                params={"q": q, "hasImages": "true", "isPublicDomain": "true"},
                timeout=30,
            )
            resp.raise_for_status()
            data = resp.json()
            ids = data.get("objectIDs") or []
            seen.update(ids)
        except (requests.RequestException, ValueError):
            continue
    pool = list(seen)
    random.seed(0)
    random.shuffle(pool)
    return pool


def fetch_object(obj_id: int, out_dir: Path) -> bool:
    out_path = out_dir / f"met-{obj_id}.jpg"
    if out_path.exists():
        return True
    try:
        r = requests.get(OBJECT_URL.format(id=obj_id), timeout=30)
        if r.status_code != 200:
            return False
        obj = r.json()
        if not obj.get("isPublicDomain"):
            return False
        img_url = obj.get("primaryImageSmall") or obj.get("primaryImage")
        if not img_url:
            return False
        img = requests.get(img_url, timeout=60)
        if img.status_code != 200 or not img.content:
            return False
        # Met returns JPEG for primaryImageSmall. Save as-is.
        out_path.write_bytes(img.content)
        return True
    except requests.RequestException:
        return False


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--n", type=int, default=500, help="Target number of images")
    parser.add_argument("--out", type=Path, default=None)
    parser.add_argument("--workers", type=int, default=8)
    args = parser.parse_args()

    out_dir = args.out or Path(__file__).parent / "data" / "images"
    out_dir.mkdir(parents=True, exist_ok=True)

    existing = len(list(out_dir.glob("met-*.jpg")))
    if existing >= args.n:
        print(f"already have {existing} images in {out_dir}, nothing to do")
        return

    print(f"target: {args.n}, existing: {existing}, fetching: {args.n - existing}")
    print("collecting object IDs...")
    ids = collect_object_ids(args.n)
    print(f"collected {len(ids)} candidate object IDs")

    saved = existing
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = {
            pool.submit(fetch_object, obj_id, out_dir): obj_id
            for obj_id in ids[: args.n * 6]
        }
        with tqdm(total=args.n - existing, desc="downloading") as pbar:
            for fut in as_completed(futures):
                if fut.result():
                    saved += 1
                    pbar.update(1)
                if saved >= args.n:
                    for f in futures:
                        f.cancel()
                    break
                time.sleep(0.005)  # gentle on the API

    final = len(list(out_dir.glob("met-*.jpg")))
    print(f"done. {final} images in {out_dir}")


if __name__ == "__main__":
    main()
