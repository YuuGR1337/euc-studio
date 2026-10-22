// Demo Compare engine (browser builds, tests): two synthetic rides of a demo scenario, so the
// workspace has something honest to show without the Rust engine. Both sides draw the same noise
// from the same seed. Stock and ours-compat ride identically; ours-clean starts its speed tiltback
// at 18 km/h, so the runs part ways when the target speed passes it. Ours (custom quirks) rides
// like compat with the compat quirks and like clean with any other mask. The summary follows the
// definitions of euc-sim's compare module (largest |A - B|, RMS, first sample beyond tolerance);
// the live engine computes it with that module itself.

import type {
  CompareClient,
  CompareFirmwareChoice,
  CompareFirmwareId,
  CompareRequest,
  CompareStatus,
  CompareSummary,
  SeriesDiff,
} from "../engine/compareTypes.ts";
import { LANE_KEYS } from "../engine/compareTypes.ts";
import { RunBuffers } from "../engine/compareLive.ts";
import { describeQuirks } from "../engine/quirkMask.ts";
import { CHANNELS, CHANNEL_IDS, type ChannelId, type ScenarioInfo } from "../engine/types.ts";
import { onFrameEarly } from "../lib/frameloop.ts";
import { SCENARIOS, type Scenario, prng, scenarioTarget } from "./synthetic.ts";

const HZ = 50;
const DT = 1 / HZ;
/** The demo's "fast" pace, sim seconds per wall second. */
const FAST_RTF = 6;
/** Where ours-clean's speed tiltback starts in the demo, km/h. */
const CLEAN_TILTBACK_KMH = 18;
/** The demo's tolerances, a few of compat-vs-stock.toml's. */
const TOLERANCES: Partial<Record<ChannelId, number>> = {
  speed: 0.5,
  pitch: 1.5,
  voltage: 0.5,
};
/** Relative difference that counts as equal without a tolerance (as euc-sim's compare). */
const ROUNDING = 1e-6;

export const DEMO_COMPARE_FIRMWARES: CompareFirmwareChoice[] = [
  { id: "stock", label: "Stock", detail: "Begode Extreme, official image" },
  { id: "ours-compat", label: "Ours-compat", detail: "Our C rewrite, stock behaviour" },
  { id: "ours-clean", label: "Ours-clean", detail: "Our C rewrite, quirks fixed" },
  {
    id: "ours-custom",
    label: "Ours-custom",
    detail: "Ours (custom quirks): our C rewrite on the host, quirks switched below",
  },
];

/** Whether a side rides the demo's clean behaviour. */
function ridesClean(side: CompareRequest["a"]): boolean {
  if (side.kind === "ours-clean") return true;
  return side.kind === "ours-custom" && describeQuirks(side.quirks) !== "compat";
}

/** One synthetic ride, stepped at 50 Hz. */
class Ride {
  private rand: () => number;
  private speed = 0;
  private accel = 0;
  private soc = 0.82;
  private temp = 34;
  private distance = 0;
  private noise = 0;

  private readonly scenario: Scenario;
  private readonly clean: boolean;

  constructor(scenario: Scenario, clean: boolean, seed: number) {
    this.scenario = scenario;
    this.clean = clean;
    this.rand = prng(seed);
  }

  step(t: number, out: RunBuffers): void {
    const target = scenarioTarget(this.scenario, t);
    // Clean eases the target down past its tiltback speed (and lifts the pedals while it does).
    const over = this.clean ? Math.max(0, target.speed - CLEAN_TILTBACK_KMH) : 0;
    const wanted = target.speed - over * 0.6;
    const prev = this.speed;
    this.speed = Math.max(0, prev + (wanted - prev) * (1 - Math.exp(-DT / 0.35)));
    const accel = (this.speed - prev) / 3.6 / DT;
    this.accel += (accel - this.accel) * 0.2;
    this.noise += (this.rand() * 2 - 1 - this.noise) * 0.05;
    const pitch =
      1.7 * this.accel +
      Math.atan(target.grade) * (180 / Math.PI) * 0.25 +
      0.18 * Math.sin(2 * Math.PI * 1.3 * t) +
      0.12 * this.noise +
      Math.min(over, 4) * 0.5;
    const current =
      (this.accel >= 0 ? 9.5 : 6.5) * this.accel +
      0.012 * this.speed ** 2 +
      target.grade * 150 +
      1.1;
    this.soc = Math.max(0, this.soc - (Math.max(current, 0) * DT) / 3600 / 17.8);
    const voltage = 32 * (3.3 + 0.9 * this.soc) - current * 0.18;
    const pwm = Math.min(
      100,
      (this.speed / 68) * 100 * (134.4 / voltage) + Math.abs(current) * 0.55 + 2,
    );
    this.temp += (current * current * 0.0009 - (this.temp - 34) * 0.01) * DT;
    this.distance += (this.speed / 3.6) * DT;

    const l = out.latest;
    Object.assign(l, {
      t,
      speed: this.speed,
      pwm,
      modulation: pwm * 0.98,
      current,
      voltage,
      pitch,
      roll: 0.35 * Math.sin(2 * Math.PI * 0.21 * t),
      temp: this.temp,
      battery: this.soc * 100,
      isr: 31 + pwm * 0.09,
      grade: target.grade,
      distance: this.distance,
      wheelAngle: this.distance / 0.28,
    });
    out.time.push(t);
    for (const id of CHANNEL_IDS) {
      out.channels[id].push(id === "suspension" ? Number.NaN : (l[id] as number));
    }
  }
}

