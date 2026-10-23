// Ride controls shared by Free Ride (workspaces/freeRide/controller.ts) and Ride's manual mode
// (workspaces/ride/manual.ts): what keys and a gamepad turn into rider intent, and when. Pure: no
// DOM, no engine, so every curve is tested on its own (rideControls.test.ts).
//
// The intent is what a rider can do: lean forward or back (the firmware answers with torque),
// turn, and crouch. Nothing here sets a speed, a torque or a pose.
//
// Several methods and starting values come from EUC Thrills by VibezZzCoder (MIT,
// https://github.com/VibezZzCoder/EUC-thrills); each is named where it is used. The plan is in
// docs/design/free-ride.md.

/** Rider intent, each part a share of what the rider can do. */
export interface Intent {
  /** -1 full back .. 1 full forward. */
  lean: number;
  /** -1 full right .. 1 full left. */
  steer: number;
  /** 0 standing .. 1 full crouch. */
  crouch: number;
}

export const STILL: Intent = { lean: 0, steer: 0, crouch: 0 };

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/**
 * A first-order lag toward `target` with time constant `tau` (s), at most `maxRate` per second.
 * Exact for any step: two short steps land where one long step does, so the feel does not depend
 * on the frame or heartbeat rate. Method from EUC Thrills (src/shared/maths.ts, `approach`).
 */
export function approach(
  current: number,
  target: number,
  tau: number,
  maxRate: number,
  dt: number,
): number {
  if (!(dt > 0)) return current;
  const blend = tau > 0 ? 1 - Math.exp(-dt / tau) : 1;
  const step = (target - current) * blend;
  const limit = maxRate * dt;
  return current + clamp(step, -limit, limit);
}

/** Keys: how fast a held key's lean and turn build, and how fast they settle back. */
export const KEY_TIMING = {
  /** Lean time constant, s. EUC Thrills settles its lean with 0.14 s ("the single most important
   * number for how responsive the game feels", src/data/tuning.ts). */
  leanTau: 0.15,
  /** Turn time constant, s: a carve starts a little faster than a lean. */
  steerTau: 0.1,
  /** Largest change per second, in shares of the full input. */
  maxRate: 6,
  /** Crouch time constant, s (EUC Thrills' knee response, 0.07 s), for every device. */
  crouchTau: 0.07,
} as const;

/** The keyboard's directions and holds. */
export type KeyHold = "forward" | "back" | "left" | "right" | "crouch";

/** Held keys to a smooth intent: `step` once per heartbeat with the elapsed seconds. */
export class KeyIntent {
  private held = new Set<KeyHold>();
  intent: Intent = { ...STILL };

  press(hold: KeyHold): void {
    this.held.add(hold);
  }

  release(hold: KeyHold): void {
    this.held.delete(hold);
  }

  /** Lets go of every key at once (pause, blur, a new run). The intent drops to still too. */
  clear(): void {
    this.held.clear();
    this.intent = { ...STILL };
  }

  /**
   * Keeps the held keys but the intent at still: while the controls are not riding, a held key
   * must not build up a lean that would go out at once when they start.
   */
  rest(): Intent {
    this.intent = { ...STILL };
    return this.intent;
  }

  /** True while a direction (not crouch) is held. */
  get moving(): boolean {
    return ["forward", "back", "left", "right"].some((h) => this.held.has(h as KeyHold));
  }

  /** Where the held keys point, before the lag. */
  target(): Intent {
    const h = (k: KeyHold) => (this.held.has(k) ? 1 : 0);
    return {
      lean: h("forward") - h("back"),
      steer: h("left") - h("right"),
      crouch: h("crouch"),
    };
  }

