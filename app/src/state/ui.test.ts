import { afterEach, describe, expect, it, vi } from "vitest";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

describe("startup sound preference", () => {
  it.each([
    [null, true],
    ["true", true],
    ["false", false],
    ["null", true],
    ["invalid JSON", true],
  ])("loads stored preference %s as %s", async (stored, expected) => {
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => (key === "euc-studio.buzzerSound" ? stored : null),
      setItem: vi.fn(),
    });
    const { useUi } = await import("./ui.ts");
    expect(useUi.getState().buzzerSound).toBe(expected);
    useUi.getState().setBuzzerSound(false);
    expect(localStorage.setItem).toHaveBeenCalledWith("euc-studio.buzzerSound", "false");
    expect(useUi.getState().buzzerSound).toBe(false);
  });

  it("uses enabled sound when storage is blocked", async () => {
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("blocked");
      },
    });
    const { useUi } = await import("./ui.ts");
    expect(useUi.getState().buzzerSound).toBe(true);
    expect(() => useUi.getState().setBuzzerSound(false)).not.toThrow();
    expect(useUi.getState().buzzerSound).toBe(false);
  });
});

describe("sound volume and mute", () => {
  it.each([
    [null, 0.8],
    ["0.35", 0.35],
    ["1.5", 0.8],
    ['"loud"', 0.8],
  ])("loads stored volume %s as %s, and keeps a set one in 0..1", async (stored, expected) => {
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => (key === "euc-studio.soundVolume" ? stored : null),
      setItem: vi.fn(),
    });
    const { useUi } = await import("./ui.ts");
    expect(useUi.getState().soundVolume).toBe(expected);
    expect(useUi.getState().soundMuted).toBe(false);
    useUi.getState().setSoundVolume(1.4);
    expect(useUi.getState().soundVolume).toBe(1);
    expect(localStorage.setItem).toHaveBeenCalledWith("euc-studio.soundVolume", "1");
    useUi.getState().setSoundMuted(true);
    expect(localStorage.setItem).toHaveBeenCalledWith("euc-studio.soundMuted", "true");
    expect(useUi.getState().soundMuted).toBe(true);
  });
});

describe("camera per context", () => {
  const storage = (stored: Record<string, string>) => {
    const items = new Map(Object.entries(stored));
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => items.get(key) ?? null,
      setItem: vi.fn((key: string, value: string) => items.set(key, value)),
    });
    return items;
  };

  it("starts Ride on Side and Free Ride on Chase, and C cycles in each", async () => {
    const items = storage({ "euc-studio.camera": '"orbit"' }); // the old key is not read
    const { useUi, nextCamera, cameraContextOf } = await import("./ui.ts");
    const ui = () => useUi.getState();
    const press = () => ui().setCamera(nextCamera(ui().camera, ui().cameraContext));
    expect(ui().camera).toBe("follow");
    press();
    expect(ui().camera).toBe("orbit"); // Ride: Side, Orbit, Chase
    press();
    expect(ui().camera).toBe("chase");
    press();
    expect(ui().camera).toBe("follow");
    ui().setCameraContext(cameraContextOf("free_ride"));
    expect(ui().camera).toBe("chase");
    press();
    expect(ui().camera).toBe("orbit"); // Free Ride: Chase, Orbit, Side
    press();
    expect(ui().camera).toBe("follow");
    press();
    press();
    expect(ui().camera).toBe("orbit");
    ui().setCameraContext(cameraContextOf("scenario"));
    expect(ui().camera).toBe("follow");
    ui().setCameraContext("freeRide");
    expect(ui().camera).toBe("orbit");
    expect(JSON.parse(items.get("euc-studio.camera.v2") ?? "null")).toEqual({
      ride: "follow",
      freeRide: "orbit",
    });
  });

  it("keeps each stored camera and falls back per context", async () => {
    storage({ "euc-studio.camera.v2": '{"ride":"orbit","freeRide":"nonsense"}' });
    const { useUi } = await import("./ui.ts");
    expect(useUi.getState().camera).toBe("orbit");
    useUi.getState().setCameraContext("freeRide");
    expect(useUi.getState().camera).toBe("chase");
  });
});
