import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  flatMap,
  freeRideApi,
  type FreeRideStatus,
  type HeightMap,
} from "../../engine/freeRide.ts";
import type { EngineClient } from "../../engine/types.ts";
import { FreeRideController } from "./controller.ts";
vi.mock("../../engine/freeRide.ts", async (original) => ({
  ...(await original<typeof import("../../engine/freeRide.ts")>()),
  freeRideApi: {
    start: vi.fn(),
    status: vi.fn(),
    map: vi.fn(),
    pause: vi.fn(),
    input: vi.fn(),
    arm: vi.fn(),
  },
}));
class Surface extends EventTarget {
  closest() {
    return null;
  }
  contains(target: unknown) {
    return target === this;
  }
}
const grid: HeightMap = {
  revision: 0,
  nx: 129,
  ny: 129,
  spacing_m: 0.25,
  origin_xy_m: [-16, -16],
  heights_mm: new Array<number>(16641).fill(0),
};
let epoch = 0;
let native: FreeRideStatus;
let controller: FreeRideController;
let close: () => void;
let engine: EngineClient;
beforeEach(async () => {
  vi.useFakeTimers({
    toFake: ["setInterval", "clearInterval", "setTimeout", "clearTimeout", "performance"],
  });
  vi.clearAllMocks();
  const root = new Surface();
  vi.stubGlobal("window", new EventTarget());
  vi.stubGlobal("Node", Surface);
  vi.stubGlobal("Element", Surface);
  vi.stubGlobal(
    "document",
    Object.assign(new EventTarget(), { hidden: false, activeElement: root, hasFocus: () => true }),
  );
  native = {
    epoch: (epoch += 10),
    mode: "free_ride",
    ready: true,
    armed: false,
    source: "keyboard_slider",
    reason: "Ready",
    rider_model: "rigid_ankle_v1",
    desired_lean_deg: 0,
    commanded_lean_deg: 0,
    actual_lean_deg: 0,
    heading_deg: 0,
    path_length_m: 0,
    yaw_aid_work_j: 0,
    roll_aid_work_j: 0,
  };
  const listeners = new Set<() => void>();
  let playing = true;
  engine = {
    kind: "live",
    get playing() {
      return playing;
    },
    targetRtf: 1,
    status: {
      state: "running",
      runMode: "free_ride",
      runEpoch: native.epoch,
      backend: "own",
      message: "",
    },
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    setPlaying: vi.fn((next: boolean) => {
      playing = next;
      for (const listener of listeners) listener();
    }),
    setTargetRtf: vi.fn(),
  } as unknown as EngineClient;
  vi.mocked(freeRideApi.status).mockImplementation(async () => ({ ...native }));
  vi.mocked(freeRideApi.map).mockResolvedValue(grid);
  vi.mocked(freeRideApi.start).mockResolvedValue(undefined);
  vi.mocked(freeRideApi.pause).mockResolvedValue(undefined);
  vi.mocked(freeRideApi.input).mockResolvedValue(undefined);
  vi.mocked(freeRideApi.arm).mockImplementation(async () => {
    native.armed = true;
  });
  controller = new FreeRideController(engine);
  close = controller.connect(root as unknown as HTMLElement);
  await vi.advanceTimersByTimeAsync(500);
});
afterEach(async () => {
  close();
  await Promise.resolve();
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
describe("rider model run boundary", () => {
  it("pauses and disarms without changing the current model or starting a run", async () => {
    await controller.arm();
    expect(controller.getSnapshot().armed).toBe(true);
    controller.setRiderModel("articulated_sagittal_v1");
    expect(engine.setPlaying).toHaveBeenCalledWith(false);
    expect(controller.getSnapshot()).toMatchObject({
      armed: false,
      playing: false,
      riderModel: "articulated_sagittal_v1",
      status: { rider_model: "rigid_ankle_v1" },
    });
    expect(freeRideApi.start).not.toHaveBeenCalled();
    expect(freeRideApi.pause).toHaveBeenCalledWith(native.epoch);
    await vi.advanceTimersByTimeAsync(300);
    expect(controller.getSnapshot().riderModel).toBe("articulated_sagittal_v1");
  });
  it("captures the draft model and waits for a newer native epoch", async () => {
    controller.setRiderModel("articulated_sagittal_v1");
    const map = flatMap(7);
    await controller.start(map);
    expect(freeRideApi.start).toHaveBeenCalledWith(map, "articulated_sagittal_v1");
    controller.setRiderModel("rigid_ankle_v1");
    expect(controller.getSnapshot().riderModel).toBe("articulated_sagittal_v1");
    await vi.advanceTimersByTimeAsync(300);
    expect(controller.getSnapshot()).toMatchObject({
      starting: true,
      applied: null,
      status: { rider_model: "rigid_ankle_v1" },
    });
    native = { ...native, epoch: native.epoch + 1, rider_model: "articulated_sagittal_v1" };
    await vi.advanceTimersByTimeAsync(300);
    expect(controller.getSnapshot()).toMatchObject({
      starting: false,
      applied: map,
      status: { epoch: native.epoch, rider_model: "articulated_sagittal_v1" },
    });
  });
  it("does not label a different native model as the requested run", async () => {
    controller.setRiderModel("articulated_sagittal_v1");
    await controller.start(flatMap());
    native = { ...native, epoch: native.epoch + 1 };
    await vi.advanceTimersByTimeAsync(300);
    expect(controller.getSnapshot()).toMatchObject({
      starting: false,
      applied: null,
      armed: false,
      status: { rider_model: "rigid_ankle_v1" },
    });
    expect(controller.getSnapshot().error).toContain("differs");
    expect(freeRideApi.pause).toHaveBeenCalledWith(native.epoch);
  });
  it("keeps actual status on a failed start", async () => {
    controller.setRiderModel("articulated_sagittal_v1");
    vi.mocked(freeRideApi.start).mockRejectedValueOnce(new Error("Build failed"));
    await controller.start(flatMap());
    expect(controller.getSnapshot()).toMatchObject({
      starting: false,
      error: "Build failed",
      status: { rider_model: "rigid_ankle_v1" },
    });
  });
  it("follows an external reset after the selected model has been applied", async () => {
    controller.setRiderModel("articulated_sagittal_v1");
    await controller.start(flatMap());
    native = { ...native, epoch: native.epoch + 1, rider_model: "articulated_sagittal_v1" };
    await vi.advanceTimersByTimeAsync(300);
    native = { ...native, epoch: native.epoch + 1, rider_model: "rigid_ankle_v1" };
    await vi.advanceTimersByTimeAsync(300);
    expect(controller.getSnapshot()).toMatchObject({
      riderModel: "rigid_ankle_v1",
      applied: null,
      status: { epoch: native.epoch, rider_model: "rigid_ankle_v1" },
    });
  });
  it("reads the actual model after an external reset when there is no draft edit", async () => {
    native = { ...native, epoch: native.epoch + 1, rider_model: "articulated_sagittal_v1" };
    await vi.advanceTimersByTimeAsync(300);
    expect(controller.getSnapshot()).toMatchObject({
      riderModel: "articulated_sagittal_v1",
      status: { epoch: native.epoch, rider_model: "articulated_sagittal_v1" },
    });
  });
  it("pauses a queued run after the control surface closes", async () => {
    controller.setRiderModel("articulated_sagittal_v1");
    let accepted!: () => void;
    vi.mocked(freeRideApi.start).mockReturnValueOnce(
      new Promise<void>((resolve) => {
        accepted = resolve;
      }),
    );
    const start = controller.start(flatMap());
    close();
    close = () => {};
    native = { ...native, epoch: native.epoch + 1, rider_model: "articulated_sagittal_v1" };
    accepted();
    await start;
    await vi.advanceTimersByTimeAsync(100);
    expect(freeRideApi.pause).toHaveBeenCalledWith(native.epoch);
    expect(controller.getSnapshot()).toMatchObject({ armed: false, starting: false });
  });
});
