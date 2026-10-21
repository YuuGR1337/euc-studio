// Manual riding in the Ride workspace. W/S or Up/Down lean forward and back, A/D or Left/Right
// turn, a gamepad's left stick does both; the first push takes the rider from the scenario. While
// riding by hand, Space steps off and Esc gives the rider back to the scenario (Space pauses the
// run otherwise). M toggles it from anywhere in Ride. The input goes to the engine at most
// 30 times a second, and only when it changed.

import { useEffect } from "react";
import { create } from "zustand";
import { engine } from "../../engine/index.ts";
import { onFrame } from "../../lib/frameloop.ts";
import {
  KeyRider,
  NEUTRAL,
  type RiderInput,
  combine,
  isNeutral,
  rideAction,
  sameInput,
  stickInput,
} from "../../lib/riderInput.ts";
import { useUi } from "../../state/ui.ts";

export type InputSource = "keyboard" | "gamepad";

interface ManualState {
  active: boolean;
  source: InputSource;
  steppedOff: boolean;
}

export const useManual = create<ManualState>(() => ({
  active: false,
  source: "keyboard",
  steppedOff: false,
}));

/** Text fields keep every key; focused controls keep the arrows (radios, tabs, sliders, menus). */
const TYPING = "input, textarea, select, [contenteditable=''], [contenteditable='true']";
const USES_ARROWS =
  "[role='radio'], [role='tab'], [role='slider'], [role='option'], [role='menuitem'], [role='menuitemradio'], [role='listbox'], [role='combobox']";

/** Seconds between two sends while the input keeps changing. */
const SEND_EVERY_MS = 33;

const keys = new KeyRider();
let sent: RiderInput | null = null;
let sentAt = 0;
/** The latest input, for the HUD. */
export const current: RiderInput = { ...NEUTRAL };

function take(source: InputSource) {
  const s = useManual.getState();
  if (!s.active || s.source !== source) useManual.setState({ active: true, source });
}

/** Gives the rider back to the scenario. */
export function giveBack() {
  keys.clear();
  keys.input = { ...NEUTRAL };
  Object.assign(current, NEUTRAL);
  sent = null;
  if (useManual.getState().active) {
    useManual.setState({ active: false });
    engine.setManual?.(null);
  }
}

/** M: ride by hand (standing upright until a key or the stick says more), or give it back. */
export function toggleManual() {
  if (
    engine.kind === "playback" ||
    engine.status.runMode === "free_ride" ||
    useUi.getState().recordingsOpen
  )
    return;
  if (useManual.getState().active) giveBack();
  else take("keyboard");
}

export function stepOff() {
  if (!useManual.getState().active) return;
  engine.stepOff?.();
  useManual.setState({ steppedOff: true });
}

const inRide = () => {
  const ui = useUi.getState();
  return (
    ui.route === "ride" &&
    !ui.paletteOpen &&
    !ui.shortcutsOpen &&
    !ui.recordingsOpen &&
    engine.kind !== "playback" &&
    engine.status.runMode !== "free_ride"
  );
};

function onKeyDown(e: KeyboardEvent) {
  if (e.defaultPrevented || e.isComposing || !inRide()) return;
  const action = rideAction(e);
  if (!action) return;
  const target = e.target instanceof Element ? e.target : null;
  if (target?.closest(TYPING)) return;
  const manual = useManual.getState().active;
  switch (action) {
    case "stepOff":
      if (!manual) return; // Space pauses the run when nobody rides by hand.
      e.preventDefault();
      if (!e.repeat) stepOff();
      return;
    case "release":
      if (!manual) return;
      e.preventDefault();
      giveBack();
      return;
    default:
      if (e.key.startsWith("Arrow") && target?.closest(USES_ARROWS)) return;
      e.preventDefault();
      keys.press(action);
      take("keyboard");
  }
}

function onKeyUp(e: KeyboardEvent) {
  // A key let go while ⌘ is down still lets go.
  const action = rideAction({ key: e.key, metaKey: false, ctrlKey: false, altKey: false });
  if (action && action !== "stepOff" && action !== "release") keys.release(action);
}

const onBlur = () => keys.clear();

function firstPad(): Gamepad | null {
  const pads = typeof navigator !== "undefined" && navigator.getGamepads?.();
  if (!pads) return null;
  for (const p of pads) if (p?.connected) return p;
  return null;
}

let lastT = 0;

function tick(now: number, dt: number) {
  // A new run (time went back) brings the rider back on the pedals.
  const t = engine.latest.t;
  if (t < lastT - 0.5 && useManual.getState().steppedOff) useManual.setState({ steppedOff: false });
  lastT = t;
  if (!inRide()) {
    giveBack();
    return;
  }
  const pad = firstPad();
  const stick = pad ? stickInput(pad.axes[0] ?? 0, pad.axes[1] ?? 0) : NEUTRAL;
  if (!isNeutral(stick)) take("gamepad");
  if (!useManual.getState().active) return;
  const input = combine(keys.step(dt), stick);
  Object.assign(current, input);
  if ((!sent || !sameInput(sent, input)) && now - sentAt >= SEND_EVERY_MS) {
    engine.setManual?.(input);
    sent = { ...input };
    sentAt = now;
  }
}

let installed = 0;

/** Listens for riding keys and the gamepad while `active` (the Ride workspace is shown). */
export function installManualRiding(): () => void {
  if (installed++ > 0) return () => void installed--;
  // Capture: riding keys win over the global shortcuts (Space) only while riding by hand.
  window.addEventListener("keydown", onKeyDown, true);
  window.addEventListener("keyup", onKeyUp, true);
  window.addEventListener("blur", onBlur);
  const off = onFrame(tick);
  return () => {
    if (--installed > 0) return;
    giveBack();
    window.removeEventListener("keydown", onKeyDown, true);
    window.removeEventListener("keyup", onKeyUp, true);
    window.removeEventListener("blur", onBlur);
    off();
  };
}

/** Riding keys and the gamepad, while the Ride workspace is shown (`active`). */
export function useManualRiding(active: boolean) {
  useEffect(() => (active ? installManualRiding() : undefined), [active]);
}
