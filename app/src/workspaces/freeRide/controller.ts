import {
  DEFAULT_RIDER_MODEL,
  freeRideApi,
  FREE_RIDE_LIMITS,
  limitsOf,
  type FreeRideInput,
  type FreeRideLimits,
  type FreeRideSource,
  type FreeRideStatus,
  type HeightMap,
} from "../../engine/freeRide.ts";
import {
  DEFAULT_DENSITIES,
  randomSeed,
  type WorldChoice,
  type WorldFrame,
} from "../../engine/world.ts";
import type { EngineClient, WheelButton } from "../../engine/types.ts";
import { readStored, writeStored } from "../../lib/storage.ts";
import {
  BrakeGate,
  DRIVE_TIMING,
  KeyIntent,
  MOTION_ACTIONS,
  PadEdges,
  STILL,
  actionFor,
  approach,
  bindKey,
  clearKeys,
  coerceBindings,
  driveAsk,
  easeCrouch,
  keyLabel,
  readPad,
  turnAuthority,
  type Bindings,
  type FreeRideAction,
  type Intent,
  type PadButton,
  type PadReading,
} from "../../lib/rideControls.ts";
import { nextCamera, useUi } from "../../state/ui.ts";
import { WheelButtonInput, type ButtonSession } from "../ride/wheelButtonInput.ts";
import { RideHints, type HintId } from "./hints.ts";
import { CLOSED, useFreeRideHud, type FreeRideHud } from "./hud.ts";

export interface ControlSnapshot {
  status: FreeRideStatus | null;
  map: HeightMap | null;
  /** The run's world and its frame, when the ground is a world. */
  world: WorldFrame | null;
  /** The device that rides: the one that moved last. */
  source: FreeRideSource;
  bindings: Bindings;
  /** What is sent now: lean in degrees, turn in degrees per second, crouch as a share. */
  lean: number;
  turn: number;
  crouch: number;
  /** The rider's choice: the keys and the stick drive (a throttle), else they set the lean. */
  drive: boolean;
  /** Drive mode rides now: chosen, and the engine takes a drive ask. */
  driving: boolean;
  /** The acceleration asked now in drive mode, m/s² (+ forward); null in lean mode. */
  accel: number | null;
  /** Near a stop a back lean is held at zero until it has been held (the brake gate). */
  stopping: boolean;
  /** A new world or a respawn is on its way. */
  starting: boolean;
  /** What is on its way: a new world, or a respawn. */
  startingKind: "world" | "respawn" | null;
  playing: boolean;
  /** The wheel's controller has power (outputs.controller_powered); null when not reported. */
  powered: boolean | null;
  /** The light and pedal modes the pad sent last (the firmware does not report them back). */
  light: number | null;
  pedalMode: number | null;
  /** The wheel button held now from the keys or the pad. */
  button: WheelButton | null;
  error: string | null;
  /** The action whose new key the next key press sets, or null. */
  capturing: FreeRideAction | null;
  /** Why the last key could not be bound. */
  bindError: string | null;
}

/** The phone commands' choices (app/src-tauri/src/engine/phone.rs: light, pedal_mode). */
export const LIGHT_MODES = ["Off", "On", "Strobe"] as const;
export const PEDAL_MODES = ["Soft", "Medium", "Hard"] as const;
/** The world a studio without a free ride starts in: the editor's old default. */
export const DEFAULT_WORLD: WorldChoice = { seed: 42, densities: { ...DEFAULT_DENSITIES } };

const rad = Math.PI / 180;
/** Keys lean this far at most (degrees), even when the run allows more: a stick reaches the rest. */
const KEY_LEAN_DEG = 10;
/** How long a passing notice ("crouch is not available") stays in the prompt, ms. */
const NOTICE_MS = 2500;
/** A start or respawn that brings no new run in this time has failed, ms. */
const START_TIMEOUT_MS = 20_000;
const BINDINGS_KEY = "freeRide.bindings";
const DRIVE_KEY = "freeRide.drive";
const HINTS_KEY = "freeRide.hints";
const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
// Keep the sequence across drawer unmounts. Native epochs increase.
let sequenceEpoch: number | null = null;
let sequence = 0;
/** The world the last start asked for: a reopened Free Ride without a run starts it again. */
let lastWorld: WorldChoice | null = null;

/**
 * What the last heartbeat sent, as shares of the run's limits (-1..1), for the 3D view's bars. In
 * drive mode `lean` is the throttle.
 */
