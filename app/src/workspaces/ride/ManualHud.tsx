import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef } from "react";
import { smooth } from "../../design/motion.ts";
import { onFrame } from "../../lib/frameloop.ts";
import { IconButton } from "../../ui/Button.tsx";
import { Keyboard } from "../../ui/icons.tsx";
import { Kbd } from "../../ui/Kbd.tsx";
import { Pill } from "../../ui/Pill.tsx";
import { current, toggleManual, useManual } from "./manual.ts";

/** M: manual control, or give control back. */
export function ManualButton() {
  const active = useManual((s) => s.active);
  return (
    <IconButton
      label={active ? "Give control back to the scenario" : "Manual control (W A S D or a gamepad)"}
      shortcut="m"
      icon={<Keyboard />}
      active={active}
      onClick={toggleManual}
      size="sm"
      className="bg-card/75 backdrop-blur-[12px]"
    />
  );
}

/**
 * A centred bar from -1 to 1: the fill grows from the middle toward the value. Written straight
 * into the DOM on the frame loop, so riding never re-renders React.
 */
function Bar({ read, label, ends }: { read: () => number; label: string; ends: [string, string] }) {
  const fill = useRef<HTMLSpanElement>(null);
  const value = useRef<HTMLSpanElement>(null);
  useEffect(
    () =>
      onFrame(() => {
        const v = Math.max(-1, Math.min(1, read()));
        const f = fill.current;
        if (f) {
          f.style.left = `${50 + Math.min(0, v) * 50}%`;
          f.style.width = `${Math.abs(v) * 50}%`;
        }
        const t = value.current;
        if (t) {
          const pct = Math.round(Math.abs(v) * 100);
          t.textContent = pct === 0 ? "0 %" : `${v > 0 ? ends[1] : ends[0]} ${pct} %`;
        }
      }),
    [read, ends],
  );
  return (
    <span className="grid grid-cols-[40px_1fr_88px] items-center gap-8">
      <span className="label">{label}</span>
      <span className="relative h-[6px] overflow-hidden rounded-pill bg-track">
        <span className="absolute inset-y-0 left-1/2 w-px bg-fg-3" />
        <span ref={fill} className="absolute inset-y-0 rounded-pill bg-accent" />
      </span>
      <span ref={value} className="num text-right text-12 text-fg" />
    </span>
  );
}

const readLean = () => current.lean;
// Left fills to the left (the input's sign is left positive).
const readTurn = () => -current.steer;
const LEAN_ENDS: [string, string] = ["back", "fwd"];
const TURN_ENDS: [string, string] = ["left", "right"];

/** What the hands ask while riding manually, and the keys that matter. */
export function ManualHud() {
  const { active, source, steppedOff } = useManual();
  return (
    <AnimatePresence>
      {active && (
        <motion.div
          data-testid="manual-hud"
          initial={{ opacity: 0, y: -8 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -8 }}
          transition={smooth}
          className="pointer-events-auto flex w-[312px] flex-col gap-8 rounded-12 border border-hairline bg-card/75 px-16 py-8 backdrop-blur-[12px] backdrop-saturate-150"
        >
          <span className="flex items-center justify-between">
            <span className="flex items-center gap-8">
              <span className="label">Manual</span>
              <Pill tone="accent" dot="live">
                {source === "gamepad" ? "Gamepad" : "Keyboard"}
              </Pill>
              {steppedOff && <Pill tone="caution">Stepped off</Pill>}
            </span>
            <button
              type="button"
              onClick={toggleManual}
              className="pressable flex items-center gap-4 rounded-8 px-4 text-12 text-fg-2 live:hover:bg-raised live:hover:text-fg-2-safe"
            >
              <Kbd>esc</Kbd> scenario
            </button>
          </span>
          <Bar read={readLean} label="Lean" ends={LEAN_ENDS} />
          <Bar read={readTurn} label="Turn" ends={TURN_ENDS} />
          <span className="flex items-center gap-8 text-11 text-fg-2">
            <Kbd>W</Kbd>
            <Kbd>S</Kbd> lean
            <Kbd>A</Kbd>
            <Kbd>D</Kbd> turn
            <Kbd>space</Kbd> step off
          </span>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