/** One series' difference, as euc-sim's compare module defines it. */
export function seriesDiff(
  name: string,
  unit: string,
  t: ArrayLike<number>,
  a: ArrayLike<number>,
  b: ArrayLike<number>,
  tolerance: number | null,
): SeriesDiff {
  let maxAbs = 0;
  let at = 0;
  let sum = 0;
  let count = 0;
  let first: number | null = null;
  const n = Math.min(t.length, a.length, b.length);
  for (let i = 0; i < n; i++) {
    const x = a[i] as number;
    const y = b[i] as number;
    const nx = Number.isNaN(x);
    const ny = Number.isNaN(y);
    const diff = nx && ny ? 0 : nx || ny ? Infinity : Math.abs(x - y);
    if (Number.isFinite(diff) && !(nx && ny)) {
      sum += diff * diff;
      count++;
    }
    if (diff > maxAbs) {
      maxAbs = diff;
      at = i;
    }
    const limit = tolerance ?? ROUNDING * Math.max(Math.abs(x), Math.abs(y), 1);
    if (first === null && diff > limit) first = t[i] as number;
  }
  const finite = (v: number) => (Number.isFinite(v) ? v : null);
  return {
    name,
    unit,
    max_abs: finite(maxAbs),
    rms: count > 0 ? Math.sqrt(sum / count) : 0,
    at_max_s: (t[at] as number | undefined) ?? 0,
    a_at_max: finite((a[at] as number | undefined) ?? Number.NaN),
    b_at_max: finite((b[at] as number | undefined) ?? Number.NaN),
    tolerance,
    rms_tolerance: null,
    first_s: first,
    within: tolerance === null || maxAbs <= tolerance,
  };
}

export class DemoCompare implements CompareClient {
  readonly kind = "demo" as const;
  readonly a = new RunBuffers(1);
  readonly b = new RunBuffers(1);
  status: CompareStatus = { state: "idle", message: "", t: 0, duration: 0, rtf: 0 };
  summary: CompareSummary | null = null;
  readonly terrain = null;
  request: CompareRequest | null = null;
  tolerance: CompareClient["tolerance"] = null;
  readonly streamHz = HZ;
  epoch = 0;

  private rides: [Ride, Ride] | null = null;
  private steps = 0;
  private summarizedAt = 0;
  private emittedAt = 0;
  private debt = 0;
  private version = 0;
  private listeners = new Set<() => void>();

  constructor() {
    onFrameEarly((_now, dt) => this.advance(dt));
  }

  firmwares(): CompareFirmwareChoice[] {
    return DEMO_COMPARE_FIRMWARES;
  }

  scenarios(): ScenarioInfo[] {
    return SCENARIOS.map(({ id, label, detail }) => ({ id, label, detail }));
  }

