"""LLM-as-judge for side-by-side modifier eval.

Given a query image and two candidate result sets, ask Claude which set better
satisfies the modifier. Shuffle order to control for position bias; aggregate
over multiple anchors to get a win rate.

Spike-local. Not a production component.
"""

from __future__ import annotations

import base64
import random
from dataclasses import dataclass
from pathlib import Path

import anthropic

_MODEL = "claude-sonnet-4-6"  # vision-capable
_MAX_IMAGES = 11  # 1 query + up to 5 per side


@dataclass(frozen=True)
class Judgement:
    winner: str  # "A", "B", or "tie"
    reason: str
    raw_response: str


def judge_pair(
    *,
    client: anthropic.Anthropic,
    modifier_name: str,
    query_image: Path,
    set_a: list[Path],
    set_b: list[Path],
    shuffle: bool = True,
) -> Judgement:
    """Ask the model which set better satisfies the modifier.

    If `shuffle`, randomly swap A/B before asking and unswap the verdict.
    """
    swapped = shuffle and random.random() < 0.5
    left, right = (set_b, set_a) if swapped else (set_a, set_b)

    content: list[dict] = [
        {
            "type": "text",
            "text": (
                f"The user wants to find artworks similar to the query image "
                f"but '{modifier_name}'. Below are two candidate result sets, "
                f"Set A and Set B. Which set better satisfies the modifier "
                f"'{modifier_name}' while still being visually related to the "
                f"query?\n\n"
                f"Reply on the first line with exactly one of: A, B, tie. "
                f"On the next line give one short sentence explaining why."
            ),
        },
        {"type": "text", "text": "Query image:"},
        _image_block(query_image),
        {"type": "text", "text": "Set A:"},
    ]
    for p in left[: _MAX_IMAGES // 2]:
        content.append(_image_block(p))
    content.append({"type": "text", "text": "Set B:"})
    for p in right[: _MAX_IMAGES // 2]:
        content.append(_image_block(p))

    msg = client.messages.create(
        model=_MODEL,
        max_tokens=200,
        messages=[{"role": "user", "content": content}],
    )
    raw = "".join(b.text for b in msg.content if b.type == "text").strip()
    winner_raw, _, reason = raw.partition("\n")
    winner_raw = winner_raw.strip().upper()

    if "A" in winner_raw and "B" not in winner_raw:
        verdict = "A"
    elif "B" in winner_raw and "A" not in winner_raw:
        verdict = "B"
    else:
        verdict = "tie"

    # Unswap if we randomized.
    if swapped and verdict in ("A", "B"):
        verdict = "B" if verdict == "A" else "A"

    return Judgement(winner=verdict, reason=reason.strip(), raw_response=raw)


def _image_block(path: Path) -> dict:
    data = path.read_bytes()
    media_type = _media_type_for(path)
    return {
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": media_type,
            "data": base64.b64encode(data).decode("ascii"),
        },
    }


def _media_type_for(path: Path) -> str:
    ext = path.suffix.lower()
    return {
        ".jpg": "image/jpeg",
        ".jpeg": "image/jpeg",
        ".png": "image/png",
        ".webp": "image/webp",
    }.get(ext, "image/jpeg")
