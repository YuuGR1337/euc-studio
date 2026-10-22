import { beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_RIDER_MODEL, flatMap, freeRideApi, RIDER_MODELS } from "./freeRide.ts";
const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc.invoke }));
beforeEach(() => {
  ipc.invoke.mockReset();
});
describe("rider model IPC", () => {
  it.each(RIDER_MODELS)("sends $value only with a new run", async ({ value }) => {
    await freeRideApi.start(flatMap(), value);
    expect(ipc.invoke).toHaveBeenCalledExactlyOnceWith("free_ride_start", {
      spec: { wheel: "begode-extreme", settings: "blank", rider_model: value, map: flatMap() },
    });
  });
  it("starts the articulated rider unless another model is chosen", async () => {
    expect(DEFAULT_RIDER_MODEL).toBe("articulated_sagittal_v1");
    expect(RIDER_MODELS[0].value).toBe(DEFAULT_RIDER_MODEL);
    await freeRideApi.start(flatMap());
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
    });
    if (rider_model === "unknown" || rider_model === undefined)
      await expect(freeRideApi.status()).rejects.toThrow("Invalid free ride status");
    else expect((await freeRideApi.status()).rider_model).toBe(rider_model);
  });
});
