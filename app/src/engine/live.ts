// The live engine: the real simulator in the Tauri backend (app/src-tauri/src/engine). Telemetry
// arrives as binary batches on one channel and status, bodies, events and phone frames as JSON
// on another. Samples go straight into the rings; React only hears about coarse changes.

import { Channel, invoke } from "@tauri-apps/api/core";
import { formatStats, measureFrames } from "../lib/perf.ts";
import { RingBuffer } from "../lib/ring.ts";
import { readStored } from "../lib/storage.ts";
import { readBatch } from "./batch.ts";
import type { Tone } from "./buzzerAudio.ts";
import { POSE_MAX_BODIES, PoseHistory } from "./poseHistory.ts";
import {
  customFirmware,
  sameQuirks,
  splitFirmware,
  storeCustomQuirks,
  storedCustomQuirks,
} from "./quirkMask.ts";
import {
  type BodyInfo,
  CH,
  type Outputs,
  POSE_FLOATS,
  type TelemetryEvent,
} from "./telemetry.gen.ts";
import {
  CHANNEL_IDS,
  type ChannelId,
  type DisplayLayout,
  type LightsLayout,
  type EngineClient,
  type EngineStatus,
  type EventKind,
  type FirmwareId,
  type FirmwareInfo,
  HISTORY_SECONDS,
  type Latest,
  type NativeAudioStatus,
  type PhoneCommand,
  type PhoneFrame,
  SAMPLE_RATE,
  type ScenarioInfo,
  type SimEvent,
  type TerrainProfile,
  type WheelButton,
} from "./types.ts";

interface BackendStatus {
  run_mode: "scenario" | "free_ride";
  run_epoch: number;
  buttons_ready: boolean;
  power_button_ready: boolean;
  state: EngineStatus["state"];
  message: string;
  label: string | null;
  build: string | null;
  backend: string;
  scenario: string;
  /** A FirmwareId, or `ours-custom:ID,ID` with the quirks of a custom run. */
  firmware: string;
  t_s: number;
  rtf: number;
  target_rtf: number;
  playing: boolean;
  /** The file "Record this run" writes, while it does. */
}

/** What the backend sends on the JSON channel (the live engine and a recording's player alike). */
export type Message =
  | ({ type: "status" } & BackendStatus)
  | { type: "audio"; status: NativeAudioStatus }
  | { type: "reset"; scenario: string }
  | { type: "bodies"; bodies: BodyInfo[] }
  | { type: "display"; layout: DisplayLayout | null }
  | { type: "lights"; layout: LightsLayout | null }
  | { type: "buzzer"; tones: Tone[] }
  | { type: "terrain"; profile: TerrainProfile | null }
  | { type: "suspension"; info: EngineClient["suspension"] }
  | { type: "events"; events: TelemetryEvent[] }
  | { type: "frames"; frames: PhoneFrame[] }
  | { type: "outputs"; outputs: Outputs };

interface EngineInfo {
  backend: string;
  audio: NativeAudioStatus;
  scenarios: ScenarioInfo[];
  commands: PhoneCommand[];
  firmwares: FirmwareInfo[];
  perf_seconds: number | null;
}

/** Which telemetry channel feeds each UI lane. */
const LANE: Record<ChannelId, number> = {
  speed: CH.speed_kmh,
  pwm: CH.pwm_pct,
  modulation: CH.modulation_pct,
  current: CH.iq_a,
  voltage: CH.battery_v,
  pitch: CH.pitch_deg,
  temp: CH.motor_temp_c,
  battery: CH.soc_pct,
  isr: CH.isr_load_pct,
  suspension: CH.suspension_mm,
};

const MAX_EVENTS = 300;
const MAX_FRAMES = 400;

const FIRMWARES: FirmwareInfo[] = [
  { id: "stock", label: "Stock", detail: "Begode Extreme, official image" },
  {
    id: "ours-compat",
    label: "Ours-compat",
    detail: "Our C rewrite, stock behaviour",
    disabledReason: "Loading firmware availability",
  },
  {
    id: "ours-clean",
    label: "Ours-clean",
    detail: "Our C rewrite, quirks fixed",
    disabledReason: "Loading firmware availability",
  },
  {
    id: "ours-custom",
    label: "Ours-custom",
    detail: "Ours (custom quirks): our firmware on the host, quirks switched in Tune",
    disabledReason: "Loading firmware availability",
  },
];