export const sentShares = { lean: 0, turn: 0 };

/**
 * The motor's two duty readings at the last heartbeat, %, NaN when not reported: the firmware's
 * own PWM figure (`pwm`), and the share of the battery voltage the simulated inverter really
 * applies (`modulation`, the true headroom: the firmware's figure can read about 70 % while the
 * modulation is at its limit).
 */
export const motorReadout = { pwm: Number.NaN, modulation: Number.NaN };

/** One heartbeat's input: lean (degrees) or a drive ask (m/s²) with its throttle share, turn
 * (degrees per second) and crouch (a share). */
interface Sent {
  lean: number;
  turn: number;
  crouch: number;
  accel: number | null;
  throttle: number;
}

/** Focus in a place where keys type text: riding keys go to it. */
function typing(target: EventTarget | null) {
  return (
    target instanceof Element &&
    !!target.closest(
      'textarea, select, [contenteditable]:not([contenteditable="false"]), [cmdk-root], input:not([type="button"]):not([type="checkbox"]):not([type="radio"]):not([type="range"])',
    )
  );
}
/** Focus in an open menu, list or dialog: it keeps every key, and riding keys do nothing. */
function inPopup(target: EventTarget | null) {
  return (
    target instanceof Element &&
    !!target.closest(
      '[role="dialog"], [role="alertdialog"], [role="listbox"], [role="menu"], [role="option"], [role="menuitem"]',
    )
  );
}
/** Controls that use the arrow keys and Space themselves (tabs, switches, sliders, selects). */
function ownsArrows(target: EventTarget | null) {
  return (
    target instanceof Element &&
    !!target.closest(
      '[role="tab"], [role="switch"], [role="slider"], [role="combobox"], [role="radio"]',
    )
  );
}
/** The rider of a ready run has left the pedals (stepped off, fell): a respawn rides again. */
export const offPedals = (status: FreeRideStatus | null | undefined) =>
  status?.mode === "free_ride" && status.ready && status.on_pedals === false;
function wheelControl(target: EventTarget | null) {
  return (
    target instanceof Element && !!target.closest("[data-wheel-control], [data-wheel-controls]")
  );
}

/**
 * Free Ride's play mode: owns wall-clock input and the run's lifecycle. No simulation state is
 * synthesized here.
 *
 * Opening Free Ride rides: without a free ride run it starts one in the city, and the engine boots
 * the firmware and plays. Keys and the stick drive as soon as the firmware is ready; there is no
 * arm step, and a crash never pauses (the engine's play mode). R respawns at the last clear spot,
 * Shift+R at the world's start; Escape pauses, and a riding key goes on. The wheel's buttons and
 * the phone's light, horn and pedal-mode commands have keys too. Forward and back are a throttle
 * (drive mode: the rider leans for the asked acceleration, EUC Thrills' feel) or the rider's lean
 * itself (lean mode). Throttle, lean, turn and crouch are shaped by `lib/rideControls.ts` and sent
 * as rider intent; the native host keeps its own limits and watchdog (input older than 250 ms is
 * neutral).
 */
export class FreeRideController {
  private state: ControlSnapshot = {
    status: null,
    map: null,
    world: null,
    source: "keyboard_slider",
    bindings: coerceBindings(readStored<unknown>(BINDINGS_KEY, null)),
    lean: 0,
    turn: 0,
    crouch: 0,
    drive: readStored<unknown>(DRIVE_KEY, true) !== false,
    driving: false,
    accel: null,
    stopping: false,
    starting: false,
    startingKind: null,
    playing: false,
    powered: null,
    light: null,
    pedalMode: null,
    button: null,
    error: null,
    capturing: null,
    bindError: null,
  };
  private listeners = new Set<() => void>();
  private active = false;
  private timer: ReturnType<typeof setInterval> | null = null;
  private unsubscribe: (() => void) | null = null;
  private lifecycle = 0;
  private keys = new KeyIntent();
  /** Codes physically down, whatever they do. */
  private heldKeys = new Set<string>();
  /** The key that holds a wheel button now. */
  private buttonKey: string | null = null;
  private padEdges = new PadEdges();
  private brake = new BrakeGate();
  /** The stick's throttle after its lag (drive mode). */
  private padThrottle = 0;
  private crouchShare = 0;
  private lastTick = 0;
  private lastPublish = 0;
  private nextPoll = 0;
  private polling = false;
  private inputBusy = false;
  private pendingInput: FreeRideInput | null = null;
  /** The last input's refusal, while inputs are refused. */
  private inputError: string | null = null;
  /** A start or respawn asked for: done when a newer free ride run arrives. */
  private pending: { kind: "world" | "respawn"; epoch: number; at: number } | null = null;
  /** The lifecycle in which a run was started for want of one: once per opening. */
  private autoStarted = -1;
  private worldEpoch: number | null = null;
  private hints = new RideHints(readStored<HintId[]>(HINTS_KEY, []));
  private hint: HintId | null = null;
  private notice: { text: string; until: number } | null = null;
  private buttons = new WheelButtonInput(
    (state) => {
      if (this.active) this.update({ button: state.button });
    },
    (_, error) => this.say(message(error)),
  );

