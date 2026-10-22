import { describe, expect, it } from "vitest";
import { keyTarget } from "./segmentKeys.ts";

const windows = [{ value: "5" }, { value: "10" }, { value: "30" }];
const firmwares = [
  { value: "stock" },
  { value: "ours-compat", disabledReason: "Not built here" },
  { value: "ours-clean" },
];

describe("segmented control keys", () => {
  it("steps left and right and wraps around", () => {
    expect(keyTarget("ArrowRight", "10", windows)).toBe("30");
    expect(keyTarget("ArrowRight", "30", windows)).toBe("5");
    expect(keyTarget("ArrowLeft", "10", windows)).toBe("5");
    expect(keyTarget("ArrowLeft", "5", windows)).toBe("30");
  });

  it("jumps to the ends", () => {
    expect(keyTarget("Home", "30", windows)).toBe("5");
    expect(keyTarget("PageUp", "30", windows)).toBe("5");
    expect(keyTarget("End", "5", windows)).toBe("30");
    expect(keyTarget("PageDown", "5", windows)).toBe("30");
  });

  it("skips disabled options, as the focus does", () => {
    expect(keyTarget("ArrowRight", "stock", firmwares)).toBe("ours-clean");
    expect(keyTarget("ArrowLeft", "ours-clean", firmwares)).toBe("stock");
    expect(keyTarget("ArrowRight", "x", [{ value: "x", disabledReason: "no" }])).toBeNull();
  });

  it("leaves other keys alone (a horizontal group ignores up and down)", () => {
    for (const key of ["ArrowUp", "ArrowDown", "Enter", " ", "a"]) {
      expect(keyTarget(key, "10", windows)).toBeNull();
    }
  });
});
