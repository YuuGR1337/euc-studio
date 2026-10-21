import { invoke } from "@tauri-apps/api/core";

export type FreeRideSource = "keyboard_slider" | "gamepad";
export type RiderModel = "rigid_ankle_v1" | "articulated_sagittal_v1";
export const RIDER_MODELS = [
  { value: "rigid_ankle_v1", label: "Rigid baseline" },
  { value: "articulated_sagittal_v1", label: "Articulated (experimental)" },
] as const;
export const isRiderModel = (value: unknown): value is RiderModel =>
  RIDER_MODELS.some((model) => model.value === value);
export interface MapFeature {
  kind: "bump" | "pothole";
  centre_xy_m: [number, number];
  width_xy_m: [number, number];
  size_m: number;
}
export interface GeneratedMapSpec {
  schema: 1;
  generator_revision: 1;
  seed: number;
  grade_pct: [number, number];
  features: MapFeature[];
}
export interface FreeRideSpec {
  wheel: "begode-extreme";
  settings: "blank";
  rider_model: RiderModel;
  map: GeneratedMapSpec;
}
export interface HeightMap {
  revision: number;
  nx: number;
  ny: number;
  spacing_m: number;
  origin_xy_m: [number, number];
  /** Row-major: iy * nx + ix, signed millimetres above the datum. */
  heights_mm: number[];
}
export interface FreeRideStatus {
  epoch: number;
  rider_model: RiderModel;
  mode: "scenario" | "free_ride";
  ready: boolean;
  armed: boolean;
  source: FreeRideSource | null;
  reason: string;
  desired_lean_deg: number | null;
  commanded_lean_deg: number | null;
  actual_lean_deg: number | null;
  heading_deg: number | null;
  path_length_m: number | null;
  yaw_aid_work_j: number | null;
  roll_aid_work_j: number | null;
}
export interface FreeRideInput {
  run_epoch: number;
  seq: number;
  source: FreeRideSource;
  desired_lean_rad: number;
  heading_rate_rad_s: number;
  armed: boolean;
}
export const FREE_RIDE_LIMITS = {
  leanDeg: 10,
  turnDegS: 30,
  heartbeatMs: 50,
  neutralMs: 250,
  watchdogMs: 250,
  features: 32,
} as const;
const radians = Math.PI / 180;
export const flatMap = (seed = 0): GeneratedMapSpec => ({
  schema: 1,
  generator_revision: 1,
  seed,
  grade_pct: [0, 0],
  features: [],
});

