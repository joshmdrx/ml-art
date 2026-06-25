#!/usr/bin/env bash
#
# T-073 — backfill artworks.medium_category from the legacy free-text
# `medium` column.
#
# Pass 1 — deterministic rules. ILIKE-matched keywords on the existing
# `medium` text cover ~70% of real data without touching an LLM. The
# rules are intentionally narrow: prefer to leave a row NULL than to
# mis-classify. The Claude residue pass (a follow-up; not in this
# script) handles the rest.
#
# Idempotent: every clause is `WHERE medium_category IS NULL AND …`,
# so re-running doesn't overwrite a previous classification (artists
# can self-correct via the studio modal; this script never stomps
# their choice). Safe to run as many times as you want.
#
# Usage:
#   AWS_PROFILE=ml-art scripts/backfill-medium-category.sh           # dry-run (prints SQL, no execute)
#   AWS_PROFILE=ml-art scripts/backfill-medium-category.sh --apply   # actually execute
#
# Stops on error. Reports the count moved per category at the end so
# you can sanity-check the spread before / after.

set -euo pipefail

APPLY=0
if [[ "${1:-}" == "--apply" ]]; then
  APPLY=1
fi

# Resolve the DB URL via the same SSM path the api Lambda uses, so
# this script Just Works against prod when AWS_PROFILE is set.
PROFILE="${AWS_PROFILE:-ml-art}"
REGION="${AWS_REGION:-us-east-1}"
DATABASE_URL="${DATABASE_URL:-$(aws --profile "$PROFILE" --region "$REGION" ssm get-parameter \
  --name /ml-art-prod/database_url --with-decryption \
  --query 'Parameter.Value' --output text)}"

if [[ -z "$DATABASE_URL" ]]; then
  echo "✘ DATABASE_URL is empty — set AWS_PROFILE or pass DATABASE_URL=… explicitly" >&2
  exit 1
fi

run_sql() {
  if [[ "$APPLY" == "1" ]]; then
    psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -c "$1"
  else
    echo "── DRY-RUN ──"
    echo "$1"
    echo
  fi
}

echo "▶ Backfill pass: rules over artworks.medium → artworks.medium_category"
echo "  database = ${DATABASE_URL%%@*}@<redacted>"
echo "  mode     = $([[ "$APPLY" == "1" ]] && echo "APPLY" || echo "DRY-RUN (pass --apply to commit)")"
echo

# Order matters slightly: classify the strongest signals first so a
# row that hits multiple buckets lands in the most specific one.
# `mixed_media` is deliberately last in the rules — by then anything
# left over with "mixed" / "found object" wording is genuinely
# multi-medium.

run_sql "UPDATE artworks SET medium_category='photography'
         WHERE medium_category IS NULL
           AND medium ~* '\\m(photograph|photo|gelatin silver|inkjet print|c-print|cyanotype)\\M';"

run_sql "UPDATE artworks SET medium_category='print'
         WHERE medium_category IS NULL
           AND medium ~* '\\m(etching|screenprint|silkscreen|woodcut|linocut|lithograph|monoprint|monotype|engraving|aquatint|edition of)\\M';"

run_sql "UPDATE artworks SET medium_category='sculpture'
         WHERE medium_category IS NULL
           AND medium ~* '\\m(bronze|marble|cast iron|stoneware sculpture|wood carving|carved|forged steel|welded)\\M';"

run_sql "UPDATE artworks SET medium_category='ceramic'
         WHERE medium_category IS NULL
           AND medium ~* '\\m(porcelain|stoneware|earthenware|glazed|terracotta|raku)\\M';"

run_sql "UPDATE artworks SET medium_category='textile'
         WHERE medium_category IS NULL
           AND medium ~* '\\m(tapestry|weaving|woven|embroider|stitched|quilt|fabric|fibre|fiber|linen thread|wool yarn)\\M';"

run_sql "UPDATE artworks SET medium_category='collage'
         WHERE medium_category IS NULL
           AND medium ~* '\\m(collage|cut paper|paper collage|assemblage)\\M';"

run_sql "UPDATE artworks SET medium_category='digital'
         WHERE medium_category IS NULL
           AND medium ~* '\\m(digital|generative|ai-generated|vector|pixel|cgi|render)\\M';"

run_sql "UPDATE artworks SET medium_category='drawing'
         WHERE medium_category IS NULL
           AND medium ~* '\\m(graphite|charcoal|pencil drawing|conté|pastel drawing|pen and ink|ink drawing)\\M';"

run_sql "UPDATE artworks SET medium_category='painting'
         WHERE medium_category IS NULL
           AND medium ~* '\\m(oil|acrylic|gouache|watercolou?r|tempera|painting|enamel paint|fresco)\\M';"

run_sql "UPDATE artworks SET medium_category='mixed_media'
         WHERE medium_category IS NULL
           AND medium ~* '\\m(mixed media|found object|multi-media|multi media)\\M';"

# WikiArt seed quirk — the seed loader uses art-movement names (style)
# in the `medium` column rather than material descriptions. Mapping
# style → category for the standard WikiArt styles so the seed corpus
# stops dragging the NULL-category count up. Future T-073 work moves
# this into the seed loader (`ml/seed/`) directly so a re-seed produces
# categorised data without needing this pass.
run_sql "UPDATE artworks SET medium_category='print'
         WHERE medium_category IS NULL
           AND medium IN ('Ukiyo E', 'Ukiyo-e');"

run_sql "UPDATE artworks SET medium_category='painting'
         WHERE medium_category IS NULL
           AND medium IN (
               'Realism', 'Rococo', 'Romanticism', 'Symbolism',
               'Abstract Expressionism', 'Art Nouveau', 'Baroque',
               'Cubism', 'Early Renaissance', 'Expressionism',
               'Fauvism', 'High Renaissance', 'Impressionism',
               'Mannerism Late Renaissance', 'Minimalism',
               'Naive Art Primitivism', 'Northern Renaissance',
               'Pointillism', 'Post Impressionism', 'Pop Art',
               'Synthetic Cubism', 'Analytical Cubism',
               'Contemporary Realism', 'New Realism',
               'Action Painting', 'Color Field Painting'
           );"

if [[ "$APPLY" == "1" ]]; then
  echo
  echo "▶ Spread after backfill:"
  psql "$DATABASE_URL" -c \
    "SELECT medium_category, count(*)
     FROM artworks
     WHERE deleted_at IS NULL
     GROUP BY medium_category
     ORDER BY count(*) DESC;"

  echo
  echo "▶ Still-null sample (10 rows). These need a Claude pass or"
  echo "  artist self-correction — they didn't match any rule."
  psql "$DATABASE_URL" -c \
    "SELECT id, title, medium
     FROM artworks
     WHERE medium_category IS NULL
       AND deleted_at IS NULL
     LIMIT 10;"
fi

echo
echo "✔ done. (re-run with --apply to commit if this was a dry-run)"
