import { invoke } from "@tauri-apps/api/core";
import { validFrame, worldProblem, type WorldChoice, type WorldFrame } from "./world.ts";

export type FreeRideSource = "keyboard_slider" | "gamepad";
export type RiderModel = "articulated_sagittal_v1" | "articulated_contact_v1" | "rigid_ankle_v1";
/** The default rider first; the same models and default as the scenarios. */
export const RIDER_MODELS = [
  { value: "articulated_sagittal_v1", label: "Articulated" },
  { value: "articulated_contact_v1", label: "Articulated, foot contact (experimental)" },
  { value: "rigid_ankle_v1", label: "Rigid baseline" },
] as const;
export const DEFAULT_RIDER_MODEL: RiderModel = "articulated_sagittal_v1";
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
/** What a free ride rides on: a streaming world (the default) or the 32 m test pad. */
export type GroundChoice = { world: WorldChoice } | { map: GeneratedMapSpec };
export const isWorldChoice = (ground: GroundChoice): ground is { world: WorldChoice } =>
  "world" in ground;
/**
 * The settings journal Free Ride boots the stock firmware with (profiles/begode-extreme/
 * settings.toml). "neutral" is the factory record with level pedals (pedal trim 4); "blank" is
 * the factory-fresh record, whose pedals sit 4.8° nose down, so the wheel rides nose down.
 */