export class LiveEngine implements EngineClient {
  readonly kind = "live" as const;
  readonly time = new RingBuffer(SAMPLE_RATE * HISTORY_SECONDS);
  readonly channels = Object.fromEntries(
    CHANNEL_IDS.map((id) => [id, new RingBuffer(SAMPLE_RATE * HISTORY_SECONDS)]),
  ) as Record<ChannelId, RingBuffer>;
  readonly latest: Latest = {
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
    distance: 0,
    wheelAngle: 0,
    suspension: Number.NaN,
    shock: Number.NaN,
    poses: null,
  };
  bodies: BodyInfo[] = [];
  events: SimEvent[] = [];
  frames: PhoneFrame[] = [];
  phoneCommands: PhoneCommand[] = [];
  outputs: Outputs | null = null;
  display: DisplayLayout | null = null;
  lights: LightsLayout | null = null;
  terrain: TerrainProfile | null = null;
  suspension: EngineClient["suspension"] = null;
  beeps = 0;
  audio?: NativeAudioStatus;
  status: EngineStatus = { state: "starting", message: "Connecting to the engine", backend: "…" };
  playing = true;
  targetRtf = 1;
  firmware: FirmwareId = "stock";
  quirks: string[] | null = null;
  private firmwareList: FirmwareInfo[] = FIRMWARES;
  /** The file "Record this run" writes, while it does. */
  scenarioId = "";

  protected scenarioList: ScenarioInfo[] = [];
  protected rtf = 0;
  private eventId = 0;
  private version = 0;
  private listeners = new Set<() => void>();
  private dataVersion = 0;
  private dataListeners = new Set<() => void>();
  private toneListeners = new Set<(tones: Tone[]) => void>();
  private buzzing = false;
  private readonly connected: Promise<void>;
  private audioRequests = Promise.resolve();
  private readonly poseHistory = new PoseHistory();
  private poseBatchSeq: number | null = null;
  private poseStepPending = false;
  private poseStepAfterT = -Infinity;
  /** Batches received, samples dropped by the backend (for diagnostics). */
  readonly stats = { batches: 0, samples: 0, dropped: 0 };

  /**
   * `connect`: attach to the live engine now; a subclass that streams something else (a recording's
   * player) passes false and never answers audio requests.
   */
  constructor(connect = true) {
    this.connected = connect ? this.connect() : Promise.resolve();
  }

  get poseEpoch(): number {
    return this.poseHistory.epoch;
  }

  samplePoses(out: Float32Array, wallNowMs: number): number {
    // A caller must own the visual output. Never let sampling rewrite the raw latest frame.
    if (out.buffer === this.latest.poses?.buffer) return Number.NaN;
    return this.poseHistory.sample(out, wallNowMs, this.measuredRtf());
  }

  private async connect(): Promise<void> {
    const stream = new Channel<ArrayBuffer>((buffer) => this.onBatch(buffer));
    const messages = new Channel<Message>((message) => this.onMessage(message));
    try {
      // Supply the preference before native startup can execute the first firmware step.
      const soundEnabled = readStored<boolean>("buzzerSound", true) !== false;
      const info = await invoke<EngineInfo>("engine_connect", { stream, messages, soundEnabled });
      this.audio = info.audio;
      this.scenarioList = info.scenarios;
      this.phoneCommands = info.commands;
      this.firmwareList = info.firmwares;
      this.status = { ...this.status, backend: info.backend };
      if (!this.scenarioId) this.scenarioId = info.scenarios[0]?.id ?? "";
      if (info.perf_seconds) void this.reportFrameTimes(info.perf_seconds);
    } catch (e) {
      this.status = { state: "error", message: String(e), backend: this.status.backend };
    }
    this.emit();
  }

  /**
   * Dev hook (EUC_STUDIO_PERF): after a warm-up, measure frame times and log them, again every
   * 10 s for a while. A hidden or covered window gets few frames; its lines show that in the
   * frame count.
   */
  private async reportFrameTimes(seconds: number): Promise<void> {
    const pause = (ms: number) => new Promise((r) => setTimeout(r, ms));
    await pause(8000);
    for (let run = 0; run < 30; run++) {
      const stats = await measureFrames(seconds);
      console.warn(
        `PERF ${formatStats(stats)}; engine ${this.status.state} at ${this.rtf.toFixed(2)}x, t ${this.latest.t.toFixed(1)} s, ${this.stats.batches} batches, ${this.stats.samples} samples, ${this.stats.dropped} dropped`,
      );
      await pause(10_000);
    }
  }

  /* ---- incoming ---- */

