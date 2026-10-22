import { describe, expect, it } from "vitest";
import type { Outputs } from "../../engine/telemetry.gen.ts";
import { headlightOutput, headlightPreviewModel } from "./headlightOutput.ts";

// Node tests target the same pure model consumed by HeadlightPreview.tsx.
describe("headlight preview model", () => {
  it.each([null, undefined, Number.NaN, Infinity, -Infinity])(
    "keeps missing or invalid duty unknown: %s",
    (duty) => {
      const model = headlightPreviewModel(duty);
      expect(headlightOutput(duty)).toEqual({ duty: null, state: "Unknown" });
      expect(model.unknown).toBe(true);
      expect(model.beamOpacity).toBe(0);
      expect(model.lenses.every((lens) => lens.opacity === 0)).toBe(true);
    },
  );
  it("clamps finite drive values and distinguishes off from unknown", () => {
    expect(headlightOutput(-0.2)).toEqual({ duty: 0, state: "Off" });
    expect(headlightOutput(2)).toEqual({ duty: 1, state: "On" });
    const off = headlightPreviewModel(0);
    expect(off.state).toBe("Off");
    expect(off.unknown).toBe(false);
    expect(off.beamOpacity).toBe(0);
  });
  it("passes the actual output field to all four horizontal lenses and the beam", () => {
    const output: Pick<Outputs, "headlight_duty"> = { headlight_duty: 0.175 };
    const model = headlightPreviewModel(output.headlight_duty);
    expect(model.state).toBe("On");
    expect(model.beamOpacity).toBe(0.175);
    expect(model.lenses).toHaveLength(4);
    expect(model.lenses.every((lens) => lens.opacity === 0.175 && lens.cy === 24)).toBe(true);
    expect(model.lenses.map((lens) => lens.cx)).toEqual([21, 39, 57, 75]);
  });
});
