// Controlled transport only. No firmware or physical simulation runs in this fixture. It answers
// as the play-mode engine does: no arming, a crash never pauses, a respawn is a new run that
// boots briefly, the wheel's buttons and the phone's commands are accepted.
import type { EngineClient, WheelButton } from "../../src/engine/types.ts";
import type { FreeRideInput, FreeRideSpec, FreeRideStatus } from "../../src/engine/freeRide.ts";
import type { WorldFrame } from "../../src/engine/world.ts";

/** Simulated boot after a start or a respawn, wall ms. */
const BOOT_MS = 400;
const listeners = new Set<() => void>();
const status: FreeRideStatus = {
  epoch: 1,
  mode: "scenario",
  rider_model: "articulated_sagittal_v1",
  ready: false,
  armed: false,
  source: null,
  reason: "Select Free ride to use manual controls",
  desired_lean_deg: null,
  commanded_lean_deg: null,
  actual_lean_deg: null,
  heading_deg: null,
  path_length_m: null,
  yaw_aid_work_j: null,
  roll_aid_work_j: null,
  ground: "none",
  turn_cap_rad_s: null,
  crouch: null,
  on_pedals: false,
  boot_remaining_s: null,
};
let frame: WorldFrame | null = null;
const inputs: FreeRideInput[] = [];
const starts: FreeRideSpec[] = [];
const restarts: { epoch: number; toStart: boolean }[] = [];
const buttons: { button: WheelButton; pressed: boolean; epoch: number }[] = [];
const commands: { id: string; value: number | undefined }[] = [];
let pauses = 0;
let refuseInput = false;
/** Drive caps the status reports (an engine with drive mode), or none (an older engine). */
let caps: Pick<FreeRideStatus, "accel_cap_mps2" | "brake_cap_mps2" | "accel_mps2"> | null = null;
let pad: {
  id: string;
  index: number;
  connected: boolean;
  mapping: string;
  axes: number[];
  buttons: { pressed: boolean; value: number }[];
} | null = null;

const client = {
  kind: "live",
  playing: false,
  targetRtf: 1,
  status: { state: "paused", runEpoch: 1, runMode: "scenario", message: "Fixture" },
  outputs: { controller_powered: true } as { controller_powered: boolean } | null,
  latest: { speed: 0, pwm: Number.NaN, modulation: Number.NaN },
  subscribe(fn: () => void) {
    listeners.add(fn);
    return () => listeners.delete(fn);
  },
  setPlaying(value: boolean) {
    if (!value) pauses++;
    this.playing = value;
    this.status.state = value ? "running" : "paused";
    for (const fn of listeners) fn();
  },
  setTargetRtf(value: number) {
    this.targetRtf = value;
  },
  async setButton(button: WheelButton, pressed: boolean, epoch: number) {
    buttons.push({ button, pressed, epoch });
    // Power pressed while the wheel is off boots it again.
    if (button === "power" && pressed && this.outputs && !this.outputs.controller_powered)
      this.outputs = { controller_powered: true };
    return pressed ? `${button} down` : `${button} up`;
  },
  async sendCommand(id: string, value?: number) {
    commands.push({ id, value });
    return id;
  },
};
export const engine = client as unknown as EngineClient;

/** A new run of a free ride: a new epoch that boots, then rides. */
function newRun(start: [number, number]) {
  status.epoch++;
  Object.assign(status, {
    mode: "free_ride",
    ready: false,
    armed: false,
    source: "keyboard_slider",
    reason: "The firmware is booting on the stand",
    ground: "world",
    turn_cap_rad_s: 2.5,
    crouch: 0,
    on_pedals: true,
    boot_remaining_s: 1.5,
  } satisfies Partial<FreeRideStatus>);
  frame = {
    spec: frame?.spec ?? {
      seed: 0,
      revision: 2,
      biome: "city",
      densities: {} as WorldFrame["spec"]["densities"],
    },
    start_xy_m: start,
    start_heading_rad: 0,
    origin_z_m: 0,
  };
  client.status.runEpoch = status.epoch;
  client.status.runMode = "free_ride";
  client.setPlaying(true);
  const epoch = status.epoch;
  setTimeout(() => {
    if (status.epoch !== epoch) return;
    Object.assign(status, { ready: true, armed: true, boot_remaining_s: 0, reason: "Riding" });
  }, BOOT_MS);
}

