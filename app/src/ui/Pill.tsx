import type { ReactNode } from "react";
import { cx } from "../lib/cx.ts";

export type PillTone = "neutral" | "accent" | "ok" | "caution" | "danger" | "info";

const tones: Record<PillTone, { pill: string; dot: string }> = {
  neutral: { pill: "bg-raised text-fg-2-safe", dot: "bg-fg-2-safe" },
  accent: { pill: "bg-accent-tint text-accent-text", dot: "bg-accent" },
  ok: { pill: "bg-ok-tint text-ok", dot: "bg-ok" },
  caution: { pill: "bg-caution-tint text-caution", dot: "bg-caution" },
  danger: { pill: "bg-danger-tint text-danger-text", dot: "bg-danger" },
  info: { pill: "bg-info-tint text-info", dot: "bg-info" },
};

export interface PillProps {
  tone?: PillTone;
  size?: "sm" | "md";
  /** Leading status dot; `live` makes it pulse (something is running). */
  dot?: boolean | "live";
  icon?: ReactNode;
  children: ReactNode;
  className?: string;
}

export function Pill({ tone = "neutral", size = "sm", dot, icon, children, className }: PillProps) {
  const t = tones[tone];
  return (
    <span
      className={cx(
        "inline-flex shrink-0 items-center gap-4 rounded-pill font-semibold whitespace-nowrap",
        size === "sm" ? "h-[20px] px-8 text-11" : "h-(--control-sm) px-8 text-12",
        "[&_svg]:size-[12px]",
        t.pill,
        className,
      )}
    >
      {dot && (
        <span
          className={cx(
            "size-[6px] rounded-pill",
            t.dot,
            dot === "live" && "animate-[live-pulse_1.6s_ease-in-out_infinite]",
          )}
        />
      )}
      {icon}
      {children}
    </span>
  );
}