  step(dt: number): Intent {
    const t = this.target();
    const s = clamp(dt, 0, 0.25);
    const { leanTau, steerTau, maxRate } = KEY_TIMING;
    // A lag only approaches its target: within a thousandth it lands, so a released key sends an
    // exact zero.
    const land = (value: number, target: number) =>
      Math.abs(value - target) < 1e-3 ? target : value;
    this.intent = {
      lean: land(approach(this.intent.lean, t.lean, leanTau, maxRate, s), t.lean),
      steer: land(approach(this.intent.steer, t.steer, steerTau, maxRate, s), t.steer),
      crouch: t.crouch,
    };
    return this.intent;
  }
}

/**
 * Eases a crouch request the way knees bend (any device), landing on the target within a
 * thousandth: a released crouch sends an exact zero, which the engine needs before it arms.
 */
export function easeCrouch(current: number, target: number, dt: number): number {
  const goal = clamp(target, 0, 1);
  const eased = approach(current, goal, KEY_TIMING.crouchTau, Infinity, clamp(dt, 0, 0.25));
  return Math.abs(eased - goal) < 1e-3 ? goal : eased;
}

// ---------------------------------------------------------------------------------------------
// Gamepad (standard mapping)

/** Standard-mapping button indices (W3C Gamepad, Xbox names). */
export const PAD = {
  a: 0,
  b: 1,
  x: 2,
  y: 3,
  lb: 4,
  rb: 5,
  lt: 6,
  rt: 7,
  back: 8,
  start: 9,
  up: 12,
  down: 13,
  left: 14,
  right: 15,
} as const;

/** Default left-stick dead zone (radial). */
export const STICK_DEAD_ZONE = 0.15;
/** Trigger dead zone. EUC Thrills uses 0.08 (src/data/tuning.ts, gamepad). */
export const TRIGGER_DEAD_ZONE = 0.08;

/**
 * A stick to x and y past a radial dead zone, rescaled so the edge of the zone reads 0 and full
 * deflection 1, and capped at magnitude 1 so square-gate corners keep their direction (EUC
 * Thrills, src/input/gamepad.ts).
 */
export function radialDeadZone(x: number, y: number, deadZone = STICK_DEAD_ZONE): [number, number] {
  const r = Math.hypot(x, y);
  if (!(r > deadZone)) return [0, 0];
  const scale = Math.min(1, (r - deadZone) / (1 - deadZone)) / r;
  return [x * scale + 0, y * scale + 0];
}

/** A sign-keeping response curve: 1 is linear, above 1 softer near the centre. */
export function responseCurve(value: number, exponent = 1): number {
  if (exponent === 1 || value === 0) return value;
  return Math.sign(value) * Math.abs(value) ** exponent;
}

/** Right trigger leans forward, left trigger back, each past its own dead zone. */
export function triggerLean(lt: number, rt: number, deadZone = TRIGGER_DEAD_ZONE): number {
  const one = (v: number) => (v > deadZone ? Math.min(1, (v - deadZone) / (1 - deadZone)) : 0);
  return one(rt) - one(lt);
}

/** The larger of two pushes on one axis, never their sum (EUC Thrills merges devices this way). */
export function strongest(a: number, b: number): number {
  return Math.abs(b) > Math.abs(a) ? b : a;
}

export interface PadOptions {
  deadZone: number;
  invertLean: boolean;
  invertSteer: boolean;
  /** Response exponent for the stick; 1 is linear. */
  exponent: number;
}

export const DEFAULT_PAD_OPTIONS: PadOptions = {
  deadZone: STICK_DEAD_ZONE,
  invertLean: false,
  invertSteer: false,
  exponent: 1,
};

/** The parts of a Gamepad this module reads. */
export interface PadLike {
  readonly axes: readonly number[];
  readonly buttons?: readonly { readonly pressed: boolean; readonly value: number }[];
}

export type PadButton = "crouch" | "reset" | "resetStart" | "stepOff" | "camera" | "pause";

/**
 * Gamepad buttons in Free Ride (EUC Thrills' layout where it applies: LB crouch, X respawn, Y
 * view); Back respawns at the world's start.
 */
