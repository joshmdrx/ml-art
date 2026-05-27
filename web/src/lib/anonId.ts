/**
 * Anonymous-identity signing helpers.
 *
 * The cookie value is `<uuid>.<base64url(hmac_sha256(uuid, secret))>`.
 * Splitting on `.` gives us both halves; the unsigned UUID is what we
 * forward to the Rust API (the secret never leaves the Next.js process).
 *
 * Uses Web Crypto only — no Node-only modules — so this is safe to use
 * from middleware (Edge runtime) and from server components alike.
 */

const COOKIE_NAME = "anon_id";

function getSecret(): string {
  const secret = process.env.ANON_COOKIE_SECRET;
  if (!secret) {
    // Local-dev fallback. The Rust side has the same dev default, so
    // signatures round-trip. Production deploys must set this to a real
    // secret via env.
    return "dev-secret-rotate-in-prod";
  }
  return secret;
}

function toBase64Url(bytes: ArrayBuffer): string {
  // Edge runtime supports Buffer in newer Next versions, but Web-Crypto
  // primitives are the safer surface.
  let bin = "";
  const view = new Uint8Array(bytes);
  for (let i = 0; i < view.length; i++) bin += String.fromCharCode(view[i]);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

async function hmac(value: string): Promise<string> {
  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey(
    "raw",
    enc.encode(getSecret()),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"]
  );
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode(value));
  return toBase64Url(sig);
}

/** Build a cookie value `<uuid>.<sig>`. */
export async function signAnonId(uuid: string): Promise<string> {
  const sig = await hmac(uuid);
  return `${uuid}.${sig}`;
}

/**
 * Verify a cookie value. Returns the unsigned UUID on success; `null` on
 * tampered / unsigned / malformed values.
 *
 * Constant-time comparison via stringification is acceptable here — the
 * signature is HMAC, so attackers can't guess incrementally to bias
 * timing.
 */
export async function verifyAnonId(cookieValue: string): Promise<string | null> {
  const dot = cookieValue.indexOf(".");
  if (dot < 0) return null;
  const uuid = cookieValue.slice(0, dot);
  const sig = cookieValue.slice(dot + 1);
  if (!isUuid(uuid) || !sig) return null;
  const expected = await hmac(uuid);
  if (!timingSafeEqual(sig, expected)) return null;
  return uuid;
}

function isUuid(s: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
    s
  );
}

function timingSafeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) {
    diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  }
  return diff === 0;
}

/**
 * Generate a UUID v7 (time-ordered). Crypto-strong random with timestamp prefix.
 *
 * Avoids BigInt so it compiles under the project's `target: ES2017` tsconfig.
 * `Date.now()` returns at most a 41-bit integer until year 2255 — well inside
 * the 53-bit safe-integer range — so we can split it into 16-bit high and
 * 32-bit low halves using ordinary number arithmetic.
 */
export function generateUuidV7(): string {
  const ms = Date.now();
  const high = Math.floor(ms / 0x100000000) & 0xffff; // top 16 bits of the 48-bit ms
  const low = ms >>> 0; // bottom 32 bits

  const rand = crypto.getRandomValues(new Uint8Array(10));
  const bytes = new Uint8Array(16);
  // 48-bit timestamp
  bytes[0] = (high >>> 8) & 0xff;
  bytes[1] = high & 0xff;
  bytes[2] = (low >>> 24) & 0xff;
  bytes[3] = (low >>> 16) & 0xff;
  bytes[4] = (low >>> 8) & 0xff;
  bytes[5] = low & 0xff;
  // 4-bit version (7) || 12-bit rand
  bytes[6] = 0x70 | (rand[0] & 0x0f);
  bytes[7] = rand[1];
  // 2-bit RFC 4122 variant (10) || 62-bit rand
  bytes[8] = 0x80 | (rand[2] & 0x3f);
  bytes[9] = rand[3];
  bytes[10] = rand[4];
  bytes[11] = rand[5];
  bytes[12] = rand[6];
  bytes[13] = rand[7];
  bytes[14] = rand[8];
  bytes[15] = rand[9];
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join(
    ""
  );
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20, 32)}`;
}

export const ANON_COOKIE_NAME = COOKIE_NAME;
export const ANON_COOKIE_MAX_AGE_SECONDS = 60 * 60 * 24 * 365; // 1 year
