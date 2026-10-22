import { useCallback, useEffect, useRef } from "react";
import { series } from "../../design/tokens.ts";
import { CHANNELS, type ChannelId } from "../../engine/types.ts";
import { LANE_KEYS, compare, useCompareVersion } from "../../engine/compare.ts";
import { onFrame } from "../../lib/frameloop.ts";
import { Activity } from "../../ui/icons.tsx";
import { Panel } from "../../ui/Panel.tsx";
import { CompareScope } from "./CompareScope.tsx";

/** The lanes, top to bottom: what a rider and the balance loop feel first. */
const COMPARE_LANES: { id: ChannelId; color: string }[] = [
  { id: "speed", color: series.speed },
  { id: "pitch", color: series.pitch },
  { id: "pwm", color: series.pwm },
  { id: "current", color: series.current },
  { id: "voltage", color: series.voltage },
];

const fmt = (v: number, decimals: number) => (Number.isFinite(v) ? v.toFixed(decimals) : "–");

/**
 * A and B (and their difference) for one lane: the latest values while the runs go on, or the
 * values under the cursor. Written straight into the DOM, at most 8 times a second for the latest
 * values; fixed widths, so the header never shifts.
 */
function useReadout(id: ChannelId) {
  const aRef = useRef<HTMLSpanElement>(null);
  const bRef = useRef<HTMLSpanElement>(null);
  const dRef = useRef<HTMLSpanElement>(null);
  const hovering = useRef(false);
  const decimals = CHANNELS[id].decimals;
  const write = useCallback(
    (a: number, b: number) => {
      const [ea, eb, ed] = [aRef.current, bRef.current, dRef.current];
      if (ea) ea.textContent = fmt(a, decimals);
      if (eb) eb.textContent = fmt(b, decimals);
      if (ed)
        ed.textContent = Number.isFinite(a - b) ? `Δ ${Math.abs(a - b).toFixed(decimals)}` : "Δ –";
    },
    [decimals],
  );
  useEffect(() => {
    let last = 0;
    return onFrame((now) => {
      if (hovering.current || now - last < 125) return;
      last = now;
      write(compare.a.latest[id] as number, compare.b.latest[id] as number);
    });
  }, [id, write]);
  const onCursor = useCallback(
    (at: { a: number; b: number } | null) => {
      hovering.current = at !== null;
      if (at) write(at.a, at.b);
    },
    [write],
  );
  return { aRef, bRef, dRef, onCursor };
}

function Lane({ id, color, last }: { id: ChannelId; color: string; last: boolean }) {
  const info = CHANNELS[id];
  const summary = compare.summary;
  const own = summary?.channels.find((c) => c.name === LANE_KEYS[id]);
  const { aRef, bRef, dRef, onCursor } = useReadout(id);
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid={`compare-lane-${id}`}>
      <div className="flex h-[24px] shrink-0 items-center gap-8 px-16">
        <span className="size-[8px] rounded-pill" style={{ background: color }} />
        <span className="w-[96px] truncate text-12 font-semibold text-fg">{info.label}</span>
        <span className="num ml-auto flex items-baseline gap-8 text-12">
          <span className="text-fg-3">A</span>
          <span ref={aRef} className="w-[44px] text-right text-fg" />
          <span className="text-fg-3">B</span>
          <span ref={bRef} className="w-[44px] text-right text-fg" />
          <span ref={dRef} className="w-[64px] text-right text-fg-2" />
          <span className="w-[32px] text-11 font-semibold text-fg-2">{info.unit}</span>
        </span>
      </div>
      <div className="relative min-h-0 flex-1 pr-8 pl-4">
        <CompareScope
          id={id}
          a={compare.a}
          b={compare.b}
          color={color}
          min={info.min}
          max={info.max}
          duration={compare.status.duration}
          divergence={summary?.first_divergence?.t_s ?? null}
          channelFirst={own?.first_s ?? null}
          showTime={last}
          onCursor={onCursor}
        />
      </div>
    </div>
  );
}

/** Line samples for the legend: A solid, B dashed (the scopes draw them the same way). */
function Legend() {
  const dash = "bg-[repeating-linear-gradient(90deg,var(--color-fg-2)_0_5px,transparent_5px_9px)]";
  return (
    <span className="flex items-center gap-16 text-12 text-fg-2">
      <span className="flex items-center gap-4">
        <span className="h-[2px] w-[16px] rounded-pill bg-fg-2" /> A
      </span>
      <span className="flex items-center gap-4">
        <span className={`h-[2px] w-[16px] ${dash}`} /> B
      </span>
    </span>
  );
}

/** The Compare workspace's right dock: both runs on shared scopes over the whole scenario. */
export function CompareScopes() {
  useCompareVersion();
  return (
    <Panel
      title="Both runs"
      icon={<Activity />}
      className="min-w-[300px]"
      bodyClassName="flex flex-col overflow-hidden"
      actions={<Legend />}
    >
      <div className="flex min-h-[300px] flex-1 flex-col gap-8 py-8" key={compare.epoch}>
        {COMPARE_LANES.map((l, i) => (
          <Lane key={l.id} id={l.id} color={l.color} last={i === COMPARE_LANES.length - 1} />
        ))}
      </div>
    </Panel>
  );
}
