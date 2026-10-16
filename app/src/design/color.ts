// Small, dependency-free color math used by the token generator, the contrast check and the
// gallery. Everything works on opaque sRGB hex colors (#RRGGBB).

export type Rgb = readonly [r: number, g: number, b: number];

export function parseHex(hex: string): Rgb {
  const m = /^#([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m?.[1]) throw new Error(`Expected an opaque #RRGGBB color, got "${hex}"`);
  const n = Number.parseInt(m[1], 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

export function toHex([r, g, b]: Rgb): string {
  const c = (v: number) =>
    Math.round(Math.min(255, Math.max(0, v)))
      .toString(16)
      .padStart(2, "0");
  return `#${c(r)}${c(g)}${c(b)}`.toUpperCase();
}

/** Mixes `amount` (0..1) of `fg` over `bg`, like painting fg at that opacity on bg. */
export function mix(fg: string, bg: string, amount: number): string {
  const a = parseHex(fg);
  const b = parseHex(bg);
  return toHex([
    a[0] * amount + b[0] * (1 - amount),
    a[1] * amount + b[1] * (1 - amount),
    a[2] * amount + b[2] * (1 - amount),
  ]);
}

function channel(v: number): number {
  const s = v / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

/** WCAG 2.x relative luminance. */
export function luminance(hex: string): number {
  const [r, g, b] = parseHex(hex);
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

/** WCAG 2.x contrast ratio, 1..21. */
export function contrast(a: string, b: string): number {
  const la = luminance(a);
  const lb = luminance(b);
  const [hi, lo] = la > lb ? [la, lb] : [lb, la];
  return (hi + 0.05) / (lo + 0.05);
}
