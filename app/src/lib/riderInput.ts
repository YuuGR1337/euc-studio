// Manual riding: keys and a gamepad's left stick turned into what the rider does, lean and turn,
// each -1..1 of what the rider can (forward and left positive). Keys ramp the way a body moves,
// a stick passes straight through past its dead zone. Pure, so the mapping is tested on its own.

export interface RiderInput {
  /** -1 full back .. 1 full forward. */
  lean: number;
  /** -1 full right .. 1 full left. */
  steer: number;
}

export const NEUTRAL: RiderInput = { lean: 0, steer: 0 };

export type Direction = "forward" | "back" | "left" | "right";
export type RideAction = Direction | "stepOff" | "release";

/** Keys (KeyboardEvent.key, lower case) and what they do. Arrows and WASD both ride. */
const KEYS: Record<string, Direction> = {
  w: "forward",
  arrowup: "forward",
  s: "back",
  arrowdown: "back",
  a: "left",
  arrowleft: "left",
  d: "right",
  arrowright: "right",
};

type KeyLike = Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey">;

/**
 * What a key does while riding by hand, or null. Space steps off and Esc gives control back; with
 * ⌘, Ctrl or Alt a key is a shortcut, never a riding key. Shift is allowed (held for long rides).
 */
export function rideAction(e: KeyLike): RideAction | null {
  if (e.metaKey || e.ctrlKey || e.altKey) return null;
  if (e.key === " ") return "stepOff";
  if (e.key === "Escape") return "release";
  return KEYS[e.key.toLowerCase()] ?? null;
}

/** Keys lean this far at most: full lean on a key would throw the rider forward at once. */
export const KEY_LEAN = 0.6;
/** Per second: how fast a held key builds up its lean or turn, and how fast it settles back. */
export const KEY_RISE = 1.5;
export const KEY_FALL = 3;

const approach = (from: number, to: number, rise: number, fall: number, dt: number) => {
  // Moving away from zero is the rise; moving back toward zero (or across it) the fall.
  const rate = Math.abs(to) > Math.abs(from) && Math.sign(to) !== -Math.sign(from) ? rise : fall;
  const step = rate * dt;
  return Math.abs(to - from) <= step ? to : from + Math.sign(to - from) * step;
};

/** Held keys to a smooth input: call `step` once per frame with the frame's seconds. */
export class KeyRider {
  private held = new Set<Direction>();
  input: RiderInput = { ...NEUTRAL };

  press(d: Direction): void {
    this.held.add(d);
  }

  release(d: Direction): void {
    this.held.delete(d);
  }

  /** Lets go of every key (the window lost focus, or control went back to the scenario). */
  clear(): void {
    this.held.clear();
  }

  get active(): boolean {
    return this.held.size > 0;
  }

  /** Where the held keys point, before the ramp. */
  target(): RiderInput {
    const h = (d: Direction) => (this.held.has(d) ? 1 : 0);
    return {
      lean: (h("forward") - h("back")) * KEY_LEAN,
      steer: h("left") - h("right"),
    };
  }

  step(dt: number): RiderInput {
    const t = this.target();
    const dts = Math.max(0, Math.min(dt, 0.1));
    this.input = {
      lean: approach(this.input.lean, t.lean, KEY_RISE * KEY_LEAN, KEY_FALL * KEY_LEAN, dts),
      steer: approach(this.input.steer, t.steer, KEY_RISE * 2, KEY_FALL * 2, dts),
    };
    return this.input;
  }
}

/** A stick's radius inside which it reads as centred. */
export const DEAD_ZONE = 0.15;

/**
 * A gamepad's left stick (axes 0 and 1: x right, y down) to an input: pushing it up leans
 * forward, left turns left. The dead zone is radial and the rest is rescaled to reach 1.
 */
export function stickInput(x: number, y: number, deadZone = DEAD_ZONE): RiderInput {
  const r = Math.hypot(x, y);
  if (!(r > deadZone)) return { ...NEUTRAL };
  const scale = Math.min(1, (r - deadZone) / (1 - deadZone)) / r;
  return { lean: -y * scale + 0, steer: -x * scale + 0 };
}

/** Keys and stick together: per axis, the one pushed further wins. */
export function combine(a: RiderInput, b: RiderInput): RiderInput {
  const pick = (p: number, q: number) => (Math.abs(q) > Math.abs(p) ? q : p);
  return { lean: pick(a.lean, b.lean), steer: pick(a.steer, b.steer) };
}

export const isNeutral = (i: RiderInput) => i.lean === 0 && i.steer === 0;

/** Close enough that sending it again would change nothing the rider can feel. */
export function sameInput(a: RiderInput, b: RiderInput, eps = 0.01): boolean {
  return Math.abs(a.lean - b.lean) < eps && Math.abs(a.steer - b.steer) < eps;
}
