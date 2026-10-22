import type { TuneSnapshot } from "../../engine/tune.ts";

export interface ProbePoint {
  t: number;
  value: number | null;
}

export type ScopeHistory = Record<string, ProbePoint[]>;
export type TriggerMode = "free" | "rising" | "falling";
export interface TriggerConfig {
  mode: TriggerMode;
  source: string;
  threshold: number | null;
  /** Simulation seconds after the first sampled crossing, from zero through 15. */
  postSeconds: number;
}

export const CAPTURE_MAX_SAMPLES = 240;

export interface Capture {
  key: string;
  session: number;
  phase: "free" | "armed" | "acquiring" | "held";
  at: number | null;
  end: number | null;
  lastTime: number;
  reason: "complete" | "manual" | "sample_limit" | "run_ended" | null;
  automatic: boolean;
  history: ScopeHistory;
  baseline: ProbePoint | null;
}

export function captureKey(config: TriggerConfig, selected: string[]): string {
  return JSON.stringify([
    config.mode,
    config.source,
    config.threshold,
    config.postSeconds,
    selected,
  ]);
}

export function triggerValid(config: TriggerConfig, selected: string[]): boolean {
  return (
    Number.isFinite(config.postSeconds) &&
    config.postSeconds >= 0 &&
    config.postSeconds <= 15 &&
    (config.mode === "free" ||
      (selected.includes(config.source) &&
        config.threshold !== null &&
        Number.isFinite(config.threshold)))
  );
}

function sourcePoint(config: TriggerConfig, snapshot: TuneSnapshot): ProbePoint | null {
  const value = snapshot.values.find((v) => v.name === config.source)?.value;
  return value != null && Number.isFinite(value) ? { t: snapshot.t_s, value } : null;
}

export function armedCapture(
  config: TriggerConfig,
  selected: string[],
  snapshot: TuneSnapshot,
): Capture {
  return {
    key: captureKey(config, selected),
    session: snapshot.run.session_id,
    phase: config.mode === "free" ? "free" : "armed",
    at: null,
    end: null,
    lastTime: snapshot.t_s,
    reason: null,
    automatic: false,
    history: {},
    baseline: sourcePoint(config, snapshot),
  };
}

function appendSnapshot(
  history: ScopeHistory,
  selected: string[],
  snapshot: TuneSnapshot,
): ScopeHistory {
  const next: ScopeHistory = {};
  for (const name of selected) {
    const points = history[name] ?? [];
    const raw = snapshot.values.find((reading) => reading.name === name)?.value;
    const value = raw != null && Number.isFinite(raw) ? raw : null;
    next[name] =
      points.at(-1)?.t === snapshot.t_s ? points : [...points, { t: snapshot.t_s, value }];
  }
  return next;
}

function initialHistory(
  history: ScopeHistory,
  selected: string[],
  snapshot: TuneSnapshot,
): ScopeHistory {
  return Object.fromEntries(
    Object.entries(appendSnapshot(history, selected, snapshot)).map(([name, points]) => [
      name,
      points.slice(-120).map((point) => ({ ...point })),
    ]),
  );
}

function ended(snapshot: TuneSnapshot): boolean {
  return snapshot.run.state === "completed" || snapshot.run.state === "halted";
}

export function advanceCapture(
  previous: Capture | null,
  config: TriggerConfig,
  selected: string[],
  snapshot: TuneSnapshot,
  history: ScopeHistory,
): Capture {
  const key = captureKey(config, selected);
  if (
    !previous ||
    previous.key !== key ||
    previous.session !== snapshot.run.session_id ||
    previous.lastTime > snapshot.t_s
  ) {
    return armedCapture(config, selected, snapshot);
  }
  if (previous.phase === "held") return previous;
  if (previous.phase === "acquiring" && previous.at !== null) {
    const history =
      snapshot.t_s > previous.lastTime
        ? appendSnapshot(previous.history, selected, snapshot)
        : previous.history;
    const reason =
      snapshot.t_s >= previous.at + config.postSeconds
        ? "complete"
        : ended(snapshot)
          ? "run_ended"
          : Object.values(history).some((points) => points.length >= CAPTURE_MAX_SAMPLES)
            ? "sample_limit"
            : null;
    return {
      ...previous,
      phase: reason ? "held" : "acquiring",
      history,
      end: snapshot.t_s,
      lastTime: snapshot.t_s,
      reason,
    };
  }
  const point = sourcePoint(config, snapshot);
  const baseline = previous.baseline;
  const threshold = config.threshold;
  if (
    triggerValid(config, selected) &&
    threshold !== null &&
    point?.value != null &&
    baseline?.value != null &&
    point.t > baseline.t
  ) {
    const crossed =
      config.mode === "rising"
        ? baseline.value < threshold && point.value >= threshold
        : config.mode === "falling" && baseline.value > threshold && point.value <= threshold;
    if (crossed)
      return {
        ...previous,
        phase: config.postSeconds === 0 || ended(snapshot) ? "held" : "acquiring",
        at: snapshot.t_s,
        end: snapshot.t_s,
        lastTime: snapshot.t_s,
        reason: config.postSeconds === 0 ? "complete" : ended(snapshot) ? "run_ended" : null,
        automatic: true,
        history: initialHistory(history, selected, snapshot),
        baseline: point,
      };
  }
  // An unreadable sample breaks the pair. No trigger spans a gap or a duplicate time.
  return { ...previous, baseline: point, lastTime: snapshot.t_s };
}

export function holdCapture(
  config: TriggerConfig,
  selected: string[],
  snapshot: TuneSnapshot,
  history: ScopeHistory,
): Capture {
  return {
    ...armedCapture(config, selected, snapshot),
    phase: "held",
    at: snapshot.t_s,
    end: snapshot.t_s,
    reason: "manual",
    history: initialHistory(history, selected, snapshot),
  };
}
