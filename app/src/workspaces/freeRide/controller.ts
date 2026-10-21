import {
  freeRideApi,
  FREE_RIDE_LIMITS,
  type FreeRideInput,
  type FreeRideSource,
  type FreeRideStatus,
  type GeneratedMapSpec,
  type HeightMap,
  type RiderModel,
} from "../../engine/freeRide.ts";
import type { EngineClient } from "../../engine/types.ts";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";

interface Pad {
  index: number;
  id: string;
  standard: boolean;
}
export interface ControlSnapshot {
  status: FreeRideStatus | null;
  map: HeightMap | null;
  applied: GeneratedMapSpec | null;
  riderModel: RiderModel;
  source: FreeRideSource;
  padIndex: number | null;
  pads: Pad[];
  gamepadSupported: boolean;
  deadzone: number;
  invertLean: boolean;
  invertTurn: boolean;
  lean: number;
  turn: number;
  armed: boolean;
  arming: boolean;
  starting: boolean;
  neutralMs: number;
  inputAgeMs: number | null;
  playing: boolean;
  reason: string;
  error: string | null;
}
const motionKeys = new Set([
  "KeyW",
  "KeyS",
  "KeyA",
  "KeyD",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
]);
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
const rad = Math.PI / 180;
const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
// Keep the sequence across drawer unmounts, as well as source changes. Native epochs increase.
let sequenceEpoch: number | null = null;
let sequence = 0;
let lastApplied: { epoch: number; map: GeneratedMapSpec } | null = null;
function textFocus(target: EventTarget | null) {
  if (
    target instanceof Element &&
    target.closest(
      '[data-free-ride-slider] [role="slider"], [data-free-ride-slider] input[type="range"]',
    )
  )
    return false;
  return (
    target instanceof Element &&
    !!target.closest(
      'textarea, select, [contenteditable]:not([contenteditable="false"]), [role="combobox"], [role="tab"], [role="switch"], [role="slider"], [cmdk-root], input:not([type="button"]):not([type="checkbox"]):not([type="radio"])',
    )
  );
}
function wheelControl(target: EventTarget | null) {
  return (
    target instanceof Element && !!target.closest("[data-wheel-control], [data-wheel-controls]")
  );
}

