import { RingBuffer } from "../lib/ring.ts";
import type { Tone } from "./buzzerAudio.ts";
import { RIDER_MODELS, type HeightMap } from "./freeRide.ts";
import { interpolatePoseFrames } from "./poseHistory.ts";
import type { Recording } from "./recording.ts";
import { CH, POSE_FLOATS } from "./telemetry.gen.ts";
import {
  CHANNEL_IDS,
  HISTORY_SECONDS,
  type ChannelId,
  type EngineClient,
  type EngineStatus,
  type FirmwareId,
  type FirmwareInfo,
  type Latest,
  type PhoneCommand,
  type PhoneFrame,
  type ScenarioInfo,
  type SimEvent,
} from "./types.ts";

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

/** Shared engine interface with the playback identity. */
export type PlaybackClient = Omit<EngineClient, "kind"> & { readonly kind: "playback" };

/** Replays recorded samples. This class never invokes a firmware or transport command. */
export class PlaybackEngine implements PlaybackClient {
  readonly recording: Recording;
  readonly kind = "playback" as const;
  readonly time: RingBuffer;
  readonly channels: Record<ChannelId, RingBuffer>;
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
    grade: Number.NaN,
    suspension: Number.NaN,
    shock: Number.NaN,
    distance: Number.NaN,
    wheelAngle: Number.NaN,
    poses: null,
  };
  readonly bodies;
  events: SimEvent[] = [];
  frames: PhoneFrame[] = [];
  readonly phoneCommands: PhoneCommand[] = [];
  outputs: EngineClient["outputs"] = null;
  readonly display: EngineClient["display"];
  readonly lights: EngineClient["lights"];
  readonly suspension: EngineClient["suspension"];
  readonly terrain: EngineClient["terrain"];
  readonly recordedMap?: HeightMap;
  readonly beeps = 0;
  readonly firmware: FirmwareId = "stock";
  readonly quirks = null;
  readonly scenarioId: string;
  status: EngineStatus;
  playing = false;
  targetRtf = 1;
  position: number;
  private cursor = -1;
  private eventCursor = 0;
  private frameCursor = 0;
  private outputCursor = 0;
  private notificationTime = 0;
  private poseGeneration = 0;
  private version = 0;
  private dataVersion = 0;
  private listeners = new Set<() => void>();
  private dataListeners = new Set<() => void>();

  constructor(recording: Recording) {
    this.recording = recording;
    this.display = recording.display ?? null;
    this.lights = recording.lights ?? null;
    this.suspension = recording.suspension ?? null;
    this.terrain = recording.terrain ?? null;
    this.recordedMap = recording.header.context?.height_map ?? undefined;
    const capacity = Math.min(
      RECORDING_RING_LIMIT,
      Math.max(1, Math.ceil(recording.header.sample_hz * HISTORY_SECONDS)),
    );
    this.time = new RingBuffer(capacity);
    this.channels = Object.fromEntries(
      CHANNEL_IDS.map((id) => [id, new RingBuffer(capacity)]),
    ) as Record<ChannelId, RingBuffer>;
    this.bodies = recording.header.bodies;
    this.scenarioId = recording.header.scenario ?? "recording";
    this.position = recording.start;
    this.status = {
      state: "paused",
      backend: "playback",
      build: recording.header.firmware,
      label: "Recording playback",
      runMode: recording.header.context?.run_mode,
      message: this.playbackMessage(false),
    };
    if (this.bodies.length) this.latest.poses = new Float32Array(this.bodies.length * POSE_FLOATS);
    this.seek(recording.start);
  }

  get poseEpoch(): number {
    return this.poseGeneration;
  }

  /** Recorded pose frames bracket the current playhead, with no live presentation delay. */
  samplePoses(out: Float32Array, _wallNowMs: number): number {
    const times = this.recording.times;
    if (
      times.length === 0 ||
      !Number.isFinite(this.position) ||
      out.buffer === this.latest.poses?.buffer
    ) {
      return Number.NaN;
    }
    const a = Math.max(0, this.upperBound(this.position) - 1);
    const b = Math.min(a + 1, times.length - 1);
    const ta = times[a] as number;
    const tb = times[b] as number;
    if (!Number.isFinite(ta) || !Number.isFinite(tb) || tb < ta) return Number.NaN;
    const t = Math.max(ta, Math.min(tb, this.position));
    const alpha = tb > ta ? (t - ta) / (tb - ta) : 0;
    const size = this.bodies.length * POSE_FLOATS;
    return interpolatePoseFrames(
      out,
      this.recording.poses,
      a * size,
      this.recording.poses,
      b * size,
      this.bodies.length,
      alpha,
    )
      ? t
      : Number.NaN;
  }

  subscribe = (fn: () => void): (() => void) => {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  };
  getVersion = (): number => this.version;
  subscribeData = (fn: () => void): (() => void) => {
    this.dataListeners.add(fn);
    return () => this.dataListeners.delete(fn);
  };
  getDataVersion = (): number => this.dataVersion;
  subscribeTones(_fn: (tones: Tone[]) => void): () => void {
    return () => {};
  }

  firmwares(): FirmwareInfo[] {
    return [
      {
        id: "stock",
        label: this.recording.header.firmware,
        detail: "Recorded firmware label",
        disabledReason: "Firmware selection is unavailable during playback",
      },
    ];
  }
  scenarios(): ScenarioInfo[] {
    return [
      {
        id: this.scenarioId,
        label:
          this.recording.header.scenario ??
          (this.recording.header.context?.run_mode === "free_ride"
            ? "Recorded free ride"
            : "Recorded run"),
        detail:
          this.recording.header.context?.run_mode === "free_ride"
            ? `${RIDER_MODELS.find((model) => model.value === this.recording.header.context?.rider_model)?.label ?? "Unknown rider model"} with recorded external aids`
            : "Recorded scenario",
      },
    ];
  }

  setPlaying(on: boolean): void {
    if (on && this.position >= this.recording.end) this.seek(this.recording.start);
    this.playing = on && this.recording.end > this.recording.start;
    this.status = {
      ...this.status,
      state: this.playing ? "running" : "paused",
      message: this.playbackMessage(this.playing),
    };
    this.emit();
  }
  togglePlaying(): void {
    this.setPlaying(!this.playing);
  }
  setTargetRtf(rtf: number): void {
    if (!Number.isFinite(rtf)) return;
    this.targetRtf = Math.min(8, Math.max(0.05, rtf));
    this.emit();
  }
  setFirmware(_id: FirmwareId): void {
    this.unavailable("Firmware selection is unavailable during playback");
  }
  setScenario(_id: string): void {
    this.unavailable("Scenario selection is unavailable during playback");
  }
  restart(): void {
    this.seek(this.recording.start);
  }
  step(): void {
    if (this.playing) return;
    const t = this.recording.times[this.cursor + 1];
    if (t !== undefined) this.seek(t);
  }
  sendCommand(_id: string, _value?: number): Promise<string> {
    return Promise.reject(new Error("Firmware commands are unavailable during playback"));
  }
  measuredRtf(): number {
    return this.playing ? this.targetRtf : 0;
  }

  /** Absolute simulation time, clamped to the recorded segment. */
  seek(t: number): void {
    if (!Number.isFinite(t)) return;
    this.poseGeneration = (this.poseGeneration + 1) >>> 0;
    this.position = Math.min(this.recording.end, Math.max(this.recording.start, t));
    const end = this.upperBound(this.position);
    const first = Math.max(
      this.lowerBound(this.position - HISTORY_SECONDS),
      end - this.time.capacity,
    );
    this.time.clear();
    for (const id of CHANNEL_IDS) this.channels[id].clear();
    for (let i = first; i < end; i++) this.append(i);
    this.cursor = end - 1;
    this.readLatest(this.cursor);
    this.events = [];
    this.eventCursor = 0;
    this.readEvents();
    this.frames = [];
    this.outputs = null;
    this.frameCursor = 0;
    this.outputCursor = 0;
    this.readTracks();
    if (this.position >= this.recording.end) this.playing = false;
    this.status = {
      ...this.status,
      state: this.playing ? "running" : "paused",
      message: this.playbackMessage(this.playing),
    };
    this.emit();
  }

  advance(wallSeconds: number): number {
    if (!this.playing || !Number.isFinite(wallSeconds) || wallSeconds <= 0) return 0;
    this.position = Math.min(this.recording.end, this.position + wallSeconds * this.targetRtf);
    const end = this.upperBound(this.position);
    const before = this.cursor;
    // A large frame gap needs only the history window, not every skipped sample.
    const first = Math.max(
      this.cursor + 1,
      this.lowerBound(this.position - HISTORY_SECONDS),
      end - this.time.capacity,
    );
    if (first > this.cursor + 1) {
      this.time.clear();
      for (const id of CHANNEL_IDS) this.channels[id].clear();
    }
    for (let i = first; i < end; i++) this.append(i);
    this.cursor = end - 1;
    if (this.cursor !== before) this.readLatest(this.cursor);
    const newEvents = this.readEvents();
    const newTracks = this.readTracks();
    this.notificationTime += wallSeconds;
    if (this.position >= this.recording.end) {
      this.playing = false;
      this.status = {
        ...this.status,
        state: "paused",
        message: `Playback reached the end of the captured segment. ${this.playbackMessage(false)}`,
      };
      this.emit();
    } else if (newEvents || newTracks || this.notificationTime >= 0.125) {
      this.emit();
    }
    return Math.max(0, this.cursor - before);
  }

  private append(i: number): void {
    this.time.push(this.recording.times[i] as number);
    for (const id of CHANNEL_IDS) this.channels[id].push(this.value(LANE[id], i));
  }
  private value(id: number, i: number): number {
    return this.recording.signals[id]?.[i] ?? Number.NaN;
  }
  private readLatest(i: number): void {
    if (i < 0) return;
    const l = this.latest;
    l.t = this.recording.times[i] as number;
    for (const id of CHANNEL_IDS) l[id] = this.value(LANE[id], i);
    l.roll = this.value(CH.roll_deg, i);
    l.distance = this.value(CH.distance_m, i);
    l.shock = this.value(CH.shock_mm, i);
    if (l.poses) {
      const size = this.bodies.length * POSE_FLOATS;
      l.poses.set(this.recording.poses.subarray(i * size, (i + 1) * size));
    }
  }
  private readEvents(): boolean {
    const start = this.eventCursor;
    const events = this.recording.events;
    while (this.eventCursor < events.length) {
      const event = events[this.eventCursor];
      if (!event || event.t_s > this.position) break;
      this.events.push({
        id: this.eventCursor + 1,
        t: event.t_s,
        kind: event.kind,
        title: event.text,
      });
      this.eventCursor++;
    }
    return start !== this.eventCursor;
  }
  private readTracks(): boolean {
    const before = this.frameCursor + this.outputCursor;
    const frames = this.recording.frames ?? [];
    const outputs = this.recording.outputs ?? [];
    while (this.frameCursor < frames.length) {
      const frame = frames[this.frameCursor];
      if (!frame || frame.t > this.position) break;
      this.frames.push(frame);
      this.frameCursor++;
    }
    while (this.outputCursor < outputs.length) {
      const output = outputs[this.outputCursor];
      if (!output || output.t_s > this.position) break;
      this.outputs = output.outputs;
      this.outputCursor++;
    }
    return before !== this.frameCursor + this.outputCursor;
  }
  private lowerBound(t: number): number {
    return this.bound(t, false);
  }
  private upperBound(t: number): number {
    return this.bound(t, true);
  }
  private bound(t: number, inclusive: boolean): number {
    const times = this.recording.times;
    let lo = 0;
    let hi = times.length;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      const value = times[mid] as number;
      if (value < t || (inclusive && value === t)) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }
  private unavailable(message: string): void {
    this.status = { ...this.status, message };
    this.emit();
  }
  private playbackMessage(playing: boolean): string {
    const state = playing ? "Recording playback is active." : "Playback is paused.";
    const ground = this.recordedMap ? "" : " The map is unavailable in this recording.";
    return `${state}${ground} Firmware commands are unavailable.`;
  }
  private emit(): void {
    this.notificationTime = 0;
    this.version++;
    this.dataVersion++;
    for (const fn of this.listeners) fn();
    for (const fn of this.dataListeners) fn();
  }
}

const RECORDING_RING_LIMIT = 30_000;
