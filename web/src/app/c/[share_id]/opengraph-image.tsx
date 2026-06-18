import { ImageResponse } from "next/og";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { getPublicCollection } from "@/lib/api";

// T-053 — OG card for the public collection page (`/c/<share_id>`).
// Mirrors the artist-card layout: name on the left, 2x2 of cover thumbs
// on the right. Bumps the "A collection on Wander" lead so the recipient
// of a shared link knows what they're looking at.

export const runtime = "nodejs";
export const alt = "A collection on Wander";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";
export const revalidate = 86_400;

type Params = Promise<{ share_id: string }>;

const BG = "#FAFAF8";
const FG = "#1A1A1A";
const MUTED = "#6B6B6B";

export default async function Image({ params }: { params: Params }) {
  const { share_id } = await params;

  // See artworks/[id]/opengraph-image.tsx for why this uses fs.readFile
  // and not fetch(new URL(...)).
  const [data, serifRegular, serifItalic] = await Promise.all([
    getPublicCollection(share_id).catch(() => null),
    readFile(
      fileURLToPath(
        new URL("../../og-fonts/InstrumentSerif-Regular.ttf", import.meta.url),
      ),
    ),
    readFile(
      fileURLToPath(
        new URL("../../og-fonts/InstrumentSerif-Italic.ttf", import.meta.url),
      ),
    ),
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

  if (!data) {
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

  const collection = data.collection;
  const thumbs = collection.cover_image_urls.slice(0, 4);
  while (thumbs.length < 4) thumbs.push("");

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
        {/* Left: name + footer */}
        <div
          style={{
            width: 600,
            height: 630,
            padding: "72px 56px 48px 56px",
            display: "flex",
            flexDirection: "column",
            justifyContent: "space-between",
          }}
        >
          <div style={{ display: "flex", flexDirection: "column" }}>
            <div
              style={{
                fontFamily: "Instrument Serif",
                fontSize: 20,
                color: MUTED,
                letterSpacing: "0.08em",
                textTransform: "uppercase",
                marginBottom: 22,
                display: "flex",
              }}
            >
              A collection on Wander
            </div>
            <div
              style={{
                fontFamily: "Instrument Serif Italic",
                fontSize: clampNameSize(collection.name),
                lineHeight: 1.05,
                color: FG,
                letterSpacing: "-0.01em",
                display: "flex",
              }}
            >
              {collection.name}
            </div>
            <div
              style={{
                fontFamily: "Instrument Serif",
                fontSize: 26,
                color: MUTED,
                marginTop: 24,
                display: "flex",
              }}
            >
              {collection.artwork_count}{" "}
              {collection.artwork_count === 1 ? "work" : "works"}
            </div>
          </div>
          <div
            style={{
              fontFamily: "Instrument Serif",
              fontSize: 22,
              color: MUTED,
              display: "flex",
            }}
          >
            wander.gallery
          </div>
        </div>

        {/* Right: 2×2 cover thumbs */}
        <div
          style={{
            width: 600,
            height: 630,
            display: "flex",
            flexWrap: "wrap",
            backgroundColor: FG,
          }}
        >
          {thumbs.map((url, i) => (
            <div
              key={i}
              style={{
                width: 300,
                height: 315,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                backgroundColor: FG,
                borderRight: i % 2 === 0 ? `1px solid ${BG}` : "none",
                borderBottom: i < 2 ? `1px solid ${BG}` : "none",
                overflow: "hidden",
              }}
            >
              {url ? (
                // eslint-disable-next-line @next/next/no-img-element, jsx-a11y/alt-text
                <img
                  src={url}
                  style={{ width: "100%", height: "100%", objectFit: "cover" }}
                />
              ) : null}
            </div>
          ))}
        </div>
      </div>
    ),
    { ...size, fonts },
  );
}

function clampNameSize(name: string): number {
  const len = name.length;
  if (len <= 16) return 88;
  if (len <= 24) return 72;
  if (len <= 36) return 60;
  return 48;
}
