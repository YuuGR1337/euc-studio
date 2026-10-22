import { useSyncExternalStore } from "react";
import type { EngineClient, EngineState } from "../../src/engine/types.ts";
import type { TuneCatalog, TuneSnapshot } from "../../src/engine/tune.ts";

// This source supplies controlled transport replies. It does not run firmware or a simulator.
let state: EngineState = "running";
let t = 0;
let signal = 0;
let auxiliary = 50;
let revision = 0;
let reads = 0;
let version = 0;
let session = 42;
let defer: "catalog" | "snapshot" | null = null;
type Deferred = {
  kind: "catalog" | "snapshot";
  session: number;
  t: number;
  resolve: () => void;
  reject: () => void;
};
const pending: Deferred[] = [];
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};
export const engine: Pick<EngineClient, "kind" | "status" | "scenarioId"> = {
  kind: "live",
  scenarioId: "controlled-probe-fixture",
  get status() {
    return {
      state,
      backend: "controlled-probe-fixture",
      message: "No native firmware runs here.",
      runEpoch: session,
    };
  },
};
export const useEngineVersion = () => useSyncExternalStore(subscribe, () => version);
function run(): TuneSnapshot["run"] {
  return {
    session_id: session,
    backend: "controlled-probe-fixture",
    scenario: "controlled-probe-fixture",
    firmware: null,
    build: null,
    state,
    source: "Explicit browser fixture snapshots",
  };
}
function reply<T>(kind: "catalog" | "snapshot", value: TuneCatalog | TuneSnapshot): Promise<T> {
  const captured = structuredClone(value);
  if (defer !== kind) return Promise.resolve(captured as T);
  defer = null;
  return new Promise<T>((resolve, reject) =>
    pending.push({
      kind,
      session: captured.run.session_id,
      t: "t_s" in captured ? captured.t_s : t,
      resolve: () => resolve(captured as T),
      reject: () => reject(new Error("Fixture delayed probe failure.")),
    }),
  );
}
function emit() {
  revision++;
  version++;
  for (const listener of listeners) listener();
}
export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (command === "engine_tune_catalog") {
    const catalog: TuneCatalog = {
      run: run(),
      available: true,
      reason: null,
      total: 2,
      truncated: false,
      variables: ["fixture_signal", "fixture_aux"].map((name) => ({
        name,
        unit: null,
        unit_reference: null,
      })),
    };
    return reply<T>("catalog", catalog);
  }
  if (command !== "engine_tune_snapshot")
    throw new Error(`Unsupported fixture command: ${command}`);
  if (args?.sessionId !== session) throw new Error("Fixture session mismatch");
  const names = args.names;
  if (!Array.isArray(names)) throw new Error("Fixture probe names missing");
  const snapshot: TuneSnapshot = {
    run: run(),
    t_s: t,
    values: names.map((name) => ({
      name: String(name),
      value: name === "fixture_signal" ? signal : name === "fixture_aux" ? auxiliary : null,
      reason: null,
    })),
    handler_counters: null,
    handler_reason: "Handler counters are not part of this fixture.",
  };
  reads++;
  return reply<T>("snapshot", snapshot);
}
export const tuneProbeFixture = {
  get reads() {
    return reads;
  },
  get revision() {
    return revision;
  },
  get pending() {
    return pending.map(({ kind, session, t }) => ({ kind, session, t }));
  },
  deferNext(kind: "catalog" | "snapshot") {
    defer = kind;
  },
  resolveNext() {
    const request = pending.shift();
    if (!request) throw new Error("No deferred fixture reply");
    request.resolve();
  },
  rejectNext() {
    const request = pending.shift();
    if (!request) throw new Error("No deferred fixture reply");
    request.reject();
  },
  reset(nextSession: number, nextState: EngineState = "running") {
    session = nextSession;
    state = nextState;
    t = 0;
    signal = 0;
    auxiliary = 0;
    emit();
  },
  quickReset(nextSession: number) {
    const beforeReads = reads;
    state = "starting";
    emit();
    session = nextSession;
    state = "completed";
    t = 0;
    signal = 0;
    auxiliary = 0;
    emit();
    return { beforeReads, afterReads: reads, epoch: session, state };
  },
  push(time: number, value: number, nextState: EngineState = "running", aux = value * 2) {
    t = time;
    signal = value;
    auxiliary = aux;
    state = nextState;
    emit();
  },
};
declare global {
  interface Window {
    tuneProbeFixture: typeof tuneProbeFixture;
  }
}
window.tuneProbeFixture = tuneProbeFixture;