export const SETTINGS_PRESETS = ["neutral", "blank"] as const;
export type SettingsPreset = (typeof SETTINGS_PRESETS)[number];
export const DEFAULT_SETTINGS: SettingsPreset = "neutral";
export interface FreeRideSpec {
  wheel: "begode-extreme";
  settings: SettingsPreset;
  rider_model: RiderModel;
  map?: GeneratedMapSpec;
  world?: WorldChoice;
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
  /** The ground of this run. */
  ground: "map" | "world" | "none";
  // Engines from Free Ride's FR2b on (app/src-tauri/src/engine/free_ride.rs) add the fields below;
  // older engines leave them out.
  /** The largest manual turn rate at the current speed, rad/s: what a full turn input gets. */
  turn_cap_rad_s?: number | null;
  /** The crouch the legs hold, 0 to 1; null when the rider cannot crouch. */
  crouch?: number | null;
  /** The rider stands on the pedals. */
  on_pedals?: boolean;
  /** Simulated seconds until the firmware has booted on the stand. */
  boot_remaining_s?: number | null;
  // Engines with drive mode add the three below (null outside a free ride); their presence means
  // a packet may carry `desired_accel_mps2`.
  /** The strongest forward acceleration the rider's drive lean asks for at this speed, m/s², net
   * of rolling resistance and air drag. The firmware may deliver less. */
  accel_cap_mps2?: number | null;
  /** The strongest braking at this speed, m/s² (positive), drag included. */
  brake_cap_mps2?: number | null;
  /** The wheel's achieved acceleration along its heading, m/s², smoothed. */
  accel_mps2?: number | null;
}
export interface FreeRideInput {
  run_epoch: number;
  seq: number;
  source: FreeRideSource;
  desired_lean_rad: number;
  heading_rate_rad_s: number;
  /** Crouch, 0 (standing) to 1 (full); only to engines that report a crouch. */
  crouch?: number;
  /**
   * Drive mode (engines that report drive caps): the asked acceleration, m/s², forward positive,
   * net of drag, at most 12 either way. With it `desired_lean_rad` must be 0: the rider leans for
   * the acceleration itself.
   */
  desired_accel_mps2?: number;
  armed: boolean;
}
export const FREE_RIDE_LIMITS = {
  leanDeg: 10,
  turnDegS: 30,
  heartbeatMs: 50,
  neutralMs: 250,
  watchdogMs: 250,
  features: 32,
  /** The largest drive ask a packet may carry, m/s² either way. */
  accelMps2: 12,
} as const;
const radians = Math.PI / 180;
/** The manual turn rate an FR2b engine accepts in a packet, rad/s (the rider's cap at a stop). */
export const MANUAL_TURN_CEILING_RAD_S = 2.5;
/** What manual input may ask for in a run, read from its status. */
export interface FreeRideLimits {
  lean_max_deg: number;
  /** What a full turn asks for now, rad/s. */
  turn_max_rad_s: number;
  /** The largest turn rate a packet may carry, rad/s. */
  turn_ceiling_rad_s: number;
  /** The engine reports its own speed-scheduled turn cap (turn_max_rad_s is it). */
  turn_scheduled: boolean;
  /** The engine takes a crouch in its packets. */
  crouch_field: boolean;
  /** The rider can crouch now (articulated, on the pedals). */
  can_crouch: boolean;
  /** The engine restarts a run where the wheel is (free_ride_restart). */
  restart: boolean;
  /** The engine lets the rider step off (free_ride_step_off). */
  step_off: boolean;
  /** The engine takes a drive ask (`desired_accel_mps2`) in its packets. */
  drive_field: boolean;
}
/** The run's limits: an FR2b engine's own, else the original 10° and 30°/s without a crouch. */
export function limitsOf(status: FreeRideStatus | null | undefined): FreeRideLimits {
  const fr2b = !!status && "on_pedals" in status;
  const cap = status?.turn_cap_rad_s;
  const ceiling = fr2b ? MANUAL_TURN_CEILING_RAD_S : FREE_RIDE_LIMITS.turnDegS * radians;
  const scheduled = typeof cap === "number" && Number.isFinite(cap) && cap >= 0;
  return {
    lean_max_deg: FREE_RIDE_LIMITS.leanDeg,
    turn_max_rad_s: scheduled ? Math.min(cap, ceiling) : FREE_RIDE_LIMITS.turnDegS * radians,
    turn_ceiling_rad_s: ceiling,
    turn_scheduled: scheduled,
    crouch_field: !!status && "crouch" in status,
    can_crouch: typeof status?.crouch === "number",
    restart: fr2b,
    step_off: fr2b,
    drive_field: !!status && "accel_cap_mps2" in status,
  };
}
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
  async start(
    ground: GroundChoice,
    riderModel: RiderModel = DEFAULT_RIDER_MODEL,
    settings: SettingsPreset = DEFAULT_SETTINGS,
  ): Promise<void> {
    const error = isWorldChoice(ground) ? worldProblem(ground.world) : mapProblem(ground.map);
    if (error) throw new Error(error);
    if (!isRiderModel(riderModel)) throw new Error("Select a supported rider model.");
    if (!SETTINGS_PRESETS.includes(settings)) throw new Error("Select a settings preset.");
    const spec: FreeRideSpec = {
      wheel: "begode-extreme",
      settings,
      rider_model: riderModel,
      ...ground,
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
      (status.source !== null &&
        status.source !== "keyboard_slider" &&
        status.source !== "gamepad") ||
      (status.ground !== "map" && status.ground !== "world" && status.ground !== "none")
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
    const optional = (value: unknown, lo: number, hi: number) =>
      value === undefined ||
      value === null ||
      (typeof value === "number" && Number.isFinite(value) && value >= lo && value <= hi);
    if (!optional(status.turn_cap_rad_s, 0, MANUAL_TURN_CEILING_RAD_S))
      throw new Error("Invalid status field: turn_cap_rad_s.");
    if (!optional(status.crouch, 0, 1)) throw new Error("Invalid status field: crouch.");
    if (!optional(status.boot_remaining_s, 0, 3600))
      throw new Error("Invalid status field: boot_remaining_s.");
    for (const key of ["accel_cap_mps2", "brake_cap_mps2", "accel_mps2"] as const) {
      if (!optional(status[key], -1000, 1000)) throw new Error(`Invalid status field: ${key}.`);
    }
    if (status.on_pedals !== undefined && typeof status.on_pedals !== "boolean")
      throw new Error("Invalid status field: on_pedals.");
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
  async world(epoch: number): Promise<WorldFrame> {
    epochCheck(epoch);
    const frame = await invoke<WorldFrame>("free_ride_world", { epoch });
    if (!validFrame(frame)) throw new Error("Invalid native world frame.");
    return frame;
  },
  async input(input: FreeRideInput, limits: FreeRideLimits = limitsOf(null)): Promise<void> {
    epochCheck(input.run_epoch);
    if (
      !Number.isSafeInteger(input.seq) ||
      input.seq < 0 ||
      input.seq > 0xffff_ffff ||
      !Number.isFinite(input.desired_lean_rad) ||
      !Number.isFinite(input.heading_rate_rad_s) ||
      Math.abs(input.desired_lean_rad) > limits.lean_max_deg * radians + 1e-12 ||
      Math.abs(input.heading_rate_rad_s) > limits.turn_ceiling_rad_s + 1e-12 ||
      (input.crouch !== undefined &&
        (!limits.crouch_field ||
          !Number.isFinite(input.crouch) ||
          input.crouch < 0 ||
          input.crouch > 1)) ||
      (input.desired_accel_mps2 !== undefined &&
        (!limits.drive_field ||
          !Number.isFinite(input.desired_accel_mps2) ||
          Math.abs(input.desired_accel_mps2) > FREE_RIDE_LIMITS.accelMps2 ||
          input.desired_lean_rad !== 0))
    ) {
      throw new Error("Free ride input exceeds its limits.");
    }
    await invoke("free_ride_input", { input });
  },
  /**
   * Respawn: a new run of the same free ride, the wheel and the rider upright at the last clear
   * spot the wheel passed (`toStart`: at the world's start), the firmware booting again.
   */
  async restart(epoch: number, toStart = false): Promise<void> {
    epochCheck(epoch);
    await invoke("free_ride_restart", { epoch, toStart });
  },
  /** The rider steps off the pedals; the run goes on, and a respawn rides again. */
  async stepOff(epoch: number): Promise<void> {
    epochCheck(epoch);
    await invoke("free_ride_step_off", { epoch });
  },
};
