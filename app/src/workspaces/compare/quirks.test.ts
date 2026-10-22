import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { hasQuirk, parseQuirks } from "./quirks.ts";

const QUIRKS = parseQuirks(
  readFileSync(new URL("../../../../firmware/config/quirks.def", import.meta.url), "utf8"),
);

describe("quirks.def", () => {
  it("parses entries across lines, with escaped quotes, under their headings", () => {
    const text = String.raw`/*
 * Header comment with EUC_QUIRK_DEF(ID, COMPAT, CLEAN, STOCK_REF, SUMMARY) in it.
 */

/* Attitude estimation and numerics */
EUC_QUIRK_DEF(ASIN_ACCEL, 1, 0, "0x080037A8 ahrs_update",
              "Each axis goes through asin().")

/* Commands and telemetry (USART1, BLE module) */
EUC_QUIRK_DEF(DEFERRED_TILTBACK_OFF, 1, 0, "0x08007498 cmd_tiltback_off",
              "'\"' sent while moving stays pending.")
EUC_QUIRK_DEF(DEBUG_PRINTF, 0, 0, "printf on USART1", "Stock prints debug text.")
`;
    const quirks = parseQuirks(text);
    expect(quirks.map((q) => [q.id, q.group, q.compat, q.clean])).toEqual([
      ["ASIN_ACCEL", "Attitude estimation and numerics", true, false],
      ["DEFERRED_TILTBACK_OFF", "Commands and telemetry (USART1, BLE module)", true, false],
      ["DEBUG_PRINTF", "Commands and telemetry (USART1, BLE module)", false, false],
    ]);
    expect(quirks[1]?.summary).toBe(`'"' sent while moving stays pending.`);
    expect(quirks[0]?.stockRef).toBe("0x080037A8 ahrs_update");
  });

  it("reads the firmware's own file", () => {
    expect(QUIRKS.length).toBeGreaterThan(30);
    const ids = QUIRKS.map((q) => q.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(ids).toContain("KEIL_FTZ");
    expect(QUIRKS.every((q) => q.group && q.summary)).toBe(true);
    // WC_DEAD is the one clean keeps on purpose.
    expect(QUIRKS.find((q) => q.id === "WC_DEAD")).toMatchObject({ compat: true, clean: true });
  });

  it("says which firmware has which behaviour", () => {
    const ftz = QUIRKS.find((q) => q.id === "KEIL_FTZ");
    if (!ftz) throw new Error("KEIL_FTZ is listed");
    expect(hasQuirk("stock", ftz)).toBe(true);
    expect(hasQuirk("ours-compat", ftz)).toBe(true);
    expect(hasQuirk("ours-clean", ftz)).toBe(false);
    expect(hasQuirk("file", ftz)).toBeNull();
  });
});
