import { AnimatePresence, motion } from "motion/react";
import { useEffect, useState } from "react";
import { smooth } from "../design/motion.ts";
import { STUCK_AFTER_MS } from "../design/tokens.ts";
import { cx } from "../lib/cx.ts";
import { Button } from "./Button.tsx";
import { RefreshCw, ScrollText, X } from "./icons.tsx";

export interface WaitProps {
  /** What is happening right now, as a step name ("Building scene"). Never a bare spinner. */
  step: string;
  /** Optional detail under the step ("Compiling shaders"). */
  detail?: string;
  /** 0..1 when known; omitted for an indeterminate sweep. */
  progress?: number;
  onShowLog?: () => void;
  onRetry?: () => void;
  onCancel?: () => void;
  /** Gallery only: show the stuck state now. */
  stuck?: boolean;
  className?: string;
}

/**
 * A named wait. After 10 s (STUCK_AFTER_MS) it offers Show log / Retry / Cancel so nobody is
 * left staring at a bar that does not move.
 */
export function Wait({
  step,
  detail,
  progress,
  onShowLog,
  onRetry,
  onCancel,
  stuck: forceStuck,
  className,
}: WaitProps) {
  // Remember which step timed out, so a new step starts its own 10 s.
  const [stuckStep, setStuckStep] = useState<string | null>(null);
  useEffect(() => {
    const id = window.setTimeout(() => setStuckStep(step), STUCK_AFTER_MS);
    return () => window.clearTimeout(id);
  }, [step]);
  const showActions = forceStuck ?? stuckStep === step;

  return (
    <div
      className={cx("flex w-[320px] flex-col gap-8", className)}
      role="status"
      aria-live="polite"
    >
      <div className="flex items-baseline justify-between gap-8">
        <span className="text-13 font-semibold text-fg">{step}…</span>
        {progress !== undefined && (
          <span className="num text-12 text-fg-2">{Math.round(progress * 100)}%</span>
        )}
      </div>
      <div className="relative h-[4px] overflow-hidden rounded-pill bg-track">
        {progress !== undefined ? (
          <div
            className="h-full rounded-pill bg-accent transition-[width] duration-(--dur-data) ease-data"
            style={{ width: `${progress * 100}%` }}
          />
        ) : (
          <div className="absolute inset-y-0 w-2/5 animate-[sweep_1.4s_cubic-bezier(.45,0,.55,1)_infinite] rounded-pill bg-accent" />
        )}
      </div>
      {detail && <span className="text-12 text-fg-2">{detail}</span>}
      <AnimatePresence initial={false}>
        {showActions && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            exit={{ opacity: 0, height: 0 }}
            transition={smooth}
            className="overflow-hidden"
          >
            <p className="pt-8 text-12 text-fg-2">
              This is taking longer than usual. The log shows the last step that finished.
            </p>
            <div className="flex gap-8 pt-8">
              <Button size="sm" icon={<ScrollText />} onClick={onShowLog}>
                Show log
              </Button>
              <Button size="sm" icon={<RefreshCw />} onClick={onRetry}>
                Retry
              </Button>
              <Button size="sm" variant="ghost" icon={<X />} onClick={onCancel}>
                Cancel
              </Button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
