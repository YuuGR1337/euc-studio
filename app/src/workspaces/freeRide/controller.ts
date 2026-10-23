import {
  DEFAULT_RIDER_MODEL,
  freeRideApi,
  FREE_RIDE_LIMITS,
  limitsOf,
  type FreeRideInput,
  type FreeRideLimits,
  type FreeRideSource,
  type FreeRideStatus,
  type GroundChoice,
  type HeightMap,
  type RiderModel,
} from "../../engine/freeRide.ts";
import type { WorldFrame } from "../../engine/world.ts";
import type { EngineClient } from "../../engine/types.ts";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import { readStored, writeStored } from "../../lib/storage.ts";
import {
  BrakeGate,
  DEFAULT_PAD_OPTIONS,
  KeyIntent,
  PadEdges,
  STILL,
  actionFor,
  bindKey,
  clearKeys,
  coerceBindings,
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
import { useUi } from "../../state/ui.ts";
import { RideHints, type HintId } from "./hints.ts";
import { CLOSED, useFreeRideHud, type FreeRideHud } from "./hud.ts";

interface Pad {
  index: number;
  id: string;
  standard: boolean;
}
export interface ControlSnapshot {
  status: FreeRideStatus | null;
  map: HeightMap | null;
  /** The run's world and its frame, when the ground is a world. */
  world: WorldFrame | null;
  applied: GroundChoice | null;
  riderModel: RiderModel;
  /** The device that rides: chosen by the first push while disarmed. */
  source: FreeRideSource;
  padIndex: number | null;
  pads: Pad[];
  gamepadSupported: boolean;
  deadzone: number;
  invertLean: boolean;
  invertTurn: boolean;
  /** Stick response exponent; 1 is linear. */
  exponent: number;
  bindings: Bindings;
  /** What is sent now: lean in degrees, turn in degrees per second, crouch as a share. */
  lean: number;
  turn: number;
  crouch: number;
  /** Near a stop a back lean is held at zero until it has been held (the brake gate). */
  stopping: boolean;
  /** A key or the stick still held from before a pause: it must be let go before riding. */
  held: boolean;
  armed: boolean;
  arming: boolean;
  starting: boolean;
  /** Acknowledged neutral heartbeats on this source, ms: Arm needs 250. */
  neutralMs: number;
  inputAgeMs: number | null;
  playing: boolean;
  reason: string;
  error: string | null;
  /** The restart countdown, if one runs. */
  countdown: { endsAt: number; here: boolean } | null;
  /** The action whose new key the next key press sets, or null. */
  capturing: FreeRideAction | null;
  /** Why the last key could not be bound. */
  bindError: string | null;
}
const sliderKeys = new Set([
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Home",
  "End",
  "PageUp",
  "PageDown",
]);
const MOTION: ReadonlySet<FreeRideAction> = new Set(["forward", "back", "left", "right"]);
const rad = Math.PI / 180;
/** Keys lean this far at most (degrees), even when the run allows more: a stick reaches the rest. */
const KEY_LEAN_DEG = 10;
/** Seconds before a restart, while R or Escape can still cancel it. */
export const RESTART_COUNTDOWN_S = 3;
/** How long a passing notice ("crouch is not available") stays in the prompt, ms. */
const NOTICE_MS = 2500;
const PAD_SETTINGS_KEY = "freeRide.pad";
const BINDINGS_KEY = "freeRide.bindings";
const HINTS_KEY = "freeRide.hints";
const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
// Keep the sequence across drawer unmounts, as well as source changes. Native epochs increase.
let sequenceEpoch: number | null = null;
let sequence = 0;
let lastApplied: { epoch: number; ground: GroundChoice } | null = null;

/** What the last heartbeat sent, as shares of the run's limits (-1..1), for the 3D view's bars. */
export const sentShares = { lean: 0, turn: 0 };

/** The Heading rate slider's range, whole degrees per second within what a packet may carry. */
export const sliderTurnMaxDeg = (limits: FreeRideLimits) =>
  Math.floor(limits.turn_ceiling_rad_s / rad + 1e-9);

/** Focus in a place where keys type text: riding stops. */
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
      '[role="tab"], [role="switch"], [role="slider"], [role="combobox"], [role="radio"], [data-free-ride-slider]',
    )
  );
}
/** The rider of a ready run has left the pedals (stepped off, let go): only a restart rides on. */
export const offPedals = (status: FreeRideStatus | null | undefined) =>
  status?.mode === "free_ride" && status.ready && status.on_pedals === false;
function wheelControl(target: EventTarget | null) {
  return (
    target instanceof Element && !!target.closest("[data-wheel-control], [data-wheel-controls]")
  );
}

/**
 * Owns wall-clock input and lifecycle. No simulation state or terrain is synthesized here.
 *
 * Riding: the first push of a riding key or the stick arms the controls (after the native neutral
 * dwell), and the input then ramps up from zero; Escape pauses. Keys work wherever focus is,
 * except while text is typed or a menu or dialog is open. Lean, turn and crouch are shaped by
 * `lib/rideControls.ts` (key lag, dead zones, speed-scheduled turns, the brake gate) and sent as
 * rider intent; the native host keeps its own limits, watchdog and arm handshake.
 */