export async function invoke(command: string, args: Record<string, unknown> = {}) {
  switch (command) {
    case "free_ride_status":
      return { ...status, ...caps };
    case "free_ride_world":
      if (args.epoch !== status.epoch || !frame) throw new Error("Fixture world epoch mismatch.");
      return structuredClone(frame);
    case "free_ride_input": {
      const input = structuredClone(args.input as FreeRideInput);
      inputs.push(input);
      if (refuseInput) throw new Error("Fixture refuses the input.");
      if (input.run_epoch !== status.epoch) throw new Error("Fixture epoch changed.");
      if (input.desired_accel_mps2 !== undefined && (!caps || input.desired_lean_rad !== 0))
        throw new Error("Fixture: a drive ask needs drive caps and no lean.");
      status.source = input.source;
      return;
    }
    case "free_ride_restart":
      if (args.epoch !== status.epoch) throw new Error("Fixture restart epoch mismatch.");
      restarts.push({ epoch: args.epoch as number, toStart: args.toStart === true });
      newRun(args.toStart === true ? [0, 0] : [12, 3]);
      return;
    case "free_ride_step_off":
      if (args.epoch !== status.epoch) throw new Error("Fixture step-off epoch mismatch.");
      Object.assign(status, { on_pedals: false, crouch: null, reason: "Off the pedals" });
      return;
    case "free_ride_start": {
      const spec = structuredClone(args.spec as FreeRideSpec);
      starts.push(spec);
      const world = spec.world;
      if (!world) throw new Error("Fixture: free ride starts in a world.");
      frame = {
        spec: { seed: world.seed, revision: 2, biome: "city", densities: world.densities },
        start_xy_m: [0, 0],
        start_heading_rad: 0,
        origin_z_m: 0,
      };
      newRun([0, 0]);
      return;
    }
    default:
      throw new Error(`Unexpected fixture command: ${command}`);
  }
}

const fixture = {
  inputs,
  starts,
  restarts,
  buttons,
  commands,
  status,
  /** Times the run was paused (a crash must never pause it). */
  get pauses() {
    return pauses;
  },
  /** The rider falls off: the run goes on. */
  crash() {
    Object.assign(status, { on_pedals: false, crouch: null, reason: "Off the pedals" });
  },
  /** The wheel's controller loses or gains power (a long press on Power, say). */
  power(on: boolean) {
    client.outputs = { controller_powered: on };
  },
  refuseInput(value: boolean) {
    refuseInput = value;
  },
  /** The motor's PWM figure and the simulated modulation the engine reports, %. */
  motor(pwm: number, modulation: number) {
    Object.assign(client.latest, { pwm, modulation });
  },
  /** Reports drive caps (m/s², forward and braking) from now on, or none. */
  driveCaps(accel: number | null, brake = 0) {
    caps = accel === null ? null : { accel_cap_mps2: accel, brake_cap_mps2: brake, accel_mps2: 0 };
  },
  /** A standard pad with these axes and these buttons held (indices), or none. */
  gamepad(axes: number[] | null, held: number[] = []) {
    pad =
      axes === null
        ? null
        : {
            id: "Controlled standard gamepad",
            index: 0,
            connected: true,
            mapping: "standard",
            axes,
            buttons: Array.from({ length: 17 }, (_, i) => ({
              pressed: held.includes(i),
              value: held.includes(i) ? 1 : 0,
            })),
          };
  },
};
Object.defineProperty(navigator, "getGamepads", { value: () => [pad] });
window.freeRideFixture = fixture;
declare global {
  interface Window {
    freeRideFixture: typeof fixture;
  }
}
