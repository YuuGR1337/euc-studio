import { readFileSync } from "node:fs";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { WheelButtonInput, wheelButtonReady } from "../workspaces/ride/wheelButtonInput.ts";

// The Tauri IPC layer, faked: engine_connect hands back the two channels the client created.
const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
let channels: {
  stream?: { onmessage: (b: ArrayBuffer) => void };
  messages?: { onmessage: (m: unknown) => void };
} = {};

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: (m: unknown) => void;
    constructor(onmessage: (m: unknown) => void) {
      this.onmessage = onmessage;
    }
  },
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    calls.push({ cmd, ...(args ? { args } : {}) });
    if (cmd === "engine_connect") {
      channels = args as typeof channels;
      return {
        backend: "unicorn",
        scenarios: [{ id: "balance-60s", label: "Balance", detail: "" }],
        commands: [{ id: "horn", name: "horn", pattern: "b", safety: "comfort" }],
        firmwares: [
          { id: "stock", label: "Stock", detail: "" },
          { id: "ours-custom", label: "Ours (custom quirks)", detail: "" },
        ],
        perf_seconds: null,
      };
    }
    return undefined;
  }),
}));

const { LiveEngine } = await import("./live.ts");

function fixture(): ArrayBuffer {
  const bytes = readFileSync(new URL("./fixtures/batch-v1.bin", import.meta.url));
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
}