export const PAD_BUTTONS: Record<PadButton, number> = {
  crouch: PAD.lb,
  reset: PAD.x,
  resetStart: PAD.back,
  stepOff: PAD.b,
  camera: PAD.y,
  pause: PAD.start,
};

export interface PadReading {
  /** Lean and turn from the stick, the triggers and the D-pad; crouch from its button. */
  intent: Intent;
  /** Buttons held now. */
  held: Record<PadButton, boolean>;
  /** Any axis outside its dead zone, or the D-pad. */
  moving: boolean;
  /** Not a number somewhere: the reading cannot be trusted. */
  invalid: boolean;
}

const pressed = (pad: PadLike, index: number) => {
  const b = pad.buttons?.[index];
  return !!b && (b.pressed || b.value > 0.5);
};
const value = (pad: PadLike, index: number) => {
  const b = pad.buttons?.[index];
  return b ? (Number.isFinite(b.value) ? b.value : NaN) : 0;
};

/**
 * One gamepad to intent: the left stick leans (up is forward) and turns (left is left), the right
 * and left triggers lean forward and back, the D-pad acts as the arrow keys. Per axis the
 * strongest push wins.
 */
export function readPad(pad: PadLike, options: PadOptions = DEFAULT_PAD_OPTIONS): PadReading {
  const ax = pad.axes[0] ?? 0;
  const ay = pad.axes[1] ?? 0;
  const lt = value(pad, PAD.lt);
  const rt = value(pad, PAD.rt);
  const invalid = ![ax, ay, lt, rt].every(Number.isFinite);
  const held = Object.fromEntries(
    (Object.keys(PAD_BUTTONS) as PadButton[]).map((b) => [b, pressed(pad, PAD_BUTTONS[b])]),
  ) as Record<PadButton, boolean>;
  if (invalid) return { intent: { ...STILL }, held, moving: false, invalid };
  const [x, y] = radialDeadZone(ax, ay, options.deadZone);
  const dpadLean = (pressed(pad, PAD.up) ? 1 : 0) - (pressed(pad, PAD.down) ? 1 : 0);
  const dpadSteer = (pressed(pad, PAD.left) ? 1 : 0) - (pressed(pad, PAD.right) ? 1 : 0);
  let lean = strongest(
    strongest(responseCurve(-y, options.exponent), triggerLean(lt, rt)),
    dpadLean,
  );
  let steer = strongest(responseCurve(-x, options.exponent), dpadSteer);
  if (options.invertLean) lean = -lean;
  if (options.invertSteer) steer = -steer;
  return {
    intent: { lean: lean + 0, steer: steer + 0, crouch: held.crouch ? 1 : 0 },
    held,
    moving: lean !== 0 || steer !== 0,
    invalid,
  };
}

/**
 * Rising edges of the buttons. The first reading of a pad counts every button as already held,
 * so the press that woke the pad does nothing (EUC Thrills, src/input/gamepad.ts).
 */
export class PadEdges {
  private last: Record<PadButton, boolean> | null = null;

  edges(held: Record<PadButton, boolean>): PadButton[] {
    const previous = this.last;
    this.last = { ...held };
    if (!previous) return [];
    return (Object.keys(held) as PadButton[]).filter((b) => held[b] && !previous[b]);
  }

  reset(): void {
    this.last = null;
  }
}

// ---------------------------------------------------------------------------------------------
// Turning and braking: intent shaped by the wheel's speed

/** Gravity, m/s². */
const G = 9.81;

/**
 * The fastest turn a full steer asks for at `speed` (m/s), rad/s. EUC Thrills' schedule: 3.6 rad/s
 * at rest falling to 0.85 rad/s by 9 m/s with an exponent of 1.5 (src/data/tuning.ts,
 * `yawRateLow`, `yawRateHigh`), so a slow pivot is tight and a fast carve is wide. On top, the
 * turn never asks for more sideways acceleration than the rider's largest turn lean holds
 * (tan(lean) = v·rate/g), and never more than the engine accepts.
 */