export class FreeRideController {
  private state: ControlSnapshot = {
    status: null,
    map: null,
    world: null,
    applied: null,
    riderModel: DEFAULT_RIDER_MODEL,
    source: "keyboard_slider",
    padIndex: null,
    pads: [],
    gamepadSupported:
      typeof navigator !== "undefined" && typeof navigator.getGamepads === "function",
    ...storedPadSettings(),
    bindings: coerceBindings(readStored<unknown>(BINDINGS_KEY, null)),
    lean: 0,
    turn: 0,
    crouch: 0,
    stopping: false,
    held: false,
    armed: false,
    arming: false,
    starting: false,
    neutralMs: 0,
    inputAgeMs: null,
    playing: false,
    reason: "Choose a world and start free ride.",
    error: null,
    countdown: null,
    capturing: null,
    bindError: null,
  };
  private listeners = new Set<() => void>();
  private active = false;
  private root: HTMLElement | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;
  private unsubscribe: (() => void) | null = null;
  private generation = 0;
  private lifecycle = 0;
  private keys = new KeyIntent();
  /** Codes physically down, whatever they do. */
  private heldKeys = new Set<string>();
  /** After a pause, held keys must all be let go before keys ride again. */
  private blockedKeys = false;
  /** After a pause, the stick and triggers must return to centre before they ride again. */
  private padNeedsRelease = false;
  private padEdges = new PadEdges();
  private brake = new BrakeGate();
  private crouchShare = 0;
  private sliderHeld = false;
  private sliderKey: string | null = null;
  private sliderLean = 0;
  private sliderTurn = 0;
  private wheelNeutralRequired = false;
  private wheelNeutralSince: number | null = null;
  private selectedPadId: string | null = null;
  private padWasConnected = false;
  private neutralAckSince: number | null = null;
  private lastAck: number | null = null;
  private lastStatus: number | null = null;
  private lastTick = 0;
  private lastPublish = 0;
  private nextPoll = 0;
  private polling = false;
  private inputBusy = false;
  private inputDrain: Promise<void> | null = null;
  private pendingInput: FreeRideInput | null = null;
  private pauses = 0;
  private pausingEpochs = new Set<number>();
  private sawPlaying = false;
  private pendingStart: {
    /** The ground of the new run; null when it restarts the current ground somewhere else. */
    ground: GroundChoice | null;
    riderModel: RiderModel;
    epoch: number;
    at: number;
    accepted: boolean;
    cancelled: boolean;
  } | null = null;
  private mapEpoch: number | null = null;
  private riderModelEdited = false;
  private hints = new RideHints(readStored<HintId[]>(HINTS_KEY, []));
  private hint: HintId | null = null;
  private notice: { text: string; until: number } | null = null;

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
  connect(root: HTMLElement) {
    this.active = true;
    this.root = root;
    this.lifecycle++;
    this.lastTick = performance.now();
    this.nextPoll = 0;
    this.update({ playing: this.engine.playing });
    this.unsubscribe = this.engine.subscribe(() => {
      if (
        this.pendingStart &&
        this.engine.status.state === "error" &&
        (this.engine.status.runEpoch ?? -1) > this.pendingStart.epoch
      ) {
        this.pendingStart = null;
        this.update({
          starting: false,
          error: this.engine.status.message || "The native run failed.",
        });
      }
      if (this.state.armed && this.engine.playing) this.sawPlaying = true;
      if (this.state.armed && this.sawPlaying && !this.engine.playing)
        this.pause("Run paused. Push to ride again.");
      this.update({ playing: this.engine.playing });
    });
    window.addEventListener("keydown", this.keyDown, true);
    window.addEventListener("keyup", this.keyUp, true);
    window.addEventListener("blur", this.windowBlur);
    window.addEventListener("gamepaddisconnected", this.padDisconnected);
    document.addEventListener("visibilitychange", this.visibility);
    document.addEventListener("focusin", this.focusIn, true);
    document.addEventListener("pointerup", this.sliderRelease, true);
    document.addEventListener("pointercancel", this.sliderRelease, true);
    this.timer = setInterval(this.tick, FREE_RIDE_LIMITS.heartbeatMs);
    this.tick();
    return () => this.disconnect();
  }
  private disconnect() {
    if (this.pendingStart) this.pendingStart.cancelled = true;
    if (this.pendingStart) this.engine.setPlaying(false);
    this.pause("Free ride controls closed.");
    this.active = false;
    this.lifecycle++;
    if (this.pendingStart?.accepted) void this.pauseLateStart(this.pendingStart);
    this.pendingInput = null;
    this.root = null;
    this.heldKeys.clear();
    this.blockedKeys = false;
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    this.unsubscribe?.();
    this.unsubscribe = null;
    window.removeEventListener("keydown", this.keyDown, true);
    window.removeEventListener("keyup", this.keyUp, true);
    window.removeEventListener("blur", this.windowBlur);
    window.removeEventListener("gamepaddisconnected", this.padDisconnected);
    document.removeEventListener("visibilitychange", this.visibility);
    document.removeEventListener("focusin", this.focusIn, true);
    document.removeEventListener("pointerup", this.sliderRelease, true);
    document.removeEventListener("pointercancel", this.sliderRelease, true);
    useFreeRideHud.setState(CLOSED);
  }
  private clearMotion() {
    this.keys.clear();
    this.blockedKeys = this.heldKeys.size > 0;
    this.padNeedsRelease = true;
    this.brake.reset();
    this.crouchShare = 0;
    this.sliderHeld = false;
    this.sliderKey = null;
    this.sliderLean = 0;
    this.sliderTurn = 0;
    this.wheelNeutralRequired = false;
    this.wheelNeutralSince = null;
    this.sawPlaying = false;
    this.neutralAckSince = null;
    this.pendingInput = null;
  }
  private limits(): FreeRideLimits {
    return limitsOf(this.state.status);
  }
  private packet(armed: boolean, lean = 0, turn = 0, crouch = 0): FreeRideInput | null {
    const status = this.state.status;
    if (!status || status.mode !== "free_ride") return null;
    // Native ManualInput uses u32; never wrap or reuse a sequence within an epoch.
    if (sequenceEpoch === null || status.epoch > sequenceEpoch) {
      sequenceEpoch = status.epoch;
      sequence = 0;
    }
    if (status.epoch !== sequenceEpoch) return null;
    if (sequence >= 0xffff_ffff) {
      this.update({ armed: false, error: "Input sequence limit reached. Start a new run." });
      return null;
    }
    const limits = this.limits();
    return {
      run_epoch: status.epoch,
      seq: ++sequence,
      source: this.state.source,
      desired_lean_rad: lean * rad,
      heading_rate_rad_s: turn * rad,
      // Older engines refuse unknown fields: crouch goes out only when the run reports it.
      ...(limits.crouch_field ? { crouch: limits.can_crouch ? crouch : 0 } : {}),
      armed,
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
        const generation = this.generation;
        try {
          await freeRideApi.input(next, this.limits());
          if (
            generation === this.generation &&
            this.active &&
            next.run_epoch === this.state.status?.epoch &&
            next.source === this.state.source
          ) {
            this.lastAck = performance.now();
            if (!next.armed && !this.pauses) this.neutralAckSince ??= this.lastAck;
          }
        } catch (error) {
          if (this.active && generation === this.generation) {
            this.update({ error: message(error) });
            this.pause("Input failed. Controls are disarmed.");
          }
          this.pendingInput = null;
        }
      }
      this.inputBusy = false;
    };
    this.inputDrain = drain();
  }
  pause = (reason = "Controls paused. Push to ride again.") => {
    // start() pauses before it creates its pending request. Later pauses cancel that request.
    if (this.pendingStart) this.pendingStart.cancelled = true;
    this.generation++;
    this.clearMotion();
    this.update({
      armed: false,
      arming: false,
      lean: 0,
      turn: 0,
      crouch: 0,
      stopping: false,
      neutralMs: 0,
      reason,
    });
    const status = this.state.status;
    if (this.engine.kind !== "live") return;
    if (this.pendingStart) this.engine.setPlaying(false);
    if (status?.mode !== "free_ride") {
      if (this.engine.status.runMode === "free_ride") this.engine.setPlaying(false);
      return;
    }
    // The neutral packet precedes the pause request. Pause also clears native input immediately.
    const input = this.packet(false);
    if (input) this.send(input);
    if (this.pausingEpochs.has(status.epoch)) return;
    this.pausingEpochs.add(status.epoch);
    this.pauses++;
    const epoch = status.epoch;
    void freeRideApi
      .pause(epoch)
      .catch((error: unknown) => {
        if (this.state.status?.epoch === epoch) this.update({ error: message(error) });
      })
      .finally(() => {
        this.pauses--;
        this.pausingEpochs.delete(epoch);
        this.neutralAckSince = null;
      });
  };
  setRiderModel = (riderModel: RiderModel) => {
    if (
      !this.active ||
      this.engine.kind !== "live" ||
      this.state.starting ||
      riderModel === this.state.riderModel
    )
      return;
    this.engine.setPlaying(false);
    this.pause("Rider model selected for the next run.");
    this.riderModelEdited = true;
    this.update({ riderModel, playing: false });
  };
  setSource = (source: FreeRideSource) => {
    if (source === this.state.source) return;
    this.pause("Input source changed. Push to ride.");
    this.update({ source });
    // seq stays monotonic across source changes. The next neutral packet selects the native source.
  };
  /**
   * While disarmed the device that pushes becomes the source, without a pause: disarmed packets
   * are neutral, the native host takes the new source from them, and its neutral dwell starts
   * again. The push then arms once the dwell is met.
   */
  private takeSource(source: FreeRideSource) {
    if (source === this.state.source || this.state.armed || this.state.arming) return;
    this.neutralAckSince = null;
    this.pendingInput = null;
    this.update({ source, neutralMs: 0 });
  }
  selectPad = (index: number | null) => {
    this.pause("Gamepad selection changed. Push the stick to ride.");
    this.selectedPadId = this.state.pads.find((pad) => pad.index === index)?.id ?? null;
    this.padWasConnected = false;
    this.padEdges.reset();
    this.update({ padIndex: index });
  };
  configure = (
    patch: Partial<Pick<ControlSnapshot, "deadzone" | "invertLean" | "invertTurn" | "exponent">>,
  ) => {
    if (this.state.armed || this.state.arming || this.engine.playing) return;
    if (
      patch.deadzone !== undefined &&
      (!Number.isFinite(patch.deadzone) || patch.deadzone < 0.05 || patch.deadzone > 0.3)
    )
      return;
    if (
      patch.exponent !== undefined &&
      (!Number.isFinite(patch.exponent) || patch.exponent < 1 || patch.exponent > 2.5)
    )
      return;
    this.pause("Input settings changed. Push to ride.");
    this.update(patch);
    const { deadzone, invertLean, invertTurn, exponent } = this.state;
    writeStored(PAD_SETTINGS_KEY, { deadzone, invertLean, invertTurn, exponent });
  };
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
    if (action && (this.state.armed || this.state.arming))
      this.pause("Changing keys. Push to ride.");
    this.update({ capturing: action, bindError: null });
  };
  private setBindings(bindings: Bindings) {
    if (this.state.armed || this.state.arming) this.pause("Keys changed. Push to ride.");
    this.keys.clear();
    this.update({ bindings });
    writeStored(BINDINGS_KEY, bindings);
  }
  sliderStart = () => {
    if (this.state.source !== "keyboard_slider" || !this.state.armed) return;
    this.keys.clear();
    this.blockedKeys = true;
    this.sliderHeld = true;
  };
  sliderValue = (axis: "lean" | "turn", value: number) => {
    if (
      !this.state.armed ||
      !this.sliderHeld ||
      this.state.source !== "keyboard_slider" ||
      !Number.isFinite(value)
    )
      return;
    const limits = this.limits();
    const turnMax = sliderTurnMaxDeg(limits);
    if (axis === "lean")
      this.sliderLean = Math.max(-limits.lean_max_deg, Math.min(limits.lean_max_deg, value));
    else this.sliderTurn = Math.max(-turnMax, Math.min(turnMax, value));
    this.update({ lean: this.sliderLean, turn: this.sliderTurn });
  };
  sliderRelease = () => {
    if (!this.sliderHeld) return;
    this.sliderHeld = false;
    this.sliderKey = null;
    this.sliderLean = 0;
    this.sliderTurn = 0;
    this.blockedKeys = this.heldKeys.size > 0;
    this.update({ lean: 0, turn: 0 });
  };
  sliderKeyDown = (event: ReactKeyboardEvent) => {
    if (!sliderKeys.has(event.code)) return;
    event.preventDefault();
    if (this.sliderKey && this.sliderKey !== event.code) {
      this.sliderLean = 0;
      this.sliderTurn = 0;
      this.update({ lean: 0, turn: 0 });
      return;
    }
    this.sliderKey = event.code;
    this.sliderStart();
    const axis = event.currentTarget.getAttribute("data-free-ride-slider");
    const sign = ["ArrowUp", "ArrowRight", "End", "PageUp"].includes(event.code) ? 1 : -1;
    const limits = this.limits();
    if (axis === "lean" || axis === "turn")
      this.sliderValue(
        axis,
        sign * (axis === "lean" ? limits.lean_max_deg : sliderTurnMaxDeg(limits)),
      );
  };
  sliderKeyUp = (event: ReactKeyboardEvent) => {
    if (sliderKeys.has(event.code)) this.sliderRelease();
  };
  private windowBlur = () => this.pause("Window lost focus. Push to ride again.");
  private padDisconnected = (event: GamepadEvent) => {
    if (event.gamepad.index !== this.state.padIndex) return;
    if (this.state.source === "gamepad") this.pause("Gamepad disconnected. Controls are paused.");
    this.selectedPadId = null;
    this.padWasConnected = false;
    this.padEdges.reset();
    this.update({ padIndex: null });
  };
  private visibility = () => {
    if (document.hidden) this.pause("Window is hidden. Controls are paused.");
  };
  private focusIn = (event: FocusEvent) => {
    if (wheelControl(event.target)) {
      if (this.state.arming) this.pause("Wheel controls selected during Arm. Controls are paused.");
      this.clearMotion();
      this.wheelNeutralRequired = true;
      this.sawPlaying = this.engine.playing;
      this.update({
        lean: 0,
        turn: 0,
        crouch: 0,
        neutralMs: 0,
        reason: "Manual input is neutral for wheel controls.",
      });
      // Keep the run and measured button hold active. Only manual lean and turn become neutral.
      if (this.state.armed) {
        const input = this.packet(true);
        if (input) this.send(input);
      }
      return;
    }
    if (typing(event.target)) this.pause("Text field selected. Controls are paused.");
    else if (inPopup(event.target) && (this.state.armed || this.state.arming))
      this.pause("A menu or dialog is open. Controls are paused.");
    else if (
      this.sliderHeld &&
      !(event.target instanceof Element && event.target.closest("[data-free-ride-slider]"))
    ) {
      this.pause("Slider lost focus. Controls are paused.");
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
    if (
      ownsArrows(target) &&
      (event.code.startsWith("Arrow") || event.code === "Space" || sliderKeys.has(event.code))
    )
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
      this.cancelCountdown();
      this.pause("Paused. Push to ride again.");
      return; // Radix dialogs and menus must still receive Escape after the pause.
    }
    const action = actionFor(this.state.bindings, event.code);
    if (!action) return;
    // A key held down before it reached the ride (typed into a field, say) only repeats: it must
    // be let go and pressed again.
    const fresh = !event.repeat || this.heldKeys.has(event.code);
    this.heldKeys.add(event.code);
    if (!this.takesKey(event)) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (!fresh) {
      this.blockedKeys = true;
      return;
    }
    if (event.repeat && !MOTION.has(action) && action !== "crouch") return;
    switch (action) {
      case "reset":
        this.toggleCountdown();
        return;
      case "stepOff":
        this.stepOff();
        return;
      case "crouch":
        if (!this.blockedKeys) this.keys.press("crouch");
        return;
      default:
        if (this.sliderHeld || this.blockedKeys) return;
        this.keys.press(action as "forward" | "back" | "left" | "right");
    }
  };
  private keyUp = (event: KeyboardEvent) => {
    this.heldKeys.delete(event.code);
    const action = actionFor(this.state.bindings, event.code);
    if (action && action !== "reset" && action !== "stepOff")
      this.keys.release(action as "forward" | "back" | "left" | "right" | "crouch");
    // A focused button activates on Space's release: crouch must not press it.
    if (action === "crouch" && this.active && this.takesKey(event)) event.preventDefault();
    if (!this.heldKeys.size) this.blockedKeys = false;
  };
  private wheelInput(intent: Intent): [Intent, boolean] {
    if (wheelControl(document.activeElement)) {
      this.wheelNeutralSince = null;
      return [STILL, this.state.armed];
    }
    if (!this.wheelNeutralRequired) return [intent, true];
    if (
      intent.lean !== 0 ||
      intent.steer !== 0 ||
      (this.state.source === "keyboard_slider" && this.heldKeys.size > 0)
    ) {
      this.wheelNeutralSince = null;
      return [STILL, this.state.armed];
    }
    this.wheelNeutralSince ??= performance.now();
    if (performance.now() - this.wheelNeutralSince >= 250) {
      this.wheelNeutralRequired = false;
      this.wheelNeutralSince = null;
    }
    return [STILL, this.state.armed || !this.wheelNeutralRequired];
  }
  /** The standard gamepad that rides: the chosen one, else the first connected. */
  private pad(): Gamepad | null {
    if (!this.state.gamepadSupported) return null;
    let pads: (Gamepad | null)[];
    try {
      pads = [...navigator.getGamepads()];
    } catch (error) {
      this.update({ error: message(error), gamepadSupported: false });
      return null;
    }
    const usable = (pad: Gamepad | null): pad is Gamepad =>
      !!pad && pad.connected && pad.mapping === "standard" && pad.axes.length >= 2;
    const chosen = this.state.padIndex === null ? null : pads[this.state.padIndex];
    if (usable(chosen ?? null) && chosen?.id === this.selectedPadId) return chosen;
    if (this.state.source === "gamepad" && (this.state.armed || this.state.arming)) return null;
    const first = pads.find(usable) ?? null;
    if (first && (first.index !== this.state.padIndex || first.id !== this.selectedPadId)) {
      this.selectedPadId = first.id;
      this.padWasConnected = false;
      this.padEdges.reset();
      this.update({ padIndex: first.index });
    }
    return first;
  }
  /** Device intent before speed shaping, and whether the controls may move the rider. */
  private read(dt: number): {
    intent: Intent;
    usable: boolean;
    keysMoving: boolean;
    pad: PadReading | null;
  } {
    // Held keys build their lean only while riding: the ramp starts after the neutral first packet.
    const keyIntent = this.state.armed ? this.keys.step(dt) : this.keys.rest();
    const raw = this.pad();
    const pad = raw
      ? readPad(raw, {
          deadZone: this.state.deadzone,
          invertLean: this.state.invertLean,
          invertSteer: this.state.invertTurn,
          exponent: this.state.exponent,
        })
      : null;
    if (pad && !pad.moving && !pad.held.crouch) this.padNeedsRelease = false;
    const keysMoving = this.keys.moving && !this.blockedKeys;
    if (!document.hasFocus() || document.hidden || typing(document.activeElement))
      return { intent: STILL, usable: false, keysMoving, pad };
    if (this.state.source === "gamepad") {
      if (!raw || !pad) {
        if (this.padWasConnected) {
          this.pause("Gamepad disconnected or changed. Push the stick to ride again.");
          this.selectedPadId = null;
          this.update({ padIndex: null });
        }
        this.padWasConnected = false;
        return { intent: STILL, usable: false, keysMoving, pad };
      }
      this.padWasConnected = true;
      if (pad.invalid) {
        this.pause("Gamepad input is invalid.");
        return { intent: STILL, usable: false, keysMoving, pad };
      }
      const [intent, usable] = this.wheelInput(this.padNeedsRelease ? STILL : pad.intent);
      return { intent, usable, keysMoving, pad };
    }
    if (this.sliderHeld) {
      const [, usable] = this.wheelInput(STILL);
      return { intent: STILL, usable, keysMoving: false, pad };
    }
    // A pause clears all keys. A full release is required before new keyboard motion.
    if (this.blockedKeys) {
      const [, usable] = this.wheelInput(STILL);
      return { intent: STILL, usable: this.state.armed && usable, keysMoving: false, pad };
    }
    const [intent, usable] = this.wheelInput(keyIntent);
    return { intent, usable, keysMoving, pad };
  }
  /** Lean, turn and crouch to send: intent shaped by the wheel's speed and the run's limits. */
  private shape(intent: Intent, dt: number): { lean: number; turn: number; crouch: number } {
    const limits = this.limits();
    const speed = this.engine.latest?.speed;
    const speedMps = Number.isFinite(speed) ? (speed as number) / 3.6 : 0;
    const leanMax =
      this.state.source === "keyboard_slider"
        ? Math.min(KEY_LEAN_DEG, limits.lean_max_deg)
        : limits.lean_max_deg;
    const lean = this.brake.step(intent.lean, speedMps, dt) * leanMax;
    // A full turn asks for the engine's own speed-scheduled cap; an older engine without one gets
    // the schedule here, held under its 20° turn lean and 30°/s.
    const authority = limits.turn_scheduled
      ? limits.turn_max_rad_s
      : turnAuthority(speedMps, 20 * rad, limits.turn_max_rad_s);
    const turn = (intent.steer * authority) / rad;
    this.crouchShare = easeCrouch(this.crouchShare, intent.crouch, dt);
    return { lean: lean + 0, turn: turn + 0, crouch: this.crouchShare };
  }
  /** Acknowledged neutral heartbeats on the current source, ms. */
  private dwellMs(now = performance.now()) {
    return this.neutralAckSince !== null && !this.pauses
      ? Math.max(0, now - this.neutralAckSince)
      : 0;
  }
  /**
   * Arms the controls. From the Ride button (`push` false) the controls must be still; from the
   * first push of a riding key or the stick (`push` true) they are not, and the push only goes out
   * after the first confirmed, neutral armed packet, ramping up from zero.
   */
  arm = async (push = false) => {
    const status = this.state.status;
    if (
      !this.active ||
      !status?.ready ||
      status.mode !== "free_ride" ||
      this.engine.status.state === "error" ||
      this.engine.status.state === "halted" ||
      this.state.armed ||
      this.state.arming ||
      this.state.starting ||
      this.state.countdown ||
      this.pauses ||
      this.dwellMs() < FREE_RIDE_LIMITS.neutralMs ||
      status.source !== this.state.source ||
      !document.hasFocus() ||
      document.hidden ||
      typing(document.activeElement)
    )
      return;
    if (
      performance.now() - this.lastTick >= 250 ||
      this.lastAck === null ||
      performance.now() - this.lastAck >= 250
    ) {
      this.pause("The input clock is stale. Push to ride again.");
      return;
    }
    const moved = () => {
      if (push) return false;
      const { intent } = this.read(0);
      return intent.lean !== 0 || intent.steer !== 0;
    };
    const generation = ++this.generation;
    const epoch = status.epoch;
    const source = this.state.source;
    const armDeadline = performance.now() + 250;
    this.update({ arming: true, error: null });
    // Stop disarmed heartbeats while arm is in flight: they would undo native arm.
    this.pendingInput = null;
    const timeout = setTimeout(() => {
      if (generation === this.generation && this.state.arming) {
        this.update({ error: "Native Arm did not confirm before the 250 ms deadline." });
        this.pause("Arm confirmation timed out. Controls are paused.");
      }
    }, 250);
    try {
      await this.inputDrain;
      if (!this.active || generation !== this.generation || moved() || !this.usableNow()) {
        this.pause("Controls changed during Arm. Push to ride again.");
        return;
      }
      await freeRideApi.arm(epoch, source);
      // The IPC reply acknowledges Cmd::ManualArm in the runner queue, not native arm.
      // Keep all ordinary heartbeats suspended until the native state confirms it.
      const deadline = Math.min(armDeadline, performance.now() + 200);
      let confirmed: FreeRideStatus | null = null;
      let lastReason = status.reason;
      for (let attempt = 0; attempt < 8 && performance.now() < deadline; attempt++) {
        if (!this.active || generation !== this.generation) {
          await freeRideApi.pause(epoch);
          return;
        }
        let timer: ReturnType<typeof setTimeout> | undefined;
        let candidate: FreeRideStatus;
        try {
          candidate = await Promise.race([
            freeRideApi.status(),
            new Promise<never>((_, reject) => {
              timer = setTimeout(
                () => reject(new Error("Native Arm status timed out.")),
                Math.max(0, deadline - performance.now()),
              );
            }),
          ]);
        } finally {
          clearTimeout(timer);
        }
        if (!this.active || generation !== this.generation) {
          await freeRideApi.pause(epoch);
          return;
        }
        if (performance.now() >= deadline) throw new Error("Native Arm status timed out.");
        if (
          epoch !== this.state.status?.epoch ||
          this.state.source !== source ||
          moved() ||
          !this.usableNow()
        ) {
          throw new Error("Controls changed during Arm. Push to ride again.");
        }
        if (
          candidate.epoch !== epoch ||
          candidate.mode !== "free_ride" ||
          !candidate.ready ||
          candidate.source !== source
        ) {
          throw new Error(
            `Native Arm did not match the ready run and input source. ${candidate.reason}`,
          );
        }
        lastReason = candidate.reason;
        if (candidate.armed) {
          confirmed = candidate;
          break;
        }
        if (candidate.reason !== status.reason)
          throw new Error(candidate.reason || "Native Arm was rejected.");
        await new Promise<void>((resolve) =>
          setTimeout(resolve, Math.min(25, Math.max(0, deadline - performance.now()))),
        );
      }
      if (!confirmed) throw new Error(`Native Arm did not confirm within 200 ms. ${lastReason}`);
      this.generation++;
      this.update({
        status: confirmed,
        armed: true,
        arming: false,
        reason: confirmed.reason,
      });
      this.lastTick = performance.now();
      this.lastStatus = this.lastTick;
      this.lastAck = this.lastTick;
      this.sawPlaying = this.engine.playing;
      this.brake.reset();
      const input = this.packet(true);
      if (input) this.send(input); // First confirmed armed packet is always neutral.
      // From the Ride button, leave the button: riding keys should not land on it.
      if (!push) this.root?.focus({ preventScroll: true });
    } catch (error) {
      if (generation === this.generation) {
        this.update({ error: message(error) });
        this.pause("Arm failed. Controls are paused.");
      }
    } finally {
      clearTimeout(timeout);
    }
  };
  /** The window has focus, is shown, and no text field has it. */
  private usableNow() {
    return document.hasFocus() && !document.hidden && !typing(document.activeElement);
  }
  start = async (ground: GroundChoice) => {
    await this.startRun(ground, () => freeRideApi.start(ground, this.state.riderModel));
  };
  /** A new run: pauses, asks the engine, and waits for its newer epoch (poll). */
  private async startRun(ground: GroundChoice | null, request: () => Promise<void>) {
    if (!this.active || this.engine.kind !== "live" || this.state.starting || !this.state.status)
      return;
    this.pause("Starting a new free ride.");
    const lifecycle = this.lifecycle;
    const pending = {
      ground: ground ? structuredClone(ground) : null,
      riderModel: this.state.riderModel,
      epoch: this.state.status.epoch,
      at: performance.now(),
      accepted: false,
      cancelled: false,
    };
    this.pendingStart = pending;
    this.update({ starting: true, error: null });
    try {
      await request();
      pending.accepted = true;
      if (pending.cancelled || !this.active || lifecycle !== this.lifecycle) {
        pending.cancelled = true;
        // This pause queues behind the accepted start, including a slow firmware build.
        this.engine.setPlaying(false);
        // start acknowledges a queue operation, not a run epoch. Observe and pause its late run.
        void this.pauseLateStart(pending);
        return;
      }
      this.nextPoll = 0;
    } catch (error) {
      if (this.active && this.pendingStart === pending) {
        this.pendingStart = null;
        this.update({
          starting: false,
          error: message(error),
          reason: ground ? "The ground was not applied." : "The run did not restart.",
        });
      }
    }
  }
  /** R: start or cancel the restart countdown. */
  toggleCountdown = () => {
    if (this.state.countdown) {
      this.cancelCountdown();
      return;
    }
    const status = this.state.status;
    if (!this.active || this.engine.kind !== "live" || status?.mode !== "free_ride") return;
    if (this.state.starting || this.pendingStart) return;
    // The engine's quick reset restarts where the wheel is on a world (the test pad: its origin).
    const here = limitsOf(status).restart && status.ground === "world";
    this.update({
      countdown: { endsAt: performance.now() + RESTART_COUNTDOWN_S * 1000, here },
    });
  };
  cancelCountdown = () => {
    if (this.state.countdown) this.update({ countdown: null });
  };
  /** The countdown ended: a truthful restart, the firmware booting again. */
  private restart() {
    const status = this.state.status;
    if (!status || status.mode !== "free_ride") return;
    const epoch = status.epoch;
    if (limitsOf(status).restart) {
      void this.startRun(null, () => freeRideApi.restart(epoch));
      return;
    }
    // An older engine restarts the run at its start point, on the same ground.
    const ground = this.state.applied ?? (lastApplied?.epoch === epoch ? lastApplied.ground : null);
    if (ground) void this.start(ground);
    else {
      this.pause("Restarting the run at its start point.");
      this.engine.restart();
    }
  }
  /** X: the rider steps off the pedals (the run then ends; R starts another one). */
  private stepOff() {
    const status = this.state.status;
    if (!this.state.armed || !status || status.mode !== "free_ride") return;
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
  private async pauseLateStart(request: NonNullable<FreeRideController["pendingStart"]>) {
    for (let attempt = 0; attempt < 200; attempt++) {
      if (this.active) return; // Active polling also observes request.cancelled.
      try {
        const status = await freeRideApi.status();
        if (status.epoch > request.epoch) {
          if (status.mode === "free_ride") await freeRideApi.pause(status.epoch);
          if (this.pendingStart === request) {
            this.pendingStart = null;
            this.update({
              starting: false,
              reason: "The late start was paused after controls closed.",
            });
          }
          return;
        }
      } catch (error) {
        this.update({ error: message(error) });
      }
      await new Promise<void>((resolve) => setTimeout(resolve, 100));
    }
    this.update({
      error: "The queued start has no run reply. Check the engine log before continuing.",
    });
  }
  continueBoot = () => {
    if (
      this.active &&
      this.state.status?.mode === "free_ride" &&
      !this.state.status.ready &&
      !this.state.starting
    ) {
      this.engine.setTargetRtf(Math.max(0.25, Math.min(1, this.engine.targetRtf)));
      this.engine.setPlaying(true);
    }
  };
  private async poll() {
    if (this.polling || this.state.arming || !this.active || this.engine.kind !== "live") return;
    this.polling = true;
    const lifecycle = this.lifecycle;
    const generation = this.generation;
    try {
      const status = await freeRideApi.status();
      if (!this.active || lifecycle !== this.lifecycle) return;
      const previous = this.state.status;
      if (previous && status.epoch < previous.epoch) return;
      if (
        generation !== this.generation &&
        previous?.epoch === status.epoch &&
        previous.mode === status.mode
      )
        return;
      this.lastStatus = performance.now();
      if (previous?.epoch !== status.epoch || previous.mode !== status.mode) {
        this.generation++;
        this.clearMotion();
        this.lastAck = null;
        this.mapEpoch = null;
        this.update({
          armed: false,
          arming: false,
          countdown: null,
          map: null,
          world: null,
          applied: lastApplied?.epoch === status.epoch ? lastApplied.ground : null,
          ...(!this.riderModelEdited ? { riderModel: status.rider_model } : {}),
          reason: status.reason,
        });
      } else if (generation === this.generation && this.state.armed && !status.armed) {
        this.pause(status.reason || "Native controls disarmed.");
      }
      this.update({ status });
      const request = this.pendingStart;
      if (request?.accepted && status.mode === "free_ride" && status.epoch > request.epoch) {
        this.pendingStart = null;
        if (status.rider_model !== request.riderModel) {
          this.engine.setPlaying(false);
          this.pause("The new run uses a different rider model.");
          this.update({
            starting: false,
            error: "The native rider model differs from the requested model.",
          });
          return;
        }
        // A restart here keeps the ground it restarted on.
        const ground = request.ground ?? (lastApplied ? lastApplied.ground : null);
        if (ground) lastApplied = { epoch: status.epoch, ground };
        this.riderModelEdited = false;
        this.engine.setTargetRtf(Math.max(0.25, Math.min(1, this.engine.targetRtf)));
        this.update({ applied: ground, starting: false, reason: status.reason });
        if (request.cancelled)
          this.pause("The queued run arrived after controls closed. Push to ride when ready.");
      } else if (request && !request.cancelled && performance.now() - request.at > 20_000) {
        request.cancelled = true;
        this.engine.setPlaying(false);
        this.update({
          error: "No new free ride run arrived within 20 s. Check the engine log.",
        });
      }
      if (status.mode === "free_ride" && this.mapEpoch !== status.epoch) {
        const ground =
          status.ground === "world"
            ? { world: await freeRideApi.world(status.epoch), map: null }
            : { map: await freeRideApi.map(status.epoch), world: null };
        if (
          !this.active ||
          lifecycle !== this.lifecycle ||
          this.state.status?.epoch !== status.epoch
        )
          return;
        this.mapEpoch = status.epoch;
        this.update(ground);
      }
    } catch (error) {
      if (this.active && lifecycle === this.lifecycle) {
        this.update({ error: message(error) });
        if (this.state.armed || this.state.arming)
          this.pause("Native status failed. Controls are paused.");
      }
    } finally {
      this.polling = false;
    }
  }
  /** Gamepad buttons, on their rising edges. */
  private padButtons(pressed: PadButton[]) {
    for (const button of pressed) {
      switch (button) {
        case "reset":
          this.toggleCountdown();
          break;
        case "stepOff":
          this.stepOff();
          break;
        case "pause":
          this.cancelCountdown();
          this.pause("Gamepad pause. Push the stick to ride again.");
          break;
        case "camera": {
          const ui = useUi.getState();
          ui.setCamera(ui.camera === "follow" ? "orbit" : "follow");
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
    if (now - this.lastTick >= 250) this.neutralAckSince = null;
    if (this.state.armed && now - this.lastTick >= 250)
      this.pause("Input clock stopped. Controls are paused.");
    if (this.state.armed && this.lastAck !== null && now - this.lastAck >= 250)
      this.pause("Input reply timed out. Controls are paused.");
    if (this.state.armed && this.lastStatus !== null && now - this.lastStatus >= 500)
      this.pause("Native status is stale. Controls are paused.");
    this.lastTick = now;
    if (now >= this.nextPoll) {
      this.nextPoll = now + 125;
      void this.poll();
    }
    // Device inventory is bounded by the browser's connected slots.
    let pads: Pad[] = [];
    if (this.state.gamepadSupported) {
      try {
        pads = navigator
          .getGamepads()
          .flatMap((pad) =>
            pad?.connected
              ? [{ index: pad.index, id: pad.id, standard: pad.mapping === "standard" }]
              : [],
          );
      } catch (error) {
        this.update({ error: message(error), gamepadSupported: false });
      }
    }
    const { intent, usable, keysMoving, pad } = this.read(dt);
    // Edges are tracked every heartbeat, so a button held while unusable does not fire later.
    const pressed = pad ? this.padEdges.edges(pad.held) : [];
    if (usable) this.padButtons(pressed);
    if (this.state.armed && !usable)
      this.pause("Input focus or device is unavailable. Controls are paused.");
    const status = this.state.status;
    const ready =
      !this.state.arming &&
      !this.state.starting &&
      !this.pauses &&
      status?.mode === "free_ride" &&
      status.ready;
    // While disarmed the device that pushes rides.
    const padPush = !!pad && pad.moving && !this.padNeedsRelease;
    if (!this.state.armed && !this.state.arming) {
      if (padPush && !keysMoving) this.takeSource("gamepad");
      else if (keysMoving && !padPush) this.takeSource("keyboard_slider");
    }
    const neutralMs = this.dwellMs(now);
    let sent = { lean: 0, turn: 0, crouch: 0 };
    if (ready) {
      if (this.state.armed) {
        sent = this.sliderHeld
          ? { lean: this.sliderLean, turn: this.sliderTurn, crouch: 0 }
          : this.shape(intent, dt);
      }
      const input = this.packet(this.state.armed, sent.lean, sent.turn, sent.crouch);
      if (input) this.send(input);
    }
    const limits = this.limits();
    sentShares.lean = sent.lean / limits.lean_max_deg;
    // The turn bar shows the share of a full turn at this speed (the slider can ask for more).
    sentShares.turn =
      limits.turn_max_rad_s > 0
        ? Math.max(-1, Math.min(1, (sent.turn * rad) / limits.turn_max_rad_s))
        : 0;
    if (this.state.countdown && now >= this.state.countdown.endsAt) {
      this.update({ countdown: null });
      this.restart();
    }
    const pushing = this.state.source === "gamepad" ? padPush : keysMoving && !this.sliderHeld;
    if (
      !this.state.armed &&
      !this.state.arming &&
      pushing &&
      usable &&
      neutralMs >= FREE_RIDE_LIMITS.neutralMs &&
      status?.source === this.state.source &&
      !offPedals(status)
    )
      void this.arm(true);
    const hint = this.hints.step({
      armed: this.state.armed,
      lean: intent.lean,
      steer: intent.steer,
      speedMps: (this.engine.latest?.speed ?? 0) / 3.6 || 0,
      dt,
    });
    if (this.hints.changed) writeStored(HINTS_KEY, this.hints.seenList);
    this.hint = hint;
    if (intent.crouch > 0 && this.state.armed && !limits.can_crouch)
      this.notice ??= {
        text: limits.crouch_field
          ? "This rider cannot crouch: the rigid model, or the feet are off the pedals."
          : "Crouching is not available in this engine build.",
        until: now + NOTICE_MS,
      };
    if (now - this.lastPublish >= 125) {
      this.lastPublish = now;
      this.update({
        pads,
        lean: sent.lean,
        turn: sent.turn,
        crouch: sent.crouch,
        stopping: this.state.armed && this.brake.holding,
        held: this.blockedKeys || (this.padNeedsRelease && !!pad?.moving),
        neutralMs,
        inputAgeMs: this.lastAck === null ? null : now - this.lastAck,
      });
      this.publishHud(now, pushing);
    }
  };
  /** The words of the overlay in the 3D view. */
  private publishHud(now: number, pushing: boolean) {
    if (this.notice && now >= this.notice.until) this.notice = null;
    const s = this.state;
    const pad = s.source === "gamepad";
    const caps = (action: FreeRideAction) => s.bindings[action].map(keyLabel);
    const first = (action: FreeRideAction) => caps(action)[0] ?? "(unbound)";
    const status = s.status;
    const hasRide = status?.mode === "free_ride";
    let prompt: string | null = null;
    if (this.notice) prompt = this.notice.text;
    else if (s.starting) prompt = "Starting the run. The firmware boots first.";
    else if (!hasRide) prompt = null;
    else if (offPedals(status) && !s.armed)
      prompt = `Off the wheel. ${first("reset")} starts another ride here.`;
    else if (!status?.ready) {
      const boot = status?.boot_remaining_s;
      prompt =
        typeof boot === "number" && boot > 0
          ? `The firmware is booting on the stand: ${Math.ceil(boot)} s`
          : "The firmware is booting on the stand.";
    } else if (s.armed) {
      prompt =
        this.hint === "turn"
          ? pad
            ? "Push the stick sideways to turn"
            : `${first("left")} and ${first("right")} turn`
          : this.hint === "brake"
            ? pad
              ? "Pull the stick back or squeeze the left trigger to brake"
              : `${first("back")} leans back to brake`
            : null;
    } else if (!s.arming) {
      const stick = s.pads.some((p) => p.standard);
      prompt =
        this.blockedKeys || (pad && this.padNeedsRelease && s.pads.length > 0)
          ? "Let go of the controls, then push to ride"
          : pushing
            ? "Getting ready…"
            : pad
              ? "Push the stick up or squeeze the right trigger to ride"
              : `Push ${first("forward")}${stick ? " or the stick" : ""} to ride`;
    }
    const keys: FreeRideHud["keys"] =
      s.armed && this.hint === "keys"
        ? pad
          ? [
              { label: "Crouch", caps: ["LB"] },
              { label: "Step off", caps: ["B"] },
              { label: "Restart here", caps: ["X"] },
              { label: "Pause", caps: ["Start"] },
            ]
          : [
              { label: "Crouch", caps: caps("crouch") },
              { label: "Step off", caps: caps("stepOff") },
              { label: "Restart here", caps: caps("reset") },
              { label: "Pause", caps: ["Esc"] },
            ]
        : null;
    const hud: FreeRideHud = {
      open: true,
      armed: s.armed,
      prompt,
      countdown: s.countdown,
      keys,
    };
    const current = useFreeRideHud.getState();
    if (
      current.open !== hud.open ||
      current.armed !== hud.armed ||
      current.prompt !== hud.prompt ||
      current.countdown !== hud.countdown ||
      JSON.stringify(current.keys) !== JSON.stringify(hud.keys)
    )
      useFreeRideHud.setState(hud);
  }
}

/** Gamepad settings from storage, coerced: anything malformed falls back to the defaults. */
function storedPadSettings(): Pick<
  ControlSnapshot,
  "deadzone" | "invertLean" | "invertTurn" | "exponent"
> {
  const raw = readStored<unknown>(PAD_SETTINGS_KEY, null);
  const r = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const num = (v: unknown, lo: number, hi: number, fallback: number) =>
    typeof v === "number" && Number.isFinite(v) && v >= lo && v <= hi ? v : fallback;
  return {
    deadzone: num(r.deadzone, 0.05, 0.3, DEFAULT_PAD_OPTIONS.deadZone),
    invertLean: typeof r.invertLean === "boolean" ? r.invertLean : false,
    invertTurn: typeof r.invertTurn === "boolean" ? r.invertTurn : false,
    exponent: num(r.exponent, 1, 2.5, 1),
  };
}
