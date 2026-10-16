import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { renderTokensCss, springToLinear } from "./lib/tokens-css.ts";
import { contrast, mix } from "../src/design/color.ts";
import { MIN_RATIO, accents, contrastPairs, spacing, surface } from "../src/design/tokens.ts";

describe("token generation", () => {
  const css = renderTokensCss();

  it("is deterministic", () => {
    expect(renderTokensCss()).toBe(css);
  });

  it("matches the committed tokens.css", () => {
    const committed = readFileSync(new URL("../src/design/tokens.css", import.meta.url), "utf8");
    expect(committed).toBe(css);
  });

  it("replaces Tailwind's spacing scale with the token values only", () => {
    expect(css).toContain("--spacing: initial;");
    expect(css).toContain("--spacing-*: initial;");
    for (const v of spacing) expect(css).toContain(`--spacing-${v}: ${v}px;`);
    expect(css).not.toMatch(/--spacing-(1|3|5|6|12)\b/);
  });

  it("emits every surface and an override block per alternate accent", () => {
    for (const [k, v] of Object.entries(surface)) expect(css).toContain(`--color-${k}: ${v};`);
    for (const name of Object.keys(accents).filter((n) => n !== "orange")) {
      expect(css).toContain(`[data-accent="${name}"]`);
    }
  });

  it("turns springs into CSS linear() curves that start at 0 and end at 1", () => {
    const curve = springToLinear(0.3, 0.25);
    const values = curve.slice("linear(".length, -1).split(", ").map(Number);
    expect(values[0]).toBe(0);
    expect(values.at(-1)).toBe(1);
    expect(Math.max(...values)).toBeGreaterThan(1); // a bouncy spring overshoots
    const smooth = springToLinear(0.55, 0).slice(7, -1).split(", ").map(Number);
    expect(Math.max(...smooth)).toBeLessThanOrEqual(1); // bounce 0 never overshoots
  });
});

describe("color math", () => {
  it("computes WCAG contrast", () => {
    expect(contrast("#000000", "#FFFFFF")).toBeCloseTo(21, 5);
    expect(contrast("#777777", "#FFFFFF")).toBeCloseTo(4.48, 2);
  });

  it("mixes colors", () => {
    expect(mix("#FFFFFF", "#000000", 0.5)).toBe("#808080");
  });

  it("every declared pair passes WCAG AA", () => {
    for (const p of contrastPairs) {
      expect(contrast(p.fg, p.bg), `${p.fgName} on ${p.bgName}`).toBeGreaterThanOrEqual(
        MIN_RATIO[p.kind],
      );
    }
  });
});
