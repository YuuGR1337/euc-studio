import { invoke } from "@tauri-apps/api/core";
import type { WorldSpec } from "./world.ts";
import type { HeightMap } from "./freeRide.ts";
import type { EngineClient, PhoneFrame } from "./types.ts";
import type { Outputs } from "./telemetry.gen.ts";
import {
  CHANNELS,
  POSE_FLOATS,
  type BodyInfo,
  type ChannelSpec,
  type TelemetryEvent,
} from "./telemetry.gen.ts";

export const RECORDING_LIMITS = {
  bytes: 32 * 1024 * 1024,
  samples: 72_000,
  values: 2_000_000,
  duration: 600,
  headerBytes: 256 * 1024,
  contextBytes: 128 * 1024,
} as const;

/** Schema 1 stores external aid settings, not personal rider measurements. */
export interface RecordingModelAids {
  roll_stiffness: number;
  roll_damping: number;
  steer_kp: number;
  steer_kd: number;
  steer_max_nm: number;
  /** Roll aid on the articulated upper body; present only for `articulated_contact_v1`. */
  upper_body_roll_stiffness?: number;
  upper_body_roll_damping?: number;
}

interface RecordingContextBase {
  schema: 1;
  rider_model: "rigid_ankle_v1" | "articulated_sagittal_v1" | "articulated_contact_v1";
  model_aids: RecordingModelAids;
}

/** A streaming world as a recording names it (never its meshes; playback regenerates it). */
export interface RecordedWorld {
  spec: WorldSpec;
  start_xy_m: [number, number];
  start_heading_rad: number;
  digest: number;
}

/** Native import validates this metadata, the saved map and the world before returning it. */
export type RecordingContext = RecordingContextBase &
  (
    | { run_mode: "free_ride"; height_map?: HeightMap | null; world?: RecordedWorld | null }
    | { run_mode: "scenario"; height_map?: HeightMap | null; world?: RecordedWorld | null }
  );

export interface RecordingHeader {
  wheel: string;
  firmware: string;
  scenario: string | null;
  sample_hz: number;
  bodies: BodyInfo[];
  channels: ChannelSpec[];
  /** Missing on legacy recordings. Missing context never supplies an inferred ground map. */
  context?: RecordingContext | null;
}

/** The desktop reader returns typed JSON, never memory addresses or filesystem paths. */
export interface RecordingResponse {
  header: RecordingHeader;
  channels?: ChannelSpec[];
  start_s: number;
  end_s: number;
  dropped_samples?: number;
  times: number[];
  signals: (number | null)[][];
  poses: number[];
  events: TelemetryEvent[];
  frames?: PhoneFrame[];
  outputs?: { t_s: number; outputs: Outputs }[];
  truncated?: string | null;
  display?: EngineClient["display"];
  lights?: EngineClient["lights"];
  terrain?: EngineClient["terrain"];
  suspension?: EngineClient["suspension"];
}

export interface Recording {
  name: string;
  header: RecordingHeader;
  start: number;
  end: number;
  droppedSamples: number;
  times: Float64Array;
  /** Indexed by the current euc-telemetry schema; absent appended channels contain NaN. */
  signals: Float32Array[];
  /** Sample order, then body order, seven floats per pose. */
  poses: Float32Array;
  events: TelemetryEvent[];
  frames?: PhoneFrame[];
  outputs?: { t_s: number; outputs: Outputs }[];
  truncated?: string | null;
  display?: EngineClient["display"];
  lights?: EngineClient["lights"];
  terrain?: EngineClient["terrain"];
  suspension?: EngineClient["suspension"];
}

export interface CaptureStatus {
  state: "idle" | "capturing" | "ready" | "error";
  can_start: boolean;
  from_start: boolean;
  samples: number;
  start_s: number | null;
  end_s: number | null;
  message: string;
}

export interface SavedCapture {
  path: string;
  status: CaptureStatus;
}

export function desktopRecordingsAvailable(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function openRecording(file: File): Promise<Recording> {
  if (!desktopRecordingsAvailable()) throw new Error("Open recordings in the desktop app");
  if (file.size > RECORDING_LIMITS.bytes) throw new Error("Recording exceeds 32 MiB");
  if (file.size < 12) throw new Error("Recording is empty or truncated");
  // Raw IPC avoids a JSON number array for the entire file. Rust checks all file dimensions.
  const bytes = await file.arrayBuffer();
  const response = await invoke<RecordingResponse>("recordings_open", bytes);
  const n = response.times.length;
  const columns = response.channels ?? response.header.channels;
  const c = columns.length;
  const b = response.header.bodies.length;
  if (
    n === 0 ||
    n > RECORDING_LIMITS.samples ||
    c > CHANNELS.length ||
    n * (c + b * POSE_FLOATS) > RECORDING_LIMITS.values ||
    response.signals.length !== c ||
    response.signals.some((column) => column.length !== n) ||
    response.poses.length !== n * b * POSE_FLOATS
  ) {
    throw new Error("Invalid recording response");
  }
  return {
    name: file.name,
    header: response.header,
    start: response.start_s,
    end: response.end_s,
    droppedSamples: response.dropped_samples ?? 0,
    times: Float64Array.from(response.times),
    signals: CHANNELS.map((_, id) => {
      const index = columns.findIndex((channel) => channel.id === id);
      const column = response.signals[index];
      return column
        ? Float32Array.from(column, (value) => value ?? Number.NaN)
        : new Float32Array(n).fill(Number.NaN);
    }),
    poses: Float32Array.from(response.poses),
    events: response.events,
    frames: response.frames ?? [],
    outputs: response.outputs ?? [],
    truncated: response.truncated,
    display: response.display,
    lights: response.lights,
    terrain: response.terrain,
    suspension: response.suspension,
  };
}

export const captureStatus = (): Promise<CaptureStatus> => invoke("recordings_status");
export const startCapture = (): Promise<CaptureStatus> => invoke("recordings_start");
export const saveCapture = (filename: string): Promise<SavedCapture> =>
  invoke("recordings_save", { filename });