/** Owns wall-clock input and lifecycle. No simulation state or terrain is synthesized here. */
export class FreeRideController {
  private state: ControlSnapshot = {
    status: null,
    map: null,
    applied: null,
    riderModel: "rigid_ankle_v1",
    source: "keyboard_slider",
    padIndex: null,
    pads: [],
    gamepadSupported:
      typeof navigator !== "undefined" && typeof navigator.getGamepads === "function",
    deadzone: 0.15,
    invertLean: false,
    invertTurn: false,
    lean: 0,
    turn: 0,
    armed: false,
    arming: false,
    starting: false,
    neutralMs: 0,
    inputAgeMs: null,
    playing: false,
    reason: "Select a map and start free ride.",
    error: null,
  };
  private listeners = new Set<() => void>();
  private active = false;
  private root: HTMLElement | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;
  private unsubscribe: (() => void) | null = null;
  private generation = 0;
  private lifecycle = 0;
  private keys = new Set<string>();
  private heldKeys = new Set<string>();
  private blockedKeys = false;
  private sliderHeld = false;
  private sliderKey: string | null = null;
  private sliderLean = 0;
  private sliderTurn = 0;
  private wheelNeutralRequired = false;
  private wheelNeutralSince: number | null = null;
  private padAxisActive = [false, false];
  private selectedPadId: string | null = null;
  private padWasConnected = false;
  private neutralSince: number | null = null;
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
    map: GeneratedMapSpec;
    riderModel: RiderModel;
    epoch: number;
    at: number;
    accepted: boolean;
    cancelled: boolean;
  } | null = null;
  private mapEpoch: number | null = null;
  private riderModelEdited = false;

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
        this.pause("Run paused. Select Arm to continue.");
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
  }
  private clearMotion() {
    this.keys.clear();
    this.blockedKeys = this.heldKeys.size > 0;
    this.sliderHeld = false;
    this.sliderKey = null;
    this.sliderLean = 0;
    this.sliderTurn = 0;
    this.wheelNeutralRequired = false;
    this.wheelNeutralSince = null;
    this.padAxisActive = [false, false];
    this.sawPlaying = false;
    this.neutralSince = null;
    this.neutralAckSince = null;
    this.pendingInput = null;
  }
  private packet(armed: boolean, lean = 0, turn = 0): FreeRideInput | null {
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
    return {
      run_epoch: status.epoch,
      seq: ++sequence,
      source: this.state.source,
      desired_lean_rad: lean * rad,
      heading_rate_rad_s: turn * rad,
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
          await freeRideApi.input(next);
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
  pause = (reason = "Controls paused. Select Arm to continue.") => {
    // start() pauses before it creates its pending request. Later pauses cancel that request.
    if (this.pendingStart) this.pendingStart.cancelled = true;
    this.generation++;
    this.clearMotion();
    this.update({ armed: false, arming: false, lean: 0, turn: 0, neutralMs: 0, reason });
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
    this.pause("Input source changed. Keep controls neutral, then select Arm.");
    this.update({ source });
    // seq stays monotonic across source changes. The next neutral packet selects the native source.
  };
  selectPad = (index: number | null) => {
    this.pause("Gamepad selection changed. Keep the stick neutral, then select Arm.");
    this.selectedPadId = this.state.pads.find((pad) => pad.index === index)?.id ?? null;
    this.padWasConnected = false;
    this.update({ padIndex: index });
  };
  configure = (patch: Partial<Pick<ControlSnapshot, "deadzone" | "invertLean" | "invertTurn">>) => {
    if (this.state.armed || this.state.arming || this.engine.playing) return;
    if (
      patch.deadzone !== undefined &&
      (!Number.isFinite(patch.deadzone) || patch.deadzone < 0.05 || patch.deadzone > 0.3)
    )
      return;
    this.pause("Input settings changed. Keep controls neutral, then select Arm.");
    this.update(patch);
  };
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
    if (axis === "lean") this.sliderLean = Math.max(-10, Math.min(10, value));
    else this.sliderTurn = Math.max(-30, Math.min(30, value));
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
    if (axis === "lean" || axis === "turn")
      this.sliderValue(axis, sign * (axis === "lean" ? 10 : 30));
  };
  sliderKeyUp = (event: ReactKeyboardEvent) => {
    if (sliderKeys.has(event.code)) this.sliderRelease();
  };
  private windowBlur = () => this.pause("Window lost focus. Controls are paused.");
  private padDisconnected = (event: GamepadEvent) => {
    if (event.gamepad.index !== this.state.padIndex) return;
    this.pause("Gamepad disconnected. Select the device again.");
    this.selectedPadId = null;
    this.padWasConnected = false;
    this.update({ padIndex: null });
  };
  private visibility = () => {
    if (document.hidden) this.pause("Window is hidden. Controls are paused.");
  };
  private focusIn = (event: FocusEvent) => {
    if (wheelControl(event.target)) {
      if (this.state.arming) this.pause("Wheel controls selected during Arm. Controls are paused.");
      const axes = this.padAxisActive;
      this.clearMotion();
      this.padAxisActive = axes;
      this.wheelNeutralRequired = true;
      this.sawPlaying = this.engine.playing;
      this.update({
        lean: 0,
        turn: 0,
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
    if (textFocus(event.target)) this.pause("Text controls have focus. Controls are paused.");
    else if ((this.state.armed || this.state.arming) && !this.ownsFocus(event.target))
      this.pause("Focus left the control surface. Controls are paused.");
    else if (
      this.sliderHeld &&
      !(event.target instanceof Element && event.target.closest("[data-free-ride-slider]"))
    ) {
      this.pause("Slider lost focus. Controls are paused.");
    }
  };
  private ownsFocus(target: EventTarget | null) {
    return wheelControl(target) || (target instanceof Node && !!this.root?.contains(target));
  }
  private keyDown = (event: KeyboardEvent) => {
    if (!this.active) return;
    if (motionKeys.has(event.code)) {
      this.heldKeys.add(event.code);
      if (this.state.source === "keyboard_slider") {
        this.neutralSince = null;
        this.update({ neutralMs: 0 });
      }
    }
    if (event.code === "Escape") {
      this.pause("Keyboard pause. Select Arm to continue.");
      return; // Radix dialogs and menus must still receive Escape after the pause.
    }
    if (
      event.code === "Space" &&
      this.ownsFocus(event.target) &&
      !wheelControl(event.target) &&
      !textFocus(event.target)
    ) {
      event.preventDefault();
      event.stopImmediatePropagation();
      this.pause("Keyboard pause. Select Arm to continue.");
      return;
    }
    if (
      event.metaKey ||
      event.ctrlKey ||
      event.altKey ||
      wheelControl(event.target) ||
      textFocus(event.target) ||
      !this.ownsFocus(event.target)
    )
      return;
    if (motionKeys.has(event.code)) {
      // Dedicated ride sliders own keyboard access while their control has focus.
      if (event.target instanceof Element && event.target.closest("[data-free-ride-slider]"))
        return;
      event.preventDefault();
      event.stopImmediatePropagation();
      if (this.state.source === "keyboard_slider" && !this.sliderHeld && !this.blockedKeys)
        this.keys.add(event.code);
    }
  };
  private keyUp = (event: KeyboardEvent) => {
    this.heldKeys.delete(event.code);
    this.keys.delete(event.code);
    if (!this.heldKeys.size) this.blockedKeys = false;
  };
  private wheelInput(lean: number, turn: number): [number, number, boolean] {
    if (wheelControl(document.activeElement)) {
      this.wheelNeutralSince = null;
      return [0, 0, this.state.armed];
    }
    if (!this.wheelNeutralRequired) return [lean, turn, true];
    if (
      lean !== 0 ||
      turn !== 0 ||
      (this.state.source === "keyboard_slider" && this.heldKeys.size > 0)
    ) {
      this.wheelNeutralSince = null;
      return [0, 0, this.state.armed];
    }
    this.wheelNeutralSince ??= performance.now();
    if (performance.now() - this.wheelNeutralSince >= 250) {
      this.wheelNeutralRequired = false;
      this.wheelNeutralSince = null;
    }
    return [0, 0, this.state.armed || !this.wheelNeutralRequired];
  }
  private read(): [number, number, boolean] {
    // Choice controls pause on focus, but a closed choice can still satisfy neutral dwell.
    // Its keyboard events stay with Radix; the separate Arm button starts motion explicitly.
    const choiceFocused =
      document.activeElement instanceof Element &&
      this.ownsFocus(document.activeElement) &&
      !!document.activeElement.closest('[data-free-ride-choice] [role="combobox"], [role="tab"]');
    if (
      !document.hasFocus() ||
      document.hidden ||
      (textFocus(document.activeElement) && !choiceFocused)
    )
      return [0, 0, false];
    if (this.state.source === "gamepad") {
      let pads: (Gamepad | null)[] = [];
      try {
        pads = this.state.gamepadSupported ? navigator.getGamepads() : [];
      } catch (error) {
        this.update({ error: message(error), gamepadSupported: false });
      }
      const pad = this.state.padIndex === null ? null : pads[this.state.padIndex];
      const valid =
        !!pad &&
        pad.connected &&
        pad.mapping === "standard" &&
        pad.id === this.selectedPadId &&
        pad.axes.length >= 2;
      if (!valid) {
        if (this.padWasConnected) {
          this.pause("Gamepad disconnected or changed. Select the device again.");
          this.selectedPadId = null;
          this.update({ padIndex: null });
        }
        this.padWasConnected = false;
        return [0, 0, false];
      }
      this.padWasConnected = true;
      const axis = (value: number, index: number) => {
        if (!Number.isFinite(value)) return NaN;
        const absolute = Math.min(1, Math.abs(value));
        const enter = this.state.deadzone;
        const exit = Math.max(0, enter - 0.03);
        this.padAxisActive[index] = this.padAxisActive[index] ? absolute > exit : absolute > enter;
        if (!this.padAxisActive[index]) return 0;
        return Math.sign(value) * Math.max(0, (absolute - exit) / (1 - exit));
      };
      // Positive native yaw turns towards world +y (left when heading along +x).
      const turn = axis(pad.axes[0] ?? NaN, 0) * 30 * (this.state.invertTurn ? 1 : -1);
      const lean = axis(pad.axes[1] ?? NaN, 1) * 10 * (this.state.invertLean ? 1 : -1);
      if (!Number.isFinite(lean) || !Number.isFinite(turn)) {
        this.pause("Gamepad input is invalid.");
        return [0, 0, false];
      }
      return this.wheelInput(lean, turn);
    }
    if (this.sliderHeld) return this.wheelInput(this.sliderLean, this.sliderTurn);
    // A pause clears all keys. A full release is required before new keyboard motion.
    if (this.blockedKeys || (!this.state.armed && this.heldKeys.size))
      return this.state.armed ? this.wheelInput(0, 0) : [0, 0, false];
    const down = (...codes: string[]) => (codes.some((code) => this.keys.has(code)) ? 1 : 0);
    const lean = (down("KeyW", "ArrowUp") - down("KeyS", "ArrowDown")) * 10;
    const turn = (down("KeyA", "ArrowLeft") - down("KeyD", "ArrowRight")) * 30;
    return this.wheelInput(lean, turn);
  }
  arm = async () => {
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
      this.pauses ||
      this.state.neutralMs < 250 ||
      status.source !== this.state.source ||
      !document.hasFocus() ||
      document.hidden ||
      textFocus(document.activeElement)
    )
      return;
    if (
      performance.now() - this.lastTick >= 250 ||
      this.lastAck === null ||
      performance.now() - this.lastAck >= 250
    ) {
      this.pause("The input clock is stale. Keep controls neutral, then select Arm.");
      return;
    }
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
      const [lean, turn, usable] = this.read();
      if (!this.active || generation !== this.generation || lean !== 0 || turn !== 0 || !usable) {
        this.pause("Controls changed during Arm. Keep controls neutral, then select Arm.");
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
        const [currentLean, currentTurn, stillUsable] = this.read();
        if (
          epoch !== this.state.status?.epoch ||
          this.state.source !== source ||
          currentLean !== 0 ||
          currentTurn !== 0 ||
          !stillUsable
        ) {
          throw new Error("Controls changed during Arm. Keep controls neutral, then select Arm.");
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
      const input = this.packet(true);
      if (input) this.send(input); // First confirmed armed packet is always neutral.
      this.root?.focus({ preventScroll: true });
    } catch (error) {
      if (generation === this.generation) {
        this.update({ error: message(error) });
        this.pause("Arm failed. Controls are paused.");
      }
    } finally {
      clearTimeout(timeout);
    }
  };
  start = async (map: GeneratedMapSpec) => {
    if (!this.active || this.engine.kind !== "live" || this.state.starting || !this.state.status)
      return;
    this.pause("Starting a new free ride.");
    const lifecycle = this.lifecycle;
    const request = {
      map: structuredClone(map),
      riderModel: this.state.riderModel,
      epoch: this.state.status.epoch,
      at: performance.now(),
      accepted: false,
      cancelled: false,
    };
    this.pendingStart = request;
    this.update({ starting: true, error: null });
    try {
      await freeRideApi.start(request.map, request.riderModel);
      request.accepted = true;
      if (request.cancelled || !this.active || lifecycle !== this.lifecycle) {
        request.cancelled = true;
        // This pause queues behind the accepted start, including a slow firmware build.
        this.engine.setPlaying(false);
        // start acknowledges a queue operation, not a run epoch. Observe and pause its late run.
        void this.pauseLateStart(request);
        return;
      }
      this.nextPoll = 0;
    } catch (error) {
      if (this.active && this.pendingStart === request) {
        this.pendingStart = null;
        this.update({ starting: false, error: message(error), reason: "Map was not applied." });
      }
    }
  };
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
          map: null,
          applied: lastApplied?.epoch === status.epoch ? lastApplied.map : null,
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
        lastApplied = { epoch: status.epoch, map: request.map };
        this.riderModelEdited = false;
        this.engine.setTargetRtf(Math.max(0.25, Math.min(1, this.engine.targetRtf)));
        this.update({ applied: request.map, starting: false, reason: status.reason });
        if (request.cancelled)
          this.pause("The queued run arrived after controls closed. Select Arm when ready.");
      } else if (request && !request.cancelled && performance.now() - request.at > 20_000) {
        request.cancelled = true;
        this.engine.setPlaying(false);
        this.update({
          error: "No new free ride run arrived within 20 s. Check the engine log.",
        });
      }
      if (status.mode === "free_ride" && this.mapEpoch !== status.epoch) {
        const map = await freeRideApi.map(status.epoch);
        if (
          !this.active ||
          lifecycle !== this.lifecycle ||
          this.state.status?.epoch !== status.epoch
        )
          return;
        this.mapEpoch = status.epoch;
        this.update({ map });
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
  private tick = () => {
    if (!this.active) return;
    const now = performance.now();
    if (now - this.lastTick >= 250) {
      this.neutralSince = null;
      this.neutralAckSince = null;
    }
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
    // Device inventory is bounded by the browser's connected slots. No automatic selection.
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
    const [lean, turn, usable] = this.read();
    if (this.state.armed && !usable)
      this.pause("Input focus or device is unavailable. Controls are paused.");
    if (usable && lean === 0 && turn === 0 && !this.pauses) this.neutralSince ??= now;
    else this.neutralSince = null;
    const neutralMs =
      this.neutralSince !== null && this.neutralAckSince !== null
        ? Math.max(0, now - Math.max(this.neutralSince, this.neutralAckSince))
        : 0;
    if (
      !this.state.arming &&
      !this.state.starting &&
      !this.pauses &&
      this.state.status?.mode === "free_ride" &&
      this.state.status.ready
    ) {
      const input = this.packet(
        this.state.armed,
        this.state.armed ? lean : 0,
        this.state.armed ? turn : 0,
      );
      if (input) this.send(input);
    }
    if (now - this.lastPublish >= 125) {
      this.lastPublish = now;
      this.update({
        pads,
        lean,
        turn,
        neutralMs,
        inputAgeMs: this.lastAck === null ? null : now - this.lastAck,
      });
    }
  };
}
