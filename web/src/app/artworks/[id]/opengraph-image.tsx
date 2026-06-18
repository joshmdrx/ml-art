import { ImageResponse } from "next/og";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { getArtwork } from "@/lib/api";

// T-051 — per-artwork OG card.
//
// Renders a 1200x630 PNG composed at request time when a social-media
// bot fetches the artwork page. The card pulls the primary image and
// overlays the title + artist byline in Instrument Serif so the share
// preview reads as a gallery card, not a generic site card.
//
// Spike risk: `next/og` (Satori + Resvg WASM) under OpenNext on Lambda.
// If this route 500s in prod we fall back to the precompute-via-Pillow
// path described in `TODO.md` T-051.

export const runtime = "nodejs";
export const alt = "Artwork on Wander";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";

// Cache hint for CloudFront. 1 day; OG cards are eventually-consistent.
// Social platforms re-crawl periodically.
export const revalidate = 86_400;

type Params = Promise<{ id: string }>;

const BG = "#FAFAF8";
const FG = "#1A1A1A";
const MUTED = "#6B6B6B";

export default async function Image({ params }: { params: Params }) {
  const { id } = await params;

  // Load fonts + artwork in parallel.
  //
  // Use `fs.readFile` (not `fetch`) because Vercel's edge runtime
  // supports `fetch('file://…')` but vanilla Node (which is what
  // OpenNext bundles into the Lambda) doesn't — undici throws "not
  // implemented... yet..." on the file: scheme. Turbopack hashes
  // these TTFs into `.next/server/assets/`; `new URL(…, import.meta.url)`
  // resolves to that path at runtime.
  const [artwork, serifRegular, serifItalic] = await Promise.all([
    getArtwork(id).catch(() => null),
    readFile(fileURLToPath(new URL("../../og-fonts/InstrumentSerif-Regular.ttf", import.meta.url))),
    readFile(fileURLToPath(new URL("../../og-fonts/InstrumentSerif-Italic.ttf", import.meta.url))),
  ]);

  const fonts = [
    {
      name: "Instrument Serif",
      data: serifRegular,
      style: "normal" as const,
      weight: 400 as const,
    },
    {
      name: "Instrument Serif Italic",
      data: serifItalic,
      style: "italic" as const,
      weight: 400 as const,
    },
  ];

  // Fallback card when the artwork isn't found (deleted, unpublished,
  // bad id). Renders something credible rather than a broken card.
  if (!artwork) {
    return new ImageResponse(
      (
        <div
          style={{
            width: "100%",
            height: "100%",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            backgroundColor: BG,
            fontFamily: "Instrument Serif Italic",
            fontSize: 120,
            color: FG,
          }}
        >
          Wander
        </div>
      ),
      { ...size, fonts },
    );
  }

  const primary =
    artwork.images.find((i) => i.is_primary) ?? artwork.images[0] ?? null;
  const title = artwork.title ?? "Untitled";
  const artistName = artwork.artist.display_name;

  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          backgroundColor: BG,
        }}
      >
        {/* Left: artwork on dark backdrop so off-square images sit cleanly. */}
        <div
          style={{
            width: 630,
            height: 630,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            backgroundColor: FG,
          }}
        >
          {primary ? (
            // eslint-disable-next-line @next/next/no-img-element, jsx-a11y/alt-text
            <img
              src={primary.url}
              style={{
                maxWidth: "100%",
                maxHeight: "100%",
                objectFit: "contain",
              }}
            />
          ) : (
            <div
              style={{
                display: "flex",
                fontFamily: "Instrument Serif Italic",
                fontSize: 120,
                color: BG,
              }}
            >
              W
            </div>
          )}
        </div>

        {/* Right: title + byline + domain footer. */}
        <div
          style={{
            flex: 1,
            height: "100%",
            padding: "72px 56px 48px 56px",
            display: "flex",
            flexDirection: "column",
            justifyContent: "space-between",
          }}
        >
          <div
            style={{
              display: "flex",
              flexDirection: "column",
            }}
          >
            <div
              style={{
                fontFamily: "Instrument Serif Italic",
                fontSize: clampTitleSize(title),
                lineHeight: 1.05,
                color: FG,
                letterSpacing: "-0.01em",
                marginBottom: 28,
                // Satori needs explicit display on text blocks.
                display: "flex",
              }}
            >
              {title}
            </div>
            <div
              style={{
                fontFamily: "Instrument Serif",
                fontSize: 32,
                color: MUTED,
                display: "flex",
              }}
            >
              {artistName}
            </div>
          </div>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              fontFamily: "Instrument Serif",
              fontSize: 22,
              color: MUTED,
            }}
          >
            <div style={{ display: "flex" }}>wander.gallery</div>
          </div>
        </div>
      </div>
    ),
    { ...size, fonts },
  );
}

// Title can be long; step down the size so a 60-char title doesn't blow
// the box. Picked by eye against the 510px text column width.
function clampTitleSize(title: string): number {
  const len = title.length;
  if (len <= 18) return 88;
  if (len <= 32) return 72;
  if (len <= 48) return 60;
  return 48;
}