export const STEER_SCHEDULE = { atRest: 3.6, atSpeed: 0.85, speedMps: 9, exponent: 1.5 } as const;

export function turnAuthority(speedMps: number, turnLeanMaxRad: number, engineMaxRadS: number) {
  const v = Math.abs(Number.isFinite(speedMps) ? speedMps : 0);
  const { atRest, atSpeed, speedMps: knee, exponent } = STEER_SCHEDULE;
  const t = Math.min(1, v / knee) ** exponent;
  const scheduled = atRest + (atSpeed - atRest) * t;
  const lateral = v > 0.05 ? (G * Math.tan(Math.max(0, turnLeanMaxRad))) / v : Infinity;
  return Math.max(0, Math.min(scheduled, lateral, engineMaxRadS));
}

/**
 * A stop is a stop: below `speedMps` a back lean is held at zero (stand up, the wheel stops)
 * until back has been held for `holdS`; only then does it lean back and roll backwards. From
 * EUC Thrills' reverse gate (src/simulation/EucController.ts, 0.6 m/s and 0.35 s). It shapes the
 * rider's intent only; the firmware decides what the wheel does.
 */
export const REVERSE_GATE = { speedMps: 0.6, holdS: 0.35 } as const;

export class BrakeGate {
  private heldS = 0;

  /** The lean to ask for, given the intent's lean, the wheel speed (m/s, forward +) and dt. */
  step(lean: number, speedMps: number, dt: number): number {
    if (!(lean < 0)) {
      this.heldS = 0;
      return lean;
    }
    const v = Number.isFinite(speedMps) ? speedMps : 0;
    // Braking from speed, or already rolling backwards: the lean passes.
    if (v > REVERSE_GATE.speedMps || v < -REVERSE_GATE.speedMps) {
      this.heldS = 0;
      return lean;
    }
    this.heldS += Math.max(0, dt);
    return this.heldS >= REVERSE_GATE.holdS ? lean : 0;
  }

  /** True while the gate holds a back lean at zero. */
  get holding(): boolean {
    return this.heldS > 0 && this.heldS < REVERSE_GATE.holdS;
  }

  reset(): void {
    this.heldS = 0;
  }
}

// ---------------------------------------------------------------------------------------------
// Key bindings (Free Ride)

export type FreeRideAction =
  | "forward"
  | "back"
  | "left"
  | "right"
  | "crouch"
  | "stepOff"
  | "reset"
  | "power"
  | "handle"
  | "light"
  | "horn"
  | "pedalMode";

/** The riding actions: they move the rider while held. */
export const MOTION_ACTIONS: ReadonlySet<FreeRideAction> = new Set([
  "forward",
  "back",
  "left",
  "right",
]);

export const FREE_RIDE_ACTIONS: { id: FreeRideAction; label: string }[] = [
  { id: "forward", label: "Lean forward" },
  { id: "back", label: "Lean back, brake" },
  { id: "left", label: "Turn left" },
  { id: "right", label: "Turn right" },
  { id: "crouch", label: "Crouch (hold)" },
  { id: "stepOff", label: "Step off" },
  { id: "reset", label: "Respawn (with Shift: at the start)" },
  { id: "power", label: "Power button (tap; hold to switch off)" },
  { id: "handle", label: "Handle button (hold)" },
  { id: "light", label: "Light: off, on, strobe" },
  { id: "horn", label: "Horn" },
  { id: "pedalMode", label: "Pedal mode: soft, medium, hard" },
];

export type Bindings = Record<FreeRideAction, string[]>;

