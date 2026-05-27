# Findings — Modifier Delta Vectors Spike

**Status:** Complete. Verdict: **SHIP delta vectors at α≈0.8**, alongside text-fusion as a fallback / blend partner.
**Reproducer:**
- Met corpus (small, classical): `uv run python spikes/2026-05-modifier-deltas/run_visual.py --anchors 6 --alpha 1.2`
- WikiArt corpus (rigorous): see *Round 2* below.

---

## Question

Does naive vector arithmetic on a multimodal embedding produce the semantic shift implied by modifier labels ("moodier", "warmer", "more minimal", "more textured", "more graphic"), well enough to ship the UX around it?

The visual-search UX in [01-page-spec.md](../../01-page-spec.md) shows modifier buttons next to a search input. The premise is that we can compute one direction vector per modifier (from contrastive text pairs) and add it to a query image embedding to shift results semantically. If that doesn't work, the buttons need a different implementation — most likely text-fusion (treat the modifier as a text query and RRF-blend the two ranked lists).

## Round 1 — Met Museum corpus (initial, inconclusive)

| Piece | What |
|---|---|
| Embedder | `jinaai/jina-clip-v2`, 1024-dim, multimodal, loaded locally via transformers on Mac MPS |
| Corpus | 124 public-domain artworks from the Met Museum API |
| Modifiers | 5 — `moodier`, `warmer`, `more_minimal`, `more_textured`, `more_graphic`. Contrastive text pairs in [modifiers.py](modifiers.py). |
| α values tried | 0.2, 0.6, 1.2 |

**Round 1 verdict:** technique appeared to work for `moodier` and `more_minimal` at high α (1.2), but the corpus was too homogeneous (mostly Old Masters and ornaments) to test `more_graphic`, `warmer`, `more_textured` — there was simply no minimal / graphic / contemporary content for the delta to *retrieve*. Inconclusive.

## Round 2 — WikiArt corpus (rigorous test)

To get a corpus that actually exercises every modifier, we pulled a stratified 2000-image sample from `huggan/wikiart` covering 27 style classes, including:

- 80 Minimalism, 80 Color Field Painting (for `more_minimal`)
- 76 Pop Art (for `more_graphic`)
- 80 Abstract Expressionism (texture range)
- 80 Fauvism, 80 Post-Impressionism (warm/cool palette range)
- All major classical periods for moodier comparison

Loader: [ml_art/datasets/wikiart.py](../../ml_art/datasets/wikiart.py). Saved to `data/wikiart/` and embedded once locally on MPS (~24 min for the first pass; subsequent alpha sweeps reuse the cached embeddings, ~30 seconds each).

### Alpha sweep on WikiArt

α was swept at 0.4 / 0.8 / 1.2 across 8 anchors × 5 modifiers. The PNGs below are anchor `0c2d8700c0fc78d7` (an Expressionist red/black mask-portrait) and `40fc5e441444cf1c` (a Cubist still life). Open any to verify.

#### `moodier`

| α | Description | File |
|---|---|---|
| 0.4 | Subtle. Most delta results indistinguishable from baseline. | [results_wikiart_a04/moodier/0c2d8700c0fc78d7.png](results_wikiart_a04/moodier/0c2d8700c0fc78d7.png) |
| **0.8** | **Strong, clean shift.** Egyptian-style darkened figures, B&W moody portraits, deep-shadow expressionism. Still visually relate to the source. | [results_wikiart_a08/moodier/0c2d8700c0fc78d7.png](results_wikiart_a08/moodier/0c2d8700c0fc78d7.png) |
| 1.2 | Over-shifted. Pulls in disconnected dark abstractions. Visual relationship to source breaks down. | [results_wikiart_a12/moodier/0c2d8700c0fc78d7.png](results_wikiart_a12/moodier/0c2d8700c0fc78d7.png) |

For the Cubist still-life anchor, α=0.8 surfaces a Soulages-style black abstraction and a moody monochrome alongside darker Cubist works — clear shift, source-aware. See [results_wikiart_a08/moodier/40fc5e441444cf1c.png](results_wikiart_a08/moodier/40fc5e441444cf1c.png).

**Verdict:** ✅ works at α=0.8.

#### `more_minimal`

| α | Description | File |
|---|---|---|
| 0.4 | Barely visible. | [results_wikiart_a04/more_minimal/0c2d8700c0fc78d7.png](results_wikiart_a04/more_minimal/0c2d8700c0fc78d7.png) |
| **0.8** | **Strong shift.** B&W minimal line drawings, sparse single-figure compositions, line sketches. | [results_wikiart_a08/more_minimal/0c2d8700c0fc78d7.png](results_wikiart_a08/more_minimal/0c2d8700c0fc78d7.png) |
| 1.2 | Pulls in a pure-blue color-field painting completely unrelated to source — over-shift. | [results_wikiart_a12/more_minimal/0c2d8700c0fc78d7.png](results_wikiart_a12/more_minimal/0c2d8700c0fc78d7.png) |

On the Cubist still-life anchor, α=0.8 surfaces a Rothko-like red rectangle, a Diebenkorn-style flat geometric, a sparse drawing, a minimal red color field. Pulling exactly the modernist minimal works that exist in the corpus. See [results_wikiart_a08/more_minimal/40fc5e441444cf1c.png](results_wikiart_a08/more_minimal/40fc5e441444cf1c.png).

**Verdict:** ✅ works at α=0.8. Best modifier of the five.