  start(request: CompareRequest): void {
    const scenario = SCENARIOS.find((s) => s.id === request.scenario);
    const builtIn = (id: CompareFirmwareId) => id !== "file";
    if (!scenario || !builtIn(request.a.kind) || !builtIn(request.b.kind)) {
      this.status = {
        state: "error",
        message: "The demo engine compares its built-in firmwares only",
        t: 0,
        duration: 0,
        rtf: 0,
      };
      this.emit();
      return;
    }
    const duration = (scenario.segments[scenario.segments.length - 1] as { until: number }).until;
    const samples = Math.ceil(duration * HZ) + 2;
    const info = (side: CompareRequest["a"]) => {
      const choice = DEMO_COMPARE_FIRMWARES.find((f) => f.id === side.kind);
      const label =
        side.kind === "ours-custom"
          ? `ours-sil (${describeQuirks(side.quirks)})`
          : (choice?.label ?? side.kind);
      return { spec: side.kind, label, sha256: "", checks_skipped: [] };
    };
    this.a.reset(samples, info(request.a), []);
    this.b.reset(samples, info(request.b), []);
    this.rides = [
      new Ride(scenario, ridesClean(request.a), 7),
      new Ride(scenario, ridesClean(request.b), 7),
    ];
    this.request = request;
    this.summary = null;
    this.tolerance = { file: "demo tolerances", note: null };
    this.steps = 0;
    this.summarizedAt = 0;
    this.emittedAt = 0;
    this.debt = 0;
    this.status = {
      state: "running",
      message: "",
      t: 0,
      duration,
      rtf: request.fast ? FAST_RTF : 1,
    };
    this.epoch++;
    this.emit();
  }

  /** Runs `seconds` of both rides at once (tests and screenshots). */
  preroll(seconds: number): void {
    for (let i = 0; i < Math.round(seconds * HZ) && this.rides; i++) this.stepOnce();
    this.summarize();
    this.emit();
  }

  stop(): void {
    if (this.status.state !== "running") return;
    this.rides = null;
    this.status = { ...this.status, state: "stopped", message: "Stopped" };
    this.summarize();
    this.emit();
  }

  private advance(wallSeconds: number): void {
    if (!this.rides || this.status.state !== "running") return;
    this.debt += Math.min(wallSeconds, 0.25) * this.status.rtf;
    let stepped = false;
    while (this.debt >= DT && this.rides) {
      this.stepOnce();
      this.debt -= DT;
      stepped = true;
    }
    if (!stepped) return;
    // Views hear about the runs four times a simulated second and the summary follows twice,
    // as with the live engine; the scopes and the 3D view read the rings every frame anyway.
    if (this.rides && this.steps - this.summarizedAt >= HZ / 2) this.summarize();
    if (!this.rides || this.steps - this.emittedAt >= HZ / 4) {
      this.emittedAt = this.steps;
      this.emit();
    }
  }

  private stepOnce(): void {
    if (!this.rides) return;
    const t = this.steps * DT;
    this.rides[0].step(t, this.a);
    this.rides[1].step(t, this.b);
    this.steps++;
    this.status.t = t;
    if (t + DT > this.status.duration) {
      this.rides = null;
      this.status = { ...this.status, state: "done" };
      this.summarize();
    }
  }

  private summarize(): void {
    this.summarizedAt = this.steps;
    const t = this.a.time.toArray();
    const channels = CHANNEL_IDS.map((id) =>
      seriesDiff(
        LANE_KEYS[id],
        CHANNELS[id].unit,
        t,
        this.a.channels[id].toArray(),
        this.b.channels[id].toArray(),
        TOLERANCES[id] ?? null,
      ),
    );
    let first: CompareSummary["first_divergence"] = null;
    for (const [i, s] of channels.entries()) {
      if (s.first_s === null || (first && first.t_s <= s.first_s)) continue;
      const id = CHANNEL_IDS[i] as ChannelId;
      const k = t.findIndex((x) => x >= (s.first_s as number));
      const value = (side: RunBuffers) => String(side.channels[id].at(k) ?? Number.NaN);
      first = { t_s: s.first_s, kind: "channel", name: s.name, a: value(this.a), b: value(this.b) };
    }
    const done = this.status.state === "done";
    const within = channels.every((c) => c.within);
    this.summary = {
      done,
      samples: t.length,
      duration_s: t[t.length - 1] ?? 0,
      sample_skew_us: 0,
      compared_until_s: null,
      first_divergence: first,
      channels,
      vars: [],
      vars_unpaired: [],
      ble: null,
      within_tolerance: within,
      check_disagreements: [],
      ok: done ? within : null,
      outcome_a: null,
      outcome_b: null,
    };
  }

  subscribe = (fn: () => void): (() => void) => {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  };

  getVersion = (): number => this.version;

  private emit(): void {
    this.version++;
    for (const fn of this.listeners) fn();
  }
}
