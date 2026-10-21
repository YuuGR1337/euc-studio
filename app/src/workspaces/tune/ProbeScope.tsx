import { useId } from "react";
import { formatProbe, TUNE_HISTORY_SECONDS } from "../../engine/tune.ts";
import type { ProbePoint } from "./triggers.ts";

/** A small time-domain plot. Null readings break the path instead of drawing a false zero. */
export function ProbeScope({
  name,
  points: storedPoints,
  unit,
  windowEnd,
  cursorOffset,
  onCursor,
  threshold,
  triggerAt,
}: {
  name: string;
  points: ProbePoint[];
  unit: string | null;
  windowEnd: number;
  cursorOffset: number | null;
  onCursor: (offset: number) => void;
  threshold: number | null;
  triggerAt: number | null;
}) {
  const titleId = useId();
  const points = storedPoints.filter(
    (point) => point.t >= windowEnd - TUNE_HISTORY_SECONDS && point.t <= windowEnd,
  );
  const colors = [
    "var(--color-series-speed)",
    "var(--color-series-current)",
    "var(--color-series-voltage)",
    "var(--color-series-temp)",
  ];
  const colorIndex = Array.from(name).reduce(
    (index, char) => (index + char.charCodeAt(0)) % colors.length,
    0,
  );
  const finite = points.flatMap((p) => (p.value === null ? [] : [p.value]));
  if (finite.length === 0) {
    return <p className="px-16 py-24 text-13 text-fg-2">No readable samples.</p>;
  }
  const low = Math.min(...finite);
  const high = Math.max(...finite);
  // The threshold expands the plot axis, but it is not a measured signal sample.
  const axisLow = threshold !== null ? Math.min(low, threshold) : low;
  const axisHigh = threshold !== null ? Math.max(high, threshold) : high;
  // Normalize first: subtracting two finite values can overflow near f64's limits.
  const scale = Math.max(Math.abs(axisLow), Math.abs(axisHigh)) || 1;
  const lo = axisLow / scale;
  const hi = axisHigh / scale;
  const span = hi - lo || 1;
  const now = windowEnd;
  const yFor = (value: number) => (hi === lo ? 60 : 16 + (1 - (value / scale - lo) / span) * 88);
  const position = (point: ProbePoint) => {
    const x = 8 + ((point.t - now + TUNE_HISTORY_SECONDS) / TUNE_HISTORY_SECONDS) * 624;
    const y = yFor(point.value ?? 0);
    return `${x.toFixed(2)},${y.toFixed(2)}`;
  };
  const path = points
    .map((point, index) => {
      if (point.value === null) return "";
      const command = index > 0 && points[index - 1]?.value != null ? "L" : "M";
      return `${command}${position(point)}`;
    })
    .join(" ");
  const last = points.at(-1);
  const cursorTime = cursorOffset === null ? null : now + cursorOffset;
  const sample =
    cursorTime !== null &&
    cursorTime >= (points[0]?.t ?? Infinity) &&
    cursorTime <= (last?.t ?? -Infinity)
      ? points.reduce((best, point) =>
          Math.abs(point.t - cursorTime) < Math.abs(best.t - cursorTime) ? point : best,
        )
      : null;

  return (
    <div className="px-16 pb-16">
      <div className="flex justify-between gap-8 text-11 text-fg-2">
        <span className="num">
          High {formatProbe(high)} {unit}
        </span>
        <span className="num">
          Low {formatProbe(low)} {unit}
        </span>
      </div>
      <svg
        className="h-[128px] w-full"
        style={{ color: colors[colorIndex] }}
        viewBox="0 0 640 128"
        preserveAspectRatio="none"
        role="img"
        aria-labelledby={titleId}
        onPointerMove={(event) => {
          const bounds = event.currentTarget.getBoundingClientRect();
          if (bounds.width <= 0) return;
          const x = ((event.clientX - bounds.left) / bounds.width) * 640;
          onCursor(
            Math.max(
              -TUNE_HISTORY_SECONDS,
              Math.min(0, ((x - 8) / 624) * TUNE_HISTORY_SECONDS - TUNE_HISTORY_SECONDS),
            ),
          );
        }}
      >
        <title id={titleId}>{name}: values over the last 30 simulation seconds</title>
        <path d="M8,16H632 M8,60H632 M8,104H632" stroke="var(--color-hairline)" fill="none" />
        <path
          d={path}
          stroke="currentColor"
          strokeWidth="1.5"
          fill="none"
          vectorEffect="non-scaling-stroke"
        />
        {threshold !== null && (
          <path
            d={`M8,${yFor(threshold)}H632`}
            stroke="var(--color-caution)"
            strokeDasharray="4 4"
            fill="none"
          />
        )}
        {triggerAt !== null && triggerAt >= now - TUNE_HISTORY_SECONDS && triggerAt <= now && (
          <path
            aria-label={`Trigger at ${triggerAt.toFixed(3)} seconds`}
            d={`M${8 + ((triggerAt - now + TUNE_HISTORY_SECONDS) / TUNE_HISTORY_SECONDS) * 624},16V104`}
            stroke="var(--color-caution)"
            strokeDasharray="4 4"
            fill="none"
          />
        )}
        {cursorOffset !== null && (
          <path
            d={`M${8 + ((cursorOffset + TUNE_HISTORY_SECONDS) / TUNE_HISTORY_SECONDS) * 624},8V112`}
            stroke="var(--color-fg-2)"
            strokeDasharray="2 4"
            fill="none"
          />
        )}
        {last?.value != null && (
          <circle
            cx={Number(position(last).split(",")[0])}
            cy={Number(position(last).split(",")[1])}
            r="3"
            fill="currentColor"
          />
        )}
      </svg>
      <div className="flex justify-between text-11 text-fg-2">
        <span>30 s ago</span>
        <span>{now.toFixed(3)} s</span>
      </div>
      {cursorTime !== null && (
        <p className="num mt-8 text-12 text-fg-2">
          Cursor {cursorTime.toFixed(3)} s.{" "}
          {sample
            ? `Sample ${sample.t.toFixed(3)} s: ${formatProbe(sample.value)} ${unit ?? ""}`
            : "No stored sample at this time."}
        </p>
      )}
      {points.length === 1 && (
        <p className="mt-8 text-12 text-fg-2">Waiting for the next simulation sample.</p>
      )}
    </div>
  );
}
