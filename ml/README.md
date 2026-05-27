# ml/ — ML & evaluation code

Python codebase for embedding work, eval-set runs, recommendation experiments, and ad-hoc data science. Lives alongside the Rust API and TS frontend.

## Layout

```
ml_art/                       importable package; durable code
  embeddings/                 embedder protocol + provider clients + cache
  vectors.py                  numpy utils: normalize, cosine, top_k
  corpus.py                   image loader + metadata
  viz.py                      matplotlib side-by-side grid for notebooks
  config.py                   env loading

spikes/                       throwaway exploration, kept by date+name
  2026-05-modifier-deltas/    visual-search modifier vector spike

tests/                        pytest
```

Anything in `spikes/<date>-<name>/` is throwaway by convention. If a spike succeeds, useful bits get promoted into `ml_art/`.

## Setup

Uses [uv](https://github.com/astral-sh/uv).

```bash
cd ml
uv venv
source .venv/bin/activate
uv pip install -e ".[notebook,eval,dev]"
cp .env.example .env  # fill in keys
```

## Running tests

```bash
uv run pytest
```

## Running a spike

See `spikes/<name>/README.md`.
