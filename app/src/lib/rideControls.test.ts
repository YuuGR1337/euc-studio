import { describe, expect, it } from "vitest";
import {
  BrakeGate,
  DEFAULT_BINDINGS,
  DEFAULT_PAD_OPTIONS,
  KeyIntent,
  PAD,
  PadEdges,
  REVERSE_GATE,
  actionFor,
  approach,
  bindKey,
  clearKeys,
  coerceBindings,
  easeCrouch,
  keyLabel,
  radialDeadZone,
  readPad,
  responseCurve,
  strongest,
  triggerLean,
  turnAuthority,
  type PadLike,
} from "./rideControls.ts";

const run = (steps: number, dt: number, f: () => void) => {
  for (let i = 0; i < steps; i++) f();
  void dt;
};

describe("approach", () => {
  it("is exact for any step: two halves land where one whole does", () => {
    const one = approach(0, 1, 0.15, Infinity, 0.1);
    const two = approach(approach(0, 1, 0.15, Infinity, 0.05), 1, 0.15, Infinity, 0.05);
    expect(two).toBeCloseTo(one, 12);
  });

  it("never overshoots and respects the rate limit", () => {
    let v = 0;
    run(100, 0.05, () => (v = approach(v, 1, 0.15, 6, 0.05)));
    expect(v).toBeLessThanOrEqual(1);
    expect(v).toBeCloseTo(1, 6);
    expect(approach(0, 1, 0.0001, 2, 0.1)).toBeCloseTo(0.2, 12);
  });

  it("does nothing without time", () => {
    expect(approach(0.3, 1, 0.15, 6, 0)).toBe(0.3);
    expect(approach(0.3, 1, 0.15, 6, Number.NaN)).toBe(0.3);
  });
});

describe("keys", () => {
  it("ramp toward full lean instead of stepping, and settle back on release", () => {
    const keys = new KeyIntent();
    keys.press("forward");
    const first = keys.step(0.05).lean;
    expect(first).toBeGreaterThan(0);
    expect(first).toBeLessThan(0.3); // one heartbeat is a fraction of the lean, not all of it
    run(40, 0.05, () => keys.step(0.05));
    expect(keys.intent.lean).toBeCloseTo(1, 3);
    keys.release("forward");
    run(40, 0.05, () => keys.step(0.05));
    expect(keys.intent.lean).toBe(0); // lands on an exact zero
  });

  it("build nothing while resting, then ramp from zero", () => {
    const keys = new KeyIntent();
    keys.press("forward");
    run(10, 0.05, () => keys.rest());
    expect(keys.moving).toBe(true);
    expect(keys.intent.lean).toBe(0);
    expect(keys.step(0.05).lean).toBeLessThan(0.3);
  });

  it("turn faster than they lean", () => {
    const keys = new KeyIntent();
    keys.press("forward");
    keys.press("left");
    const i = keys.step(0.1);
    expect(i.steer).toBeGreaterThan(i.lean);
  });

  it("cancel opposite keys and hold crouch as a level", () => {
    const keys = new KeyIntent();
    keys.press("left");
    keys.press("right");
    keys.press("crouch");
    expect(keys.target()).toEqual({ lean: 0, steer: 0, crouch: 1 });
    expect(keys.moving).toBe(true);
    keys.clear();
    expect(keys.intent).toEqual({ lean: 0, steer: 0, crouch: 0 });
    expect(keys.moving).toBe(false);
  });

  it("bend the knees within a few tenths of a second, and stand up to an exact zero", () => {
    let c = 0;
    run(6, 0.05, () => (c = easeCrouch(c, 1, 0.05)));
    expect(c).toBeGreaterThan(0.95);
    run(20, 0.05, () => (c = easeCrouch(c, 0, 0.05)));
    expect(c).toBe(0);
  });
});