/** Keys by KeyboardEvent.code, so they stay where they are on any layout. */
export const DEFAULT_BINDINGS: Bindings = {
  forward: ["KeyW", "ArrowUp"],
  back: ["KeyS", "ArrowDown"],
  left: ["KeyA", "ArrowLeft"],
  right: ["KeyD", "ArrowRight"],
  crouch: ["Space"],
  stepOff: ["KeyX"],
  reset: ["KeyR"],
  power: ["KeyP"],
  handle: ["KeyH"],
  light: ["KeyL"],
  horn: ["KeyB"],
  pedalMode: ["KeyM"],
};

/**
 * Keys that keep their meaning in Free Ride: Escape pauses, Tab and Enter move and press focus,
 * and the studio's own shortcuts (C camera, . step a frame, [ and ] speed, ? shortcut sheet).
 */
export const RESERVED_KEYS: ReadonlySet<string> = new Set([
  "Escape",
  "Tab",
  "Enter",
  "NumpadEnter",
  "KeyC",
  "Period",
  "BracketLeft",
  "BracketRight",
  "Slash",
  "MetaLeft",
  "MetaRight",
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
]);

/** At most this many keys per action. */
export const KEYS_PER_ACTION = 2;

const CODE = /^[A-Za-z][A-Za-z0-9]{0,23}$/;

/** Stored bindings, treated as hostile: anything malformed falls back to the defaults. */
export function coerceBindings(raw: unknown): Bindings {
  const out = structuredClone(DEFAULT_BINDINGS);
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return out;
  const used = new Set<string>();
  for (const { id } of FREE_RIDE_ACTIONS) {
    const keys = (raw as Record<string, unknown>)[id];
    if (!Array.isArray(keys)) continue;
    const valid = [
      ...new Set(
        keys.filter(
          (k): k is string => typeof k === "string" && CODE.test(k) && !RESERVED_KEYS.has(k),
        ),
      ),
    ]
      .filter((k) => !used.has(k))
      .slice(0, KEYS_PER_ACTION);
    for (const k of valid) used.add(k);
    out[id] = valid;
  }
  // A key that the stored set left on a default action and gave to another one stays there.
  for (const { id } of FREE_RIDE_ACTIONS) {
    const keys = (raw as Record<string, unknown>)[id];
    if (!Array.isArray(keys)) out[id] = out[id].filter((k) => !used.has(k));
  }
  return out;
}

/** The action a key code triggers, or null. */
export function actionFor(bindings: Bindings, code: string): FreeRideAction | null {
  for (const { id } of FREE_RIDE_ACTIONS) if (bindings[id].includes(code)) return id;
  return null;
}

/**
 * Binds `code` to `action`, taking it away from any other action; the action keeps at most
 * KEYS_PER_ACTION keys (the oldest goes). A reserved key is refused with the reason.
 */
export function bindKey(
  bindings: Bindings,
  action: FreeRideAction,
  code: string,
): { bindings: Bindings } | { error: string } {
  if (RESERVED_KEYS.has(code) || !CODE.test(code))
    return { error: `${keyLabel(code)} is kept for the studio. Choose another key.` };
  const next = structuredClone(bindings);
  for (const { id } of FREE_RIDE_ACTIONS) next[id] = next[id].filter((k) => k !== code);
  next[action] = [code, ...next[action]].slice(0, KEYS_PER_ACTION);
  return { bindings: next };
}

/** Leaves the action without keys. */
export function clearKeys(bindings: Bindings, action: FreeRideAction): Bindings {
  return { ...structuredClone(bindings), [action]: [] };
}

const NAMED: Record<string, string> = {
  Space: "Space",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  ShiftLeft: "Left Shift",
  ShiftRight: "Right Shift",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Backslash: "\\",
  Backspace: "Backspace",
  CapsLock: "Caps Lock",
};

/** A key code as its cap: KeyW → W, Digit1 → 1, ArrowUp → ↑. */
export function keyLabel(code: string): string {
  if (NAMED[code]) return NAMED[code];
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^Numpad[0-9]$/.test(code)) return `Num ${code.slice(6)}`;
  return code;
}
