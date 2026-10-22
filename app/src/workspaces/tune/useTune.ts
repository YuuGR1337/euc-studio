import { useEffect, useEffectEvent, useRef, useState } from "react";
import { engine, useEngineVersion } from "../../engine/index.ts";
import {
  TUNE_HISTORY_SAMPLES,
  TUNE_HISTORY_SECONDS,
  TUNE_PAGE_SIZE,
  TUNE_POLL_MS,
  tuneCatalog,
  tuneError,
  tuneSnapshot,
  type TuneCatalog,
  type TuneSnapshot,
} from "../../engine/tune.ts";
import { handlerInterval, type HandlerInterval } from "./handlers.ts";
import {
  advanceCapture,
  captureKey,
  holdCapture,
  type Capture,
  type ScopeHistory,
  type TriggerConfig,
} from "./triggers.ts";

interface TuneView {
  catalog: TuneCatalog | null;
  snapshot: TuneSnapshot | null;
  history: ScopeHistory;
  capture: Capture | null;
  handlers: HandlerInterval | null;
  error: string | null;
}

const empty = (): TuneView => ({
  catalog: null,
  snapshot: null,
  history: {},
  capture: null,
  handlers: null,
  error: null,
});

export function useTune(
  active: boolean,
  filter: string,
  page: number,
  selected: string[],
  trigger: TriggerConfig,
  includeHandlers: boolean,
) {
  useEngineVersion();
  const source = engine;
  const epoch = source.status.runEpoch;
  const generation = useRef(0);
  const [view, setView] = useState<TuneView>(empty);
  const [attempt, setAttempt] = useState(0);
  const query = useEffectEvent((catalog: TuneCatalog) => {
    const matched = catalog.variables.filter((v) =>
      v.name.toLowerCase().includes(filter.toLowerCase().trim()),
    );
    const currentPage = Math.min(page, Math.max(0, Math.ceil(matched.length / TUNE_PAGE_SIZE) - 1));
    const visible = matched.slice(currentPage * TUNE_PAGE_SIZE, (currentPage + 1) * TUNE_PAGE_SIZE);
    return {
      names: [...new Set([...visible.map((v) => v.name), ...selected])],
      selected,
      trigger,
      includeHandlers,
    };
  });

  useEffect(() => {
    if (!active || source.kind !== "live") return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let catalog: TuneCatalog | null = null;

    async function poll() {
      // Defer all state changes, including unavailable states, beyond the effect setup.
      await Promise.resolve();
      if (stopped) return;
      const requestedGeneration = generation.current;
      const current = () =>
        !stopped &&
        engine === source &&
        source.status.runEpoch === epoch &&
        generation.current === requestedGeneration;
      let delay = TUNE_POLL_MS;
      try {
        if (["starting", "error", "unavailable"].includes(source.status.state)) {
          throw new Error(source.status.message || "The simulator is not ready.");
        }
        catalog ??= await tuneCatalog();
        if (!current()) {
          if (!stopped) timer = setTimeout(() => void poll(), TUNE_POLL_MS);
          return;
        }
        {
          const currentCatalog = catalog;
          const requested = query(currentCatalog);
          const snapshot = await tuneSnapshot(
            currentCatalog.run.session_id,
            requested.names,
            requested.includeHandlers,
          );
          if (!current()) {
            if (!stopped) timer = setTimeout(() => void poll(), TUNE_POLL_MS);
            return;
          }
          if (
            snapshot.run.session_id !== currentCatalog.run.session_id ||
            (epoch !== undefined && snapshot.run.session_id !== epoch) ||
            !Number.isFinite(snapshot.t_s)
          ) {
            throw new Error("The session changed. Reload the probe catalog.");
          }
          const latest = query(currentCatalog);
          if (
            captureKey(latest.trigger, latest.selected) !==
            captureKey(requested.trigger, requested.selected)
          ) {
            timer = setTimeout(() => void poll(), TUNE_POLL_MS);
            return;
          }
          setView((previous) => {
            if (!current()) return previous;
            const sameRun =
              previous.snapshot?.run.session_id === snapshot.run.session_id &&
              previous.snapshot.t_s <= snapshot.t_s;
            const readings = new Map(snapshot.values.map((v) => [v.name, v]));
            const history: ScopeHistory = {};
            for (const name of requested.selected) {
              const points = sameRun ? (previous.history[name] ?? []) : [];
              const raw = readings.get(name)?.value;
              const value = raw != null && Number.isFinite(raw) ? raw : null;
              // A paused session is one observation, not repeated samples of a flat trace.
              const next =
                points.at(-1)?.t === snapshot.t_s
                  ? points
                  : [...points, { t: snapshot.t_s, value }];
              history[name] = next
                .filter((p) => p.t >= snapshot.t_s - TUNE_HISTORY_SECONDS)
                .slice(-TUNE_HISTORY_SAMPLES);
            }
            const capture = advanceCapture(
              sameRun ? previous.capture : null,
              requested.trigger,
              requested.selected,
              snapshot,
              history,
            );
            const handlers = handlerInterval(sameRun ? previous.snapshot : null, snapshot);
            return { catalog: currentCatalog, snapshot, history, capture, handlers, error: null };
          });
        }
      } catch (error) {
        if (!current()) {
          if (!stopped) timer = setTimeout(() => void poll(), TUNE_POLL_MS);
          return;
        }
        catalog = null;
        setView((previous) => (current() ? { ...empty(), error: tuneError(error) } : previous));
        delay = 1000;
      }
      if (!stopped) timer = setTimeout(() => void poll(), delay);
    }

    void poll();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [active, attempt, source, epoch]);

  // Status changes can precede the next poll. Hide prior measurements immediately.
  const unavailable =
    source.kind !== "live" || ["starting", "error", "unavailable"].includes(source.status.state);
  const oldRun =
    epoch !== undefined && view.catalog !== null && view.catalog.run.session_id !== epoch;
  return {
    ...(unavailable || oldRun ? empty() : view),
    unavailable,
    hold: () => {
      generation.current++;
      setView((previous) =>
        previous.snapshot
          ? {
              ...previous,
              capture:
                previous.capture?.phase === "acquiring" &&
                previous.capture.key === captureKey(trigger, selected)
                  ? { ...previous.capture, phase: "held", reason: "manual" }
                  : holdCapture(trigger, selected, previous.snapshot, previous.history),
            }
          : previous,
      );
    },
    rearm: () => {
      generation.current++;
      // The first fresh response establishes the new baseline. A pre-click value
      // cannot establish that a crossing happened after the user rearmed.
      setView((previous) => ({ ...previous, capture: null }));
    },
    retry: () => {
      generation.current++;
      setAttempt((n) => n + 1);
    },
  };
}
