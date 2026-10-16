import { useEffect, useEffectEvent, useState, useSyncExternalStore } from "react";
import { onFrame, onFrameEarly } from "../lib/frameloop.ts";
import { DemoEngine, type Latest } from "./synthetic.ts";

/**
 * The engine the studio talks to. Today it is the synthetic demo; later an `EngineClient`
 * backed by Tauri IPC (and a WASM build) will expose the same rings and `latest` view.
 */
export const engine = new DemoEngine(1);

const params = new URLSearchParams(typeof location !== "undefined" ? location.search : "");
const preroll = Number(params.get("preroll"));
if (preroll > 0) engine.preroll(Math.min(preroll, 25));
if (params.get("paused") === "1") engine.setPlaying(false);

onFrameEarly((now, dt) => {
  engine.advance(dt, now / 1000);
});

/** Re-renders on coarse engine changes only (play state, firmware, scenario, new events). */
export function useEngineVersion(): number {
  return useSyncExternalStore(engine.subscribe, engine.getVersion, engine.getVersion);
}

/**
 * Reads a live value at a bounded rate (default 8 Hz, the rolling-digit limit) and only
 * re-renders when the rounded value changes.
 */
export function useLive(read: (l: Latest) => number, hz = 8, decimals = 1): number {
  const quantize = (v: number) => Math.round(v * 10 ** decimals) / 10 ** decimals;
  const [value, setValue] = useState(() => quantize(read(engine.latest)));
  const sample = useEffectEvent(() => {
    const q = quantize(read(engine.latest));
    setValue((prev) => (prev === q ? prev : q));
  });
  useEffect(() => {
    let lastEmit = 0;
    return onFrame((now) => {
      if (now - lastEmit < 1000 / hz) return;
      lastEmit = now;
      sample();
    });
  }, [hz]);
  return value;
}
