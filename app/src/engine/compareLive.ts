// The live Compare engine: the backend (app/src-tauri/src/engine/compare.rs) rides the scenario
// with both firmwares on one thread and streams each run on its own binary channel; status, the
// runs' identities and the summary come as JSON. A new comparison gets new channels, and whatever
// an older run still sends is dropped.

import { Channel, invoke } from "@tauri-apps/api/core";
import { RingBuffer } from "../lib/ring.ts";
import { readBatch } from "./batch.ts";
import type {
  CompareClient,
  CompareFirmwareChoice,
  CompareRequest,
  CompareRun,
  CompareStatus,
  CompareSummary,
  RunInfo,
  Side,
} from "./compareTypes.ts";
import { LANE_KEYS } from "./compareTypes.ts";
import { type BodyInfo, CH, type ChannelKey, POSE_FLOATS } from "./telemetry.gen.ts";
import {
  CHANNEL_IDS,
  type ChannelId,
  type Latest,
  type ScenarioInfo,
  type TerrainProfile,
} from "./types.ts";

type Message =
  | {
      type: "status";
      state: CompareStatus["state"];
      message: string;
      t_s: number;
      duration_s: number;
      rtf: number;
    }
  | {
      type: "reset";
      scenario: string;
      duration_s: number;
      stream_hz: number;
      a: RunInfo;
      b: RunInfo;
      bodies_a: BodyInfo[];
      bodies_b: BodyInfo[];
      terrain: TerrainProfile | null;
      tolerance_file: string;
      tolerance_note: string | null;
    }
  | ({ type: "summary" } & CompareSummary);

interface Info {
  firmwares: (Omit<CompareFirmwareChoice, "disabledReason"> & { disabled_reason: string | null })[];
  scenarios: ScenarioInfo[];
  tolerance_file: string;
}

/** Room past the scenario's length, so a run never wraps its rings. */
const SLACK_S = 5;

export function emptyLatest(): Latest {
  return {
    t: 0,
    speed: Number.NaN,
    pwm: Number.NaN,
    modulation: Number.NaN,
    current: Number.NaN,
    voltage: Number.NaN,
    pitch: Number.NaN,
    roll: Number.NaN,
    temp: Number.NaN,
    battery: Number.NaN,
    isr: Number.NaN,
    grade: 0,
    suspension: Number.NaN,
    shock: Number.NaN,
    distance: 0,
    wheelAngle: 0,
    poses: null,
  };
}

/** One run's rings, sized for the whole scenario. */
export class RunBuffers implements CompareRun {
  time: RingBuffer;
  channels: Record<ChannelId, RingBuffer>;
  readonly latest: Latest = emptyLatest();
  bodies: BodyInfo[] = [];
  info: RunInfo | null = null;

  constructor(samples: number) {
    this.time = new RingBuffer(samples);
    this.channels = RunBuffers.rings(samples);
  }

  private static rings(samples: number): Record<ChannelId, RingBuffer> {
    return Object.fromEntries(CHANNEL_IDS.map((id) => [id, new RingBuffer(samples)])) as Record<
      ChannelId,
      RingBuffer
    >;
  }

  /** New, empty rings for `samples` samples (views holding the old ones remount on the epoch). */
  reset(samples: number, info: RunInfo | null, bodies: BodyInfo[]): void {
    this.time = new RingBuffer(samples);
    this.channels = RunBuffers.rings(samples);
    Object.assign(this.latest, emptyLatest());
    this.info = info;
    this.bodies = bodies;
  }

  /** Appends a telemetry batch (crates/euc-telemetry/src/batch.rs). */
  push(buffer: ArrayBuffer): void {
    const b = readBatch(buffer);
    if (b.n === 0) return;
    const cols = CHANNEL_IDS.map(
      (id) => [this.channels[id], b.column(CH[LANE_KEYS[id] as ChannelKey])] as const,
    );
    for (let i = 0; i < b.n; i++) {
      this.time.push(b.t[i] as number);
      for (const [ring, col] of cols) ring.push(col[i] ?? Number.NaN);
    }
    const last = b.n - 1;
    const at = (id: number) => b.column(id)[last] ?? Number.NaN;
    const l = this.latest;
    l.t = b.t[last] as number;
    l.speed = at(CH.speed_kmh);
    l.pwm = at(CH.pwm_pct);
    l.modulation = at(CH.modulation_pct);
    l.current = at(CH.iq_a);
    l.voltage = at(CH.battery_v);
    l.pitch = at(CH.pitch_deg);
    l.roll = at(CH.roll_deg);
    l.temp = at(CH.motor_temp_c);
    l.battery = at(CH.soc_pct);
    l.isr = at(CH.isr_load_pct);
    l.distance = at(CH.distance_m);
    l.suspension = at(CH.suspension_mm);
    l.shock = at(CH.shock_mm);
    if (b.bodies > 0) {
      const size = b.bodies * POSE_FLOATS;
      const from = last * size;
      if (!l.poses || l.poses.length !== size) l.poses = new Float32Array(size);
      l.poses.set(b.poses.subarray(from, from + size));
    }
  }
}

