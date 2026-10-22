import { describe, expect, it } from "vitest";
import type { CompareSummary, SeriesDiff } from "../../engine/compareTypes.ts";
import {
  divergenceText,
  formatDiff,
  formatLimit,
  formatNumber,
  formatTime,
  formatValue,
  overallVerdict,
  seriesLabel,
  seriesVerdict,
  summaryRows,
} from "./summaryFormat.ts";

const series = (over: Partial<SeriesDiff>): SeriesDiff => ({
  name: "speed_kmh",
  unit: "km/h",
  max_abs: 0.263512,
  rms: 0.051654,
  at_max_s: 30.92,
  a_at_max: 20.1,
  b_at_max: 19.84,
  tolerance: 0.5,
  rms_tolerance: null,
  first_s: null,
  within: true,
  ...over,
});

const summary = (over: Partial<CompareSummary>): CompareSummary => ({
  done: true,
  samples: 4000,
  duration_s: 40,
  sample_skew_us: 0,
  compared_until_s: null,
  first_divergence: null,
  channels: [],
  vars: [],
  vars_unpaired: [],
  ble: null,
  within_tolerance: true,
  check_disagreements: [],
  ok: true,
  outcome_a: null,
  outcome_b: null,
  ...over,
});

describe("numbers", () => {
  it("keeps three significant digits and marks infinite differences", () => {
    expect(formatNumber(0.263512)).toBe("0.264");
    expect(formatNumber(31.123476)).toBe("31.1");
    expect(formatNumber(1706)).toBe("1,706");
    expect(formatNumber(0)).toBe("0");
    expect(formatNumber(0.000244)).toBe("2.4e-4");
    expect(formatNumber(-0.00014776)).toBe("-1.5e-4");
    // JSON has no infinity: the report writes null for a value only one run has.
    expect(formatNumber(null)).toBe("∞");
    expect(formatNumber(Number.NaN)).toBe("NaN");
  });

  it("adds units, times and limits", () => {
    expect(formatDiff(0.263512, "km/h")).toBe("0.264 km/h");
    expect(formatDiff(9, "")).toBe("9");
    expect(formatTime(2.510000013888889)).toBe("2.51 s");
    expect(formatTime(2.5100001, 3)).toBe("2.510 s");
    expect(formatTime(null)).toBe("—");
    expect(formatLimit(series({}))).toBe("≤ 0.5 km/h");
    expect(
      formatLimit(series({ name: "iq_a", unit: "A", tolerance: null, rms_tolerance: 6 })),
    ).toBe("RMS ≤ 6 A");
    expect(formatLimit(series({ tolerance: null }))).toBe("—");
  });

  it("shortens report values: f32 digits and BLE frames", () => {
    expect(formatValue("-0.00014776184980291873")).toBe("-1.5e-4");
    expect(formatValue("20.47")).toBe("20.5");
    expect(formatValue("none")).toBe("none");
    expect(formatValue("55aa19500000007500000000f1120c80")).toBe("55aa1950000000…");
  });
});

describe("verdicts", () => {
  it("judges one series", () => {
    expect(seriesVerdict(series({}))).toBe("within");
    expect(seriesVerdict(series({ within: false }))).toBe("over");
    expect(seriesVerdict(series({ max_abs: 0, tolerance: null }))).toBe("identical");
    expect(seriesVerdict(series({ tolerance: null }))).toBe("unjudged");
  });

  it("sums the comparison up while it runs and when it is done", () => {
    expect(overallVerdict(null).label).toBe("Not compared yet");
    expect(overallVerdict(summary({ done: false, ok: null })).label).toBe(
      "Within tolerance so far",
    );
    expect(overallVerdict(summary({ done: false, ok: null, within_tolerance: false })).tone).toBe(
      "caution",
    );
    expect(overallVerdict(summary({}))).toEqual({ tone: "ok", label: "Within tolerance" });
    expect(overallVerdict(summary({ ok: false, within_tolerance: false })).label).toBe(
      "Exceeds the tolerance",
    );
    expect(overallVerdict(summary({ ok: false })).label).toBe("The checks disagree");
  });
});

describe("rows and labels", () => {
  it("lists what differs or is judged, and counts the identical rest", () => {
    const rows = summaryRows(
      summary({
        channels: [
          series({}),
          series({ name: "roll_deg", unit: "°", max_abs: 0, tolerance: null }),
          series({ name: "phase_a_a", unit: "A", tolerance: null }),
        ],
        vars: [series({ name: "speed_cms", unit: "", tolerance: null, max_abs: 9 })],
      }),
    );
    expect(rows.rows.map((r) => [r.name, r.group])).toEqual([
      ["speed_kmh", "channel"],
      ["phase_a_a", "channel"],
      ["speed_cms", "var"],
    ]);
    expect(rows.identical).toBe(1);
  });

  it("names channels and divergences", () => {
    expect(seriesLabel("phase_b_a")).toBe("Phase B current");
    expect(seriesLabel("drive_command")).toBe("drive_command");
    expect(
      divergenceText({
        t_s: 2.51,
        kind: "channel",
        name: "phase_b_a",
        a: "-0.000147",
        b: "-0.000141",
      }),
    ).toBe("Phase B current: A -1.5e-4, B -1.4e-4");
    expect(
      divergenceText({
        t_s: 2.0,
        kind: "ble",
        name: "frame 00 byte 12: board temperature",
        a: "55aa",
        b: "none",
      }),
    ).toBe("BLE frame 00 byte 12: board temperature: A 55aa, B none");
  });
});
