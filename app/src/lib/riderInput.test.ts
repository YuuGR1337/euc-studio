import { describe, expect, it } from "vitest";
import {
  DEAD_ZONE,
  KEY_LEAN,
  KeyRider,
  NEUTRAL,
  combine,
  isNeutral,
  rideAction,
  sameInput,
  stickInput,
} from "./riderInput.ts";

const key = (k: string, mods: Partial<KeyboardEvent> = {}) => ({
  key: k,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  ...mods,
});

describe("riding keys", () => {
  it("maps WASD and the arrows, Space and Esc", () => {
    expect(rideAction(key("w"))).toBe("forward");
    expect(rideAction(key("W"))).toBe("forward");
    expect(rideAction(key("ArrowUp"))).toBe("forward");
    expect(rideAction(key("s"))).toBe("back");
    expect(rideAction(key("ArrowDown"))).toBe("back");
    expect(rideAction(key("a"))).toBe("left");
    expect(rideAction(key("ArrowLeft"))).toBe("left");
    expect(rideAction(key("d"))).toBe("right");
    expect(rideAction(key("ArrowRight"))).toBe("right");
    expect(rideAction(key(" "))).toBe("stepOff");
    expect(rideAction(key("Escape"))).toBe("release");
  });

  it("leaves shortcuts and other keys alone", () => {
    expect(rideAction(key("s", { metaKey: true }))).toBeNull(); // ⌘S
    expect(rideAction(key("a", { ctrlKey: true }))).toBeNull();
    expect(rideAction(key("d", { altKey: true }))).toBeNull();
    for (const k of ["r", "c", "m", ".", "[", "]", "?", "1", "Enter", "Tab"]) {
      expect(rideAction(key(k))).toBeNull();
    }
  });
});

describe("keys to lean and turn", () => {
  it("builds up while held and settles back when let go", () => {
    const rider = new KeyRider();
    rider.press("forward");
    rider.press("left");
    expect(rider.target()).toEqual({ lean: KEY_LEAN, steer: 1 });
    const first = rider.step(0.1);
    expect(first.lean).toBeGreaterThan(0);
    expect(first.lean).toBeLessThan(KEY_LEAN);
    for (let i = 0; i < 20; i++) rider.step(0.05);
    expect(rider.input).toEqual({ lean: KEY_LEAN, steer: 1 });
    rider.release("forward");
    rider.release("left");
    expect(rider.active).toBe(false);
    for (let i = 0; i < 10; i++) rider.step(0.05);
    expect(isNeutral(rider.input)).toBe(true);
  });

  it("cancels opposite keys and crosses over at the faster rate", () => {
    const rider = new KeyRider();
    rider.press("forward");
    rider.press("back");
    expect(rider.target().lean).toBe(0);
    rider.release("back");
    for (let i = 0; i < 20; i++) rider.step(0.05);
    rider.release("forward");
    rider.press("back");
    const settle = new KeyRider();
    settle.press("back");
    // From full forward, one step back covers more ground than a step from rest.
    expect(KEY_LEAN - rider.step(0.1).lean).toBeGreaterThan(-settle.step(0.1).lean);
    rider.clear();
    expect(rider.active).toBe(false);
  });

  it("ignores long frame gaps (a hidden window)", () => {
    const rider = new KeyRider();
    rider.press("right");
    const after = rider.step(5);
    expect(after.steer).toBeGreaterThan(-1);
    expect(after.steer).toBeLessThan(0);
  });
});

describe("the left stick", () => {
  it("has a radial dead zone and reaches full scale", () => {
    expect(stickInput(0.1, -0.1)).toEqual(NEUTRAL);
    expect(stickInput(0, -1)).toEqual({ lean: 1, steer: 0 });
    expect(stickInput(-1, 0)).toEqual({ lean: 0, steer: 1 });
    const half = stickInput(0, 0.5);
    expect(half.lean).toBeCloseTo(-(0.5 - DEAD_ZONE) / (1 - DEAD_ZONE));
    // A diagonal past the rim stays within reach.
    const corner = stickInput(1, -1);
    expect(Math.hypot(corner.lean, corner.steer)).toBeCloseTo(1);
  });

  it("combines with the keys: the bigger push wins per axis", () => {
    expect(combine({ lean: 0.6, steer: 0 }, { lean: -0.2, steer: -0.9 })).toEqual({
      lean: 0.6,
      steer: -0.9,
    });
    expect(sameInput({ lean: 0.5, steer: 0 }, { lean: 0.505, steer: 0 })).toBe(true);
    expect(sameInput({ lean: 0.5, steer: 0 }, { lean: 0.52, steer: 0 })).toBe(false);
  });
});
