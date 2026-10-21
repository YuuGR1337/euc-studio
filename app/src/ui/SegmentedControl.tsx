import { LayoutGroup, motion } from "motion/react";
import { RadioGroup } from "radix-ui";
import { type KeyboardEvent, type ReactNode, useId } from "react";
import { snappy } from "../design/motion.ts";
import { cx } from "../lib/cx.ts";
import { keyTarget } from "./segmentKeys.ts";
import { Tooltip } from "./Tooltip.tsx";

export interface SegmentOption<T extends string> {
  value: T;
  label: ReactNode;
  icon?: ReactNode;
  tooltip?: string;
  shortcut?: string;
  disabledReason?: string;
}

export interface SegmentedControlProps<T extends string> {
  value: T;
  onValueChange: (value: T) => void;
  options: SegmentOption<T>[];
  "aria-label": string;
  size?: "sm" | "md";
  className?: string;
  /** Gallery only: show this option hovered or focused. */
  force?: { value: T; state: "hover" | "focus" };
}

/**
 * Single choice with a knob that slides between options (snappy spring). Arrow keys move the
 * selection with the focus. Radix checks a radio when an arrow key moves focus onto it, but only
 * while the key is still down when its deferred focus lands, so a quick tap (or a test's key
 * press) moved the focus without the selection; the keydown here selects the target itself.
 */
export function SegmentedControl<T extends string>({
  value,
  onValueChange,
  options,
  size = "md",
  className,
  force,
  ...aria
}: SegmentedControlProps<T>) {
  const id = useId();
  return (
    <LayoutGroup id={id}>
      <RadioGroup.Root
        value={value}
        onValueChange={(v) => onValueChange(v as T)}
        orientation="horizontal"
        aria-label={aria["aria-label"]}
        onKeyDown={(e: KeyboardEvent) => {
          const next = keyTarget(e.key, value, options);
          if (next !== null && next !== value) onValueChange(next);
        }}
        className={cx(
          "relative inline-flex shrink-0 items-center rounded-pill border border-hairline bg-card p-2",
          size === "md" ? "h-(--control-md)" : "h-(--control-sm)",
          className,
        )}
      >
        {options.map((o) => {
          const selected = o.value === value;
          const disabled = !!o.disabledReason;
          const forced = force?.value === o.value ? force.state : undefined;
          const item = (
            <RadioGroup.Item
              key={o.value}
              value={o.value}
              disabled={disabled}
              data-force={forced}
              className={cx(
                "group pressable relative flex h-full items-center justify-center gap-4 rounded-pill font-semibold",
                size === "md" ? "px-16 text-13" : "px-8 text-12",
                selected ? "text-fg" : "text-fg-2 live:hover:text-fg force-hover:text-fg",
                "data-[disabled]:opacity-40",
                "[&_svg]:size-[14px]",
              )}
            >
              {selected && (
                <motion.span
                  layoutId="knob"
                  transition={snappy}
                  className="absolute inset-0 rounded-pill border border-hairline-strong bg-raised shadow-[inset_0_1px_0_rgba(255,255,255,0.05)]"
                />
              )}
              {!selected && (
                <span className="absolute inset-0 rounded-pill bg-raised/0 transition-colors group-hover:bg-raised/40 group-data-[force=hover]:bg-raised/40" />
              )}
              <span className="relative z-10 flex items-center gap-4">
                {o.icon}
                {o.label}
              </span>
            </RadioGroup.Item>
          );
          const tip = o.disabledReason ?? o.tooltip;
          return tip ? (
            <Tooltip key={o.value} content={tip} shortcut={disabled ? undefined : o.shortcut}>
              {item}
            </Tooltip>
          ) : (
            item
          );
        })}
      </RadioGroup.Root>
    </LayoutGroup>
  );
}
