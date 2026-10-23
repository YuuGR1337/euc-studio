// What the 3D view shows about Free Ride (FreeRideOverlay): one prompt, the boot countdown, the
// respawn buttons and the key reminder. Only the controller writes it.

import { create } from "zustand";

export interface FreeRideHud {
  /** Free Ride controls are connected (the panel is open on the Ride route). */
  open: boolean;
  /** The rider rides now: ready, on the pedals, powered and playing. */
  riding: boolean;
  /** One line telling the rider what to do next, or null. */
  prompt: string | null;
  /** Simulated seconds until the firmware has booted, while it boots; else null. */
  boot: number | null;
  /** The reminder row: what each key does, in the current device's words. */
  keys: { label: string; caps: string[] }[] | null;
  /** The respawn buttons and their keys, when a run can respawn; else null. */
  respawn: { here: string; start: string } | null;
  /** Respawns at the last clear spot, or at the world's start. */
  onRespawn?: (toStart: boolean) => void;
}

export const CLOSED: FreeRideHud = {
  open: false,
  riding: false,
  prompt: null,
  boot: null,
  keys: null,
  respawn: null,
};

export const useFreeRideHud = create<FreeRideHud>(() => CLOSED);
