# Spike — modifier delta vectors

**Question:** does naive vector arithmetic on a multimodal embedding produce
the semantic shift implied by modifier labels ("moodier", "warmer", etc.)
well enough to ship the UX around it?

**Hypotheses compared:**
- **A. Baseline** — k-NN on image embedding only.
- **B. Text fusion (RRF)** — RRF of image-emb top-k and modifier-text top-k.
- **C. Delta** — `image_emb + α · delta_modifier`, then k-NN.

If C doesn't beat B by a meaningful margin, we ship B and skip the delta machinery.

**Timebox:** 1–2 days.

## Run

```bash
# from ml/
uv pip install -e ".[notebook,eval,dev]"

# put 500–2000 art images in spikes/2026-05-modifier-deltas/data/images/
# easiest sources: WikiArt (Kaggle), Are.na public channels, your own seed set

# either open notebook.py in VSCode and "Run Cell" each # %% block,
# or convert to .ipynb:
jupytext --to ipynb spikes/2026-05-modifier-deltas/notebook.py
jupyter notebook spikes/2026-05-modifier-deltas/notebook.ipynb
```

## What gets written

- `.cache/jina-clip-v2/api/text/*.npy` — cached text embeddings (across all runs)
- `.cache/jina-clip-v2/api/image/*.npy` — cached image embeddings (slowest step; persists)
- `results/delta_vs_baseline.json` — per-anchor judgements
- `results/delta_vs_textfusion.json` — per-anchor judgements

## Cost estimate

- Jina embeddings: 2000 images + ~50 text queries → ~$2–5 on first run, ~$0 on subsequent runs (cached).
- Anthropic judge: ~5 modifiers × 20 anchors × 2 comparisons × 1 call = ~200 calls, ~$3–8.

Roughly **$5–15 total** for a full spike pass.

## Iterating

When you change anything that affects the deltas — the contrastive pairs in
`modifiers.py`, the alpha, the normalization order — re-run the relevant cells.
Image embeddings are cached so the re-run is cheap.

When you change the embedding model, **delete `.cache/`** to invalidate.

## Writing up the verdict

The last `# %%` cell in the notebook has a checklist. Fill it in after running.
The decision is one of:
- **SHIP** — delta beats both baseline and text-fusion across modifiers
- **SHIP-PARTIAL** — works for some modifiers, not others; ship only those
- **SKIP-USE-TEXTFUSION** — text-fusion is as good; drop the delta complexity
- **NEEDS-MORE-WORK** — promising but needs better contrastive pairs / tuning

Promote useful code from `deltas.py` / `modifiers.py` / `eval.py` into
`ml_art/` only after writing the verdict.
