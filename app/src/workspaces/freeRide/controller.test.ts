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
    restart: vi.fn(),
    stepOff: vi.fn(),
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
    ground: "map",
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
    await controller.start({ map });
    expect(freeRideApi.start).toHaveBeenCalledWith({ map }, "articulated_sagittal_v1");
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
      applied: { map },
      status: { epoch: native.epoch, rider_model: "articulated_sagittal_v1" },
    });
  });
  it("does not label a different native model as the requested run", async () => {
    controller.setRiderModel("articulated_sagittal_v1");
    await controller.start({ map: flatMap() });
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
    await controller.start({ map: flatMap() });
    expect(controller.getSnapshot()).toMatchObject({
      starting: false,
      error: "Build failed",
      status: { rider_model: "rigid_ankle_v1" },
    });
  });
  it("follows an external reset after the selected model has been applied", async () => {
    controller.setRiderModel("articulated_sagittal_v1");
    await controller.start({ map: flatMap() });
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
    const start = controller.start({ map: flatMap() });
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

const press = (type: "keydown" | "keyup", code: string, repeat = false) => {
  const event = Object.assign(new Event(type, { cancelable: true }), {
    code,
    repeat,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
  });
  window.dispatchEvent(event);
  return event;
};
const sentInputs = () => vi.mocked(freeRideApi.input).mock.calls.map(([input]) => input);
const lastSent = () => sentInputs().at(-1);
const setSpeedKmh = (kmh: number) =>
  Object.assign(engine as unknown as { latest: { speed: number } }, { latest: { speed: kmh } });
const radians = (deg: number) => (deg * Math.PI) / 180;

describe("riding by push", () => {
  it("arms on the first push and ramps the lean up from a neutral first packet", async () => {
    expect(controller.getSnapshot().armed).toBe(false);
    const down = press("keydown", "KeyW");
    expect(down.defaultPrevented).toBe(true);
    await vi.advanceTimersByTimeAsync(100);
    expect(freeRideApi.arm).toHaveBeenCalledWith(native.epoch, "keyboard_slider");
    expect(controller.getSnapshot().armed).toBe(true);
    const firstArmed = sentInputs().find((input) => input.armed);
    expect(firstArmed?.desired_lean_rad).toBe(0);
    await vi.advanceTimersByTimeAsync(50);
    const early = lastSent()?.desired_lean_rad ?? 0;
    expect(early).toBeGreaterThan(0);
    expect(early).toBeLessThan(radians(10) / 2);
    await vi.advanceTimersByTimeAsync(1500);
    expect(lastSent()?.desired_lean_rad).toBeCloseTo(radians(10), 3);
    press("keyup", "KeyW");
    await vi.advanceTimersByTimeAsync(1500);
    expect(Math.abs(lastSent()?.desired_lean_rad ?? 1)).toBeLessThan(1e-3);
  });

  it("does not ride on keys typed into a text field", async () => {
    const field = Object.assign(new Surface(), {
      closest: (selector: string) => (selector.includes("textarea") ? {} : null),
    });
    const event = Object.assign(new Event("keydown", { cancelable: true }), {
      code: "KeyW",
      repeat: false,
      metaKey: false,
      ctrlKey: false,
      altKey: false,
    });
    field.addEventListener("keydown", () => {});
    Object.defineProperty(event, "target", { value: field });
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    await vi.advanceTimersByTimeAsync(300);
    expect(freeRideApi.arm).not.toHaveBeenCalled();
  });

  it("on an older engine, asks for no more turn at speed than a 20° turn lean holds", async () => {
    setSpeedKmh(36);
    press("keydown", "KeyA");
    await vi.advanceTimersByTimeAsync(1500);
    const rate = lastSent()?.heading_rate_rad_s ?? 0;
    // 10 m/s with a 20° turn lean: g·tan(20°)/v = 0.357 rad/s, under the 30°/s (0.524) limit.
    expect(rate).toBeCloseTo((9.81 * Math.tan(radians(20))) / 10, 2);
    press("keyup", "KeyA");
  });

  it("gives a full turn the engine's own cap at the current speed", async () => {
    native = { ...native, on_pedals: true, crouch: 0, turn_cap_rad_s: 1.7 };
    await vi.advanceTimersByTimeAsync(300);
    press("keydown", "KeyD");
    await vi.advanceTimersByTimeAsync(1500);
    expect(lastSent()?.heading_rate_rad_s).toBeCloseTo(-1.7, 3);
    native.turn_cap_rad_s = 0.6;
    await vi.advanceTimersByTimeAsync(300);
    expect(lastSent()?.heading_rate_rad_s).toBeCloseTo(-0.6, 3);
    press("keyup", "KeyD");
  });

  it("stops before it reverses: a back lean near a stop waits 0.35 s", async () => {
    setSpeedKmh(0);
    press("keydown", "KeyS");
    await vi.advanceTimersByTimeAsync(300);
    expect(controller.getSnapshot().armed).toBe(true);
    expect(lastSent()?.desired_lean_rad).toBe(0);
    await vi.advanceTimersByTimeAsync(800);
    expect(lastSent()?.desired_lean_rad ?? 0).toBeLessThan(0);
    press("keyup", "KeyS");
  });

  it("keeps Space for crouch and sends it only when the run reports a crouch", async () => {
    const space = press("keydown", "Space");
    expect(space.defaultPrevented).toBe(true);
    press("keyup", "Space");
    press("keydown", "KeyW");
    await vi.advanceTimersByTimeAsync(300);
    // An older engine refuses unknown fields: no crouch in its packets.
    expect(lastSent()).not.toHaveProperty("crouch");
    press("keyup", "KeyW");
    native = { ...native, on_pedals: true, crouch: null, turn_cap_rad_s: 2.5 };
    await vi.advanceTimersByTimeAsync(300);
    press("keydown", "Space");
    await vi.advanceTimersByTimeAsync(500);
    // The rigid rider (crouch null) gets 0; an articulated one gets the eased share.
    expect(lastSent()?.crouch).toBe(0);
    native.crouch = 0;
    await vi.advanceTimersByTimeAsync(500);
    expect(lastSent()?.crouch).toBeCloseTo(1, 2);
    press("keyup", "Space");
    await vi.advanceTimersByTimeAsync(500);
    expect(lastSent()?.crouch).toBeCloseTo(0, 2);
  });

  it("Escape pauses, and a held key must be let go before it rides again", async () => {
    press("keydown", "KeyW");
    await vi.advanceTimersByTimeAsync(300);
    expect(controller.getSnapshot().armed).toBe(true);
    press("keydown", "Escape");
    expect(controller.getSnapshot().armed).toBe(false);
    native.armed = false;
    press("keydown", "KeyW", true);
    await vi.advanceTimersByTimeAsync(600);
    expect(controller.getSnapshot().armed).toBe(false);
    press("keyup", "KeyW");
    press("keydown", "KeyW");
    await vi.advanceTimersByTimeAsync(600);
    expect(controller.getSnapshot().armed).toBe(true);
  });
});

describe("restart countdown", () => {
  it("R counts down and restarts the run on its ground; R again cancels", async () => {
    const map = flatMap(3);
    await controller.start({ map });
    native = { ...native, epoch: native.epoch + 1 };
    await vi.advanceTimersByTimeAsync(300);
    vi.mocked(freeRideApi.start).mockClear();
    press("keydown", "KeyR");
    expect(controller.getSnapshot().countdown).not.toBeNull();
    press("keydown", "KeyR");
    expect(controller.getSnapshot().countdown).toBeNull();
    press("keydown", "KeyR");
    await vi.advanceTimersByTimeAsync(2900);
    expect(freeRideApi.start).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(200);
    expect(freeRideApi.start).toHaveBeenCalledWith({ map }, controller.getSnapshot().riderModel);
  });

  it("restarts where the wheel is on an engine with the quick reset", async () => {
    native = { ...native, ground: "world", on_pedals: true, crouch: 0, turn_cap_rad_s: 2.5 };
    await vi.advanceTimersByTimeAsync(300);
    press("keydown", "KeyR");
    expect(controller.getSnapshot().countdown?.here).toBe(true);
    await vi.advanceTimersByTimeAsync(3100);
    expect(freeRideApi.restart).toHaveBeenCalledWith(native.epoch);
    expect(controller.getSnapshot().starting).toBe(true);
    native = { ...native, epoch: native.epoch + 1 };
    await vi.advanceTimersByTimeAsync(300);
    expect(controller.getSnapshot()).toMatchObject({ starting: false, armed: false });
  });

  it("X steps off on an engine that can, and never arms a rider off the wheel", async () => {
    vi.mocked(freeRideApi.stepOff).mockResolvedValue(undefined);
    press("keydown", "KeyW");
    await vi.advanceTimersByTimeAsync(300);
    expect(controller.getSnapshot().armed).toBe(true);
    press("keydown", "KeyX");
    expect(freeRideApi.stepOff).not.toHaveBeenCalled(); // an older engine has no step-off
    native = { ...native, on_pedals: true, crouch: 0, turn_cap_rad_s: 2.5 };
    await vi.advanceTimersByTimeAsync(300);
    press("keyup", "KeyX");
    press("keydown", "KeyX");
    expect(freeRideApi.stepOff).toHaveBeenCalledWith(native.epoch);
    native = { ...native, armed: false, on_pedals: false, crouch: null };
    await vi.advanceTimersByTimeAsync(300);
    expect(controller.getSnapshot().armed).toBe(false);
    press("keyup", "KeyW");
    vi.mocked(freeRideApi.arm).mockClear();
    press("keydown", "KeyW");
    await vi.advanceTimersByTimeAsync(600);
    expect(freeRideApi.arm).not.toHaveBeenCalled();
  });

  it("Escape cancels the countdown", async () => {
    press("keydown", "KeyR");
    press("keydown", "Escape");
    await vi.advanceTimersByTimeAsync(3500);
    expect(controller.getSnapshot().countdown).toBeNull();
    expect(freeRideApi.restart).not.toHaveBeenCalled();
    expect(freeRideApi.start).not.toHaveBeenCalled();
  });
});

describe("key bindings", () => {
  it("binds the next key press and refuses the studio's keys", () => {
    controller.captureKey("crouch");
    press("keydown", "KeyC");
    expect(controller.getSnapshot().bindError).toMatch(/kept for the studio/);
    press("keydown", "ShiftLeft");
    expect(controller.getSnapshot()).toMatchObject({ capturing: null, bindError: null });
    expect(controller.getSnapshot().bindings.crouch).toEqual(["ShiftLeft", "Space"]);
    controller.resetKeys();
    expect(controller.getSnapshot().bindings.crouch).toEqual(["Space"]);
  });
});