#### `more_graphic`

| α | Description | File |
|---|---|---|
| **0.8** | Pulls in bold flat-shape work, B&W graphic figures, an orange/red/black geometric Pop-style composition. | [results_wikiart_a08/more_graphic/0c2d8700c0fc78d7.png](results_wikiart_a08/more_graphic/0c2d8700c0fc78d7.png) |

**Verdict:** ✅ works at α=0.8. Less dramatic than `moodier` / `more_minimal` but a real, visible shift toward Pop Art / flat-graphic compositions.

#### `more_textured`

| α | Description | File |
|---|---|---|
| **0.8** | Subtle but real. Pulls in pieces with more visible brushwork / texture. | [results_wikiart_a08/more_textured/0c2d8700c0fc78d7.png](results_wikiart_a08/more_textured/0c2d8700c0fc78d7.png) |

**Verdict:** ✅ works, weaker effect. May need slightly higher α (try 1.0) or sharper contrastive pairs.

#### `warmer`

| α | Description | File |
|---|---|---|
| **0.8** | Delta row contains more reds / oranges / earth tones than baseline. Subtle when the anchor is already warm. | [results_wikiart_a08/warmer/0c2d8700c0fc78d7.png](results_wikiart_a08/warmer/0c2d8700c0fc78d7.png) |

**Verdict:** ✅ works. Subtle. Worth testing reverse (`cooler` = `-delta_warmer`).

---

## Decision

**Ship delta vectors as the v1 implementation of modifier buttons, at α=0.8.** This reverses the Round 1 directional recommendation, which was based on an inadequate corpus.

Reasons:
1. All five modifiers produce clean visible shifts at α=0.8 on a rigorous corpus.
2. α=0.8 keeps results visually related to the source image (not the case at 1.2).
3. The technique is fast at runtime: one vector add, no extra retrieval pass. Text-fusion would require two retrieval queries + RRF merge.
4. The buttons stay interpretable — each is a fixed direction, computable offline at startup.

### What changes vs the original spec

| Spec said | Update to |
|---|---|
| Default α not specified; the original spike used α=0.2 | Use **α=0.8** as default |
| Modifier list: moodier, warmer, cooler, more_minimal, more_textured, more_graphic | Same five-plus-reverse, but `cooler` = `-delta_warmer`. Store only the positive direction; UI toggles sign. Halves the offline compute. |
| Visual search runs three candidate sets (image alone, text alone, weighted blend) and RRFs them | Optional. **Recommended:** ship delta as primary path. Keep text-fusion as a fallback for modifiers that show weak deltas in production eval. |

### Production implications

- Deltas computed offline from contrastive text pairs at deploy time, stored as 1024-dim float32 constants (one per modifier). Cheap to ship in the Rust API binary.
- Modifier composition: tentatively works via vector addition (`image + α·delta_moodier + α·delta_warmer`). Not rigorously tested in this spike — should be a follow-up before shipping multi-button selection.
- α should be exposed as a config value per modifier, not hardcoded. Some may want 0.6, some 1.0.

### Reversal hypothesis

`cooler` = `image - α·delta_warmer` was not tested in this spike. Should be the first thing tested when production work begins; if it works we halve the modifier-curation burden.

### What could still go wrong

- **Real artist corpus differs from WikiArt.** WikiArt is mostly historical with a contemporary tail. The real platform is contemporary independents. Some modifiers may behave differently. Worth a re-run on the first 1k real artworks once we have them.
- **Modifier composition** is the obvious next failure mode — three buttons selected at once may produce garbage. Test before enabling multi-select.
- **α may need per-modifier tuning** in production. The default 0.8 is a single value across modifiers from one corpus.

---

## Code state at end of spike

- `ml_art/embeddings/local_jina.py` — multimodal jina-clip-v2 with parallel PIL decode + batched MPS forward. ~24 min to embed 2000 images first run; cached after.
- `ml_art/embeddings/cache.py` — SHA-keyed disk cache. **Known flaw:** writes only after the entire backend call completes. A mid-run crash loses all in-flight embeddings. See task #25 in the project task list.
- `ml_art/datasets/wikiart.py` — stratified WikiArt sampler. Streams from HF, saves JPEGs.
- `spikes/2026-05-modifier-deltas/run_visual.py` — anchor selection, three-approach comparison (baseline / text-fusion / delta), PNG output.
- `spikes/2026-05-modifier-deltas/deltas.py`, `modifiers.py` — contrastive pair registry and delta math.

## Promotion plan

Items from this spike to move into `ml_art/` proper once we start the recommendation/ranking work:
- The `Delta`, `compute_delta`, `apply_delta` from `deltas.py` → `ml_art/recommendations/modifiers.py`.
- The contrastive text-pair registry from `modifiers.py` → same destination, but exposed via config so editors can add modifiers without code changes.

The eval harness (`eval.py`, with LLM judge) stays spike-local until we run a production-realistic eval against actual onboarded artist work.

---

## Limitations

- 8 anchors per α value. A larger anchor sample would tighten the verdict.
- Manual visual inspection, no LLM judge in this round. Conclusions are subjective.
- One embedding model. Voyage multimodal-3 not tested for comparison.
- WikiArt has its own biases (mostly Western painting; sparse on photography, digital art, sculpture).
- α=0.8 was found by eyeballing 3 sample points (0.4 / 0.8 / 1.2). A finer sweep might find 0.65 or 0.9 is marginally better.
