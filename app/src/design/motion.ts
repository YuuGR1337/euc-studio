// Motion transitions built from the four motion tokens. Components import these instead of
// writing their own spring values.

import type { Transition } from "motion/react";
import { digits, motion } from "./tokens.ts";

export const snappy: Transition = { ...motion.snappy };
export const smooth: Transition = { ...motion.smooth };
export const bouncy: Transition = { ...motion.bouncy };
export const data: Transition = { ...motion.data };

// The enter curve ends above 1 (1.15), which is right for position and scale but would push
// blur below zero, so blur and opacity use the same duration without the overshoot.
export const digitEnter: Transition = {
  duration: digits.enter.duration,
  ease: [...digits.enter.ease],
  filter: { duration: digits.enter.duration, ease: "easeOut" },
  opacity: { duration: digits.enter.duration, ease: "easeOut" },
};
export const digitExit: Transition = { duration: digits.exit.duration, ease: digits.exit.ease };
