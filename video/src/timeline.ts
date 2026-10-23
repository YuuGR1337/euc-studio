import footage from "./footage.gen.json";

// One place for time. The frame rate is the capture's (capture.mjs steps the studio 32 ms per
// frame: 31.25 fps), so every footage frame is one video frame and the runs play at 1x.
export const FPS: number = footage.fps;

/** The music's tempo (public/audio/music.mp3 starts on a downbeat): every cut lands on a beat. */
export const BPM = 110;
export const BEAT = (60 / BPM) * FPS;

export type ShotId = "city" | "pothole" | "cutoff" | "compare";
export type Shot = { video: string; frames: number; t0: number; audio: string; audioT0: number };
export const SHOTS = footage.shots as Record<ShotId, Shot>;

/** Cross-dissolve after each cut, frames: the next scene starts on the beat and fades in. */
export const XF = 10;

/** The title sits over the city shot's first frames, then clears on the fourth beat. */
export const TITLE = { from: 0, dur: Math.round(4 * BEAT) + XF };

export type SceneName = ShotId | "close";

/** Beats per scene; a shot plays its beats and the dissolve into the next. */
const BEATS: Array<[SceneName, number]> = [
  ["city", 12],
  ["pothole", 10],
  ["cutoff", 9],
  ["compare", 11],
  ["close", 6],
];

const starts: Partial<Record<SceneName, number>> = {};
let acc = 0;
for (const [name, beats] of BEATS) {
  starts[name] = Math.round(acc * BEAT);
  acc += beats;
}
export const TOTAL = Math.round(acc * BEAT);

/** A scene's first frame and length: to the next cut, plus the dissolve that covers it. */
export const scene = (name: SceneName) => {
  const i = BEATS.findIndex(([n]) => n === name);
  const from = starts[name]!;
  const next = i + 1 < BEATS.length ? starts[BEATS[i + 1][0]]! : TOTAL;
  const dur = next - from + (i + 1 < BEATS.length ? XF : 0);
  const shot = SHOTS[name as ShotId];
  if (shot && dur > shot.frames)
    throw new Error(`${name}: ${dur} frames, the clip has ${shot.frames}`);
  return { from, dur };
};