describe("gamepad", () => {
  it("uses a radial, rescaled dead zone capped at full deflection", () => {
    expect(radialDeadZone(0.1, 0.1, 0.15)).toEqual([0, 0]);
    // A diagonal counts even when neither axis alone passes the zone (EUC Thrills' test).
    const [x, y] = radialDeadZone(0.12, 0.12, 0.15);
    expect(x).toBeGreaterThan(0);
    expect(y).toBeGreaterThan(0);
    const [cx, cy] = radialDeadZone(1, 1, 0.15);
    expect(Math.hypot(cx, cy)).toBeCloseTo(1, 12);
    const [ex] = radialDeadZone(0.15 + 0.0425, 0, 0.15);
    expect(ex).toBeCloseTo(0.05, 6);
  });

  it("curves keep the sign and the ends", () => {
    expect(responseCurve(-0.5, 2)).toBeCloseTo(-0.25);
    expect(responseCurve(1, 1.8)).toBe(1);
    expect(responseCurve(0.4, 1)).toBe(0.4);
  });

  it("triggers lean forward and back past their own dead zone", () => {
    expect(triggerLean(0, 1)).toBe(1);
    expect(triggerLean(1, 0)).toBe(-1);
    expect(triggerLean(0.05, 0)).toBe(0);
  });

  it("takes the strongest push per axis, never the sum", () => {
    expect(strongest(0.6, 0.5)).toBe(0.6);
    expect(strongest(0.3, -0.7)).toBe(-0.7);
    const pad: PadLike = {
      axes: [0, -0.6],
      buttons: Array.from({ length: 16 }, (_, i) => ({
        pressed: i === PAD.rt,
        value: i === PAD.rt ? 0.9 : 0,
      })),
    };
    const lean = readPad(pad).intent.lean;
    expect(lean).toBeCloseTo((0.9 - 0.08) / 0.92, 6);
  });

  it("maps the stick up to forward and left to left, the D-pad as arrows", () => {
    const stick = readPad({ axes: [-1, -1] });
    expect(stick.intent.lean).toBeGreaterThan(0);
    expect(stick.intent.steer).toBeGreaterThan(0);
    expect(stick.moving).toBe(true);
    const dpad = readPad({
      axes: [0, 0],
      buttons: Array.from({ length: 16 }, (_, i) => ({
        pressed: i === PAD.down || i === PAD.right,
        value: i === PAD.down || i === PAD.right ? 1 : 0,
      })),
    });
    expect(dpad.intent).toMatchObject({ lean: -1, steer: -1 });
    const inverted = readPad({ axes: [-1, 0] }, { ...DEFAULT_PAD_OPTIONS, invertSteer: true });
    expect(inverted.intent.steer).toBeLessThan(0);
  });

  it("reports a broken reading instead of moving", () => {
    const r = readPad({ axes: [Number.NaN, 0] });
    expect(r.invalid).toBe(true);
    expect(r.intent).toEqual({ lean: 0, steer: 0, crouch: 0 });
  });

  it("fires buttons on their rising edge, never on the first reading", () => {
    const edges = new PadEdges();
    const held = {
      crouch: false,
      reset: true,
      resetStart: false,
      stepOff: false,
      camera: false,
      pause: false,
    };
    expect(edges.edges(held)).toEqual([]);
    expect(edges.edges(held)).toEqual([]);
    expect(edges.edges({ ...held, reset: false })).toEqual([]);
    expect(edges.edges(held)).toEqual(["reset"]);
  });
});

describe("turning", () => {
  const lean20 = (20 * Math.PI) / 180;

  it("is tight at walking pace and wide at speed", () => {
    expect(turnAuthority(0, lean20, 10)).toBeCloseTo(3.6, 6);
    const at3 = turnAuthority(3, lean20, 10);
    const at12 = turnAuthority(12, lean20, 10);
    expect(at3).toBeGreaterThan(at12);
    // Turn radius v / rate: under 3 m at 3 m/s, over 12 m at 12 m/s (EUC Thrills' checks).
    expect(3 / at3).toBeLessThan(3);
    expect(12 / at12).toBeGreaterThan(12);
  });

  it("never asks for more sideways pull than the largest turn lean holds", () => {
    for (const v of [4, 8, 15, 25]) {
      const rate = turnAuthority(v, lean20, 10);
      expect(v * rate).toBeLessThanOrEqual(9.81 * Math.tan(lean20) + 1e-9);
    }
  });

  it("stays inside the engine's limit", () => {
    expect(turnAuthority(0, lean20, 0.5)).toBe(0.5);
    expect(turnAuthority(Number.NaN, lean20, 0.5)).toBe(0.5);
  });
});

