import { describe, expect, it } from "vitest";
import { formatClock, formatNumber, throttleDelay, toSlots } from "./rolling.ts";

describe("toSlots", () => {
  it("keys slots from the right so units keep their slot when the width changes", () => {
    const a = toSlots("9.8");
    const b = toSlots("10.2");
    expect(a.map((s) => s.key)).toEqual(["p2", "p1", "p0"]);
    expect(b.map((s) => s.key)).toEqual(["p3", "p2", "p1", "p0"]);
    // the tenths digit is p0 in both
    expect(a.at(-1)?.char).toBe("8");
    expect(b.at(-1)?.char).toBe("2");
  });

  it("marks digits", () => {
    expect(toSlots("1:2.").map((s) => s.digit)).toEqual([true, false, true, false]);
  });
});

describe("formatNumber", () => {
  it("formats decimals, padding and signs", () => {
    expect(formatNumber(3.14159, { decimals: 2 })).toBe("3.14");
    expect(formatNumber(7, { minIntegerDigits: 3 })).toBe("007");
    expect(formatNumber(-2.5, { decimals: 1 })).toBe("−2.5");
    expect(formatNumber(2.5, { decimals: 1, signed: true })).toBe("+2.5");
    expect(formatNumber(-0.04, { decimals: 1 })).toBe("0.0");
    expect(formatNumber(Number.NaN)).toBe("–");
  });
});

describe("formatClock", () => {
  it("formats mm:ss.t", () => {
    expect(formatClock(0)).toBe("00:00.0");
    expect(formatClock(12.34)).toBe("00:12.3");
    expect(formatClock(125.06)).toBe("02:05.1");
    expect(formatClock(59.97)).toBe("01:00.0");
  });
});

describe("throttleDelay", () => {
  it("allows at most N updates per second", () => {
    expect(throttleDelay(0, 200, 8)).toBe(0);
    expect(throttleDelay(1000, 1050, 8)).toBe(75);
    expect(throttleDelay(1000, 1125, 8)).toBe(0);
  });
});