/** Cheap editor validation; the native generator checks final heights and triangle grades. */
export function mapProblem(map: GeneratedMapSpec): string | null {
  if (map.schema !== 1 || map.generator_revision !== 1) return "Unsupported map version.";
  if (!Number.isInteger(map.seed) || map.seed < 0 || map.seed > 0xffff_ffff)
    return "Seed must be a uint32.";
  if (!map.grade_pct.every(Number.isFinite) || Math.hypot(...map.grade_pct) > 20)
    return "Combined x/y grade must be at most 20%.";
  if (map.features.length > 32) return "Use at most 32 features.";
  for (const [index, f] of map.features.entries()) {
    const prefix = `Feature ${index + 1}: `;
    if (f.kind !== "bump" && f.kind !== "pothole") return prefix + "unknown kind.";
    if (![...f.centre_xy_m, ...f.width_xy_m, f.size_m].every(Number.isFinite))
      return prefix + "enter finite numbers.";
    if (f.size_m <= 0 || f.size_m > 0.3)
      return prefix + "height or depth must be above 0 and at most 0.30 m.";
    if (f.width_xy_m.some((v) => v < 1 || v > 32))
      return prefix + "support widths must be 1 to 32 m.";
    const [x, y] = f.centre_xy_m;
    const [wx, wy] = f.width_xy_m;
    if (Math.abs(x) + wx / 2 > 16 || Math.abs(y) + wy / 2 > 16)
      return prefix + "support must stay inside the map.";
    if (
      Math.hypot(
        Math.max(0, Math.abs(x) - wx / 2 - 0.25),
        Math.max(0, Math.abs(y) - wy / 2 - 0.25),
      ) <= 2
    ) {
      return prefix + "support and its 0.25 m edge must stay outside the 2 m spawn zone.";
    }
  }
  if (new TextEncoder().encode(JSON.stringify(map)).length > 128 * 1024)
    return "Map data exceeds 128 KiB.";
  return null;
}
function epochCheck(epoch: number) {
  if (!Number.isSafeInteger(epoch) || epoch < 0) throw new Error("Invalid run epoch.");
}
export const freeRideApi = {
  async start(map: GeneratedMapSpec, riderModel: RiderModel = "rigid_ankle_v1"): Promise<void> {
    const error = mapProblem(map);
    if (error) throw new Error(error);
    if (!isRiderModel(riderModel)) throw new Error("Select a supported rider model.");
    const spec: FreeRideSpec = {
      wheel: "begode-extreme",
      settings: "blank",
      rider_model: riderModel,
      map,
    };
    await invoke("free_ride_start", { spec });
  },
  async status(): Promise<FreeRideStatus> {
    const status = await invoke<FreeRideStatus>("free_ride_status");
    epochCheck(status.epoch);
    if (
      (status.mode !== "scenario" && status.mode !== "free_ride") ||
      typeof status.ready !== "boolean" ||
      typeof status.armed !== "boolean" ||
      typeof status.reason !== "string" ||
      !isRiderModel(status.rider_model) ||
      (status.source !== null && status.source !== "keyboard_slider" && status.source !== "gamepad")
    ) {
      throw new Error("Invalid free ride status.");
    }
    const keys = [
      "desired_lean_deg",
      "commanded_lean_deg",
      "actual_lean_deg",
      "heading_deg",
      "path_length_m",
      "yaw_aid_work_j",
      "roll_aid_work_j",
    ] as const;
    for (const key of keys) {
      if (status[key] !== null && !Number.isFinite(status[key]))
        throw new Error(`Invalid status field: ${key}.`);
    }
    return status;
  },
  async map(epoch: number): Promise<HeightMap> {
    epochCheck(epoch);
    const map = await invoke<HeightMap>("free_ride_map", { epoch });
    if (
      !Number.isInteger(map.revision) ||
      map.revision < 0 ||
      map.revision > 0xffff_ffff ||
      map.nx !== 129 ||
      map.ny !== 129 ||
      map.spacing_m !== 0.25 ||
      map.origin_xy_m?.[0] !== -16 ||
      map.origin_xy_m?.[1] !== -16 ||
      !Array.isArray(map.heights_mm) ||
      map.heights_mm.length !== 16641 ||
      map.heights_mm.some((v) => !Number.isInteger(v) || Math.abs(v) > 12000)
    ) {
      throw new Error("Invalid native height map.");
    }
    return map;
  },
  async arm(epoch: number, source: FreeRideSource): Promise<void> {
    epochCheck(epoch);
    await invoke("free_ride_arm", { epoch, source });
  },
  async input(input: FreeRideInput): Promise<void> {
    epochCheck(input.run_epoch);
    if (
      !Number.isSafeInteger(input.seq) ||
      input.seq < 0 ||
      input.seq > 0xffff_ffff ||
      !Number.isFinite(input.desired_lean_rad) ||
      !Number.isFinite(input.heading_rate_rad_s) ||
      Math.abs(input.desired_lean_rad) > 10 * radians ||
      Math.abs(input.heading_rate_rad_s) > 30 * radians
    ) {
      throw new Error("Free ride input exceeds its limits.");
    }
    await invoke("free_ride_input", { input });
  },
  async pause(epoch: number): Promise<void> {
    epochCheck(epoch);
    await invoke("free_ride_pause", { epoch });
  },
};
