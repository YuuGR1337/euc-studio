// Canvas and WebGL need concrete colors, not CSS variables.

/** Resolves `var(--x)` against an element's computed style; other colors pass through. */
export function resolveColor(el: Element, color: string): string {
  const m = /^var\((--[^)]+)\)$/.exec(color);
  return m?.[1] ? getComputedStyle(el).getPropertyValue(m[1]).trim() || "#888888" : color;
}

/** #RRGGBB → rgba() with alpha; other formats pass through unchanged. */
export function withAlpha(color: string, alpha: number): string {
  const m = /^#([0-9a-f]{6})$/i.exec(color);
  if (!m?.[1]) return color;
  const n = Number.parseInt(m[1], 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}
