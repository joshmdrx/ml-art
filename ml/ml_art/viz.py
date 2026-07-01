"""Notebook-friendly visualization helpers.

Side-by-side image grids for comparing result sets. Pure matplotlib; only used
in notebooks and spike code, not in any production path.
"""

from __future__ import annotations

from collections.abc import Sequence
from pathlib import Path

import matplotlib.pyplot as plt
from PIL import Image


def show_grid(
    image_paths: Sequence[Path],
    *,
    cols: int = 5,
    captions: Sequence[str] | None = None,
    title: str | None = None,
    figsize_per_cell: tuple[float, float] = (2.4, 2.4),
) -> None:
    """Display images in a grid. Captions optional."""
    n = len(image_paths)
    if n == 0:
        return
    rows = (n + cols - 1) // cols
    fig, axes = plt.subplots(
        rows,
        cols,
        figsize=(figsize_per_cell[0] * cols, figsize_per_cell[1] * rows),
    )
    if title:
        fig.suptitle(title, fontsize=12)
    axes_flat = axes.flatten() if rows * cols > 1 else [axes]
    for i, ax in enumerate(axes_flat):
        if i < n:
            with Image.open(image_paths[i]) as im:
                ax.imshow(im)
            if captions and i < len(captions):
                ax.set_title(captions[i], fontsize=8)
        ax.axis("off")
    plt.tight_layout()
    plt.show()


def show_comparison(
    query_path: Path,
    set_a: Sequence[Path],
    set_b: Sequence[Path],
    *,
    label_a: str = "A",
    label_b: str = "B",
    cols: int = 5,
) -> None:
    """Show: [query] then row [set_a] then row [set_b]. Useful for side-by-side eval."""
    fig, axes = plt.subplots(
        3,
        cols,
        figsize=(2.4 * cols, 7.2),
        gridspec_kw={"height_ratios": [1.2, 1, 1]},
    )

    # Row 0: query in the first cell, rest blank
    for j, ax in enumerate(axes[0]):
        if j == 0:
            with Image.open(query_path) as im:
                ax.imshow(im)
            ax.set_title("query", fontsize=10)
        ax.axis("off")

    # Rows 1 and 2: result sets
    for row_idx, (label, paths) in enumerate(
        [(label_a, set_a), (label_b, set_b)], start=1
    ):
        for j, ax in enumerate(axes[row_idx]):
            if j < len(paths):
                with Image.open(paths[j]) as im:
                    ax.imshow(im)
                if j == 0:
                    ax.set_ylabel(label, fontsize=10)
            ax.axis("off")

    plt.tight_layout()
    plt.show()
