/**
 * Loose price parser for artist-facing inputs.
 *
 * Artists won't type "12000" + currency code "USD" in two separate
 * fields — they'll type "$120", "£4,500", "120.50", "EUR 12000".
 * This helper accepts the messy human form and returns the clean
 * `{ amount_minor, currency }` shape the server expects.
 *
 * Conventions:
 *   - Currency symbols are recognised at either end ($, £, €, ¥) and
 *     map to their canonical ISO 4217 code if no explicit code was
 *     present. A literal code (`USD`, `gbp`) wins over a symbol.
 *   - Thousands separators are stripped (commas in en, spaces in fr,
 *     dots in de — we accept all three for input but always emit
 *     dots for the decimal on the way back out).
 *   - Decimal places are parsed as the currency's minor-unit count
 *     (USD/EUR/GBP/etc = 2; JPY/KRW = 0). Inputs with more decimals
 *     than the currency allows are an error; fewer get zero-padded.
 *   - When the caller can't infer the currency from the input, the
 *     `fallback` argument is used (defaults to "USD").
 *
 * Returns `null` for empty / whitespace-only input. Throws (via
 * `Error`) on shapes we can't make sense of — caller catches and
 * surfaces a form-validation message.
 */

/** ISO 4217 minor-unit fractional digits for the currencies we
 * realistically see. Defaults to 2 for anything not listed. */
const MINOR_UNITS: Record<string, number> = {
  USD: 2,
  EUR: 2,
  GBP: 2,
  CAD: 2,
  AUD: 2,
  NZD: 2,
  CHF: 2,
  SEK: 2,
  NOK: 2,
  DKK: 2,
  PLN: 2,
  CZK: 2,
  HUF: 2,
  MXN: 2,
  BRL: 2,
  INR: 2,
  SGD: 2,
  HKD: 2,
  CNY: 2,
  // Zero-decimal currencies — common ones only.
  JPY: 0,
  KRW: 0,
  ISK: 0,
  CLP: 0,
};

/** Recognised currency symbols → ISO code. £/€/$ are common; ¥ is
 * ambiguous (JPY vs CNY) — we map to JPY by convention since "¥"
 * unqualified usually means yen. Artists who mean RMB should type
 * `CNY 120`. */
const SYMBOL_TO_CODE: Record<string, string> = {
  $: "USD",
  "£": "GBP",
  "€": "EUR",
  "¥": "JPY",
};

export interface ParsedPrice {
  amount_minor: number;
  currency: string;
}

