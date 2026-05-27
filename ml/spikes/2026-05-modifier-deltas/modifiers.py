"""Modifier registry: name → contrastive text pairs.

Each modifier is a direction in embedding space. We compute the direction as
the mean of `positive` minus the mean of `negative` text embeddings.

These contrastive pairs are the *definition* of each modifier. Get them wrong
and the whole feature fails for reasons unrelated to the math — so iterate on
them deliberately and re-run the spike when you change them.

Spike-local; not promoted to ml_art/ until validated.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Modifier:
    name: str
    positive: tuple[str, ...]
    negative: tuple[str, ...]


MODIFIERS: dict[str, Modifier] = {
    "moodier": Modifier(
        name="moodier",
        positive=(
            "a moody atmospheric painting with dark tones",
            "a brooding melancholic artwork in shadow",
            "a somber overcast landscape painting",
            "a dark contemplative piece, low-key lighting",
        ),
        negative=(
            "a bright cheerful painting with vivid colors",
            "an uplifting joyful artwork in sunlight",
            "a vibrant happy scene, high-key lighting",
            "a light and airy artwork, optimistic mood",
        ),
    ),
    "warmer": Modifier(
        name="warmer",
        positive=(
            "an artwork with warm tones, red orange and yellow",
            "a painting dominated by amber, ochre, and crimson",
            "a sunlit warm-palette artwork",
            "an artwork with warm earthy reds and golds",
        ),
        negative=(
            "an artwork with cool tones, blue and teal",
            "a painting dominated by navy, cyan, and slate",
            "a cold-palette winter artwork",
            "an artwork with cool blues and greens",
        ),
    ),
    "more_minimal": Modifier(
        name="more_minimal",
        positive=(
            "a minimalist artwork with empty negative space",
            "a sparse composition with a single simple form",
            "a clean reductive piece, few elements",
            "an artwork with quiet emptiness and restraint",
        ),
        negative=(
            "a maximalist artwork densely packed with detail",
            "a busy composition with many elements",
            "an ornate baroque piece, intricate and crowded",
            "an artwork full of pattern, texture, and visual noise",
        ),
    ),
    "more_textured": Modifier(
        name="more_textured",
        positive=(
            "a heavily textured painting with thick impasto",
            "an artwork with rough tactile surface",
            "a painting where you can see brushwork and material",
            "an artwork with grainy, gritty, palpable texture",
        ),
        negative=(
            "a smooth flat artwork with no visible texture",
            "a digital crisp clean illustration",
            "an artwork with even uniform surface",
            "a glossy flat-finish painting",
        ),
    ),
    "more_graphic": Modifier(
        name="more_graphic",
        positive=(
            "a graphic artwork with bold flat shapes",
            "a poster-like illustration with strong outlines",
            "a stylized graphic composition",
            "an artwork with clean vector-like forms",
        ),
        negative=(
            "a painterly artwork with soft brushwork",
            "a naturalistic painting with blended tones",
            "a representational artwork with realistic shading",
            "a loose expressive oil painting",
        ),
    ),
}


def get(name: str) -> Modifier:
    if name not in MODIFIERS:
        raise KeyError(f"unknown modifier {name!r}; have {sorted(MODIFIERS)}")
    return MODIFIERS[name]


def all_names() -> list[str]:
    return sorted(MODIFIERS)
