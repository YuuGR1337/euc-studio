// Pure helpers for RollingNumber, kept apart from React so they can be unit tested.

export interface Slot {
  /** Stable key: position counted from the right, so units stay in the same slot. */
  key: string;
  char: string;
  digit: boolean;
}

/** Splits text into per-character slots keyed from the right end. */
export function toSlots(text: string): Slot[] {
  const chars = Array.from(text);
  return chars.map((char, i) => ({
    key: `p${chars.length - 1 - i}`,
    char,
    digit: char >= "0" && char <= "9",
  }));
}

export function formatNumber(
  value: number,
  {
    decimals = 0,
    minIntegerDigits = 1,
    signed = false,
  }: {
    decimals?: number;
    minIntegerDigits?: number;
    signed?: boolean;
  } = {},
): string {
  if (!Number.isFinite(value)) return "–";
  const fixed = Math.abs(value).toFixed(decimals);
  const [int = "0", frac] = fixed.split(".");
  const body = int.padStart(minIntegerDigits, "0") + (frac !== undefined ? `.${frac}` : "");
  const isZero = Number(fixed) === 0;
  const sign = value < 0 && !isZero ? "−" : signed && !isZero ? "+" : "";
  return sign + body;
}

/** mm:ss.t sim clock. */
export function formatClock(seconds: number): string {
  // Round to tenths first so 59.97 s becomes 01:00.0, not 00:60.0.
  const tenths = Math.round(Math.max(0, seconds) * 10);
  const m = Math.floor(tenths / 600);
  const rest = (tenths - m * 600) / 10;
  return `${String(m).padStart(2, "0")}:${rest.toFixed(1).padStart(4, "0")}`;
}

/**
 * Limits how often a value may change on screen. Returns the delay (ms) until `value` may be
 * shown, or 0 when it can be shown now.
 */
export function throttleDelay(lastShownAt: number, now: number, maxPerSecond: number): number {
  const gap = 1000 / maxPerSecond;
  const wait = lastShownAt + gap - now;
  return wait > 0 ? wait : 0;
}
