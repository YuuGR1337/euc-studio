// Controlled transport for the live pose and compare checks: stands in for @tauri-apps/api/core
// in src/engine/live.ts and src/engine/compareLive.ts only. `engine_connect` and `compare_start`
// hand their channels to the spec, which delivers messages and batches in the order Tauri does.
// Every other command answers nothing. No simulator, firmware or native audio runs.

type Deliver = (value: unknown) => void;

const wire = {
  stream: null as Deliver | null,
  messages: null as Deliver | null,
  /** The last comparison's channels: run A, run B and its messages. */
  compare: null as { a: Deliver; b: Deliver; messages: Deliver } | null,
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

const firmware = (id: string, label: string) => ({
  id,
  label,
  detail: "Fixture",
  disabled_reason: null,
});

export async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  wire.calls.push(command);
  const channel = (key: string) => (args[key] as Channel<unknown>).onmessage;
  switch (command) {
    case "engine_connect":
      wire.stream = channel("stream");
      wire.messages = channel("messages");
      return {
        backend: "fixture",
        audio: { backend: "unsupported", state: "muted", enabled: false, error: null },
        scenarios: [
          { id: "ride-20kmh-stop", label: "Ride to 20 km/h and stop", detail: "Fixture" },
        ],
        commands: [],
        firmwares: [{ id: "stock", label: "Stock", detail: "Fixture" }],
        perf_seconds: null,
      } as T;
    case "compare_info":
      return {
        firmwares: [
          firmware("stock", "Stock"),
          firmware("ours-compat", "Ours-compat"),
          firmware("ours-clean", "Ours-clean"),
        ],
        scenarios: [
          { id: "ride-20kmh-stop", label: "Ride to 20 km/h and stop", detail: "Fixture" },
        ],
        tolerance_file: "fixture.toml",
      } as T;
    case "compare_start":
      wire.compare = {
        a: channel("streamA"),
        b: channel("streamB"),
        messages: channel("messages"),
      };
      return undefined as T;
    default:
      return undefined as T;
  }
}
