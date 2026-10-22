// The Compare workspace's engine: two firmware images ride one scenario on the same inputs and
// both runs stream in. The live engine judges them with euc-sim's compare module (the code behind
// `euc sim compare`); the summary types below mirror its JSON report (docs/compare.md), field
// names included.

import type { RingBuffer } from "../lib/ring.ts";
import type { BodyInfo } from "./telemetry.gen.ts";
import type { ChannelId, Latest, ScenarioInfo, TerrainProfile } from "./types.ts";

/** `ours-custom`: our firmware as the host SIL library, with the quirks its request names. */
export type CompareFirmwareId = "stock" | "ours-compat" | "ours-clean" | "ours-custom" | "file";

/** A firmware the workspace offers, and why it cannot run here if it cannot. */
export interface CompareFirmwareChoice {
  id: CompareFirmwareId;
  label: string;
  detail: string;
  disabledReason?: string;
}

/** One side's firmware, as the backend takes it. */
export type CompareFirmware =
  | { kind: "stock" | "ours-compat" | "ours-clean" }
  /** The quirks that stay on (IDs of quirks.def); the rest are fixed. */
  | { kind: "ours-custom"; quirks: string[] }
  | { kind: "file"; name: string; bytes: number[] };

export interface CompareRequest {
  a: CompareFirmware;
  b: CompareFirmware;
  scenario: string;
  /** As fast as the machine can (true) or in real time. */
  fast: boolean;
}

/** Which run: A is drawn solid, B dashed and as the ghost. */
export type Side = "a" | "b";

/** One side's identity once it is built. */
export interface RunInfo {
  spec: string;
  label: string;
  sha256: string;
  checks_skipped: string[];
}

/** One run as it streams in: its samples, its latest state and its bodies. */
export interface CompareRun {
  readonly time: RingBuffer;
  readonly channels: Record<ChannelId, RingBuffer>;
  readonly latest: Latest;
  readonly bodies: BodyInfo[];
  readonly info: RunInfo | null;
}

/** How one telemetry channel or firmware variable compares. `null` numbers are infinite. */
export interface SeriesDiff {
  name: string;
  unit: string;
  max_abs: number | null;
  rms: number | null;
  at_max_s: number;
  a_at_max: number | null;
  b_at_max: number | null;
  tolerance: number | null;
  rms_tolerance: number | null;
  first_s: number | null;
  within: boolean;
}

export interface Divergence {
  t_s: number;
  kind: "channel" | "var" | "ble";
  name: string;
  a: string;
  b: string;
}

export interface FrameDiff {
  index: number;
  t_a: number | null;
  t_b: number | null;
  field: string;
  a: string;
  b: string;
}

export interface BleDiff {
  bytes_a: number;
  bytes_b: number;
  frames_a: number;
  frames_b: number;
  frames_identical: boolean;
  first_frame: FrameDiff | null;
  max_frame_skew_us: number;
  text_bytes_a: number;
  text_bytes_b: number;
  text_identical: boolean;
}

export interface CheckResult {
  what: string;
  passed: boolean;
  measured: string;
}

export interface Outcome {
  checks: CheckResult[];
  top_speed_kmh: number;
  max_abs_pitch_deg: number;
  distance_m: number;
}

/** The comparison so far; `done` once it covers the whole run (BLE and checks included). */
export interface CompareSummary {
  done: boolean;
  samples: number;
  duration_s: number;
  sample_skew_us: number;
  compared_until_s: number | null;
  first_divergence: Divergence | null;
  channels: SeriesDiff[];
  /** Channels a run states it does not measure (the ISR load of a SIL run), not judged; absent from `channels`. */
  channels_unavailable?: string[];
  vars: SeriesDiff[];
  vars_unpaired: string[];
  /** Null for the demo, which has no BLE stream. */
  ble: BleDiff | null;
  within_tolerance: boolean;
  check_disagreements: string[];
  ok: boolean | null;
  outcome_a: Outcome | null;
  outcome_b: Outcome | null;
}

export type CompareState = "idle" | "starting" | "running" | "done" | "stopped" | "error";

export interface CompareStatus {
  state: CompareState;
  /** What it is doing ("Booting ours-compat") or why it stopped. */
  message: string;
  /** Simulated seconds so far and of the whole scenario. */
  t: number;
  duration: number;
  /** Measured sim seconds per wall second. */
  rtf: number;
}

export interface CompareClient {
  readonly kind: "demo" | "live";
  readonly a: CompareRun;
  readonly b: CompareRun;
  readonly status: CompareStatus;
  readonly summary: CompareSummary | null;
  /** The ground along the path (physics frame); null on flat ground. */
  readonly terrain: TerrainProfile | null;
  /** What runs now (or ran last). */
  readonly request: CompareRequest | null;
  /** The tolerance file the verdict uses, and its note for this scenario. */
  readonly tolerance: { file: string; note: string | null } | null;
  /** Samples per second on the streams. */
  readonly streamHz: number;
  /** Bumped on every reset: views that hold rings remount. */
  readonly epoch: number;

  firmwares(): CompareFirmwareChoice[];
  scenarios(): ScenarioInfo[];
  start(request: CompareRequest): void;
  stop(): void;

  subscribe(fn: () => void): () => void;
  getVersion(): number;
}

/** Telemetry channel keys (the summary's names) of the lanes the workspace draws. */
export const LANE_KEYS: Record<ChannelId, string> = {
  speed: "speed_kmh",
  pwm: "pwm_pct",
  modulation: "modulation_pct",
  current: "iq_a",
  voltage: "battery_v",
  pitch: "pitch_deg",
  temp: "motor_temp_c",
  battery: "soc_pct",
  isr: "isr_load_pct",
  suspension: "suspension_mm",
};