export class LiveCompare implements CompareClient {
  readonly kind = "live" as const;
  readonly a = new RunBuffers(1);
  readonly b = new RunBuffers(1);
  status: CompareStatus = { state: "idle", message: "", t: 0, duration: 0, rtf: 0 };
  summary: CompareSummary | null = null;
  terrain: TerrainProfile | null = null;
  request: CompareRequest | null = null;
  tolerance: CompareClient["tolerance"] = null;
  streamHz = 50;
  epoch = 0;

  private choices: CompareFirmwareChoice[] = [];
  private scenarioList: ScenarioInfo[] = [];
  private run = 0;
  /**
   * The running comparison's batches that came before its reset, in order. Tauri fetches the
   * reset (both body lists, about 17 KB) but runs a small batch at once, so in real time the
   * first batches arrive first; the reset then replays them into the new rings.
   */
  private early: Record<Side, ArrayBuffer[]> | null = null;
  private version = 0;
  private listeners = new Set<() => void>();

  constructor() {
    void this.load();
  }

  private async load(): Promise<void> {
    try {
      const info = await invoke<Info>("compare_info");
      this.choices = info.firmwares.map(({ disabled_reason, ...f }) => ({
        ...f,
        ...(disabled_reason ? { disabledReason: disabled_reason } : {}),
      }));
      this.scenarioList = info.scenarios;
    } catch (e) {
      this.status = { ...this.status, state: "error", message: String(e) };
    }
    this.emit();
  }

  firmwares(): CompareFirmwareChoice[] {
    return this.choices;
  }

  scenarios(): ScenarioInfo[] {
    return this.scenarioList;
  }

  start(request: CompareRequest): void {
    const run = ++this.run;
    const mine = <T>(fn: (v: T) => void) => new Channel<T>((v) => run === this.run && fn(v));
    this.early = { a: [], b: [] };
    const streamA = mine<ArrayBuffer>((buffer) => this.sample("a", buffer));
    const streamB = mine<ArrayBuffer>((buffer) => this.sample("b", buffer));
    const messages = mine<Message>((m) => this.onMessage(m));
    this.request = request;
    this.summary = null;
    this.status = {
      state: "starting",
      message: "Starting the comparison",
      t: 0,
      duration: 0,
      rtf: 0,
    };
    invoke("compare_start", { spec: request, streamA, streamB, messages }).catch((e: unknown) => {
      if (run !== this.run) return;
      this.status = { ...this.status, state: "error", message: String(e) };
      this.emit();
    });
    this.emit();
  }

  stop(): void {
    this.run++;
    this.early = null;
    if (this.status.state === "starting" || this.status.state === "running") {
      this.status = { ...this.status, state: "stopped", message: "Stopped" };
    }
    invoke("compare_stop").catch(() => {});
    this.emit();
  }

  /** A batch of the running comparison: held until its reset, then into the rings. */
  private sample(side: Side, buffer: ArrayBuffer): void {
    if (this.early) this.early[side].push(buffer);
    else this[side].push(buffer);
  }

  private onMessage(m: Message): void {
    switch (m.type) {
      case "status":
        this.status = {
          state: m.state,
          message: m.message,
          t: m.t_s,
          duration: m.duration_s,
          rtf: m.rtf,
        };
        break;
      case "reset": {
        const samples = Math.ceil((m.duration_s + SLACK_S) * m.stream_hz);
        this.a.reset(samples, m.a, m.bodies_a);
        this.b.reset(samples, m.b, m.bodies_b);
        const early = this.early;
        this.early = null;
        for (const buffer of early?.a ?? []) this.a.push(buffer);
        for (const buffer of early?.b ?? []) this.b.push(buffer);
        this.terrain = m.terrain;
        this.streamHz = m.stream_hz;
        this.tolerance = { file: m.tolerance_file, note: m.tolerance_note };
        this.status = { ...this.status, duration: m.duration_s };
        this.epoch++;
        break;
      }
      case "summary": {
        const { type, ...summary } = m;
        if (type === "summary") this.summary = summary;
        break;
      }
    }
    this.emit();
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
