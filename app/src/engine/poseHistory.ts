import { POSE_FLOATS } from "./telemetry.gen.ts";

/** Hard limits for the visual sidecar; raw telemetry keeps its existing storage. */
export const POSE_HISTORY_FRAMES = 64;
export const POSE_MAX_BODIES = 128;
/** Presentation delay in wall milliseconds, converted to sim time with measured RTF. */
export const POSE_DELAY_MS = 50;
/** Do not interpolate across a suspended renderer or a stalled input stream. */
export const POSE_WALL_GAP_MS = 250;
/** Maximum continuous live sample gap, in simulation seconds. */
export const POSE_SIM_GAP_S = 0.25;

const MIN_QUATERNION_NORM = 1e-12;

/** Validate a complete frame before modifying any destination. Quaternion order is w/x/y/z. */
export function validPoseFrame(poses: Float32Array, offset: number, bodies: number): boolean {
  if (
    !Number.isInteger(bodies) ||
    bodies < 1 ||
    bodies > POSE_MAX_BODIES ||
    !Number.isSafeInteger(offset) ||
    offset < 0 ||
    offset + bodies * POSE_FLOATS > poses.length
  )
    return false;
  for (let body = 0; body < bodies; body++) {
    const at = offset + body * POSE_FLOATS;
    for (let lane = 0; lane < POSE_FLOATS; lane++) {
      if (!Number.isFinite(poses[at + lane])) return false;
    }
    const norm = Math.hypot(
      poses[at + 3] as number,
      poses[at + 4] as number,
      poses[at + 5] as number,
      poses[at + 6] as number,
    );
    if (!Number.isFinite(norm) || norm < MIN_QUATERNION_NORM) return false;
  }
  return true;
}

/**
 * Linear positions and shortest-path quaternion slerp, with normalized endpoints/output.
 * Offsets index floats, not bodies. Output may be larger than the frame; its tail is untouched.
 * No allocation, Three.js dependency, extrapolation, or source mutation occurs here.
 * Reject source/output buffer aliases so a visual read cannot rewrite recorded or raw poses.
 */
export function interpolatePoseFrames(
  out: Float32Array,
  a: Float32Array,
  aOffset: number,
  b: Float32Array,
  bOffset: number,
  bodies: number,
  alpha: number,
): boolean {
  if (
    !Number.isFinite(alpha) ||
    alpha < 0 ||
    alpha > 1 ||
    out.length < bodies * POSE_FLOATS ||
    out.buffer === a.buffer ||
    out.buffer === b.buffer ||
    !validPoseFrame(a, aOffset, bodies) ||
    !validPoseFrame(b, bOffset, bodies)
  )
    return false;
  for (let body = 0; body < bodies; body++) {
    const at = aOffset + body * POSE_FLOATS;
    const bt = bOffset + body * POSE_FLOATS;
    const to = body * POSE_FLOATS;
    for (let lane = 0; lane < 3; lane++) {
      out[to + lane] = (1 - alpha) * (a[at + lane] as number) + alpha * (b[bt + lane] as number);
    }
    let aw = a[at + 3] as number;
    let ax = a[at + 4] as number;
    let ay = a[at + 5] as number;
    let az = a[at + 6] as number;
    let bw = b[bt + 3] as number;
    let bx = b[bt + 4] as number;
    let by = b[bt + 5] as number;
    let bz = b[bt + 6] as number;
    const an = Math.hypot(aw, ax, ay, az);
    const bn = Math.hypot(bw, bx, by, bz);
    aw /= an;
    ax /= an;
    ay /= an;
    az /= an;
    bw /= bn;
    bx /= bn;
    by /= bn;
    bz /= bn;
    let dot = aw * bw + ax * bx + ay * by + az * bz;
    if (dot < 0) {
      bw = -bw;
      bx = -bx;
      by = -by;
      bz = -bz;
      dot = -dot;
    }
    dot = Math.min(1, dot);
    let wa = 1 - alpha;
    let wb = alpha;
    // Near-identical quaternions use normalized lerp to avoid division by a tiny sine.
    if (dot < 0.9995) {
      const angle = Math.acos(dot);
      const sine = Math.sin(angle);
      wa = Math.sin((1 - alpha) * angle) / sine;
      wb = Math.sin(alpha * angle) / sine;
    }
    const w = wa * aw + wb * bw;
    const x = wa * ax + wb * bx;
    const y = wa * ay + wb * by;
    const z = wa * az + wb * bz;
    const norm = Math.hypot(w, x, y, z);
    out[to + 3] = w / norm;
    out[to + 4] = x / norm;
    out[to + 5] = y / norm;
    out[to + 6] = z / norm;
  }
  return true;
}

/**
 * Bounded live-only visual history. Every incoming pose sample keeps its actual sim timestamp.
 * Storage is fixed at 64 × 128 × 7 float32 values (224 KiB), plus 64 timestamps.
 * Wall times use performance.now()/RAF milliseconds, never Date.now() or wall seconds.
 */