export function parsePrice(
  input: string,
  fallback: string = "USD"
): ParsedPrice | null {
  const raw = input.trim();
  if (raw.length === 0) return null;

  // Pull out an explicit 3-letter currency code if present (case
  // insensitive, with optional surrounding whitespace).
  let currency: string | null = null;
  let rest = raw;
  const codeMatch = rest.match(/\b([A-Za-z]{3})\b/);
  if (codeMatch) {
    const candidate = codeMatch[1].toUpperCase();
    if (candidate in MINOR_UNITS || /^[A-Z]{3}$/.test(candidate)) {
      currency = candidate;
      rest = rest.replace(codeMatch[0], "").trim();
    }
  }

  // Always strip recognised currency symbols from `rest`, regardless
  // of whether we found a code above. An input like `EUR $100` means
  // "EUR 100"; the `$` is a typo we ignore rather than a second
  // currency. Without this, `$` survives into the number parser and
  // poisons the integer match.
  for (const [sym, code] of Object.entries(SYMBOL_TO_CODE)) {
    if (rest.includes(sym)) {
      if (!currency) currency = code;
      rest = rest.split(sym).join("").trim();
    }
  }

  if (!currency) currency = fallback.toUpperCase();
  const minor = MINOR_UNITS[currency] ?? 2;

  // Now `rest` should be a number with optional thousands separators
  // and an optional decimal.
  rest = rest.replace(/\s+/g, "");
  if (rest.length === 0) {
    throw new Error("missing amount");
  }

  // Zero-decimal currencies (JPY etc) genuinely can't have a `.` —
  // surface it explicitly rather than silently treat as thousands.
  if (minor === 0 && rest.includes(".")) {
    throw new Error(`${currency} doesn't allow decimal places`);
  }

  // Decimal vs thousands disambiguation:
  //   - Both `.` and `,` present → the LAST-occurring one is decimal,
  //     the other is thousands. (Handles `1,234.50` and `1.234,50`.)
  //   - Single separator + it's followed by ≤ minor digits → decimal.
  //   - Otherwise → all separators are thousands; integer amount.
  //
  // Concrete cases this gets right:
  //   £1,200       → comma followed by 3 digits, minor=2 → thousands → 120000
  //   £120.50      → dot followed by 2 digits, minor=2 → decimal → 12050
  //   €120,50      → comma followed by 2 digits, minor=2 → decimal → 12050
  //   £1.234,50    → both present, comma last → comma decimal → 123450
  //   $1.5         → dot followed by 1 digit, minor=2 → decimal (pad) → 150
  const lastDot = rest.lastIndexOf(".");
  const lastComma = rest.lastIndexOf(",");
  const hasDot = lastDot > -1;
  const hasComma = lastComma > -1;

  let decimalSepIdx = -1;
  if (hasDot && hasComma) {
    decimalSepIdx = Math.max(lastDot, lastComma);
  } else if (hasDot || hasComma) {
    const idx = hasDot ? lastDot : lastComma;
    const afterLen = rest.length - idx - 1;
    if (afterLen >= 1 && afterLen <= minor) {
      decimalSepIdx = idx;
    }
  }

  let integerPart: string;
  let fractionPart: string;
  if (decimalSepIdx > -1) {
    integerPart = rest.slice(0, decimalSepIdx).replace(/[.,]/g, "");
    fractionPart = rest.slice(decimalSepIdx + 1);
  } else {
    integerPart = rest.replace(/[.,]/g, "");
    fractionPart = "";
  }

  if (!/^\d+$/.test(integerPart)) {
    throw new Error(`couldn't parse integer part: ${input}`);
  }
  if (fractionPart && !/^\d+$/.test(fractionPart)) {
    throw new Error(`couldn't parse fraction part: ${input}`);
  }
  if (fractionPart.length > minor) {
    throw new Error(
      `${currency} doesn't allow ${fractionPart.length} decimal places`
    );
  }
  // Zero-pad fraction to the currency's minor-unit count.
  const padded = fractionPart.padEnd(minor, "0");
  const combined = integerPart + padded;
  // `Number.parseInt` strips any leading zeros which is what we want
  // for storage but check for overflow against a sensible cap. ~£10M
  // is the highest plausible single artwork price; we cap at
  // 1B-minor-units to leave room without overflowing i64 on the
  // server side.
  const amount_minor = Number.parseInt(combined, 10);
  if (!Number.isFinite(amount_minor) || amount_minor > 1_000_000_000) {
    throw new Error("amount too large");
  }
  return { amount_minor, currency };
}

/** Inverse of `parsePrice` — pretty-print a stored minor-units
 * integer back into the input field. Always uses dot as the decimal
 * separator (matches what the form sends back when re-submitted). */
export function formatPriceForInput(
  amount_minor: number,
  currency: string
): string {
  const minor = MINOR_UNITS[currency.toUpperCase()] ?? 2;
  if (minor === 0) return String(amount_minor);
  const padded = String(amount_minor).padStart(minor + 1, "0");
  const cut = padded.length - minor;
  return `${padded.slice(0, cut)}.${padded.slice(cut)}`;
}

/** Returns the canonical minor-unit count for a currency. Exposed so
 * the form can hint the input's `inputMode` / placeholder. */
export function minorUnitsFor(currency: string): number {
  return MINOR_UNITS[currency.toUpperCase()] ?? 2;
}
