// What the 3D view shows about Free Ride (FreeRideOverlay): one prompt, the restart countdown
// and the key reminder. Only the controller writes it.

import { create } from "zustand";

export interface FreeRideHud {
  /** Free Ride controls are connected (the panel is open on the Ride route). */
  open: boolean;
  /** Riding by hand now. */
  armed: boolean;
  /** One line telling the rider what to do next, or null. */
  prompt: string | null;
  /** The restart countdown: when it ends (performance.now(), ms) and whether it restarts here. */
  countdown: { endsAt: number; here: boolean } | null;
  /** The reminder row: what each key does, in the current device's words. */
  keys: { label: string; caps: string[] }[] | null;
}

export const CLOSED: FreeRideHud = {
  open: false,
  armed: false,
  prompt: null,
  countdown: null,
  keys: null,
};

export const useFreeRideHud = create<FreeRideHud>(() => CLOSED);
