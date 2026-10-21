import { useEffect, useRef } from "react";
import uPlot from "uplot";
import { font, semantic } from "../../design/tokens.ts";
import type { CompareRun } from "../../engine/compareTypes.ts";
import type { ChannelId } from "../../engine/types.ts";
import { resolveColor, withAlpha } from "../../lib/canvas.ts";
import { onFrame } from "../../lib/frameloop.ts";
import { isLowPower } from "../../lib/power.ts";
import { niceStep, scopeRange } from "../ride/scopeRange.ts";

export interface CompareScopeProps {
  id: ChannelId;
  a: CompareRun;
  b: CompareRun;
  color: string;
  min: number;
  max: number;
  /** The scenario's length: the time axis spans the whole run, filling from the left. */
  duration: number;
  /** The first divergence of the comparison (all lanes), seconds. */
  divergence: number | null;
  /** This channel's first difference beyond its tolerance, seconds. */
  channelFirst: number | null;
  showTime?: boolean;
  /** Called with the values under the synced cursor (null: the cursor left the lanes). */
  onCursor?: (at: { t: number; a: number; b: number } | null) => void;
}

const AXIS_FONT = `500 10px ${font.mono}`;
/** Run B: the same colour, dashed. */
const DASH = [5, 4];

/**
 * One lane with both runs over the whole scenario: A solid, B dashed in the same colour. The time
 * axis is the run (0 to its length), the y range grows past both runs' extremes (scopeRange.ts),
 * cursors are synced across lanes, and the first divergence is a dashed red line on every lane.
 */
export function CompareScope({
  id,
  a,
  b,
  color,
  min,
  max,
  duration,
  divergence,
  channelFirst,
  showTime,
  onCursor,
}: CompareScopeProps) {
  const host = useRef<HTMLDivElement>(null);
  const marks = useRef({ divergence, channelFirst });
  const plotRef = useRef<uPlot | null>(null);
  const cursor = useRef(onCursor);

  useEffect(() => {
    cursor.current = onCursor;
  }, [onCursor]);

  useEffect(() => {
    marks.current = { divergence, channelFirst };
    plotRef.current?.redraw(false, false);
  }, [divergence, channelFirst]);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const stroke = resolveColor(el, color);
    const fg2 = resolveColor(el, "var(--color-fg-2)");
    const danger = resolveColor(el, semantic.danger);
    const grid = "rgba(255, 255, 255, 0.06)";
    const cap = Math.max(a.time.capacity, b.time.capacity);
    const xs = new Float64Array(cap);
    const ya = new Float64Array(cap);
    const yb = new Float64Array(cap);
    const span = Math.max(duration, 1);
    let [lo, hi] = [min, max];
    let seenLo = Infinity;
    let seenHi = -Infinity;

    const vline = (u: uPlot, t: number, colour: string, dash: number[], width: number) => {
      const x = u.valToPos(t, "x", true);
      const { top, height } = u.bbox;
      const ctx = u.ctx;
      ctx.save();
      ctx.strokeStyle = colour;
      ctx.lineWidth = width * devicePixelRatio;
      ctx.setLineDash(dash.map((d) => d * devicePixelRatio));
      ctx.beginPath();
      ctx.moveTo(x, top);
      ctx.lineTo(x, top + height);
      ctx.stroke();
      ctx.restore();
    };

    const opts: uPlot.Options = {
      width: Math.max(10, el.clientWidth),
      height: Math.max(10, el.clientHeight),
      pxAlign: false,
      padding: [8, 8, showTime ? 2 : 8, 0],
      legend: { show: false },
      cursor: {
        sync: { key: "compare", setSeries: false },
        drag: { x: false, y: false, setScale: false },
        points: { size: 6, width: 0, fill: stroke },
        y: false,
      },
      select: { show: false, left: 0, top: 0, width: 0, height: 0 },
      scales: {
        x: { time: false, range: () => [0, span] },
        y: { range: () => [lo, hi] },
      },
      axes: [
        {
          stroke: fg2,
          font: AXIS_FONT,
          size: showTime ? 22 : 0,
          gap: 4,
          ticks: { show: false },
          grid: { stroke: grid, width: 1 },
          // Round seconds, and none at the right edge, where its label would be cut.
          splits: () => {
            const step = niceStep(span / 6);
            const out: number[] = [];
            for (let v = step; v < span - step * 0.4; v += step) out.push(v);
            return out;
          },
          values: (_u, splits) => splits.map((v) => (showTime ? `${Math.round(v)} s` : "")),
        },
        {
          stroke: fg2,
          font: AXIS_FONT,
          size: 38,
          gap: 4,
          ticks: { show: false },
          grid: { stroke: grid, width: 1 },
          splits: () => [lo, (lo + hi) / 2, hi],
          values: (_u, splits) => splits.map((v) => `${Math.round(v)}`),
        },
      ],
      series: [
        {},
        {
          label: "A",
          stroke,
          width: 1.5,
          points: { show: false },
          fill: (u: uPlot) => {
            const g = u.ctx.createLinearGradient(0, u.bbox.top, 0, u.bbox.top + u.bbox.height);
            g.addColorStop(0, withAlpha(stroke, 0.12));
            g.addColorStop(1, withAlpha(stroke, 0));
            return g;
          },
        },
        {
          label: "B",
          stroke: withAlpha(stroke, 0.85),
          width: 1.5,
          dash: DASH,
          points: { show: false },
        },
      ],
      hooks: {
        draw: [
          (u) => {
            const { divergence: d, channelFirst: f } = marks.current;
            if (f !== null && f !== d) vline(u, f, withAlpha(stroke, 0.55), [2, 3], 1);
            if (d !== null) vline(u, d, danger, [3, 3], 1.25);
          },
        ],
        setCursor: [
          (u) => {
            const i = u.cursor.idx;
            if (i === null || i === undefined) return cursor.current?.(null);
            const t = u.data[0][i];
            const va = u.data[1]?.[i];
            const vb = u.data[2]?.[i];
            if (t === undefined || va == null || vb == null) return cursor.current?.(null);
            cursor.current?.({ t, a: va, b: vb });
          },
        ],
      },
    };

    const plot = new uPlot(opts, [[], [], []], el);
    plotRef.current = plot;
    const ro = new ResizeObserver(() => {
      plot.setSize({ width: Math.max(10, el.clientWidth), height: Math.max(10, el.clientHeight) });
    });
    ro.observe(el);

    // Redraw only when new samples arrived, at most 30 times a second without a GPU.
    let drawn = -1;
    let lastAt = 0;
    const off = onFrame((frameAt) => {
      const version = a.time.version + b.time.version;
      if (version === drawn) return;
      if (isLowPower() && frameAt - lastAt < 1000 / 30) return;
      drawn = version;
      lastAt = frameAt;
      const n = Math.min(
        a.time.copyTo(xs, cap),
        a.channels[id].copyTo(ya, cap),
        b.channels[id].copyTo(yb, cap),
      );
      for (let i = 0; i < n; i++) {
        for (const v of [ya[i] as number, yb[i] as number]) {
          if (v < seenLo) seenLo = v;
          if (v > seenHi) seenHi = v;
        }
      }
      [lo, hi] = scopeRange(min, max, seenLo, seenHi);
      plot.setData([xs.subarray(0, n), ya.subarray(0, n), yb.subarray(0, n)], true);
    });

    return () => {
      off();
      ro.disconnect();
      plot.destroy();
      plotRef.current = null;
    };
  }, [id, a, b, color, min, max, duration, showTime]);

  return <div ref={host} className="h-full w-full" />;
}
