import { Switch } from "radix-ui";
import { type ReactNode, useId } from "react";
import { cx } from "../lib/cx.ts";
import { Tooltip } from "./Tooltip.tsx";

export interface ToggleProps {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  label: ReactNode;
  description?: ReactNode;
  disabledReason?: string;
  force?: "hover" | "press" | "focus";
  className?: string;
}

/**
 * On/off switch. The thumb travels with the snappy spring and stretches while pressed, the way
 * a finger drags it.
 */
export function Toggle({
  checked,
  onCheckedChange,
  label,
  description,
  disabledReason,
  force,
  className,
}: ToggleProps) {
  const id = useId();
  const disabled = !!disabledReason;
  const control = (
    <Switch.Root
      id={id}
      checked={checked}
      onCheckedChange={onCheckedChange}
      disabled={disabled}
      data-force={force}
      className={cx(
        "group pressable relative h-[20px] w-[36px] shrink-0 rounded-pill bg-track",
        "data-[state=checked]:bg-accent live:hover:bg-lift data-[state=checked]:live:hover:bg-accent-hover",
        "force-hover:bg-lift data-[state=checked]:force-hover:bg-accent-hover",
        "disabled:opacity-40",
      )}
    >
      <Switch.Thumb
        className={cx(
          "absolute top-[2px] left-[2px] block h-[16px] w-[16px] rounded-pill bg-fg",
          "shadow-[0_1px_2px_rgba(0,0,0,0.4)]",
          "transition-[transform,width] duration-(--dur-snappy) ease-snappy",
          "data-[state=checked]:translate-x-[16px]",
          "group-active:w-[20px] group-active:data-[state=checked]:translate-x-[12px]",
          "group-data-[force=press]:w-[20px] group-data-[force=press]:data-[state=checked]:translate-x-[12px]",
        )}
      />
    </Switch.Root>
  );
  return (
    <div className={cx("flex items-center justify-between gap-16", className)}>
      <label
        htmlFor={id}
        className={cx("flex min-w-0 flex-col", disabled ? "text-fg-3" : "text-fg")}
      >
        <span className="text-13 font-medium">{label}</span>
        {description && <span className="text-12 text-fg-2">{description}</span>}
      </label>
      {disabled ? (
        <Tooltip content={disabledReason}>
          <span tabIndex={0} className="rounded-pill">
            {control}
          </span>
        </Tooltip>
      ) : (
        control
      )}
    </div>
  );
}