describe("brake to a stop", () => {
  it("lets a back lean brake from speed", () => {
    const gate = new BrakeGate();
    expect(gate.step(-1, 5, 0.05)).toBe(-1);
  });

  it("holds a back lean at zero near a stop until it has been held", () => {
    const gate = new BrakeGate();
    let t = 0;
    let out = gate.step(-1, 0.3, 0.05);
    expect(out).toBe(0);
    expect(gate.holding).toBe(true);
    while (t < REVERSE_GATE.holdS) {
      out = gate.step(-1, 0.3, 0.05);
      t += 0.05;
    }
    expect(out).toBe(-1);
    expect(gate.holding).toBe(false);
  });

  it("starts over when back is released, and never touches forward or reversing", () => {
    const gate = new BrakeGate();
    gate.step(-1, 0, 0.3);
    expect(gate.step(0.5, 0, 0.05)).toBe(0.5);
    expect(gate.step(-1, 0, 0.05)).toBe(0);
    expect(gate.step(-1, -2, 0.05)).toBe(-1);
  });
});

describe("bindings", () => {
  it("default to WASD and the arrows, Space crouch, X step off, R restart here", () => {
    expect(actionFor(DEFAULT_BINDINGS, "KeyW")).toBe("forward");
    expect(actionFor(DEFAULT_BINDINGS, "ArrowLeft")).toBe("left");
    expect(actionFor(DEFAULT_BINDINGS, "Space")).toBe("crouch");
    expect(actionFor(DEFAULT_BINDINGS, "KeyX")).toBe("stepOff");
    expect(actionFor(DEFAULT_BINDINGS, "KeyR")).toBe("reset");
    expect(actionFor(DEFAULT_BINDINGS, "KeyC")).toBeNull();
  });

  it("move a key to its new action and keep two keys per action", () => {
    const r = bindKey(DEFAULT_BINDINGS, "crouch", "KeyW");
    if (!("bindings" in r)) throw new Error(r.error);
    expect(r.bindings.crouch).toEqual(["KeyW", "Space"]);
    expect(r.bindings.forward).toEqual(["ArrowUp"]);
    const r2 = bindKey(r.bindings, "crouch", "ShiftLeft");
    if (!("bindings" in r2)) throw new Error(r2.error);
    expect(r2.bindings.crouch).toEqual(["ShiftLeft", "KeyW"]);
    expect(clearKeys(r2.bindings, "reset").reset).toEqual([]);
  });

  it("refuse the studio's keys with a reason", () => {
    for (const code of ["Escape", "KeyC", "Tab", "Enter", "Slash"]) {
      const r = bindKey(DEFAULT_BINDINGS, "forward", code);
      expect("error" in r && r.error).toBeTruthy();
    }
  });

  it("read stored data as hostile and fall back to the defaults", () => {
    expect(coerceBindings(null)).toEqual(DEFAULT_BINDINGS);
    expect(coerceBindings("x")).toEqual(DEFAULT_BINDINGS);
    expect(coerceBindings({ forward: "KeyW" })).toEqual(DEFAULT_BINDINGS);
    const stored = coerceBindings({
      forward: ["KeyI", "Escape", 4, "KeyI"],
      crouch: ["KeyW"],
      reset: [],
    });
    expect(stored.forward).toEqual(["KeyI"]);
    expect(stored.crouch).toEqual(["KeyW"]);
    expect(stored.reset).toEqual([]);
    expect(stored.back).toEqual(DEFAULT_BINDINGS.back);
    // A key stored on another action leaves the default action it had.
    expect(coerceBindings({ crouch: ["KeyS"] }).back).toEqual(["ArrowDown"]);
  });

  it("label key caps", () => {
    expect(keyLabel("KeyW")).toBe("W");
    expect(keyLabel("ArrowUp")).toBe("↑");
    expect(keyLabel("Digit3")).toBe("3");
    expect(keyLabel("Space")).toBe("Space");
  });
});
