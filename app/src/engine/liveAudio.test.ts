import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NativeAudioStatus } from "./types.ts";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    readonly onmessage: (message: unknown) => void;
    constructor(onmessage: (message: unknown) => void) {
      this.onmessage = onmessage;
    }
  },
  invoke: ipc.invoke,
}));

const { LiveEngine } = await import("./live.ts");
const ready = { backend: "native", state: "ready", enabled: true, error: null } as const;
const muted = { ...ready, state: "muted", enabled: false } as const;

function info(audio: NativeAudioStatus = ready) {
  return { backend: "own", scenarios: [], commands: [], firmwares: [], perf_seconds: null, audio };
}

describe("native audio client", () => {
  beforeEach(() => {
    ipc.invoke.mockReset();
    ipc.invoke.mockResolvedValue(info());
    vi.stubGlobal("localStorage", { getItem: vi.fn(() => null) });
  });

  afterEach(() => vi.unstubAllGlobals());

  it.each([
    [null, true],
    ["true", true],
    ["false", false],
    ["null", true],
    ["invalid JSON", true],
  ])("passes stored preference %s in the first connect call", async (stored, enabled) => {
    vi.stubGlobal("localStorage", { getItem: vi.fn(() => stored) });
    const engine = new LiveEngine();
    expect(ipc.invoke).toHaveBeenCalledExactlyOnceWith(
      "engine_connect",
      expect.objectContaining({ soundEnabled: enabled }),
    );
    await vi.waitFor(() => expect(engine.audio).toEqual(ready));
  });

  it("defaults to enabled when storage is blocked", async () => {
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("blocked");
      },
    });
    const engine = new LiveEngine();
    expect(ipc.invoke).toHaveBeenCalledWith(
      "engine_connect",
      expect.objectContaining({ soundEnabled: true }),
    );
    await vi.waitFor(() => expect(engine.audio).toEqual(ready));
  });

  it("takes sound status from native IPC and notifies subscribers", async () => {
    const engine = new LiveEngine();
    await vi.waitFor(() => expect(engine.audio).toEqual(ready));
    const changed = vi.fn();
    engine.subscribe(changed);
    ipc.invoke.mockResolvedValueOnce(muted);
    await engine.setSound(false);
    expect(ipc.invoke).toHaveBeenLastCalledWith("engine_sound", { enabled: false });
    expect(engine.audio).toEqual(muted);
    expect(changed).toHaveBeenCalledOnce();
  });

  it("refreshes device errors without changing simulation status", async () => {
    const engine = new LiveEngine();
    await vi.waitFor(() => expect(engine.audio).toEqual(ready));
    const simulation = engine.status;
    const failure = { ...ready, state: "error", error: "Device disconnected" };
    ipc.invoke.mockResolvedValueOnce(failure);
    await engine.refreshAudioStatus();
    expect(ipc.invoke).toHaveBeenLastCalledWith("engine_audio_status", undefined);
    expect(engine.audio).toEqual(failure);
    expect(engine.status).toBe(simulation);
  });

  it("takes pushed audio status and notifies subscribers without polling", async () => {
    const engine = new LiveEngine();
    await vi.waitFor(() => expect(engine.audio).toEqual(ready));
    const args = ipc.invoke.mock.calls[0]?.[1] as {
      messages: { onmessage: (message: unknown) => void };
    };
    const simulation = engine.status;
    const changed = vi.fn();
    const dataChanged = vi.fn();
    engine.subscribe(changed);
    engine.subscribeData(dataChanged);
    const statuses: NativeAudioStatus[] = [
      { ...ready, state: "error", error: "Device disconnected" },
      muted,
      { backend: "unsupported", state: "unsupported", enabled: false, error: null },
      ready,
    ];
    for (const status of statuses) {
      args.messages.onmessage({ type: "audio", status });
      expect(engine.audio).toEqual(status);
      expect(engine.status).toBe(simulation);
    }
    expect(changed).toHaveBeenCalledTimes(statuses.length);
    expect(dataChanged).toHaveBeenCalledTimes(statuses.length);
    expect(ipc.invoke).toHaveBeenCalledTimes(1);
  });

  it.each<NativeAudioStatus>([
    { backend: "unsupported", state: "unsupported", enabled: true, error: null },
    { ...ready, state: "error", error: "Device unavailable" },
    muted,
  ])("preserves native connect status $state without a renderer fallback", async (audio) => {
    ipc.invoke.mockResolvedValueOnce(info(audio));
    const engine = new LiveEngine();
    await vi.waitFor(() => expect(engine.audio).toEqual(audio));
    expect(ipc.invoke).toHaveBeenCalledTimes(1);
  });

  it("reports IPC failures as audio errors and keeps simulation status", async () => {
    const engine = new LiveEngine();
    await vi.waitFor(() => expect(engine.audio).toEqual(ready));
    const simulation = engine.status;
    ipc.invoke.mockRejectedValueOnce(new Error("No audio device"));
    await expect(engine.setSound(false)).rejects.toThrow("No audio device");
    expect(engine.audio).toEqual({ ...ready, state: "error", error: "Error: No audio device" });
    expect(engine.status).toBe(simulation);
    ipc.invoke.mockResolvedValueOnce(muted);
    await engine.setSound(false);
    expect(engine.audio).toEqual(muted);
  });

  it("waits for connect before a sound change and preserves request order", async () => {
    let connected!: (value: ReturnType<typeof info>) => void;
    ipc.invoke.mockReturnValueOnce(
      new Promise((resolve) => {
        connected = resolve;
      }),
    );
    const engine = new LiveEngine();
    ipc.invoke.mockResolvedValueOnce(muted).mockResolvedValueOnce(ready);
    const first = engine.setSound(false);
    const second = engine.setSound(true);
    expect(ipc.invoke).toHaveBeenCalledTimes(1);
    connected(info());
    await Promise.all([first, second]);
    expect(ipc.invoke.mock.calls.slice(1)).toEqual([
      ["engine_sound", { enabled: false }],
      ["engine_sound", { enabled: true }],
    ]);
    expect(engine.audio).toEqual(ready);
  });
});
