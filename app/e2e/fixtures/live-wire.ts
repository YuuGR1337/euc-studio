// Controlled transport for the live pose check: stands in for @tauri-apps/api/core in
// src/engine/live.ts only. `engine_connect` hands the two channels to the spec, which delivers
// messages and batches in the order Tauri does. Every other command answers nothing. No
// simulator, firmware or native audio runs.

type Deliver = (value: unknown) => void;

const wire = {
  stream: null as Deliver | null,
  messages: null as Deliver | null,
  calls: [] as string[],
};
window.liveWire = wire;
declare global {
  interface Window {
    liveWire: typeof wire;
  }
}

export class Channel<T> {
  readonly onmessage: (value: T) => void;

  constructor(onmessage: (value: T) => void) {
    this.onmessage = onmessage;
  }
}

export async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  wire.calls.push(command);
  if (command !== "engine_connect") return undefined as T;
  wire.stream = (args.stream as Channel<unknown>).onmessage;
  wire.messages = (args.messages as Channel<unknown>).onmessage;
  return {
    backend: "fixture",
    audio: { backend: "unsupported", state: "muted", enabled: false, error: null },
    scenarios: [{ id: "ride-20kmh-stop", label: "Ride to 20 km/h and stop", detail: "Fixture" }],
    commands: [],
    firmwares: [{ id: "stock", label: "Stock", detail: "Fixture" }],
    perf_seconds: null,
  } as T;
}
