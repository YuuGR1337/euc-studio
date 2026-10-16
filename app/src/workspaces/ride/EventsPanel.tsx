import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { smooth } from "../../design/motion.ts";
import { cx } from "../../lib/cx.ts";
import { onFrame } from "../../lib/frameloop.ts";
import { engine, useEngineVersion } from "../../sim/engine.ts";
import type { EventKind, SimEvent } from "../../sim/synthetic.ts";
import { Flag } from "../../ui/icons.tsx";
import { Panel } from "../../ui/Panel.tsx";
import { Pill, type PillTone } from "../../ui/Pill.tsx";
import { formatClock } from "../../ui/rolling.ts";
import { Tabs } from "../../ui/Tabs.tsx";
import { Tooltip } from "../../ui/Tooltip.tsx";

const WINDOW = 30; // seconds on the strip

const KIND: Record<EventKind, { label: string; tone: PillTone; dot: string }> = {
  firmware: { label: "Firmware", tone: "info", dot: "bg-info" },
  ride: { label: "Ride", tone: "neutral", dot: "bg-fg-2-safe" },
  command: { label: "Command", tone: "accent", dot: "bg-accent" },
  alarm: { label: "Alarm", tone: "caution", dot: "bg-caution" },
};

type Filter = "all" | EventKind;

/** Markers ride a 30 s strip; one CSS variable per frame moves them, React never re-renders. */
function Strip({ events }: { events: SimEvent[] }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(
    () =>
      onFrame(() => {
        ref.current?.style.setProperty("--now", String(engine.latest.t));
      }),
    [],
  );
  return (
    <div ref={ref} className="relative mx-16 mt-8 h-[44px] shrink-0">
      <div className="absolute inset-x-0 top-[14px] h-px bg-hairline-strong" />
      {Array.from({ length: 7 }, (_, i) => (
        <div
          key={i}
          className={cx(
            "absolute top-[24px] text-[10px] text-fg-3",
            i === 0 ? "" : i === 6 ? "-translate-x-full" : "-translate-x-1/2",
          )}
          style={{ left: `${(i / 6) * 100}%` }}
        >
          <span className="num">{i === 6 ? "now" : `${-WINDOW + i * 5} s`}</span>
        </div>
      ))}
      <div className="absolute inset-0 overflow-hidden">
        {events.map((e) => (
          <Tooltip key={e.id} content={`${formatClock(e.t)}  ${e.title}`} side="top">
            <span
              className={cx(
                "absolute top-[9px] size-[10px] -translate-x-1/2 rounded-pill border-2 border-card",
                KIND[e.kind].dot,
              )}
              style={{ left: `calc((${e.t} - var(--now, 0) + ${WINDOW}) / ${WINDOW} * 100%)` }}
            />
          </Tooltip>
        ))}
      </div>
      <span className="absolute top-[8px] right-0 size-[12px] translate-x-1/2 rounded-pill border-2 border-card bg-accent" />
    </div>
  );
}

export function EventsPanel() {
  useEngineVersion();
  const [filter, setFilter] = useState<Filter>("all");
  const all = engine.events;
  const shown = (filter === "all" ? all : all.filter((e) => e.kind === filter))
    .slice(-60)
    .reverse();
  const count = (k: EventKind) => all.filter((e) => e.kind === k).length;

  return (
    <Panel
      title="Events"
      icon={<Flag />}
      className="min-h-[140px]"
      bodyClassName="flex flex-col overflow-hidden"
      actions={
        <Tabs<Filter>
          aria-label="Filter events"
          value={filter}
          onValueChange={setFilter}
          tabs={[
            { value: "all", label: "All", count: all.length },
            { value: "firmware", label: "Firmware", count: count("firmware") },
            { value: "ride", label: "Ride", count: count("ride") },
            { value: "alarm", label: "Alarms", count: count("alarm") },
            { value: "command", label: "Commands", count: count("command") },
          ]}
          className="-mb-px h-(--dock-header) [&>div]:h-full"
        />
      }
    >
      <Strip events={filter === "all" ? all : all.filter((e) => e.kind === filter)} />
      <ul className="min-h-0 flex-1 overflow-y-auto px-8 pb-8">
        <AnimatePresence initial={false}>
          {shown.map((e) => (
            <motion.li
              key={e.id}
              layout="position"
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: 32 }}
              exit={{ opacity: 0, height: 0 }}
              transition={smooth}
              className="overflow-hidden"
            >
              <div className="grid h-[32px] grid-cols-[64px_80px_1fr] items-center gap-8 rounded-8 px-8 hover:bg-raised">
                <span className="num text-12 text-fg-2">{formatClock(e.t)}</span>
                <span>
                  <Pill tone={KIND[e.kind].tone}>{KIND[e.kind].label}</Pill>
                </span>
                <span className="truncate text-13 text-fg">{e.title}</span>
              </div>
            </motion.li>
          ))}
        </AnimatePresence>
        {shown.length === 0 && (
          <li className="px-8 py-16 text-13 text-fg-2">
            No {filter === "all" ? "" : KIND[filter].label.toLowerCase()} events yet in this run.
          </li>
        )}
      </ul>
    </Panel>
  );
}