export class PoseHistory {
  private readonly poses = new Float32Array(POSE_HISTORY_FRAMES * POSE_MAX_BODIES * POSE_FLOATS);
  private readonly times = new Float64Array(POSE_HISTORY_FRAMES);
  private bodies = 0;
  private head = 0;
  private count = 0;
  private generation = 0;
  private receivedWallMs = Number.NaN;
  private sampledWallMs = Number.NaN;
  private presentedTime = Number.NaN;
  private lastPositiveRtf = 1;

  /** Changes on body/reset, invalid input, gaps, and clock discontinuities. */
  get epoch(): number {
    return this.generation;
  }

  /** Clear visual state only. Invalid or absent body counts disable visual sampling. */
  reset(bodies = this.bodies): void {
    this.bodies = Number.isInteger(bodies) && bodies >= 1 && bodies <= POSE_MAX_BODIES ? bodies : 0;
    this.head = 0;
    this.count = 0;
    this.receivedWallMs = Number.NaN;
    this.sampledWallMs = Number.NaN;
    this.presentedTime = Number.NaN;
    this.lastPositiveRtf = 1;
    this.generation = (this.generation + 1) >>> 0;
  }

  /** Copy one complete sample; do not retain an incoming IPC buffer or allocate a frame view. */
  push(t_s: number, poses: Float32Array, offset: number, wallNowMs: number): boolean {
    if (
      !Number.isFinite(t_s) ||
      t_s < 0 ||
      !Number.isFinite(wallNowMs) ||
      !validPoseFrame(poses, offset, this.bodies)
    ) {
      this.reset();
      return false;
    }
    if (this.count > 0) {
      const previous = this.times[this.newestIndex()] as number;
      if (
        t_s <= previous ||
        t_s - previous > POSE_SIM_GAP_S ||
        wallNowMs < this.receivedWallMs ||
        wallNowMs - this.receivedWallMs > POSE_WALL_GAP_MS
      ) {
        this.reset();
      }
    }
    const size = this.bodies * POSE_FLOATS;
    const to = this.head * size;
    for (let lane = 0; lane < size; lane++) this.poses[to + lane] = poses[offset + lane] as number;
    this.times[this.head] = t_s;
    this.head = (this.head + 1) % POSE_HISTORY_FRAMES;
    this.count = Math.min(this.count + 1, POSE_HISTORY_FRAMES);
    this.receivedWallMs = wallNowMs;
    return true;
  }

  /** An explicit paused step presents its completed batch immediately, without changing raw data. */
  finishStep(): void {
    if (this.count > 0) this.presentedTime = this.times[this.newestIndex()] as number;
  }

  /**
   * Call once per render frame, then share that output/time with all models and cameras.
   * The last positive measured RTF drains the 50 ms buffer after pause, then stops at latest.
   * A late frame, overwritten bracket, or backward wall clock snaps to latest and bumps epoch.
   * Invalid output/empty history returns NaN without writing. There is no extrapolation.
   */
  sample(out: Float32Array, wallNowMs: number, measuredRtf: number): number {
    if (
      this.count === 0 ||
      !Number.isFinite(wallNowMs) ||
      out.length < this.bodies * POSE_FLOATS ||
      out.buffer === this.poses.buffer
    )
      return Number.NaN;
    const newest = this.newestIndex();
    let oldest = (this.head - this.count + POSE_HISTORY_FRAMES) % POSE_HISTORY_FRAMES;
    const latest = this.times[newest] as number;
    if (
      (Number.isFinite(this.sampledWallMs) &&
        (wallNowMs < this.sampledWallMs || wallNowMs - this.sampledWallMs > POSE_WALL_GAP_MS)) ||
      (Number.isFinite(this.presentedTime) && this.presentedTime < (this.times[oldest] as number))
    ) {
      // Retain only the newest frame. Never synthesize a path through a renderer freeze.
      this.count = 1;
      oldest = newest;
      this.presentedTime = latest;
      this.generation = (this.generation + 1) >>> 0;
    }
    this.sampledWallMs = wallNowMs;
    if (Number.isFinite(measuredRtf) && measuredRtf > 0) this.lastPositiveRtf = measuredRtf;
    const ageMs = Math.max(0, wallNowMs - this.receivedWallMs);
    const desired = latest + ((ageMs - POSE_DELAY_MS) / 1000) * this.lastPositiveRtf;
    const t = Math.min(
      latest,
      Math.max(
        this.times[oldest] as number,
        Number.isFinite(this.presentedTime) ? this.presentedTime : -Infinity,
        desired,
      ),
    );
    let a = oldest;
    let b = oldest;
    for (let frame = 1; frame < this.count; frame++) {
      b = (oldest + frame) % POSE_HISTORY_FRAMES;
      if ((this.times[b] as number) >= t) break;
      a = b;
    }
    const ta = this.times[a] as number;
    const tb = this.times[b] as number;
    const alpha = tb > ta ? Math.min(1, Math.max(0, (t - ta) / (tb - ta))) : 0;
    const size = this.bodies * POSE_FLOATS;
    if (
      !interpolatePoseFrames(out, this.poses, a * size, this.poses, b * size, this.bodies, alpha)
    ) {
      return Number.NaN;
    }
    this.presentedTime = t;
    return t;
  }

  private newestIndex(): number {
    return (this.head - 1 + POSE_HISTORY_FRAMES) % POSE_HISTORY_FRAMES;
  }
}
