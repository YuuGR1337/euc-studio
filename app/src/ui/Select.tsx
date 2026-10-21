import { Select as S } from "radix-ui";
import type { ReactNode } from "react";
import { cx } from "../lib/cx.ts";
import { Check, ChevronDown } from "./icons.tsx";

export interface SelectOption<T extends string> {
  value: T;
  label: string;
  detail?: string;
  /** Shown instead of the detail; the option cannot be chosen. */
  disabledReason?: string | undefined;
}

export interface SelectProps<T extends string> {
  value: T;
  onValueChange: (value: T) => void;
  options: SelectOption<T>[];
  "aria-label": string;
  icon?: ReactNode;
  /** Custom trigger text (defaults to the selected label). */
  display?: ReactNode;
  className?: string;
  contentClassName?: string;
  force?: "hover" | "focus";
}

/** A dropdown choice. The list pops with the snappy spring and marks the current value. */
export function Select<T extends string>({
  value,
  onValueChange,
  options,
  icon,
  display,
  className,
  contentClassName,
  force,
  ...aria
}: SelectProps<T>) {
  const current = options.find((o) => o.value === value);
  return (
    <S.Root value={value} onValueChange={(v) => onValueChange(v as T)}>
      <S.Trigger
        aria-label={aria["aria-label"]}
        data-force={force}
        className={cx(
          "group pressable inline-flex h-(--control-md) min-w-0 items-center gap-8 rounded-8 border border-hairline bg-card pr-8 pl-16 text-13 font-medium text-fg",
          "live:hover:bg-raised force-hover:bg-raised data-[state=open]:bg-raised",
          "[&>svg]:size-[14px]",
          className,
        )}
      >
        {icon && <span className="text-fg-3 [&_svg]:size-[14px]">{icon}</span>}
        <span className="truncate">{display ?? current?.label}</span>
        <ChevronDown className="ml-auto shrink-0 text-fg-3 transition-transform duration-(--dur-snappy) ease-snappy group-data-[state=open]:rotate-180" />
      </S.Trigger>
      <S.Portal>
        <S.Content
          position="popper"
          sideOffset={8}
          collisionPadding={8}
          className={cx(
            "pop z-50 flex max-h-[min(360px,var(--radix-select-content-available-height))] min-w-(--radix-select-trigger-width) flex-col overflow-hidden rounded-12 border border-hairline-strong bg-raised p-4 shadow-overlay",
            contentClassName,
          )}
        >
          <S.Viewport className="min-h-0 overflow-y-auto overscroll-contain">
            {options.map((o) => (
              <S.Item
                key={o.value}
                value={o.value}
                disabled={!!o.disabledReason}
                className={cx(
                  "relative flex cursor-default flex-col rounded-8 py-8 pr-32 pl-8 outline-none select-none",
                  "data-[highlighted]:bg-track data-[disabled]:opacity-40",
                )}
              >
                <S.ItemText>
                  <span className="text-13 font-medium text-fg">{o.label}</span>
                </S.ItemText>
                {(o.disabledReason ?? o.detail) && (
                  <span className="text-12 text-fg-2-safe">{o.disabledReason ?? o.detail}</span>
                )}
                <S.ItemIndicator className="absolute top-8 right-8 text-accent-text [&_svg]:size-[16px]">
                  <Check />
                </S.ItemIndicator>
              </S.Item>
            ))}
          </S.Viewport>
        </S.Content>
      </S.Portal>
    </S.Root>
  );
}