  private engine: EngineClient;
  constructor(engine: EngineClient) {
    this.engine = engine;
  }
  subscribe = (fn: () => void) => {
    this.listeners.add(fn);
    return () => {
      this.listeners.delete(fn);
    };
  };
  getSnapshot = () => this.state;
  private update(patch: Partial<ControlSnapshot>) {
    this.state = { ...this.state, ...patch };
    for (const listener of this.listeners) listener();
  }
  connect() {
    this.active = true;
    this.lifecycle++;
    this.lastTick = performance.now();
    this.nextPoll = 0;
    this.update({ playing: this.engine.playing });
    this.unsubscribe = this.engine.subscribe(() => this.update({ playing: this.engine.playing }));
    window.addEventListener("keydown", this.keyDown, true);
    window.addEventListener("keyup", this.keyUp, true);
    window.addEventListener("blur", this.release);
    document.addEventListener("visibilitychange", this.visibility);
    document.addEventListener("focusin", this.focusIn, true);
    this.timer = setInterval(this.tick, FREE_RIDE_LIMITS.heartbeatMs);
    this.tick();
    return () => this.disconnect();
  }
  /** Closing Free Ride stops the input; the run goes on, its rider still (the native watchdog). */
  private disconnect() {
    this.release();
    this.active = false;
    this.lifecycle++;
    this.pendingInput = null;
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    this.unsubscribe?.();
    this.unsubscribe = null;
    window.removeEventListener("keydown", this.keyDown, true);
    window.removeEventListener("keyup", this.keyUp, true);
    window.removeEventListener("blur", this.release);
    document.removeEventListener("visibilitychange", this.visibility);
    document.removeEventListener("focusin", this.focusIn, true);
    useFreeRideHud.setState(CLOSED);
  }
  /** Lets go of every key and the wheel's button: the rider's controls go neutral. */
  private release = () => {
    this.keys.clear();
    this.heldKeys.clear();
    this.brake.reset();
    this.padThrottle = 0;
    this.crouchShare = 0;
    this.buttonKey = null;
    this.buttons.release();
  };
  private visibility = () => {
    if (document.hidden) this.release();
  };
  private limits(): FreeRideLimits {
    return limitsOf(this.state.status);
  }
  /** `accel`: a drive ask (m/s²) in place of the lean, or null. */
  private packet(
    lean = 0,
    turn = 0,
    crouch = 0,
    accel: number | null = null,
  ): FreeRideInput | null {
    const status = this.state.status;
    if (!status || status.mode !== "free_ride") return null;
    // Native ManualInput uses u32; never wrap or reuse a sequence within an epoch.
    if (sequenceEpoch === null || status.epoch > sequenceEpoch) {
      sequenceEpoch = status.epoch;
      sequence = 0;
    }
    if (status.epoch !== sequenceEpoch) return null;
    if (sequence >= 0xffff_ffff) {
      this.update({ error: "Input sequence limit reached. Respawn to ride on." });
      return null;
    }
    const limits = this.limits();
    return {
      run_epoch: status.epoch,
      seq: ++sequence,
      source: this.state.source,
      desired_lean_rad: accel === null ? lean * rad : 0,
      heading_rate_rad_s: turn * rad,
      ...(limits.crouch_field ? { crouch: limits.can_crouch ? crouch : 0 } : {}),
      ...(accel !== null && limits.drive_field ? { desired_accel_mps2: accel } : {}),
      // The play-mode host rides every input; the field stays for its wire format.
      armed: true,
    };
  }
  /** One invoke in flight and one latest pending value. Never build an input queue. */
  private send(input: FreeRideInput) {
    this.pendingInput = input;
    if (this.inputBusy) return;
    const drain = async () => {
      this.inputBusy = true;
      while (this.pendingInput) {
        const next = this.pendingInput;
        this.pendingInput = null;
        try {
          await freeRideApi.input(next, this.limits());
          // A refused input's error goes once inputs get through again; other errors stay.
          if (this.inputError !== null && this.state.error === this.inputError && this.active)
            this.update({ error: null });
          this.inputError = null;
        } catch (error) {
          this.inputError = message(error);
          if (this.active) this.update({ error: this.inputError });
        }
      }
      this.inputBusy = false;
    };
    void drain();
  }
  private setSource(source: FreeRideSource) {
    if (source !== this.state.source) this.update({ source });
  }
  /** Drive mode (forward and back are a throttle) or lean mode; kept for the next session. */
  setDrive = (drive: boolean) => {
    if (drive === this.state.drive) return;
    // A held key starts again from still in the other mode: no full lean from a full throttle.
    this.keys.clear();
    this.brake.reset();
    this.padThrottle = 0;
    this.update({ drive });
    writeStored(DRIVE_KEY, drive);
  };
  /** A new run in `world`: the engine builds it and boots the firmware. */
  start = async (world: WorldChoice) => {
    if (!this.active || this.engine.kind !== "live" || this.pending) return;
    this.pending = { kind: "world", epoch: this.state.status?.epoch ?? -1, at: performance.now() };
    this.release();
    this.update({ starting: true, startingKind: "world", error: null });
    try {
      await freeRideApi.start({ world }, DEFAULT_RIDER_MODEL);
      lastWorld = structuredClone(world);
    } catch (error) {
      this.pending = null;
      this.update({ starting: false, startingKind: null, error: message(error) });
    }
  };
  /** A new world from a random seed. */
  newWorld = () => {
    void this.start({ seed: randomSeed(), densities: { ...DEFAULT_DENSITIES } });
  };
  /** R: the wheel and the rider upright at the last clear spot; Shift+R: at the world's start. */
  respawn = (toStart = false) => {
    const status = this.state.status;
    if (!this.active || this.engine.kind !== "live" || status?.mode !== "free_ride") return;
    if (this.pending) return;
    const epoch = status.epoch;
    this.pending = { kind: "respawn", epoch, at: performance.now() };
    this.release();
    this.update({ starting: true, startingKind: "respawn", error: null });
    freeRideApi.restart(epoch, toStart).catch((error: unknown) => {
      if (this.pending?.epoch === epoch) this.pending = null;
      this.update({ starting: false, startingKind: null, error: message(error) });
    });
  };
  /** Escape (the gamepad's Start): pause the run, or go on. */
  togglePause = () => {
    if (this.engine.kind !== "live" || this.state.status?.mode !== "free_ride") return;
    this.release();
    this.engine.setPlaying(!this.engine.playing);
  };
  /** X: the rider steps off the pedals; the run goes on. */
  private stepOff() {
    const status = this.state.status;
    if (!status || status.mode !== "free_ride" || !status.ready) return;
    if (!limitsOf(status).step_off) {
      this.say("Stepping off is not available in this engine build.");
      return;
    }
    const epoch = status.epoch;
    freeRideApi.stepOff(epoch).catch((error: unknown) => {
      if (this.state.status?.epoch === epoch) this.update({ error: message(error) });
    });
  }
  private say(text: string) {
    this.notice = { text, until: performance.now() + NOTICE_MS };
  }
  /** The wheel's button for the current run: down now, released with the same run epoch. */
  private session(): ButtonSession | null {
    const engine = this.engine;
    const setButton = engine.setButton?.bind(engine);
    const epoch = engine.status.runEpoch;
    if (engine.kind !== "live" || !setButton || epoch === undefined) return null;
    return {
      source: engine,
      epoch,
      setButton,
      isCurrent: () => this.active && engine.status.runEpoch === epoch,
    };
  }
  /** Holds a wheel button until `releaseButton` (the firmware times a tap or a long press). */
  pressButton = (button: WheelButton) => {
    const session = this.session();
    if (session) this.buttons.down(session, button);
  };
  releaseButton = () => {
    this.buttonKey = null;
    this.buttons.release();
  };
  /** The light: off, on, strobe in turn, or the given mode. */
  light = (mode?: number) => {
    const next = mode ?? ((this.state.light ?? 0) + 1) % LIGHT_MODES.length;
    this.command("light", next, { light: next });
  };
  horn = () => this.command("horn");
  /** The pedal mode: soft, medium, hard in turn, or the given mode. */
  pedalMode = (mode?: number) => {
    const next = mode ?? ((this.state.pedalMode ?? -1) + 1) % PEDAL_MODES.length;
    this.command("pedal_mode", next, { pedalMode: next });
  };
  private command(id: string, value?: number, done?: Partial<ControlSnapshot>) {
    if (!this.active || this.engine.kind !== "live") return;
    this.engine.sendCommand(id, value).then(
      () => {
        if (done && this.active) this.update(done);
      },
      (error: unknown) => this.say(message(error)),
    );
  }
  /** Binds a key to an action (taking it from any other); returns why not, or null. */
  bindKey = (action: FreeRideAction, code: string): string | null => {
    const result = bindKey(this.state.bindings, action, code);
    if ("error" in result) return result.error;
    this.setBindings(result.bindings);
    return null;
  };
  clearKeys = (action: FreeRideAction) => this.setBindings(clearKeys(this.state.bindings, action));
  resetKeys = () => this.setBindings(coerceBindings(null));
  /** Waits for the next key press to bind it to `action` (null stops waiting); Escape cancels. */
  captureKey = (action: FreeRideAction | null) => {
    this.update({ capturing: action, bindError: null });
  };
  private setBindings(bindings: Bindings) {
    this.release();
    this.update({ bindings });
    writeStored(BINDINGS_KEY, bindings);
  }
  private focusIn = (event: FocusEvent) => {
    // Typing, a menu or a dialog, or the wheel's own buttons take the keys: the rider goes still.
    if (typing(event.target) || inPopup(event.target) || wheelControl(event.target)) {
      this.keys.clear();
      this.crouchShare = 0;
    }
  };
  /** Riding keys act here: not while typing, in a menu or dialog, or on the wheel's buttons. */
  private takesKey(event: KeyboardEvent) {
    const target = event.target;
    if (typing(target) || inPopup(target) || wheelControl(target)) return false;
    if (event.metaKey || event.ctrlKey || event.altKey) return false;
    const ui = useUi.getState();
    if (ui.paletteOpen || ui.shortcutsOpen || ui.recordingsOpen) return false;
    // Tabs, switches, sliders and selects keep their arrows and Space.
    if (ownsArrows(target) && (event.code.startsWith("Arrow") || event.code === "Space"))
      return false;
    return true;
  }
  private keyDown = (event: KeyboardEvent) => {
    if (!this.active) return;
    const capturing = this.state.capturing;
    if (capturing) {
      event.preventDefault();
      event.stopImmediatePropagation();
      if (event.code === "Escape") this.update({ capturing: null, bindError: null });
      else if (!event.repeat) {
        const error = this.bindKey(capturing, event.code);
        this.update(error ? { bindError: error } : { capturing: null, bindError: null });
      }
      return;
    }
    if (event.code === "Escape") {
      // Radix dialogs and menus must still receive Escape.
      if (this.takesKey(event) && !event.repeat) this.togglePause();
      return;
    }
    const action = actionFor(this.state.bindings, event.code);
    if (!action) return;
    // A key held down before it reached the ride (typed into a field, say) only repeats: it must
    // be let go and pressed again.
    const fresh = !event.repeat || this.heldKeys.has(event.code);
    if (!this.takesKey(event)) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (!fresh) return;
    this.heldKeys.add(event.code);
    if (event.repeat) return;
    if (MOTION_ACTIONS.has(action) || action === "crouch") {
      this.keys.press(action as "forward" | "back" | "left" | "right" | "crouch");
      // A riding key goes on after a pause: no click.
      if (MOTION_ACTIONS.has(action) && !this.engine.playing) this.goOn();
      return;
    }
    switch (action) {
      case "reset":
        this.respawn(event.shiftKey);
        return;
      case "stepOff":
        this.stepOff();
        return;
      case "power":
      case "handle":
        if (this.buttonKey) return;
        this.buttonKey = event.code;
        this.pressButton(action);
        return;
      case "light":
        this.light();
        return;
      case "horn":
        this.horn();
        return;
      case "pedalMode":
        this.pedalMode();
        return;
    }
  };
  private keyUp = (event: KeyboardEvent) => {
    this.heldKeys.delete(event.code);
    const action = actionFor(this.state.bindings, event.code);
    if (action && (MOTION_ACTIONS.has(action) || action === "crouch"))
      this.keys.release(action as "forward" | "back" | "left" | "right" | "crouch");
    if (event.code === this.buttonKey) this.releaseButton();
    // A focused button activates on Space's release: crouch must not press it.
    if (action === "crouch" && this.active && this.takesKey(event)) event.preventDefault();
  };
  /** Plays a paused free ride. */
  private goOn() {
    if (this.engine.kind === "live" && this.state.status?.mode === "free_ride")
      this.engine.setPlaying(true);
  }
  /** The window has focus, is shown, and no text field has it. */
  private usable() {
    return document.hasFocus() && !document.hidden && !typing(document.activeElement);
  }
  /** The first connected standard gamepad. */
  private pad(): Gamepad | null {
    if (typeof navigator === "undefined" || typeof navigator.getGamepads !== "function")
      return null;
    try {
      return (
        [...navigator.getGamepads()].find(
          (pad): pad is Gamepad =>
            !!pad && pad.connected && pad.mapping === "standard" && pad.axes.length >= 2,
        ) ?? null
      );
    } catch {
      return null;
    }
  }
  /** Device intent before speed shaping. Keys build their lean only while the rider rides. */
  private read(dt: number, riding: boolean): { intent: Intent; pad: PadReading | null } {
    const keyIntent = riding ? this.keys.step(dt) : this.keys.rest();
    const raw = this.pad();
    const pad = raw ? readPad(raw) : null;
    if (!this.usable() || wheelControl(document.activeElement)) return { intent: STILL, pad };
    if (this.state.source === "gamepad")
      return { intent: pad && !pad.invalid ? pad.intent : STILL, pad };
    return { intent: keyIntent, pad };
  }
  /**
   * Lean (or a drive ask), turn and crouch to send: intent shaped by the wheel's speed and the
   * run's limits. In drive mode forward and back are a throttle scaled to the engine's drive caps
   * (`throttle` is its share); the engine's rider leans for it and ends a brake in a clean stop
   * itself, so the brake gate stays open.
   */
  private shape(intent: Intent, dt: number): Sent {
    const limits = this.limits();
    const speed = this.engine.latest?.speed;
    const speedMps = Number.isFinite(speed) ? (speed as number) / 3.6 : 0;
    let lean = 0;
    let accel: number | null = null;
    let throttle = 0;
    if (this.state.drive && limits.drive_field) {
      // The keys' throttle has its lag already; the stick's gets EUC Thrills' lean lag here.
      this.padThrottle =
        this.state.source === "gamepad"
          ? approach(this.padThrottle, intent.lean, DRIVE_TIMING.tau, DRIVE_TIMING.maxRate, dt)
          : 0;
      throttle = this.state.source === "gamepad" ? this.padThrottle : intent.lean;
      const status = this.state.status;
      accel = driveAsk(throttle, status?.accel_cap_mps2, status?.brake_cap_mps2);
      this.brake.reset();
    }
    if (accel === null) {
      throttle = 0;
      const leanMax =
        this.state.source === "keyboard_slider"
          ? Math.min(KEY_LEAN_DEG, limits.lean_max_deg)
          : limits.lean_max_deg;
      lean = this.brake.step(intent.lean, speedMps, dt) * leanMax;
    }
    // A full turn asks for the engine's own speed-scheduled cap; an older engine without one gets
    // the schedule here, held under its 20° turn lean and 30°/s.
    const authority = limits.turn_scheduled
      ? limits.turn_max_rad_s
      : turnAuthority(speedMps, 20 * rad, limits.turn_max_rad_s);
    const turn = (intent.steer * authority) / rad;
    this.crouchShare = easeCrouch(this.crouchShare, intent.crouch, dt);
    return { lean: lean + 0, turn: turn + 0, crouch: this.crouchShare, accel, throttle };
  }
  private async poll() {
    if (this.polling || !this.active || this.engine.kind !== "live") return;
    this.polling = true;
    const lifecycle = this.lifecycle;
    try {
      const status = await freeRideApi.status();
      if (!this.active || lifecycle !== this.lifecycle) return;
      const previous = this.state.status;
      if (previous && status.epoch < previous.epoch) return;
      const newRun = previous?.epoch !== status.epoch || previous?.mode !== status.mode;
      if (newRun) {
        this.keys.clear();
        this.brake.reset();
        this.padThrottle = 0;
        this.crouchShare = 0;
        this.worldEpoch = null;
        this.update({ map: null, world: null });
      }
      this.update({ status });
      const pending = this.pending;
      if (pending && status.mode === "free_ride" && status.epoch > pending.epoch) {
        this.pending = null;
        this.update({ starting: false, startingKind: null });
      } else if (pending && performance.now() - pending.at > START_TIMEOUT_MS) {
        this.pending = null;
        this.update({
          starting: false,
          startingKind: null,
          error: "No new free ride run arrived within 20 s. Check the engine log.",
        });
      }
      if (status.mode === "free_ride") {
        // A new run plays at once, at most at real time.
        if (newRun) {
          this.engine.setTargetRtf(Math.max(0.25, Math.min(1, this.engine.targetRtf)));
          if (!this.engine.playing) this.engine.setPlaying(true);
        }
      } else if (!this.pending && this.autoStarted !== lifecycle) {
        // Opening Free Ride rides: a studio without a free ride run starts one.
        this.autoStarted = lifecycle;
        void this.start(lastWorld ?? DEFAULT_WORLD);
      }
      if (status.mode === "free_ride" && this.worldEpoch !== status.epoch) {
        const ground =
          status.ground === "map"
            ? { map: await freeRideApi.map(status.epoch), world: null }
            : { world: await freeRideApi.world(status.epoch), map: null };
        if (
          !this.active ||
          lifecycle !== this.lifecycle ||
          this.state.status?.epoch !== status.epoch
        )
          return;
        this.worldEpoch = status.epoch;
        this.update(ground);
      }
    } catch (error) {
      if (this.active && lifecycle === this.lifecycle) this.update({ error: message(error) });
    } finally {
      this.polling = false;
    }
  }
  /** Gamepad buttons, on their rising edges. */
  private padButtons(pressed: PadButton[]) {
    for (const button of pressed) {
      switch (button) {
        case "reset":
          this.respawn(false);
          break;
        case "resetStart":
          this.respawn(true);
          break;
        case "stepOff":
          this.stepOff();
          break;
        case "pause":
          this.togglePause();
          break;
        case "camera": {
          const ui = useUi.getState();
          ui.setCamera(nextCamera(ui.camera, ui.cameraContext));
          break;
        }
        case "crouch":
          break;
      }
    }
  }
  private tick = () => {
    if (!this.active) return;
    const now = performance.now();
    const dt = Math.min(0.25, Math.max(0, (now - this.lastTick) / 1000));
    this.lastTick = now;
    if (now >= this.nextPoll) {
      this.nextPoll = now + 125;
      void this.poll();
    }
    const status = this.state.status;
    const riding = status?.mode === "free_ride" && status.ready && !this.pending;
    const { intent, pad } = this.read(dt, riding);
    // Edges are tracked every heartbeat, so a button held while unusable does not fire later.
    const pressed = pad ? this.padEdges.edges(pad.held) : [];
    if (this.usable()) this.padButtons(pressed);
    // The device that moves rides; a stick moved while paused goes on.
    const padPush = !!pad && pad.moving && !pad.invalid;
    if (padPush && !this.keys.moving) {
      this.setSource("gamepad");
      if (!this.engine.playing && this.usable()) this.goOn();
    } else if (this.keys.moving && !padPush) this.setSource("keyboard_slider");
    let sent: Sent = { lean: 0, turn: 0, crouch: 0, accel: null, throttle: 0 };
    if (riding) {
      sent = this.shape(intent, dt);
      const input = this.packet(sent.lean, sent.turn, sent.crouch, sent.accel);
      if (input) this.send(input);
    }
    const limits = this.limits();
    const driving = this.state.drive && limits.drive_field;
    sentShares.lean = driving ? sent.throttle : sent.lean / limits.lean_max_deg;
    const latest = this.engine.latest;
    motorReadout.pwm = Number.isFinite(latest?.pwm) ? (latest?.pwm as number) : Number.NaN;
    motorReadout.modulation = Number.isFinite(latest?.modulation)
      ? (latest?.modulation as number)
      : Number.NaN;
    // The turn bar shows the share of a full turn at this speed.
    sentShares.turn =
      limits.turn_max_rad_s > 0
        ? Math.max(-1, Math.min(1, (sent.turn * rad) / limits.turn_max_rad_s))
        : 0;
    const hint = this.hints.step({
      armed: riding && !offPedals(status) && this.engine.playing,
      lean: intent.lean,
      steer: intent.steer,
      speedMps: (this.engine.latest?.speed ?? 0) / 3.6 || 0,
      dt,
    });
    if (this.hints.changed) writeStored(HINTS_KEY, this.hints.seenList);
    this.hint = hint;
    if (intent.crouch > 0 && riding && !limits.can_crouch)
      this.notice ??= {
        text: limits.crouch_field
          ? "This rider cannot crouch: the rigid model, or the feet are off the pedals."
          : "Crouching is not available in this engine build.",
        until: now + NOTICE_MS,
      };
    if (now - this.lastPublish >= 125) {
      this.lastPublish = now;
      const powered = this.engine.outputs?.controller_powered;
      this.update({
        lean: sent.lean,
        turn: sent.turn,
        crouch: sent.crouch,
        driving,
        accel: sent.accel,
        stopping: riding && this.brake.holding,
        powered: typeof powered === "boolean" ? powered : null,
      });
      this.publishHud(now);
    }
  };
  /** The words of the overlay in the 3D view. */
  private publishHud(now: number) {
    if (this.notice && now >= this.notice.until) this.notice = null;
    const s = this.state;
    const pad = s.source === "gamepad";
    const caps = (action: FreeRideAction) => s.bindings[action].map(keyLabel);
    const first = (action: FreeRideAction) => caps(action)[0] ?? "(unbound)";
    const status = s.status;
    const hasRide = status?.mode === "free_ride";
    const respawnKey = pad ? "X" : first("reset");
    const startKey = pad ? "Back" : `Shift ${first("reset")}`;
    const riding =
      hasRide && !!status?.ready && !s.starting && !offPedals(status) && s.powered !== false;
    let prompt: string | null;
    if (this.notice) prompt = this.notice.text;
    else if (s.starting)
      prompt =
        s.startingKind === "respawn"
          ? "Respawning. The firmware boots again."
          : "Building the world.";
    else if (!hasRide) prompt = s.error ? null : "Starting free ride.";
    else if (s.powered === false)
      prompt = `The wheel is off. Hold ${pad ? "Power in the Display card" : first("power")} until it wakes.`;
    else if (!status?.ready) prompt = "The firmware is booting on the stand.";
    else if (offPedals(status))
      prompt = `Off the pedals. ${respawnKey} respawns here, ${startKey} at the start.`;
    else if (!s.playing) prompt = `Paused. ${pad ? "Start" : "Esc"} or a riding key goes on.`;
    else
      prompt =
        this.hint === "turn"
          ? pad
            ? "Push the stick sideways to turn"
            : `${first("left")} and ${first("right")} turn`
          : this.hint === "brake"
            ? pad
              ? "Pull the stick back or squeeze the left trigger to brake"
              : s.driving
                ? `${first("back")} brakes`
                : `${first("back")} leans back to brake`
            : null;
    const keys: FreeRideHud["keys"] =
      riding && this.hint === "keys"
        ? pad
          ? [
              { label: "Crouch", caps: ["LB"] },
              { label: "Step off", caps: ["B"] },
              { label: "Respawn", caps: ["X"] },
              { label: "Pause", caps: ["Start"] },
            ]
          : [
              { label: "Crouch", caps: caps("crouch") },
              { label: "Respawn", caps: caps("reset") },
              { label: "Power", caps: caps("power") },
              { label: "Light", caps: caps("light") },
              { label: "Pause", caps: ["Esc"] },
            ]
        : null;
    const boot =
      hasRide && !status?.ready && !s.starting && typeof status?.boot_remaining_s === "number"
        ? status.boot_remaining_s
        : null;
    const hud: FreeRideHud = {
      open: true,
      riding: riding && s.playing,
      drive: s.driving,
      prompt,
      boot,
      keys,
      respawn: hasRide && !s.starting ? { here: respawnKey, start: startKey } : null,
    };
    const current = useFreeRideHud.getState();
    if (
      current.open !== hud.open ||
      current.riding !== hud.riding ||
      current.drive !== hud.drive ||
      current.prompt !== hud.prompt ||
      current.boot !== hud.boot ||
      JSON.stringify(current.keys) !== JSON.stringify(hud.keys) ||
      JSON.stringify(current.respawn) !== JSON.stringify(hud.respawn)
    )
      useFreeRideHud.setState({ ...hud, onRespawn: this.respawn });
  }
}
