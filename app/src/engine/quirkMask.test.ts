import { describe, expect, it } from "vitest";
import { QUIRKS, type Quirk } from "./quirks.ts";
import {
  customFirmware,
  describeQuirks,
  normalizeQuirks,
  profileQuirks,
  quirksOf,
  sameQuirks,
  setQuirk,
  splitFirmware,
} from "./quirkMask.ts";

const quirk = (id: string, compat: boolean, clean: boolean): Quirk => ({
  id,
  group: "g",
  compat,
  clean,
  stockRef: "",
  summary: "",
});
// The same three quirks as the engine's own test (crates/euc-sim/src/sil/mod.rs).
const LIST = [
  quirk("ASIN_ACCEL", true, false),
  quirk("NO_WATCHDOG", true, false),
  quirk("WC_DEAD", true, true),
];

describe("quirk masks", () => {
  it("names a mask from the profile it is closest to, as the engine does", () => {
    expect(describeQuirks(["ASIN_ACCEL", "NO_WATCHDOG", "WC_DEAD"], LIST)).toBe("compat");
    expect(describeQuirks(["WC_DEAD"], LIST)).toBe("clean");
    expect(describeQuirks([], LIST)).toBe("none");
    // A tie goes to the first profile, as in SilLibrary::describe.
    expect(describeQuirks(["ASIN_ACCEL", "WC_DEAD"], LIST)).toBe("compat-NO_WATCHDOG");
    expect(describeQuirks(["ASIN_ACCEL"], LIST)).toBe("none+ASIN_ACCEL");
  });

  it("keeps known quirks once, in the file's order", () => {
    expect(normalizeQuirks(["WC_DEAD", "NOPE", "ASIN_ACCEL", "WC_DEAD"], LIST)).toEqual([
      "ASIN_ACCEL",
      "WC_DEAD",
    ]);
    expect(setQuirk(["WC_DEAD"], "ASIN_ACCEL", true, LIST)).toEqual(["ASIN_ACCEL", "WC_DEAD"]);
    expect(setQuirk(["ASIN_ACCEL", "WC_DEAD"], "ASIN_ACCEL", false, LIST)).toEqual(["WC_DEAD"]);
    expect(profileQuirks("clean", LIST)).toEqual(["WC_DEAD"]);
  });

  it("reads the firmware's own list", () => {
    const compat = profileQuirks("compat");
    const clean = profileQuirks("clean");
    expect(compat.length).toBe(QUIRKS.filter((q) => q.compat).length);
    expect(clean).toContain("WC_DEAD");
    expect(describeQuirks(compat)).toBe("compat");
    expect(describeQuirks(clean)).toBe("clean");
    expect(sameQuirks([...compat].reverse(), compat)).toBe(true);
    expect(sameQuirks(compat, clean)).toBe(false);
  });

  it("says which quirks each firmware keeps", () => {
    expect(quirksOf("stock", null)).toEqual(QUIRKS.map((q) => q.id));
    expect(quirksOf("ours-compat", null)).toEqual(profileQuirks("compat"));
    expect(quirksOf("ours-clean", null)).toEqual(profileQuirks("clean"));
    expect(quirksOf("ours-custom", ["WC_DEAD", "ASIN_ACCEL"])).toEqual(["ASIN_ACCEL", "WC_DEAD"]);
    expect(quirksOf("ours-custom", null)).toEqual([]);
  });

  it("writes and reads the engine's firmware choice", () => {
    expect(customFirmware(["KEIL_FTZ", "ASIN_ACCEL"])).toBe("ours-custom:ASIN_ACCEL,KEIL_FTZ");
    expect(customFirmware([])).toBe("ours-custom:");
    expect(splitFirmware("ours-custom:ASIN_ACCEL,KEIL_FTZ")).toEqual({
      id: "ours-custom",
      quirks: ["ASIN_ACCEL", "KEIL_FTZ"],
    });
    expect(splitFirmware("ours-custom")).toEqual({ id: "ours-custom", quirks: [] });
    expect(splitFirmware("ours-clean")).toEqual({ id: "ours-clean", quirks: null });
  });
});
