import { Easing, interpolate } from "remotion";

// Every motion in the video is a pure function of the frame, never a CSS transition: Remotion
// renders frames out of order and in parallel, so clock-based animation lies.

const OUT = Easing.bezier(0.16, 1, 0.3, 1); // enter: ease-out
const IN = Easing.bezier(0.7, 0, 0.84, 0); // leave: ease-in

const clamp = { extrapolateLeft: "clamp", extrapolateRight: "clamp" } as const;

/** 0 -> 1 over [start, start+dur], eased out. */
export const enter = (f: number, start: number, dur = 20) =>
  interpolate(f, [start, start + dur], [0, 1], { ...clamp, easing: OUT });

/** 0 -> 1 over [start, start+dur], eased in (for exits). */
export const leave = (f: number, start: number, dur = 14) =>
  interpolate(f, [start, start + dur], [0, 1], { ...clamp, easing: IN });

/** Linear, clamped. */
export const lin = (f: number, input: number[], output: number[]) =>
  interpolate(f, input, output, clamp);

/**
 * The video's one transition: blur + fade + a short rise on the way in, blur + fade on the way
 * out. `inP` and `outP` are 0..1 progress values from enter()/leave().
 */
export const blurFade = (inP: number, outP = 0, rise = 18, maxBlur = 14) => ({
  opacity: inP * (1 - outP),
  filter: `blur(${(1 - inP) * maxBlur + outP * maxBlur}px)`,
  transform: `translateY(${(1 - inP) * rise - outP * rise * 0.4}px)`,
});
