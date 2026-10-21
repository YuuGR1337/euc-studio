import { invoke } from "@tauri-apps/api/core";
import { engine } from "./index.ts";

export const TUNE_MAX_SELECTED = 4;
export const TUNE_PAGE_SIZE = 48;
export const TUNE_HISTORY_SAMPLES = 120;
export const TUNE_HISTORY_SECONDS = 30;
export const TUNE_POLL_MS = 250;

export interface TuneRun {
  session_id: number;
  backend: string;
  scenario: string;
  firmware: string | null;
  build: string | null;
  state: string;
  source: string;
}

export interface TuneVariable {
  name: string;
  unit: string | null;
  unit_reference: string | null;
}

export interface TuneCatalog {
  run: TuneRun;
  available: boolean;
  reason: string | null;
  variables: TuneVariable[];
  total: number;
  truncated: boolean;
}

export interface TuneReading {
  name: string;
  value: number | null;
  reason: string | null;
}

export interface TuneSnapshot {
  run: TuneRun;
  t_s: number;
  values: TuneReading[];
  handler_counters?: HandlerCounters | null;
  handler_reason?: string | null;
}

export interface HandlerCounters {
  cycle: string;
  isr_cycles: string;
  rows: { exception: number; name: string; occupied_cycles: string }[];
  total_handlers: number;
  truncated: boolean;
}

// One outstanding IPC call per web view. The server also has a one-slot queue and a deadline.
let pending = false;

async function request<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (engine.kind !== "live")
    throw new Error("Firmware probes are unavailable in demo and playback.");
  if (pending) throw new Error("A probe request is pending.");
  pending = true;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      invoke<T>(command, args),
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("The probe request timed out. Try again.")),
          3000,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
    pending = false;
  }
}

export function tuneCatalog(): Promise<TuneCatalog> {
  return request("engine_tune_catalog");
}

export function tuneSnapshot(
  sessionId: number,
  names: string[],
  includeHandlers = false,
): Promise<TuneSnapshot> {
  if (names.length > 64) return Promise.reject(new Error("Request at most 64 probe names."));
  return request("engine_tune_snapshot", { sessionId, names, includeHandlers });
}

export function tuneError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function formatProbe(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return "Unavailable";
  if (Number.isInteger(value) && Math.abs(value) < 1e12) return String(value);
  return Number(value.toPrecision(7)).toString();
}
