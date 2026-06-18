import { ImageResponse } from "next/og";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { getArtist } from "@/lib/api";

// T-051 — per-artist OG card.
//
// 1200x630 split: name + city on the left, 2x2 grid of representative
// works on the right. Uses `ArtistFull.representative_image_urls` which
// the API already provides.

export const runtime = "nodejs";
export const alt = "Artist on Wander";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";
export const revalidate = 86_400;

type Params = Promise<{ slug: string }>;

const BG = "#FAFAF8";
const FG = "#1A1A1A";
const MUTED = "#6B6B6B";

export default async function Image({ params }: { params: Params }) {
  const { slug } = await params;

  // See the artwork OG route for why this uses `fs.readFile` instead
  // of `fetch(new URL(…))`.
  const [data, serifRegular, serifItalic] = await Promise.all([
    getArtist(slug).catch(() => null),
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

  const artist = data.artist;
  // Up to 4 representative thumbs; pad if fewer.
  const thumbs = artist.representative_image_urls.slice(0, 4);
  while (thumbs.length < 4) thumbs.push("");

  const city = artist.city ?? null;
  const country = artist.country ?? null;
  const location =
    city && country ? `${city}, ${country}` : (city ?? country ?? null);

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
        {/* Left: name + location + footer */}
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
                fontFamily: "Instrument Serif Italic",
                fontSize: clampNameSize(artist.display_name),
                lineHeight: 1.05,
                color: FG,
                letterSpacing: "-0.01em",
                marginBottom: 24,
                display: "flex",
              }}
            >
              {artist.display_name}
            </div>
            {location && (
              <div
                style={{
                  fontFamily: "Instrument Serif",
                  fontSize: 28,
                  color: MUTED,
                  display: "flex",
                }}
              >
                {location}
              </div>
            )}
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

        {/* Right: 2x2 thumb grid */}
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
                  style={{
                    width: "100%",
                    height: "100%",
                    objectFit: "cover",
                  }}
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
  if (len <= 16) return 96;
  if (len <= 24) return 80;
  if (len <= 36) return 64;
  return 52;
}
