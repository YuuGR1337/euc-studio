import { describe, expect, it, vi } from "vitest";
import {
  nextOffset,
  toneGain,
  prepareFirmwareAudio,
  usesRendererAudio,
  firmwareAudioHold,
  type FirmwareAudioRoute,
} from "./buzzerAudio.ts";

describe("firmware audio run status", () => {
  const status = { state: "running", message: "", backend: "own" } as const;
  it("shows paused for a ready device while simulation is paused", () => {
    expect(firmwareAudioHold({ status, playing: false })?.label).toBe("Paused");
  });
  it.each(["halted", "error", "unavailable", "completed"] as const)(
    "shows stopped for %s",
    (state) => {
      expect(firmwareAudioHold({ status: { ...status, state }, playing: true })?.label).toBe(
        "Stopped",
      );
    },
  );
  it("allows new tones during a running simulation", () => {
    expect(firmwareAudioHold({ status, playing: true })).toBeNull();
  });
});

describe("toneGain", () => {
  it("is loudest at half duty and silent at the ends", () => {
    expect(toneGain(0.5)).toBeGreaterThan(toneGain(0.226));
    expect(toneGain(0)).toBe(0);
    expect(toneGain(1)).toBeCloseTo(0);
    // The stock firmware's default volume (compare 151 of 667).
    expect(toneGain(151 / 667)).toBeGreaterThan(0.1);
  });
});

describe("nextOffset", () => {
  it("maps simulated time a little ahead of the audio clock", () => {
    const offset = nextOffset(null, 10, 4);
    // Sim time 4 s plays 0.12 s from now.
    expect(4 + offset).toBeCloseTo(10.12);
  });

  it("follows small drift slowly and jumps on large ones", () => {
    const first = nextOffset(null, 10, 4);
    const drifted = nextOffset(first, 10.02, 4);
    expect(drifted).toBeGreaterThan(first);
    expect(drifted - first).toBeLessThan(0.001);
    const jumped = nextOffset(first, 11, 4);
    expect(jumped).toBeCloseTo(11.12 - 4);
  });
});

describe("native audio handoff", () => {
  it.each(["ready", "muted", "error", "unsupported"] as const)(
    "never opens renderer audio for native state %s",
    async (state) => {
      const web = vi.fn<() => Promise<boolean>>().mockResolvedValue(true);
      const setSound = vi.fn<(enabled: boolean) => Promise<void>>().mockResolvedValue(undefined);
      const source: FirmwareAudioRoute = {
        audio: {
          backend: "native",
          state,
          enabled: state === "ready",
          error: state === "error" ? "device failed" : null,
        },
        setSound,
      };
      expect(usesRendererAudio(source)).toBe(false);
      await prepareFirmwareAudio(source, web);
      expect(web).not.toHaveBeenCalled();
      if (state !== "ready") expect(setSound).toHaveBeenCalledWith(true);
    },
  );
  it("does not fall back after a native enable failure", async () => {
    const web = vi.fn<() => Promise<boolean>>().mockResolvedValue(true);
    const source: FirmwareAudioRoute = {
      audio: { backend: "native", state: "error", enabled: false, error: "device failed" },
      setSound: vi.fn().mockRejectedValue(new Error("device failed")),
    };
    expect(await prepareFirmwareAudio(source, web)).toBe(false);
    expect(web).not.toHaveBeenCalled();
  });
  it("waits for capability and uses renderer audio only for explicit unsupported output", async () => {
    const web = vi.fn<() => Promise<boolean>>().mockResolvedValue(false);
    expect(usesRendererAudio({})).toBe(false);
    expect(await prepareFirmwareAudio({}, web)).toBe(false);
    expect(web).not.toHaveBeenCalled();
    const source: FirmwareAudioRoute = {
      audio: { backend: "unsupported", state: "unsupported", enabled: false, error: null },
    };
    expect(usesRendererAudio(source)).toBe(true);
    expect(await prepareFirmwareAudio(source, web)).toBe(false);
    expect(web).toHaveBeenCalledOnce();
  });
});
