import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { freeRideApi, type FreeRideStatus } from "../../engine/freeRide.ts";
import type { WorldFrame } from "../../engine/world.ts";
import type { EngineClient } from "../../engine/types.ts";
import { DEFAULT_WORLD, FreeRideController } from "./controller.ts";
import { useFreeRideHud } from "./hud.ts";
vi.mock("../../engine/freeRide.ts", async (original) => ({
  ...(await original<typeof import("../../engine/freeRide.ts")>()),
  freeRideApi: {
    start: vi.fn(),
    status: vi.fn(),
    map: vi.fn(),
    world: vi.fn(),
    input: vi.fn(),
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
const frame: WorldFrame = {
  spec: { seed: 42, revision: 2, biome: "city", densities: { ...DEFAULT_WORLD.densities } },
  start_xy_m: [0, 0],
  start_heading_rad: 0,
  origin_z_m: 0,
};
let epoch = 0;
let native: FreeRideStatus;
let controller: FreeRideController;
let close: () => void;
let engine: EngineClient;
let outputs: { controller_powered: boolean };
const setButton = vi.fn(async (_button: string, _pressed: boolean, _epoch: number) => "ok");
const sendCommand = vi.fn(async (_id: string, _value?: number) => "sent");
const ride = (patch: Partial<FreeRideStatus> = {}): FreeRideStatus => ({
  epoch: (epoch += 10),
  mode: "free_ride",
  ready: true,
  armed: true,
  source: "keyboard_slider",
  reason: "Ready",
  rider_model: "articulated_sagittal_v1",
  desired_lean_deg: 0,
  commanded_lean_deg: 0,
  actual_lean_deg: 0,
  heading_deg: 0,
  path_length_m: 0,
  yaw_aid_work_j: 0,
  roll_aid_work_j: 0,
  ground: "world",
  turn_cap_rad_s: 2.5,
  crouch: 0,
  on_pedals: true,
  boot_remaining_s: 0,
  ...patch,
});

async function open(status: FreeRideStatus) {
  native = status;
  controller = new FreeRideController(engine);
  close = controller.connect();
  await vi.advanceTimersByTimeAsync(300);
}

beforeEach(() => {
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
  const listeners = new Set<() => void>();
  let playing = false;
  outputs = { controller_powered: true };
  engine = {
    kind: "live",
    get playing() {
      return playing;
    },
    targetRtf: 1,
    status: { state: "running", runMode: "free_ride", runEpoch: 1, backend: "own", message: "" },
    get outputs() {
      return outputs;
    },
    latest: { speed: 0 },
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    setPlaying: vi.fn((next: boolean) => {
      playing = next;
      for (const listener of listeners) listener();
    }),
    setTargetRtf: vi.fn(),
    setButton,
    sendCommand,
  } as unknown as EngineClient;
  vi.mocked(freeRideApi.status).mockImplementation(async () => {
    engine.status.runEpoch = native.epoch;
    return { ...native };
  });
  vi.mocked(freeRideApi.world).mockResolvedValue(frame);
  vi.mocked(freeRideApi.start).mockResolvedValue(undefined);
  vi.mocked(freeRideApi.input).mockResolvedValue(undefined);
  vi.mocked(freeRideApi.restart).mockResolvedValue(undefined);
  vi.mocked(freeRideApi.stepOff).mockResolvedValue(undefined);
});
afterEach(async () => {
  close();
  await Promise.resolve();
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const press = (
  type: "keydown" | "keyup",
  code: string,
  { repeat = false, shiftKey = false } = {},
) => {
  const event = Object.assign(new Event(type, { cancelable: true }), {
    code,
    repeat,
    shiftKey,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
  });
  window.dispatchEvent(event);
  return event;
};
const tap = (code: string, options: { shiftKey?: boolean } = {}) => {
  press("keydown", code, options);
  press("keyup", code, options);
};
const sentInputs = () => vi.mocked(freeRideApi.input).mock.calls.map(([input]) => input);
const lastSent = () => sentInputs().at(-1);
const radians = (deg: number) => (deg * Math.PI) / 180;

describe("opening Free Ride", () => {
  it("starts a ride in the default city when there is none, then plays it", async () => {
    await open({ ...ride(), mode: "scenario", ready: false, armed: false, ground: "none" });
    expect(freeRideApi.start).toHaveBeenCalledWith(
      { world: DEFAULT_WORLD },
      "articulated_sagittal_v1",
    );
    expect(controller.getSnapshot()).toMatchObject({ starting: true, startingKind: "world" });
    native = ride({ ready: false, boot_remaining_s: 1.2 });
    await vi.advanceTimersByTimeAsync(300);
    expect(engine.setPlaying).toHaveBeenCalledWith(true);
    expect(controller.getSnapshot()).toMatchObject({ starting: false, world: frame });
    expect(useFreeRideHud.getState().boot).toBeCloseTo(1.2);
    // Only once per opening, even while the new run is on its way.
    expect(freeRideApi.start).toHaveBeenCalledTimes(1);
  });

  it("rides a free ride that already runs, and starts nothing", async () => {
    await open(ride());
    expect(freeRideApi.start).not.toHaveBeenCalled();
    expect(engine.setPlaying).toHaveBeenCalledWith(true);
  });
});

describe("riding without arming", () => {
  it("keys ride as soon as the firmware is ready, the lean ramping up", async () => {
    await open(ride({ ready: false, armed: false, boot_remaining_s: 0.5 }));
    const down = press("keydown", "KeyW");
    expect(down.defaultPrevented).toBe(true);
    await vi.advanceTimersByTimeAsync(500);
    expect(sentInputs()).toHaveLength(0);
    native = { ...native, ready: true, armed: true, boot_remaining_s: 0 };
    await vi.advanceTimersByTimeAsync(200);
    const early = lastSent()?.desired_lean_rad ?? 0;
    expect(early).toBeGreaterThan(0);
    expect(early).toBeLessThan(radians(10));
    await vi.advanceTimersByTimeAsync(1500);
    expect(lastSent()).toMatchObject({ armed: true, source: "keyboard_slider" });
    expect(lastSent()?.desired_lean_rad).toBeCloseTo(radians(10), 3);
    press("keyup", "KeyW");
    await vi.advanceTimersByTimeAsync(1500);
    expect(Math.abs(lastSent()?.desired_lean_rad ?? 1)).toBeLessThan(1e-3);
  });

  it("losing the window lets go of the keys, and the next press rides without a click", async () => {
    await open(ride());
    press("keydown", "KeyW");
    await vi.advanceTimersByTimeAsync(1500);
    expect(lastSent()?.desired_lean_rad).toBeCloseTo(radians(10), 3);
    window.dispatchEvent(new Event("blur"));
    await vi.advanceTimersByTimeAsync(1500);
    expect(Math.abs(lastSent()?.desired_lean_rad ?? 1)).toBeLessThan(1e-3);
    press("keydown", "KeyW");
    await vi.advanceTimersByTimeAsync(1500);
    expect(lastSent()?.desired_lean_rad).toBeCloseTo(radians(10), 3);
  });

  it("Escape pauses, and a riding key goes on", async () => {
    await open(ride());
    expect(engine.playing).toBe(true);
    press("keydown", "Escape");
    expect(engine.playing).toBe(false);
    await vi.advanceTimersByTimeAsync(200);
    expect(useFreeRideHud.getState().prompt).toContain("Paused");
    press("keydown", "KeyW");
    expect(engine.playing).toBe(true);
  });

  it("does not ride on keys typed into a text field", async () => {
    await open(ride());
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
    Object.defineProperty(event, "target", { value: field });
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    await vi.advanceTimersByTimeAsync(500);
    expect(Math.abs(lastSent()?.desired_lean_rad ?? 0)).toBe(0);
  });
});

describe("crash and respawn", () => {
  it("a crash never pauses; R respawns at the last clear spot and the keys ride again", async () => {
    await open(ride());
    const before = native.epoch;
    native = { ...native, on_pedals: false, crouch: null, reason: "Off the pedals" };
    await vi.advanceTimersByTimeAsync(300);
    expect(engine.setPlaying).not.toHaveBeenCalledWith(false);
    expect(useFreeRideHud.getState().prompt).toBe(
      "Off the pedals. R respawns here, Shift R at the start.",
    );
    tap("KeyR");
    expect(freeRideApi.restart).toHaveBeenCalledWith(before, false);
    expect(controller.getSnapshot()).toMatchObject({ starting: true, startingKind: "respawn" });
    // A second R while the respawn is on its way does nothing.
    tap("KeyR");
    expect(freeRideApi.restart).toHaveBeenCalledTimes(1);
    native = ride();
    await vi.advanceTimersByTimeAsync(300);
    expect(controller.getSnapshot().starting).toBe(false);
    press("keydown", "KeyW");
    await vi.advanceTimersByTimeAsync(1500);
    expect(lastSent()).toMatchObject({ run_epoch: native.epoch });
    expect(lastSent()?.desired_lean_rad).toBeCloseTo(radians(10), 3);
  });

  it("Shift+R respawns at the world's start; the HUD's button does too", async () => {
    await open(ride());
    tap("KeyR", { shiftKey: true });
    expect(freeRideApi.restart).toHaveBeenLastCalledWith(native.epoch, true);
    native = ride();
    await vi.advanceTimersByTimeAsync(300);
    useFreeRideHud.getState().onRespawn?.(false);
    expect(freeRideApi.restart).toHaveBeenLastCalledWith(native.epoch, false);
  });

  it("a failed respawn says why and can be tried again", async () => {
    await open(ride());
    vi.mocked(freeRideApi.restart).mockRejectedValueOnce(new Error("No clear spot"));
    tap("KeyR");
    await vi.advanceTimersByTimeAsync(50);
    expect(controller.getSnapshot()).toMatchObject({ starting: false, error: "No clear spot" });
    tap("KeyR");
    expect(freeRideApi.restart).toHaveBeenCalledTimes(2);
  });
});

describe("the wheel's pad", () => {
  it("P and H hold the power and handle buttons until they are let go", async () => {
    await open(ride());
    press("keydown", "KeyP");
    expect(setButton).toHaveBeenLastCalledWith("power", true, native.epoch);
    await vi.advanceTimersByTimeAsync(350);
    // The hold renews its lease while the key is down.
    expect(setButton.mock.calls.filter(([, pressed]) => pressed).length).toBeGreaterThan(2);
    press("keyup", "KeyP");
    expect(setButton).toHaveBeenLastCalledWith("power", false, native.epoch);
    await vi.advanceTimersByTimeAsync(50);
    press("keydown", "KeyH");
    expect(setButton).toHaveBeenLastCalledWith("handle", true, native.epoch);
    press("keyup", "KeyH");
    expect(setButton).toHaveBeenLastCalledWith("handle", false, native.epoch);
  });

  it("a switched-off wheel says so, and how to switch it on", async () => {
    await open(ride());
    outputs = { controller_powered: false };
    await vi.advanceTimersByTimeAsync(300);
    expect(controller.getSnapshot().powered).toBe(false);
    expect(useFreeRideHud.getState().prompt).toBe("The wheel is off. Hold P until it wakes.");
    expect(engine.setPlaying).not.toHaveBeenCalledWith(false);
  });

  it("L, B and M send the light, the horn and the pedal mode", async () => {
    await open(ride());
    tap("KeyL");
    await vi.advanceTimersByTimeAsync(10);
    expect(sendCommand).toHaveBeenLastCalledWith("light", 1);
    expect(controller.getSnapshot().light).toBe(1);
    tap("KeyL");
    await vi.advanceTimersByTimeAsync(10);
    expect(sendCommand).toHaveBeenLastCalledWith("light", 2);
    tap("KeyB");
    expect(sendCommand).toHaveBeenLastCalledWith("horn", undefined);
    tap("KeyM");
    await vi.advanceTimersByTimeAsync(10);
    expect(sendCommand).toHaveBeenLastCalledWith("pedal_mode", 0);
    controller.pedalMode(2);
    await vi.advanceTimersByTimeAsync(10);
    expect(controller.getSnapshot().pedalMode).toBe(2);
  });

  it("a refused command shows in the prompt and changes nothing", async () => {
    await open(ride());
    sendCommand.mockRejectedValueOnce(new Error("The wheel ignores it at speed"));
    tap("KeyL");
    await vi.advanceTimersByTimeAsync(200);
    expect(controller.getSnapshot().light).toBe(null);
    expect(useFreeRideHud.getState().prompt).toBe("The wheel ignores it at speed");
  });
});

describe("a new world", () => {
  it("starts a run from a random seed and waits for it", async () => {
    await open(ride());
    controller.newWorld();
    const [ground] = vi.mocked(freeRideApi.start).mock.calls[0] ?? [];
    expect(ground && "world" in ground && ground.world.seed).not.toBe(undefined);
    expect(controller.getSnapshot()).toMatchObject({ starting: true, startingKind: "world" });
    native = ride();
    await vi.advanceTimersByTimeAsync(300);
    expect(controller.getSnapshot().starting).toBe(false);
  });
});
