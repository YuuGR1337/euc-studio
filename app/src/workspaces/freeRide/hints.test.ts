import { describe, expect, it } from "vitest";
import { HINT_TIMING, RideHints, type HintInput } from "./hints.ts";

const riding = (patch: Partial<HintInput> = {}): HintInput => ({
  armed: true,
  lean: 0,
  steer: 0,
  speedMps: 0,
  dt: 0.05,
  ...patch,
});
const ride = (hints: RideHints, seconds: number, patch: Partial<HintInput> = {}) => {
  let shown = null;
  for (let t = 0; t < seconds - 1e-9; t += 0.05) shown = hints.step(riding(patch));
  return shown;
};

describe("first-ride hints", () => {
  it("wait a moment, then ask to turn, then to brake, then show the keys once", () => {
    const hints = new RideHints();
    expect(ride(hints, HINT_TIMING.firstS - 0.1)).toBeNull();
    expect(ride(hints, 0.2)).toBe("turn");
    ride(hints, HINT_TIMING.turnS + 0.05, { steer: 1 });
    expect(hints.seenList).toEqual(["turn"]);
    expect(ride(hints, HINT_TIMING.gapS - 0.1)).toBeNull();
    expect(ride(hints, 0.2)).toBe("brake");
    ride(hints, HINT_TIMING.brakeS + 0.05, { lean: -1, speedMps: 3 });
    ride(hints, HINT_TIMING.gapS + 0.1);
    expect(hints.step(riding())).toBe("keys");
    ride(hints, HINT_TIMING.keysS + 0.1);
    expect(hints.seenList).toEqual(["turn", "brake", "keys"]);
    expect(ride(hints, 5)).toBeNull();
  });

  it("retire a skill shown before its hint and remember what was seen", () => {
    const hints = new RideHints(["keys"]);
    ride(hints, 0.6, { steer: -1 });
    expect(hints.seenList).toEqual(["turn", "keys"]);
    // Braking needs speed: a back lean at a stop does not count.
    ride(hints, 1, { lean: -1, speedMps: 0 });
    expect(hints.seenList).not.toContain("brake");
  });

  it("say nothing while paused, and ignore unknown stored names", () => {
    const hints = new RideHints(["bogus" as never]);
    expect(ride(hints, 3, { armed: false })).toBeNull();
    expect(hints.seenList).toEqual([]);
  });
});
