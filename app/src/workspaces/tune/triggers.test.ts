import { describe, expect, it } from "vitest";
import type { TuneSnapshot } from "../../engine/tune.ts";
import {
  advanceCapture,
  armedCapture,
  holdCapture,
  triggerValid,
  type TriggerConfig,
  type ScopeHistory,
} from "./triggers.ts";

const selected = ["current", "speed"];
const config: TriggerConfig = { mode: "rising", source: "current", threshold: 10, postSeconds: 1 };
function snapshot(t: number, value: number | null, session = 1, state = "running"): TuneSnapshot {
  return {
    run: {
      session_id: session,
      backend: "own",
      scenario: "test",
      firmware: "stock",
      build: null,
      state,
      source: "Session",
    },
    t_s: t,
    values: [
      { name: "current", value, reason: null },
      { name: "speed", value: t * 2, reason: null },
    ],
  };
}
const history: ScopeHistory = {
  current: [
    { t: 0, value: 9 },
    { t: 0.25, value: 11 },
  ],
  speed: [
    { t: 0, value: 0 },
    { t: 0.25, value: 0.5 },
  ],
};
function crossed(c = config) {
  return advanceCapture(
    armedCapture(c, selected, snapshot(0, 9)),
    c,
    selected,
    snapshot(0.25, 11),
    history,
  );
}

describe("sampled trigger acquisition", () => {
  it("keeps pre-trigger data and all selected signals until the first sample at the requested time", () => {
    let capture = crossed();
    expect(capture.phase).toBe("acquiring");
    capture = advanceCapture(capture, config, selected, snapshot(0.75, 12), {});
    expect(capture.phase).toBe("acquiring");
    capture = advanceCapture(capture, config, selected, snapshot(1.25, 13), {});
    expect(capture.phase).toBe("held");
    expect(capture.reason).toBe("complete");
    expect(capture.at).toBe(0.25);
    expect(capture.end).toBe(1.25);
    expect(capture.history.current?.map((p) => p.t)).toEqual([0, 0.25, 0.75, 1.25]);
    expect(capture.history.speed?.map((p) => p.value)).toEqual([0, 0.5, 1.5, 2.5]);
    expect(advanceCapture(capture, config, selected, snapshot(2, 15), {})).toBe(capture);
    expect(history.current).toHaveLength(2);
  });

  it("holds immediately when after-trigger time is zero", () => {
    const capture = crossed({ ...config, postSeconds: 0 });
    expect(capture.phase).toBe("held");
    expect(capture.end).toBe(0.25);
    expect(capture.reason).toBe("complete");
  });

  it("uses simulation time, ignores paused duplicates, and retains unreadable values as gaps", () => {
    let capture = crossed();
    const paused = advanceCapture(capture, config, selected, snapshot(0.25, 11, 1, "paused"), {});
    expect(paused.history).toEqual(capture.history);
    expect(paused.phase).toBe("acquiring");
    capture = advanceCapture(paused, config, selected, snapshot(0.5, null), {});
    expect(capture.history.current?.at(-1)).toEqual({ t: 0.5, value: null });
    expect(capture.phase).toBe("acquiring");
  });

  it.each(["completed", "halted"])("labels an incomplete capture when the run is %s", (state) => {
    const capture = advanceCapture(crossed(), config, selected, snapshot(0.5, 12, 1, state), {});
    expect(capture.phase).toBe("held");
    expect(capture.reason).toBe("run_ended");
    expect(capture.end).toBe(0.5);
  });

  it("retains the crossing and reports a sample limit at very slow simulation rates", () => {
    let capture = crossed();
    for (let i = 1; i <= 300; i++)
      capture = advanceCapture(capture, config, selected, snapshot(0.25 + i * 0.001, 12), {});
    expect(capture.phase).toBe("held");
    expect(capture.reason).toBe("sample_limit");
    expect(capture.history.current).toHaveLength(240);
    expect(capture.history.current?.[1]?.t).toBe(0.25);
    expect(capture.end).toBeLessThan(1.25);
  });

  it("resets on a new session, time reversal, or configuration change", () => {
    for (const next of [snapshot(0.5, 12, 2), snapshot(0.1, 9)]) {
      const reset = advanceCapture(crossed(), config, selected, next, {});
      expect(reset.phase).toBe("armed");
      expect(reset.at).toBeNull();
      expect(reset.history).toEqual({});
    }
    const reset = advanceCapture(
      crossed(),
      { ...config, postSeconds: 5 },
      selected,
      snapshot(0.5, 12),
      {},
    );
    expect(reset.phase).toBe("armed");
  });

  it("does not trigger across a missing value and handles falling crossings", () => {
    let capture = armedCapture(config, selected, snapshot(0, 9));
    capture = advanceCapture(capture, config, selected, snapshot(0.1, null), {});
    capture = advanceCapture(capture, config, selected, snapshot(0.2, 11), {});
    expect(capture.phase).toBe("armed");
    const falling: TriggerConfig = { ...config, mode: "falling" };
    const fall = advanceCapture(
      armedCapture(falling, selected, snapshot(0, 11)),
      falling,
      selected,
      snapshot(0.25, 10),
      {},
    );
    expect(fall.phase).toBe("acquiring");
  });

  it("manual hold is immediate and invalid acquisition durations cannot arm", () => {
    expect(holdCapture(config, selected, snapshot(0.25, 11), history).reason).toBe("manual");
    for (const postSeconds of [-1, 16, NaN, Infinity])
      expect(triggerValid({ ...config, postSeconds }, selected)).toBe(false);
  });
});
