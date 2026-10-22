import { Tooltip as T } from "radix-ui";
import type { ReactNode } from "react";
import { cx } from "../lib/cx.ts";
import { Shortcut } from "./Kbd.tsx";

export const TooltipProvider = ({ children }: { children: ReactNode }) => (
  <T.Provider delayDuration={450} skipDelayDuration={250}>
    {children}
  </T.Provider>
);

export interface TooltipProps {
  content: ReactNode;
  shortcut?: string | undefined;
  side?: "top" | "right" | "bottom" | "left";
  children: ReactNode;
  /** Shows the tooltip without hovering (gallery). */
  open?: boolean;
}

export function TooltipBubble({
  content,
  shortcut,
  className,
}: {
  content: ReactNode;
  shortcut?: string | undefined;
  className?: string;
}) {
  return (
    <div
      className={cx(
        "flex max-w-[min(360px,calc(100vw-16px))] items-center gap-8 rounded-8 border border-hairline-strong bg-raised py-4 pl-8 text-12 font-medium text-fg shadow-overlay",
        shortcut ? "pr-4" : "pr-8",
        className,
      )}
    >
      <span className="min-w-0 [overflow-wrap:anywhere]">{content}</span>
      {shortcut && <Shortcut keys={shortcut} />}
    </div>
  );
}

export function Tooltip({ content, shortcut, side = "bottom", children, open }: TooltipProps) {
  return (
    <T.Root {...(open !== undefined ? { open } : {})}>
      <T.Trigger asChild>{children}</T.Trigger>
      <T.Portal>
        <T.Content side={side} sideOffset={8} collisionPadding={8} className="pop z-50">
          <TooltipBubble content={content} shortcut={shortcut} />
        </T.Content>
      </T.Portal>
    </T.Root>
  );
}
