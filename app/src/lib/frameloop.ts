// One requestAnimationFrame loop for the whole app. The engine advances first, then every
// subscriber (scopes, sparklines, HUD readouts) reads the rings it needs. react-three-fiber runs
// its own loop and reads the same `latest` object in useFrame.

type FrameFn = (now: number, dt: number) => void;

const early = new Set<FrameFn>();
const subscribers = new Set<FrameFn>();
let raf = 0;
let last = 0;

function tick(now: number) {
  const dt = last ? (now - last) / 1000 : 0;
  last = now;
  for (const fn of early) fn(now, dt);
  for (const fn of subscribers) fn(now, dt);
  raf = requestAnimationFrame(tick);
}

function ensureRunning() {
  if (!raf && typeof requestAnimationFrame !== "undefined") {
    last = 0;
    raf = requestAnimationFrame(tick);
  }
}

/** Runs before regular subscribers each frame (the engine's `advance`). */
export function onFrameEarly(fn: FrameFn): () => void {
  early.add(fn);
  ensureRunning();
  return () => early.delete(fn);
}

export function onFrame(fn: FrameFn): () => void {
  subscribers.add(fn);
  ensureRunning();
  return () => subscribers.delete(fn);
}
