import { useId } from "react";
import { headlightPreviewModel } from "./headlightOutput.ts";

/** Four front-facing lenses. GPIO duty controls light; this is not an optical measurement. */
export function HeadlightPreview({ duty }: { duty: number | null | undefined }) {
  const output = headlightPreviewModel(duty);
  const gradient = `headlight-${useId().replace(/:/g, "")}`;
  const unknown = output.duty === null;
  return (
    <svg
      viewBox="0 0 96 48"
      className="h-full w-[112px] max-w-full"
      role="img"
      aria-label={`Headlight ${output.state.toLowerCase()}`}
      aria-description="Four horizontal lenses. Simulated GPIO drive."
      data-state={output.state.toLowerCase()}
      data-duty={output.duty ?? undefined}
    >
      <defs>
        <radialGradient id={gradient}>
          <stop offset="0" stopColor="var(--color-fg)" stopOpacity="0.9" />
          <stop offset="1" stopColor="var(--color-fg)" stopOpacity="0.2" />
        </radialGradient>
      </defs>
      <rect
        x="8"
        y="12"
        width="80"
        height="24"
        rx="8"
        fill="var(--color-bg)"
        stroke="var(--color-hairline-strong)"
      />
      {output.lenses.map((lens) => (
        <circle
          key={lens.cx}
          cx={lens.cx}
          cy={lens.cy}
          r={lens.radius}
          fill={`url(#${gradient})`}
          fillOpacity={lens.opacity}
          stroke="var(--color-fg-3)"
        />
      ))}
      {unknown && (
        <text x="48" y="47" textAnchor="middle" fill="var(--color-fg-2)" fontSize="10">
          ?
        </text>
      )}
    </svg>
  );
}
