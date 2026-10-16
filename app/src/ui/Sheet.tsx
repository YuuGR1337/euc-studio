import { AnimatePresence, motion } from "motion/react";
import { Dialog } from "radix-ui";
import type { ReactNode } from "react";
import { smooth } from "../design/motion.ts";
import { cx } from "../lib/cx.ts";

export interface SheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Accessible title; shown unless `hideTitle`. */
  title: string;
  description?: string;
  hideTitle?: boolean;
  children: ReactNode;
  className?: string;
  /** Distance from the top of the window. */
  top?: string;
}

/** Modal sheet: dims the app and rises in with the smooth spring. */
export function Sheet({
  open,
  onOpenChange,
  title,
  description,
  hideTitle,
  children,
  className,
  top = "14vh",
}: SheetProps) {
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <AnimatePresence>
        {open && (
          <Dialog.Portal forceMount>
            <Dialog.Overlay asChild forceMount>
              <motion.div
                className="fixed inset-0 z-50 bg-bg/60"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={smooth}
              />
            </Dialog.Overlay>
            <Dialog.Content asChild forceMount>
              <motion.div
                style={{ top }}
                className={cx(
                  "fixed inset-x-0 z-50 mx-auto flex max-h-[72vh] flex-col overflow-hidden rounded-16 border border-hairline-strong bg-card shadow-overlay outline-none",
                  className,
                )}
                initial={{ opacity: 0, y: 12, scale: 0.98 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{
                  opacity: 0,
                  y: 6,
                  scale: 0.98,
                  transition: { duration: 0.14, ease: "easeIn" },
                }}
                transition={smooth}
              >
                <Dialog.Title
                  className={hideTitle ? "sr-only" : "px-24 pt-24 text-20 font-semibold text-fg"}
                >
                  {title}
                </Dialog.Title>
                {description ? (
                  <Dialog.Description
                    className={hideTitle ? "sr-only" : "px-24 pt-4 text-13 text-fg-2"}
                  >
                    {description}
                  </Dialog.Description>
                ) : (
                  <Dialog.Description className="sr-only">{title}</Dialog.Description>
                )}
                {children}
              </motion.div>
            </Dialog.Content>
          </Dialog.Portal>
        )}
      </AnimatePresence>
    </Dialog.Root>
  );
}