  protected onBatch(buffer: ArrayBuffer): void {
    const b = readBatch(buffer);
    const receivedWallMs = performance.now();
    this.stats.batches++;
    this.stats.samples += b.n;
    this.stats.dropped += b.dropped;
    if (b.dropped > 0 || (this.poseBatchSeq !== null && b.seq !== (this.poseBatchSeq + 1) >>> 0)) {
      this.poseHistory.reset(this.bodies.length);
    }
    this.poseBatchSeq = b.seq;
    if (b.n === 0) return;
    const hasVisualPoses =
      b.bodies > 0 && b.bodies <= POSE_MAX_BODIES && b.bodies === this.bodies.length;
    if (!hasVisualPoses) this.poseHistory.reset(this.bodies.length);
    const cols = CHANNEL_IDS.map((id) => [this.channels[id], b.column(LANE[id])] as const);
    for (let i = 0; i < b.n; i++) {
      this.time.push(b.t[i] as number);
      for (const [ring, col] of cols) ring.push(col[i] ?? Number.NaN);
      if (hasVisualPoses) {
        this.poseHistory.push(
          b.t[i] as number,
          b.poses,
          i * b.bodies * POSE_FLOATS,
          receivedWallMs,
        );
      }
    }
    const last = b.n - 1;
    if (this.poseStepPending && (b.t[last] as number) > this.poseStepAfterT) {
      this.poseHistory.finishStep();
      this.poseStepPending = false;
    }
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

  protected onMessage(m: Message): void {
    switch (m.type) {
      case "audio":
        this.audio = m.status;
        break;
      case "status": {
        if (this.status.runEpoch !== undefined && this.status.runEpoch !== m.run_epoch) {
          this.poseHistory.reset(this.bodies.length);
          this.poseBatchSeq = null;
          this.poseStepPending = false;
        }
        this.status = {
          runMode: m.run_mode,
          runEpoch: m.run_epoch,
          buttonsReady: m.buttons_ready === true,
          powerButtonReady: m.power_button_ready === true,
          state: m.state,
          message: m.message,
          backend: m.backend,
          ...(m.build ? { build: m.build } : {}),
          ...(m.label ? { label: m.label } : {}),
        };
        this.playing = m.playing;
        this.targetRtf = m.target_rtf;
        this.rtf = m.rtf;
        this.scenarioId = m.scenario;
        if (typeof m.firmware === "string") {
          const { id, quirks } = splitFirmware(m.firmware);
          this.firmware = id as FirmwareId;
          this.quirks = quirks;
        }
        break;
      }
      case "reset":
        this.status = { ...this.status, buttonsReady: false, powerButtonReady: false };
        this.poseHistory.reset(this.bodies.length);
        this.poseBatchSeq = null;
        this.poseStepPending = false;
        this.time.clear();
        for (const id of CHANNEL_IDS) this.channels[id].clear();
        this.events = [];
        this.frames = [];
        this.latest.poses = null;
        for (const key of Object.keys(this.latest) as (keyof Latest)[]) {
          if (key !== "poses") this.latest[key] = key === "t" ? 0 : Number.NaN;
        }
        this.outputs = null;
        this.beeps = 0;
        this.buzzing = false;
        this.scenarioId = m.scenario;
        break;
      case "bodies":
        this.bodies = m.bodies;
        this.poseHistory.reset(this.bodies.length);
        this.poseBatchSeq = null;
        this.poseStepPending = false;
        break;
      case "display":
        this.display = m.layout;
        break;
      case "lights":
        this.lights = m.layout;
        break;
      case "terrain":
        this.terrain = m.profile;
        break;
      case "suspension":
        this.suspension = m.info;
        break;
      case "buzzer":
        for (const t of m.tones) {
          if (t.hz > 0 && !this.buzzing) this.beeps++;
          this.buzzing = t.hz > 0;
        }
        for (const fn of this.toneListeners) fn(m.tones);
        this.emitData();
        return;
      case "events":
        this.events = [
          ...this.events,
          ...m.events.map((e) => ({
            id: ++this.eventId,
            t: e.t_s,
            kind: e.kind as EventKind,
            title: e.text,
          })),
        ].slice(-MAX_EVENTS);
        break;
      case "frames":
        this.frames = [...this.frames, ...m.frames].slice(-MAX_FRAMES);
        this.emitData();
        return;
      case "outputs":
        this.outputs = m.outputs;
        this.emitData();
        return;
    }
    this.emit();
  }

  /* ---- EngineClient ---- */

  setSound(enabled: boolean): Promise<void> {
    return this.requestAudioStatus("engine_sound", { enabled });
  }

  refreshAudioStatus(): Promise<void> {
    return this.requestAudioStatus("engine_audio_status");
  }

  private requestAudioStatus(command: string, args?: Record<string, unknown>): Promise<void> {
    // Wait for native startup and serialize mutations/reads so late replies cannot undo a mute.
    const request = this.audioRequests.then(async () => {
      await this.connected;
      try {
        this.audio = await invoke<NativeAudioStatus>(command, args);
      } catch (error) {
        this.audio = {
          backend: this.audio?.backend ?? "unsupported",
          enabled: this.audio?.enabled ?? false,
          state: "error",
          error: String(error),
        };
        throw error;
      } finally {
        this.emit();
      }
    });
    // Keep the next command usable after an IPC failure; the caller still receives the error.
    this.audioRequests = request.catch(() => {});
    return request;
  }

  subscribe = (fn: () => void): (() => void) => {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  };

  getVersion = (): number => this.version;

  protected emit(): void {
    this.version++;
    for (const fn of this.listeners) fn();
    this.emitData();
  }

  subscribeTones(fn: (tones: Tone[]) => void): () => void {
    this.toneListeners.add(fn);
    return () => this.toneListeners.delete(fn);
  }

  subscribeData = (fn: () => void): (() => void) => {
    this.dataListeners.add(fn);
    return () => this.dataListeners.delete(fn);
  };

  getDataVersion = (): number => this.dataVersion;

  private emitData(): void {
    this.dataVersion++;
    for (const fn of this.dataListeners) fn();
  }

  firmwares(): FirmwareInfo[] {
    // The stock entry names the build the engine loaded (the update server's label).
    const build = this.firmware === "stock" ? this.status.build : undefined;
    return build
      ? this.firmwareList.map((f) =>
          f.id === "stock" ? { ...f, detail: `${f.detail} ${build}` } : f,
        )
      : this.firmwareList;
  }

  scenarios(): ScenarioInfo[] {
    return this.scenarioList;
  }

  async importScenario(text: string): Promise<ScenarioInfo> {
    const info = await invoke<ScenarioInfo>("engine_import_scenario", { text });
    this.scenarioList = [...this.scenarioList, info];
    this.emit();
    this.setScenario(info.id);
    return info;
  }

  protected call(cmd: string, args?: Record<string, unknown>): void {
    invoke(cmd, args).catch((e: unknown) => {
      this.status = { ...this.status, state: "error", message: String(e) };
      this.emit();
    });
  }

  setPlaying(playing: boolean): void {
    this.playing = playing;
    this.call("engine_play", { playing });
    this.emit();
  }

  togglePlaying(): void {
    this.setPlaying(!this.playing);
  }

  setTargetRtf(rtf: number): void {
    this.targetRtf = rtf;
    this.call("engine_set_rtf", { rtf });
    this.emit();
  }

  setFirmware(id: FirmwareId, quirks?: readonly string[]): void {
    if (this.firmwareList.find((f) => f.id === id)?.disabledReason) return;
    if (id !== "ours-custom") {
      if (id !== this.firmware) this.call("engine_set_firmware", { firmware: id });
      return;
    }
    const mask = quirks ?? this.quirks ?? storedCustomQuirks();
    if (quirks) storeCustomQuirks(quirks);
    if (this.firmware === "ours-custom" && this.quirks && sameQuirks(mask, this.quirks)) return;
    this.call("engine_set_firmware", { firmware: customFirmware(mask) });
  }

  setScenario(id: string): void {
    this.scenarioId = id;
    this.call("engine_restart", { scenario: id });
    this.emit();
  }

  restart(): void {
    this.call("engine_restart", { scenario: null });
  }

  step(): void {
    if (!this.playing) {
      this.poseStepPending = true;
      this.poseStepAfterT = this.latest.t;
    }
    this.call("engine_step");
  }

  advance(): number {
    return 0;
  }

  measuredRtf(): number {
    return this.playing ? this.rtf : 0;
  }

  sendCommand(id: string, value?: number): Promise<string> {
    return invoke<string>("engine_send", { id, value: value ?? null });
  }

  setManual(input: { lean: number; steer: number } | null): void {
    this.call("engine_manual", { manual: input });
  }

  stepOff(): void {
    this.call("engine_step_off");
  }

  pressButton(button: WheelButton, durationMs: number): Promise<string> {
    return invoke<string>("engine_button", { button, durationMs });
  }

  setButton(button: WheelButton, pressed: boolean, epoch = this.status.runEpoch): Promise<string> {
    if (epoch === undefined)
      return Promise.reject(new Error("The simulator has no active input epoch"));
    return invoke<string>("engine_button_state", { button, pressed, epoch });
  }
}
