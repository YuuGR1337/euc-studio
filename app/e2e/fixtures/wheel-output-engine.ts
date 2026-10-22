import { useSyncExternalStore } from "react";
import { DemoEngine } from "../../src/sim/synthetic.ts";
import type {
  EngineClient,
  DisplayLayout,
  LightsLayout,
  WheelButton,
} from "../../src/engine/types.ts";
import type { Outputs } from "../../src/engine/telemetry.gen.ts";
import layouts from "./wheel-output-layouts.json";

// Geometry uses the native profile adapters. All state below is a layout fixture, not a boot trace.
const base: EngineClient = new DemoEngine(1);
let version = 0;
const listeners = new Set<() => void>();
const calls: { button: WheelButton; pressed: boolean; epoch?: number }[] = [];
let releaseFailures = 0;
const emit = () => {
  version++;
  for (const listener of listeners) listener();
};
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};
const status: EngineClient["status"] = {
  state: "running",
  runEpoch: 1,
  buttonsReady: true,
  backend: "layout-fixture",
  message: "Layout fixture only",
};
const outputs: Outputs = {
  display_grid: Array<number>(16).fill(255),
  display_on: true,
  display_brightness: 7,
  leds: Array.from({ length: layouts.lights.slots }, (_, i): [number, number, number] =>
    i < 24 ? [64, 0, 0] : [0, 0, 0],
  ),
  buzzer_hz: 0,
  headlight_duty: 0.6,
  headlight_source: "Layout fixture",
  headlight_pin_high: true,
  power_latched: true,
  controller_powered: true,
};
const fields: Partial<EngineClient> = {
  kind: "live",
  playing: true,
  status,
  outputs,
  display: layouts.display as DisplayLayout,
  lights: layouts.lights as LightsLayout,
  audio: { backend: "native", state: "ready", enabled: true, error: null },
  subscribe,
  subscribeData: subscribe,
  getVersion: () => version,
  setButton: async (button, pressed, epoch) => {
    calls.push({ button, pressed, epoch });
    if (!pressed && releaseFailures > 0) {
      releaseFailures--;
      throw new Error("Fixture release rejected.");
    }
    return "Fixture input only";
  },
  setSound: async (enabled) => {
    update({
      audio: { backend: "native", state: enabled ? "ready" : "muted", enabled, error: null },
    });
  },
  subscribeTones: () => () => {},
  advance: () => 0,
};
export const engine: EngineClient = new Proxy(base, {
  get(target, key) {
    if (Object.prototype.hasOwnProperty.call(fields, key)) return Reflect.get(fields, key);
    const value: unknown = Reflect.get(target, key);
    return typeof value === "function" ? value.bind(target) : value;
  },
});
function update(patch: Partial<EngineClient>) {
  Object.assign(fields, patch);
  emit();
}
export const useEngineData = () => useSyncExternalStore(subscribe, () => version);
export const useEngineVersion = useEngineData;
export const wheelOutputFixture = {
  calls,
  failNextReleases(count: number) {
    releaseFailures = count;
  },
  setReady(ready: boolean) {
    update({ status: { ...engine.status, buttonsReady: ready, powerButtonReady: ready } });
  },
  setDuty(duty: number | null) {
    update({ outputs: { ...outputs, headlight_duty: duty } });
  },
  powerOff() {
    update({
      status: { ...engine.status, buttonsReady: false, powerButtonReady: true },
      outputs: {
        ...outputs,
        display_on: false,
        controller_powered: false,
        leds: Array.from({ length: layouts.lights.slots }, (): [number, number, number] => [
          0, 0, 0,
        ]),
        headlight_duty: 0,
        headlight_pin_high: false,
        power_latched: false,
        buzzer_hz: 0,
      },
    });
  },
};
declare global {
  interface Window {
    wheelOutputFixture: typeof wheelOutputFixture;
  }
}
window.wheelOutputFixture = wheelOutputFixture;
