import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_RIDER_MODEL,
  flatMap,
  freeRideApi,
  limitsOf,
  MANUAL_TURN_CEILING_RAD_S,
  RIDER_MODELS,
  type FreeRideStatus,
} from "./freeRide.ts";
import { DEFAULT_DENSITIES } from "./world.ts";
const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc.invoke }));
beforeEach(() => {
  ipc.invoke.mockReset();
});
describe("world IPC", () => {
  it("sends a world choice instead of a map", async () => {
    const world = { seed: 7, densities: { ...DEFAULT_DENSITIES } };
    await freeRideApi.start({ world });
    expect(ipc.invoke).toHaveBeenCalledExactlyOnceWith("free_ride_start", {
      spec: {
        wheel: "begode-extreme",
        settings: "blank",
        rider_model: "articulated_sagittal_v1",
        world,
      },
    });
  });
  it("refuses a seed or a density out of range before the engine sees it", async () => {
    await expect(
      freeRideApi.start({ world: { seed: -1, densities: { ...DEFAULT_DENSITIES } } }),
    ).rejects.toThrow(/Seed/);
    await expect(
      freeRideApi.start({ world: { seed: 1, densities: { ...DEFAULT_DENSITIES, potholes: 99 } } }),
    ).rejects.toThrow(/potholes/);
    expect(ipc.invoke).not.toHaveBeenCalled();
  });
});
describe("rider model IPC", () => {
  it.each(RIDER_MODELS)("sends $value only with a new run", async ({ value }) => {
    await freeRideApi.start({ map: flatMap() }, value);
    expect(ipc.invoke).toHaveBeenCalledExactlyOnceWith("free_ride_start", {
      spec: { wheel: "begode-extreme", settings: "blank", rider_model: value, map: flatMap() },
    });
  });
  it("starts the articulated rider unless another model is chosen", async () => {
    expect(DEFAULT_RIDER_MODEL).toBe("articulated_sagittal_v1");
    expect(RIDER_MODELS[0].value).toBe(DEFAULT_RIDER_MODEL);
    await freeRideApi.start({ map: flatMap() });
    expect(ipc.invoke).toHaveBeenCalledExactlyOnceWith("free_ride_start", {
      spec: {
        wheel: "begode-extreme",
        settings: "blank",
        rider_model: "articulated_sagittal_v1",
        map: flatMap(),
      },
    });
  });
  it.each([
    "rigid_ankle_v1",
    "articulated_sagittal_v1",
    "articulated_contact_v1",
    "unknown",
    undefined,
  ])("validates actual native model %s", async (rider_model) => {
    ipc.invoke.mockResolvedValue({
      epoch: 1,
      mode: "scenario",
      ready: false,
      armed: false,
      source: null,
      reason: "Ready",
      rider_model,
      desired_lean_deg: null,
      commanded_lean_deg: null,
      actual_lean_deg: null,
      heading_deg: null,
      path_length_m: null,
      yaw_aid_work_j: null,
      roll_aid_work_j: null,
      ground: "none",
    });
    if (rider_model === "unknown" || rider_model === undefined)
      await expect(freeRideApi.status()).rejects.toThrow("Invalid free ride status");
    else expect((await freeRideApi.status()).rider_model).toBe(rider_model);
  });
});
describe("manual limits (FR2b engines)", () => {
  const base: FreeRideStatus = {
    epoch: 3,
    mode: "free_ride",
    ready: true,
    armed: false,
    source: null,
    reason: "Ready",
    rider_model: "articulated_sagittal_v1",
    desired_lean_deg: null,
    commanded_lean_deg: null,
    actual_lean_deg: null,
    heading_deg: null,
    path_length_m: null,
    yaw_aid_work_j: null,
    roll_aid_work_j: null,
    ground: "world",
  };
  const fr2b = { ...base, turn_cap_rad_s: 1.2, crouch: 0.3, on_pedals: true, boot_remaining_s: 0 };
  const input = { run_epoch: 3, seq: 1, source: "gamepad", armed: true } as const;

  it("keep the original limits for an engine without the new fields", () => {
    expect(limitsOf(base)).toMatchObject({
      lean_max_deg: 10,
      turn_ceiling_rad_s: (30 * Math.PI) / 180,
      turn_scheduled: false,
      crouch_field: false,
      can_crouch: false,
      restart: false,
    });
  });

  it("read the speed-scheduled turn cap, the crouch and the quick reset", () => {
    expect(limitsOf(fr2b)).toMatchObject({
      turn_max_rad_s: 1.2,
      turn_ceiling_rad_s: MANUAL_TURN_CEILING_RAD_S,
      turn_scheduled: true,
      crouch_field: true,
      can_crouch: true,
      restart: true,
    });
    expect(limitsOf({ ...fr2b, crouch: null })).toMatchObject({
      crouch_field: true,
      can_crouch: false,
    });
  });

  it("refuse a status whose new fields are out of range", async () => {
    for (const bad of [
      { turn_cap_rad_s: 9 },
      { crouch: 1.5 },
      { boot_remaining_s: -1 },
      { on_pedals: "yes" },
    ]) {
      ipc.invoke.mockResolvedValueOnce({ ...fr2b, ...bad });
      await expect(freeRideApi.status()).rejects.toThrow("Invalid status field");
    }
    ipc.invoke.mockResolvedValueOnce(fr2b);
    expect((await freeRideApi.status()).turn_cap_rad_s).toBe(1.2);
  });

  it("send a crouch only to an engine that takes one, and a turn up to its ceiling", async () => {
    const motion = { desired_lean_rad: 0, heading_rate_rad_s: 2.4, crouch: 0.5 };
    await expect(freeRideApi.input({ ...input, ...motion }, limitsOf(base))).rejects.toThrow(
      /limits/,
    );
    await freeRideApi.input({ ...input, ...motion }, limitsOf(fr2b));
    expect(ipc.invoke).toHaveBeenCalledWith("free_ride_input", { input: { ...input, ...motion } });
    await expect(
      freeRideApi.input({ ...input, ...motion, heading_rate_rad_s: 2.6 }, limitsOf(fr2b)),
    ).rejects.toThrow(/limits/);
    await freeRideApi.restart(3);
    expect(ipc.invoke).toHaveBeenLastCalledWith("free_ride_restart", { epoch: 3 });
  });
});