describe("LiveEngine", () => {
  beforeEach(() => {
    calls.length = 0;
    channels = {};
  });

  it("connects and takes the scenarios and phone commands from the backend", async () => {
    const e = new LiveEngine();
    await vi.waitFor(() => expect(e.phoneCommands).toHaveLength(1));
    expect(calls[0]?.cmd).toBe("engine_connect");
    expect(e.scenarios().map((s) => s.id)).toEqual(["balance-60s"]);
    expect(e.scenarioId).toBe("balance-60s");
    expect(e.status.backend).toBe("unicorn");
  });

  it("runs Ours (custom quirks) with its mask and reads the mask back from the status", async () => {
    const e = new LiveEngine();
    await vi.waitFor(() => expect(e.firmwares().map((f) => f.id)).toContain("ours-custom"));
    e.setFirmware("ours-custom", ["KEIL_FTZ", "ASIN_ACCEL"]);
    expect(calls.at(-1)).toEqual({
      cmd: "engine_set_firmware",
      args: { firmware: "ours-custom:ASIN_ACCEL,KEIL_FTZ" },
    });
    channels.messages?.onmessage({
      type: "status",
      run_epoch: 1,
      state: "running",
      message: "",
      backend: "own",
      scenario: "balance-60s",
      firmware: "ours-custom:ASIN_ACCEL,KEIL_FTZ",
      t_s: 1,
      rtf: 1,
      target_rtf: 1,
      playing: true,
    });
    expect(e.firmware).toBe("ours-custom");
    expect(e.quirks).toEqual(["ASIN_ACCEL", "KEIL_FTZ"]);
    // The same mask again is no restart; another one is.
    const sent = calls.length;
    e.setFirmware("ours-custom", ["ASIN_ACCEL", "KEIL_FTZ"]);
    expect(calls.length).toBe(sent);
    e.setFirmware("ours-custom", []);
    expect(calls.at(-1)?.args).toEqual({ firmware: "ours-custom:" });
    e.setFirmware("stock");
    expect(calls.at(-1)?.args).toEqual({ firmware: "stock" });
  });

  it("takes physical button readiness from the native input gate", async () => {
    const e = new LiveEngine();
    await vi.waitFor(() => expect(channels.messages).toBeDefined());
    const status = {
      type: "status",
      run_epoch: 1,
      state: "running",
      message: "",
      backend: "own",
      scenario: "balance-60s",
      firmware: "stock",
      t_s: 1,
      rtf: 1,
      target_rtf: 1,
      playing: true,
    };
    channels.messages?.onmessage({ ...status, buttons_ready: false });
    expect(e.status.buttonsReady).toBe(false);
    channels.messages?.onmessage({ ...status, buttons_ready: true });
    expect(e.status.buttonsReady).toBe(true);
  });

  it.each([true, false])(
    "clears both readiness flags immediately on reset (Handle ready %s)",
    async (handle) => {
      const e = new LiveEngine();
      await vi.waitFor(() => expect(channels.messages).toBeDefined());
      const status = {
        type: "status",
        run_epoch: 7,
        run_mode: "scenario",
        state: "running",
        message: "",
        backend: "own",
        scenario: "old",
        firmware: "stock",
        playing: true,
        buttons_ready: handle,
        power_button_ready: true,
        rtf: 1,
        target_rtf: 1,
      };
      channels.messages?.onmessage(status);
      expect(wheelButtonReady(e.status, "power")).toBe(true);
      let readinessAtReset: boolean[] = [];
      const off = e.subscribe(() => {
        readinessAtReset = [e.status.buttonsReady === true, e.status.powerButtonReady === true];
      });
      channels.messages?.onmessage({ type: "reset", scenario: "new" });
      expect(readinessAtReset).toEqual([false, false]);
      expect(e.status.buttonsReady).toBe(false);
      expect(e.status.powerButtonReady).toBe(false);
      const input = new WheelButtonInput();
      const epoch = e.status.runEpoch;
      if (epoch === undefined) throw new Error("missing input epoch");
      for (const button of ["power", "handle"] as const) {
        expect(
          input.down(
            {
              source: e,
              epoch,
              setButton: e.setButton.bind(e),
              isCurrent: () =>
                e.playing && e.status.state === "running" && wheelButtonReady(e.status, button),
            },
            button,
          ),
        ).toBe(false);
      }
      expect(calls.filter((call) => call.cmd === "engine_button_state")).toEqual([]);
      channels.messages?.onmessage({ type: "bodies", bodies: [] });
      expect(wheelButtonReady(e.status, "power")).toBe(false);
      channels.messages?.onmessage({ ...status, run_epoch: 8, buttons_ready: false });
      expect(e.status.runEpoch).toBe(8);
      expect(wheelButtonReady(e.status, "power")).toBe(true);
      expect(wheelButtonReady(e.status, "handle")).toBe(false);
      off();
    },
  );

  it("feeds batches into the rings and the latest view", async () => {
    const e = new LiveEngine();
    await vi.waitFor(() => expect(channels.stream).toBeDefined());
    channels.messages?.onmessage({
      type: "bodies",
      bodies: [
        { name: "wheel", role: "wheel", geoms: [] },
        { name: "shell", role: "shell", geoms: [] },
      ],
    });
    channels.stream?.onmessage(fixture());
    expect(e.time.length).toBe(3);
    expect(Array.from(e.channels.speed.toArray())).toEqual([0.5, 1.5, 2.5]);
    expect(e.latest.voltage).toBe(130);
    expect(Number.isNaN(e.latest.pwm)).toBe(true);
    expect(e.latest.poses?.[0]).toBe(2); // body 0 x of the last sample
    expect(e.bodies.map((b) => b.role)).toEqual(["wheel", "shell"]);
  });

  it("maps events, frames, status and resets", async () => {
    const e = new LiveEngine();
    await vi.waitFor(() => expect(channels.messages).toBeDefined());
    const m = channels.messages;
    if (!m) throw new Error("no message channel");
    m.onmessage({ type: "events", events: [{ t_s: 2, kind: "cutoff", text: "Roll cut-off" }] });
    m.onmessage({
      type: "frames",
      frames: [{ id: 1, t: 2, kind: "live", summary: "x", fields: [], hex: "" }],
    });
    m.onmessage({
      type: "status",
      state: "paused",
      message: "",
      build: "GW1825020",
      backend: "unicorn",
      scenario: "ride-20kmh-stop",
      t_s: 3,
      rtf: 1.02,
      target_rtf: 1,
      playing: false,
    });
    expect(e.events[0]).toMatchObject({ kind: "cutoff", title: "Roll cut-off", t: 2 });
    expect(e.frames).toHaveLength(1);
    expect([e.status.state, e.status.build, e.playing, e.scenarioId]).toEqual([
      "paused",
      "GW1825020",
      false,
      "ride-20kmh-stop",
    ]);
    m.onmessage({ type: "reset", scenario: "balance-60s" });
    expect([e.events.length, e.frames.length, e.time.length, e.scenarioId]).toEqual([
      0,
      0,
      0,
      "balance-60s",
    ]);
  });

  it("takes the run's streaming world and drops one that is not a valid frame", async () => {
    const e = new LiveEngine();
    await vi.waitFor(() => expect(channels.messages).toBeDefined());
    const m = channels.messages;
    if (!m) throw new Error("no message channel");
    const frame = {
      spec: {
        seed: 42,
        revision: 2,
        biome: "city",
        densities: { potholes: 1.5 },
      },
      start_xy_m: [70, 3.05],
      start_heading_rad: 0,
      origin_z_m: 0.4,
    };
    m.onmessage({ type: "world", frame });
    expect(e.liveWorld).toEqual(frame);
    m.onmessage({ type: "world", frame: { ...frame, spec: { ...frame.spec, revision: 1 } } });
    expect(e.liveWorld).toBeNull();
    m.onmessage({ type: "world", frame });
    m.onmessage({ type: "world", frame: null });
    expect(e.liveWorld).toBeNull();
  });

  it("sends controls to the backend", async () => {
    const e = new LiveEngine();
    await vi.waitFor(() => expect(channels.stream).toBeDefined());
    e.setPlaying(false);
    e.setTargetRtf(0.5);
    e.setScenario("ride-20kmh-stop");
    await e.sendCommand("horn");
    expect(calls.slice(1).map((c) => c.cmd)).toEqual([
      "engine_play",
      "engine_set_rtf",
      "engine_restart",
      "engine_send",
    ]);
    expect(calls.at(-1)?.args).toEqual({ id: "horn", value: null });
  });
});
