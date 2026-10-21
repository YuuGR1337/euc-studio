import { useSyncExternalStore } from "react";
import { DemoCompare } from "../sim/syntheticCompare.ts";
import { LiveCompare } from "./compareLive.ts";
import type { CompareClient } from "./compareTypes.ts";

// The Compare workspace's engine, chosen like the Ride engine: the real one inside Tauri, the
// demo in a browser and in tests (`?engine=demo` forces the demo in the app too).

const params = new URLSearchParams(typeof location !== "undefined" ? location.search : "");
const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function create(): CompareClient {
  if (inTauri && params.get("engine") !== "demo") return new LiveCompare();
  const demo = new DemoCompare();
  // `?compare=<scenario>[,<seconds>]` (tests, screenshots): stock against ours-clean, prerolled.
  const preset = params.get("compare");
  if (preset) {
    const [scenario = "ride-20", seconds = "0"] = preset.split(",");
    demo.start({ a: { kind: "stock" }, b: { kind: "ours-clean" }, scenario, fast: false });
    demo.preroll(Math.min(Number(seconds) || 0, 60));
  }
  return demo;
}

/** The engine the Compare workspace talks to. */
export const compare: CompareClient = create();

/** Re-renders on coarse comparison changes (status, a new summary, a reset). */
export function useCompareVersion(): number {
  return useSyncExternalStore(compare.subscribe, compare.getVersion, compare.getVersion);
}

export * from "./compareTypes.ts";
