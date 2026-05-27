"""Environment-backed configuration.

Single helper that loads `.env` once and exposes typed accessors. Importable
from anywhere without side effects beyond the first `load_dotenv()` call.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path

from dotenv import load_dotenv


@dataclass(frozen=True)
class Config:
    jina_api_key: str | None
    voyage_api_key: str | None
    anthropic_api_key: str | None
    cache_dir: Path


@lru_cache(maxsize=1)
def get_config() -> Config:
    load_dotenv()
    cache_dir = Path(os.environ.get("ML_ART_CACHE_DIR", ".cache")).resolve()
    cache_dir.mkdir(parents=True, exist_ok=True)
    return Config(
        jina_api_key=os.environ.get("JINA_API_KEY"),
        voyage_api_key=os.environ.get("VOYAGE_API_KEY"),
        anthropic_api_key=os.environ.get("ANTHROPIC_API_KEY"),
        cache_dir=cache_dir,
    )


def require(value: str | None, name: str) -> str:
    if not value:
        raise RuntimeError(f"Missing required config: {name}")
    return value
