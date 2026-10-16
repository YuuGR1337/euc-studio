import type { ReactNode } from "react";
import { cx } from "../lib/cx.ts";
import { shortcutKeys } from "../lib/platform.ts";

export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <kbd
      className={cx(
        "inline-flex h-[20px] min-w-[20px] items-center justify-center rounded-4 px-4 font-ui",
        "border border-hairline-strong bg-raised text-11 font-semibold text-fg-2-safe",
        "shadow-[inset_0_-1px_0_rgba(255,255,255,0.06)]",
        className,
      )}
    >
      {children}
    </kbd>
  );
}

/** A full shortcut as separate key caps. */
export function Shortcut({ keys, className }: { keys: string; className?: string }) {
  return (
    <span className={cx("inline-flex items-center gap-2", className)} aria-label={keys}>
      {shortcutKeys(keys).map((k, i) => (
        <Kbd key={i}>{k}</Kbd>
      ))}
    </span>
  );
}
