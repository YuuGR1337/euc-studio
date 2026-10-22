// Controlled transport only. No firmware or physical simulation runs in this fixture.
import type { EngineClient } from "../../src/engine/types.ts";
import type { FreeRideInput, FreeRideSpec, FreeRideStatus } from "../../src/engine/freeRide.ts";

const listeners = new Set<() => void>();
const status: FreeRideStatus = {
  epoch: 1,
  mode: "free_ride",
  rider_model: "rigid_ankle_v1",
  ready: true,
  armed: false,
  source: "keyboard_slider",
  reason: "Fixture ready. No firmware runs.",
  desired_lean_deg: 0,
  commanded_lean_deg: 0,
  actual_lean_deg: 0,
  heading_deg: 0,
  path_length_m: 0,
  yaw_aid_work_j: 0,
  roll_aid_work_j: 0,
};
const inputs: FreeRideInput[] = [];
const starts: FreeRideSpec[] = [];
let pauses = 0;
let pad: { id: string; index: number; connected: boolean; mapping: string; axes: number[] } | null =
  null;
let rejectStart = false;
let delayInputs = false;
const pending: (() => void)[] = [];
const client = {
  kind: "live",
  playing: false,
  targetRtf: 1,
  status: { state: "paused", runEpoch: 1, runMode: "free_ride", message: "Fixture" },
  subscribe(fn: () => void) {
    listeners.add(fn);
    return () => listeners.delete(fn);
  },
  setPlaying(value: boolean) {
    this.playing = value;
    this.status.state = value ? "running" : "paused";
    for (const fn of listeners) fn();
  },
  setTargetRtf(value: number) {
    this.targetRtf = value;
  },
};
export const engine = client as unknown as EngineClient;

export async function invoke(command: string, args: Record<string, unknown> = {}) {
  switch (command) {
    case "free_ride_status":
      return { ...status };
    case "free_ride_map":
      if (args.epoch !== status.epoch) throw new Error("Fixture map epoch mismatch.");
      return {
        revision: status.epoch,
        nx: 129,
        ny: 129,
        spacing_m: 0.25,
        origin_xy_m: [-16, -16],
        heights_mm: Array.from({ length: 16641 }, () => 0),
      };
    case "free_ride_input": {
      const input = structuredClone(args.input as FreeRideInput);
      inputs.push(input);
      if (delayInputs) await new Promise<void>((resolve) => pending.push(resolve));
      if (input.run_epoch !== status.epoch) throw new Error("Fixture epoch changed.");
      status.source = input.source;
      if (!input.armed) status.armed = false;
      return;
    }
    case "free_ride_arm":
      if (args.epoch !== status.epoch || args.source !== status.source)
        throw new Error("Fixture Arm epoch or source mismatch.");
      status.armed = true;
      client.setPlaying(true);
      return;
    case "free_ride_pause":
      if (args.epoch !== status.epoch) throw new Error("Fixture Pause epoch mismatch.");
      pauses++;
      status.armed = false;
      client.setPlaying(false);
      return;
    case "free_ride_start": {
      const spec = structuredClone(args.spec as FreeRideSpec);
      starts.push(spec);
      if (rejectStart) throw new Error("Fixture native terrain rejection.");
      status.epoch++;
      status.rider_model = spec.rider_model;
      status.armed = false;
      client.status.runEpoch = status.epoch;
      client.setPlaying(false);
      return;
    }
    default:
      throw new Error(`Unexpected fixture command: ${command}`);
  }
}

const fixture = {
  inputs,
  starts,
  status,
  get pauses() {
    return pauses;
  },
  rejectMap(value: boolean) {
    rejectStart = value;
  },
  delayInput(value: boolean) {
    delayInputs = value;
    if (!value) pending.splice(0).forEach((resolve) => resolve());
  },
  gamepad(axes: number[] | null) {
    pad =
      axes === null
        ? null
        : {
            id: "Controlled standard gamepad",
            index: 0,
            connected: true,
            mapping: "standard",
            axes,
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
