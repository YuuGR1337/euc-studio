import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef } from "react";
import { smooth } from "../../design/motion.ts";
import { onFrame } from "../../lib/frameloop.ts";
import { Kbd } from "../../ui/Kbd.tsx";
import { Pill } from "../../ui/Pill.tsx";
import { sentShares } from "./controller.ts";
import { useFreeRideHud } from "./hud.ts";

/**
 * A centred bar from -1 to 1 whose fill grows from the middle. Written into the DOM on the frame
 * loop, so riding never re-renders React.
 */
function Bar({ read, label, ends }: { read: () => number; label: string; ends: [string, string] }) {
  const fill = useRef<HTMLSpanElement>(null);
  const value = useRef<HTMLSpanElement>(null);
  useEffect(
    () =>
      onFrame(() => {
        const v = Math.max(-1, Math.min(1, read()));
        if (fill.current) {
          fill.current.style.left = `${50 + Math.min(0, v) * 50}%`;
          fill.current.style.width = `${Math.abs(v) * 50}%`;
        }
        if (value.current) {
          const pct = Math.round(Math.abs(v) * 100);
          value.current.textContent = pct === 0 ? "0 %" : `${v > 0 ? ends[1] : ends[0]} ${pct} %`;
        }
      }),
    [read, ends],
  );
  return (
    <span className="grid grid-cols-[48px_1fr_88px] items-center gap-8">
      <span className="label">{label}</span>
      <span className="relative h-[6px] overflow-hidden rounded-pill bg-track">
        <span className="absolute inset-y-0 left-1/2 w-px bg-fg-3" />
        <span ref={fill} className="absolute inset-y-0 rounded-pill bg-accent" />
      </span>
      <span ref={value} className="num text-right text-12 text-fg" />
    </span>
  );
}

/** Seconds left on the restart countdown, written on the frame loop. */
function Countdown({ endsAt }: { endsAt: number }) {
  const seconds = useRef<HTMLSpanElement>(null);
  useEffect(
    () =>
      onFrame((now) => {
        if (seconds.current)
          seconds.current.textContent = String(Math.max(0, Math.ceil((endsAt - now) / 1000)));
      }),
    [endsAt],
  );
  return <span ref={seconds} className="num text-20 font-semibold text-fg" />;
}

const readLean = () => sentShares.lean;
// Left fills to the left (the turn's sign is left positive).
const readTurn = () => -sentShares.turn;
const LEAN_ENDS: [string, string] = ["back", "fwd"];
const TURN_ENDS: [string, string] = ["left", "right"];

/** Free Ride in the 3D view: what to do next, the restart countdown, and what the hands ask. */
export function FreeRideOverlay() {
  const { open, armed, prompt, countdown, keys } = useFreeRideHud();
  const show = open && (armed || !!prompt || !!countdown);
  return (
    <AnimatePresence>
      {show && (
        <motion.div
          data-testid="free-ride-hud"
          initial={{ opacity: 0, y: -8 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -8 }}
          transition={smooth}
          className="flex w-[336px] flex-col gap-8 rounded-12 border border-hairline bg-card/75 px-16 py-8 backdrop-blur-[12px] backdrop-saturate-150"
        >
          <span className="flex items-center justify-between gap-8">
            <span className="flex items-center gap-8">
              <span className="label">Free ride</span>
              {armed && (
                <Pill tone="accent" dot="live">
                  Riding
                </Pill>
              )}
            </span>
            {armed && (
              <span className="flex items-center gap-4 text-11 text-fg-2">
                <Kbd>Esc</Kbd> pause
              </span>
            )}
          </span>
          {countdown && (
            <span
              role="status"
              className="flex items-center justify-between gap-8 rounded-8 bg-caution-tint px-8 py-4 text-12 text-caution"
            >
              <span>
                Restarting {countdown.here ? "here" : "at the start point"}. The firmware boots
                again.
              </span>
              <Countdown endsAt={countdown.endsAt} />
            </span>
          )}
          {armed && (
            <>
              <Bar read={readLean} label="Lean" ends={LEAN_ENDS} />
              <Bar read={readTurn} label="Turn" ends={TURN_ENDS} />
            </>
          )}
          {prompt && (
            <p role="status" className="text-13 text-fg">
              {prompt}
            </p>
          )}
          {keys && (
            <span className="flex flex-wrap items-center gap-x-8 gap-y-4 text-11 text-fg-2">
              {keys.map(({ label, caps }) => (
                <span key={label} className="flex items-center gap-4">
                  {caps.map((cap) => (
                    <Kbd key={cap}>{cap}</Kbd>
                  ))}
                  {label.toLowerCase()}
                </span>
              ))}
            </span>
          )}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
