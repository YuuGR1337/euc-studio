import { useEffect, useState } from "react";
import { engine, type EngineClient } from "../../engine/index.ts";
import { useUi } from "../../state/ui.ts";

/**
 * The gain native output applies for a slider position (0 to 1): squared, so the slider's middle
 * sounds like the middle of its range rather than nearly full.
 */
export const volumeGain = (volume: number) => Math.min(1, Math.max(0, volume)) ** 2;

/**
 * Keeps native output's mix on the user's volume and mute, and the ride sound (motor, tyre,
 * wind) off while the window is hidden: the run goes on, the buzzer still sounds (its alarms
 * matter), the ride sound fades out over 0.3 s and back in when the window shows again.
 */
export function useSoundMix(source: EngineClient = engine) {
  const volume = useUi((s) => s.soundVolume);
  const muted = useUi((s) => s.soundMuted);
  const [hidden, setHidden] = useState(() => typeof document !== "undefined" && document.hidden);
  useEffect(() => {
    const update = () => setHidden(document.hidden);
    document.addEventListener("visibilitychange", update);
    return () => document.removeEventListener("visibilitychange", update);
  }, []);
  useEffect(() => sendMix(source, volume, muted, hidden), [source, volume, muted, hidden]);
}

/** Sends one mix to a live engine's native output; a failure is logged, never thrown. */
export function sendMix(source: EngineClient, volume: number, muted: boolean, hidden: boolean) {
  if (source.kind !== "live" || !source.setSoundMix) return;
  source.setSoundMix(muted ? 0 : volumeGain(volume), !hidden).catch((error: unknown) => {
    console.warn(`[sound] ${String(error)}`);
  });
}
