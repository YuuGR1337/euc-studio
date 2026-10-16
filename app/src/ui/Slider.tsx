import { Slider as S } from "radix-ui";
import { useState } from "react";
import { cx } from "../lib/cx.ts";

export interface SliderProps {
  value: number;
  onValueChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  label: string;
  unit?: string;
  decimals?: number;
  disabledReason?: string;
  force?: "hover" | "press" | "focus";
  className?: string;
}

/** Horizontal slider with a value readout; the thumb grows on hover and shows the value. */
export function Slider({
  value,
  onValueChange,
  min = 0,
  max = 100,
  step = 1,
  label,
  unit,
  decimals = 0,
  disabledReason,
  force,
  className,
}: SliderProps) {
  const [dragging, setDragging] = useState(false);
  const disabled = !!disabledReason;
  const active = dragging || force === "press";
  return (
    <div
      className={cx("flex flex-col gap-8", disabled && "opacity-40", className)}
      title={disabledReason}
    >
      <div className="flex items-baseline justify-between">
        <span className="text-13 font-medium text-fg">{label}</span>
        {/* While dragging, the readout lights up instead of a bubble covering the label row. */}
        <span
          className={cx(
            "num text-12 transition-colors duration-(--dur-hover)",
            active ? "text-accent-text" : "text-fg-2",
          )}
        >
          {value.toFixed(decimals)}
          {unit && (
            <span className={cx("ml-2", active ? "text-accent-text" : "text-fg-3")}>{unit}</span>
          )}
        </span>
      </div>
      <S.Root
        value={[value]}
        min={min}
        max={max}
        step={step}
        disabled={disabled}
        aria-label={label}
        onValueChange={([v]) => v !== undefined && onValueChange(v)}
        onPointerDown={() => setDragging(true)}
        onPointerUp={() => setDragging(false)}
        onLostPointerCapture={() => setDragging(false)}
        className="group relative flex h-[20px] touch-none items-center select-none"
      >
        <S.Track className="relative h-[4px] grow overflow-hidden rounded-pill bg-track">
          <S.Range className="absolute h-full rounded-pill bg-accent" />
        </S.Track>
        <S.Thumb
          data-force={force}
          className={cx(
            "relative block size-[16px] rounded-pill border border-hairline-strong bg-fg",
            "shadow-[0_1px_3px_rgba(0,0,0,0.5)]",
            "transition-transform duration-(--dur-snappy) ease-snappy",
            "hover:scale-[1.15] active:scale-[1.25] force-hover:scale-[1.15] force-press:scale-[1.25]",
          )}
        />
      </S.Root>
    </div>
  );
}
