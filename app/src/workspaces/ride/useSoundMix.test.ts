import { describe, expect, it, vi } from "vitest";
import type { EngineClient } from "../../engine/index.ts";
import { sendMix, volumeGain } from "./useSoundMix.ts";

const live = () => {
  const setSoundMix = vi.fn(async (_gain: number, _ride: boolean) => {});
  return { source: { kind: "live", setSoundMix } as unknown as EngineClient, setSoundMix };
};

describe("sound mix", () => {
  it("squares the slider into the gain, held to 0..1", () => {
    expect(volumeGain(0.5)).toBeCloseTo(0.25, 9);
    expect(volumeGain(1)).toBe(1);
    expect(volumeGain(-1)).toBe(0);
    expect(volumeGain(2)).toBe(1);
  });

  it("sends the gain, silence when muted, and the ride sound only when asked", () => {
    const { source, setSoundMix } = live();
    sendMix(source, 0.8, false, true);
    expect(setSoundMix).toHaveBeenLastCalledWith(expect.closeTo(0.64, 9), true);
    sendMix(source, 0.8, true, true);
    expect(setSoundMix).toHaveBeenLastCalledWith(0, true);
    sendMix(source, 0.8, false, false);
    expect(setSoundMix).toHaveBeenLastCalledWith(expect.closeTo(0.64, 9), false);
  });

  it("leaves a source without native output alone, and logs a refusal", async () => {
    const demo = { kind: "demo" } as unknown as EngineClient;
    expect(() => sendMix(demo, 1, false, false)).not.toThrow();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const failing = {
      kind: "live",
      setSoundMix: vi.fn(async () => {
        throw new Error("no runner");
      }),
    } as unknown as EngineClient;
    sendMix(failing, 1, false, false);
    await Promise.resolve();
    await Promise.resolve();
    expect(warn).toHaveBeenCalledWith("[sound] Error: no runner");
    warn.mockRestore();
  });
});
