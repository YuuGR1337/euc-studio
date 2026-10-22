import type { HandlerCounters, TuneSnapshot } from "../../engine/tune.ts";

export interface HandlerRow {
  exception: number;
  name: string;
  total: string;
  cycles: string | null;
  percent: number | null;
}

export interface HandlerInterval {
  reason: string | null;
  elapsed_cycles: string | null;
  seconds: number | null;
  percent: number | null;
  rows: HandlerRow[];
  truncated: boolean;
}

function counter(raw: string): bigint {
  if (!/^\d{1,20}$/.test(raw)) throw new Error("A handler counter is invalid.");
  const value = BigInt(raw);
  if (value > 0xffffffffffffffffn) throw new Error("A handler counter exceeds 64 bits.");
  return value;
}

function rowsOf(counters: HandlerCounters): HandlerRow[] {
  return counters.rows.slice(0, 128).map((row) => ({
    exception: row.exception,
    name: row.name,
    total: row.occupied_cycles,
    cycles: null,
    percent: null,
  }));
}

export function handlerInterval(
  previous: TuneSnapshot | null,
  current: TuneSnapshot,
): HandlerInterval {
  const counts = current.handler_counters;
  const empty: HandlerInterval = {
    reason: current.handler_reason ?? "Handler counters are unavailable.",
    elapsed_cycles: null,
    seconds: null,
    percent: null,
    rows: [],
    truncated: false,
  };
  if (!counts) return empty;
  const baseline: HandlerInterval = {
    ...empty,
    reason: "Waiting for a second counter sample.",
    rows: rowsOf(counts),
    truncated: counts.truncated,
  };
  const before = previous?.handler_counters;
  if (!previous || !before || previous.run.session_id !== current.run.session_id) return baseline;
  try {
    const elapsed = counter(counts.cycle) - counter(before.cycle);
    const occupied = counter(counts.isr_cycles) - counter(before.isr_cycles);
    const earlier = new Map(before.rows.map((row) => [row.exception, row]));
    const present = new Set(counts.rows.map((row) => row.exception));
    const disappeared =
      !counts.truncated &&
      before.rows.some((row) => !present.has(row.exception) && counter(row.occupied_cycles) > 0n);
    const decreased = counts.rows.some((row) => {
      const old = earlier.get(row.exception);
      return old && counter(row.occupied_cycles) < counter(old.occupied_cycles);
    });
    if (elapsed < 0n || occupied < 0n || decreased || disappeared || current.t_s < previous.t_s) {
      return { ...baseline, reason: "Counters reset. Waiting for a new interval." };
    }
    if (elapsed === 0n) {
      return {
        ...baseline,
        reason:
          occupied === 0n
            ? "Simulation time did not advance. No interval measurement."
            : "Counters changed without elapsed simulation cycles.",
      };
    }
    if (occupied > elapsed)
      return { ...baseline, reason: "ISR counters exceed the simulation interval." };
    const rows = counts.rows.slice(0, 128).map((row): HandlerRow => {
      const old = earlier.get(row.exception);
      const result = {
        exception: row.exception,
        name: row.name,
        total: row.occupied_cycles,
        cycles: null,
        percent: null,
      };
      if ((!old && before.truncated) || (old && old.name !== row.name)) return result;
      const delta = counter(row.occupied_cycles) - (old ? counter(old.occupied_cycles) : 0n);
      if (delta < 0n || delta > elapsed) return result;
      return {
        ...result,
        cycles: delta.toString(),
        percent: (Number(delta) / Number(elapsed)) * 100,
      };
    });
    return {
      reason: null,
      elapsed_cycles: elapsed.toString(),
      seconds: current.t_s - previous.t_s,
      percent: (Number(occupied) / Number(elapsed)) * 100,
      rows,
      truncated: counts.truncated,
    };
  } catch (error) {
    return {
      ...baseline,
      reason: error instanceof Error ? error.message : "Handler counters are invalid.",
    };
  }
}
