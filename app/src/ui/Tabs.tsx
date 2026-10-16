import { LayoutGroup, motion } from "motion/react";
import { Tabs as T } from "radix-ui";
import { type ReactNode, useId } from "react";
import { snappy } from "../design/motion.ts";
import { cx } from "../lib/cx.ts";

export interface TabsProps<T extends string> {
  value: T;
  onValueChange: (value: T) => void;
  tabs: { value: T; label: ReactNode; count?: number; disabledReason?: string }[];
  "aria-label": string;
  children?: ReactNode;
  className?: string;
  force?: { value: T; state: "hover" | "focus" };
}

/** Text tabs with an accent underline that slides to the selected tab. */
export function Tabs<T extends string>({
  value,
  onValueChange,
  tabs,
  children,
  className,
  force,
  ...aria
}: TabsProps<T>) {
  const id = useId();
  return (
    <T.Root value={value} onValueChange={(v) => onValueChange(v as T)} className={className}>
      <LayoutGroup id={id}>
        <T.List
          aria-label={aria["aria-label"]}
          className="flex h-(--control-md) items-stretch gap-16"
        >
          {tabs.map((t) => {
            const selected = t.value === value;
            return (
              <T.Trigger
                key={t.value}
                value={t.value}
                disabled={!!t.disabledReason}
                title={t.disabledReason}
                data-force={force?.value === t.value ? force.state : undefined}
                className={cx(
                  "relative flex items-center gap-4 rounded-4 text-13 font-semibold transition-colors",
                  selected ? "text-fg" : "text-fg-2 live:hover:text-fg force-hover:text-fg",
                  "data-[disabled]:text-fg-3",
                )}
              >
                {t.label}
                {t.count !== undefined && (
                  <span
                    className={cx(
                      "num rounded-pill px-4 text-11",
                      selected ? "bg-accent-tint text-accent-text" : "bg-raised text-fg-2-safe",
                    )}
                  >
                    {t.count}
                  </span>
                )}
                {selected && (
                  <motion.span
                    layoutId="underline"
                    transition={snappy}
                    className="absolute inset-x-0 -bottom-px h-[2px] rounded-pill bg-accent"
                  />
                )}
              </T.Trigger>
            );
          })}
        </T.List>
      </LayoutGroup>
      {children}
    </T.Root>
  );
}

export const TabPanel = T.Content;
