// Text for the comparison summary: numbers at a precision that shows small differences without
// noise, times, limits and verdicts. The numbers themselves come from euc-sim's compare module
// (or the demo); nothing here judges anything.

import { CHANNELS as TELEMETRY } from "../../engine/telemetry.gen.ts";
import type { CompareSummary, Divergence, SeriesDiff } from "../../engine/compareTypes.ts";

/** A difference or value: 3 significant digits, `∞` for an infinite one (null in the JSON). */
export function formatNumber(value: number | null): string {
  if (value === null || value === Infinity) return "∞";
  if (Number.isNaN(value)) return "NaN";
  if (value === 0) return "0";
  const abs = Math.abs(value);
  if (abs < 1e-3) return value.toExponential(1);
  if (abs >= 1000) return Math.round(value).toLocaleString("en-US");
  return String(Number(value.toPrecision(3)));
}

/** A difference with its unit ("0.26 km/h"). */
export function formatDiff(value: number | null, unit: string): string {
  const n = formatNumber(value);
  return unit ? `${n} ${unit}` : n;
}

/** A simulated time: "2.51 s", or "—" for never. */
export function formatTime(seconds: number | null, decimals = 2): string {
  return seconds === null ? "—" : `${seconds.toFixed(decimals)} s`;
}

/** The tolerance a series is judged by: "≤ 0.5 km/h", "RMS ≤ 6 A", or "—" when not judged. */
export function formatLimit(s: SeriesDiff): string {
  const unit = s.unit ? ` ${s.unit}` : "";
  if (s.tolerance !== null) return `≤ ${formatNumber(s.tolerance)}${unit}`;
  if (s.rms_tolerance !== null) return `RMS ≤ ${formatNumber(s.rms_tolerance)}${unit}`;
  return "—";
}

export type SeriesVerdict = "identical" | "within" | "over" | "unjudged";

export function seriesVerdict(s: SeriesDiff): SeriesVerdict {
  if (!s.within) return "over";
  if (s.max_abs === 0) return "identical";
  return s.tolerance !== null || s.rms_tolerance !== null ? "within" : "unjudged";
}

export const VERDICT_LABEL: Record<SeriesVerdict, string> = {
  identical: "Identical",
  within: "Within",
  over: "Over",
  unjudged: "Not judged",
};

/** The telemetry channel's name and unit, or the variable's own name. */
export function seriesLabel(name: string): string {
  return TELEMETRY.find((c) => c.key === name)?.label ?? name;
}

export interface OverallVerdict {
  tone: "ok" | "danger" | "caution" | "neutral";
  label: string;
}

/** The whole comparison in a few words, while it runs and once it is done. */
export function overallVerdict(summary: CompareSummary | null): OverallVerdict {
  if (!summary) return { tone: "neutral", label: "Not compared yet" };
  if (!summary.done) {
    return summary.within_tolerance
      ? { tone: "neutral", label: "Within tolerance so far" }
      : { tone: "caution", label: "Over tolerance so far" };
  }
  if (summary.ok) return { tone: "ok", label: "Within tolerance" };
  return summary.within_tolerance
    ? { tone: "danger", label: "The checks disagree" }
    : { tone: "danger", label: "Exceeds the tolerance" };
}

/** A value as the report gives it (every digit of an f32, or a frame in hex), shortened. */
export function formatValue(text: string): string {
  if (text === "none") return "none";
  const n = Number(text);
  if (text !== "" && Number.isFinite(n)) return formatNumber(n);
  return text.length > 16 ? `${text.slice(0, 14)}…` : text;
}

/** "Phase B current: A -1.5e-4, B -1.4e-4" and the like, for the first divergence. */
export function divergenceText(d: Divergence): string {
  const what = d.kind === "ble" ? `BLE ${d.name}` : d.kind === "var" ? d.name : seriesLabel(d.name);
  return `${what}: A ${formatValue(d.a)}, B ${formatValue(d.b)}`;
}

export interface SummaryRows {
  /** Series that differ or are judged, channels first, then variables. */
  rows: (SeriesDiff & { group: "channel" | "var" })[];
  /** Series that are identical and not judged (listed as a count). */
  identical: number;
}

/** What the table lists: like `euc sim compare`, only series that differ or are judged. */
export function summaryRows(summary: CompareSummary): SummaryRows {
  const shown = (s: SeriesDiff) =>
    s.max_abs !== 0 || s.tolerance !== null || s.rms_tolerance !== null;
  const all = [
    ...summary.channels.map((s) => ({ ...s, group: "channel" as const })),
    ...summary.vars.map((s) => ({ ...s, group: "var" as const })),
  ];
  const rows = all.filter(shown);
  return { rows, identical: all.length - rows.length };
}
