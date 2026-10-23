import type * as THREE from "three";
import { engine } from "../../engine/index.ts";
import { validPoseFrame } from "../../engine/poseHistory.ts";
import { type BodyInfo, POSE_FLOATS } from "../../engine/telemetry.gen.ts";

/** The wheel's position in three.js coordinates (y up), or null before the first snapshot. */
export function wheelPosition(out: THREE.Vector3): THREE.Vector3 | null {
  const p = engine.latest.poses;
  const i = engine.bodies.findIndex((b) => b.role === "wheel");
  if (!p || i < 0) return null;
  const o = i * POSE_FLOATS;
  // MuJoCo (x, y, z) → three.js (x, z, -y).
  return out.set(p[o] as number, p[o + 2] as number, -(p[o + 1] as number));
}

/** Shell heading avoids the rotor spin present in the wheel quaternion. */
export function shellHeading(): number {
  const poses = engine.latest.poses;
  const body = engine.bodies.findIndex((info) => info.role === "shell");
  if (!poses || body < 0) return 0;
  const offset = body * POSE_FLOATS + 3;
  const [w, x, y, z] = Array.from(poses.subarray(offset, offset + 4)) as [
    number,
    number,
    number,
    number,
  ];
  if (![w, x, y, z].every(Number.isFinite)) return 0;
  return Math.atan2(2 * (x * y + w * z), 1 - 2 * (y * y + z * z));
}

// The scene samples this once before any body, light, shadow, or camera frame callback.
// Raw engine.latest remains the source for instruments and recorded measurements.
const rendered = {
  source: null as typeof engine | null,
  bodies: engine.bodies,
  epoch: undefined as number | undefined,
  poses: null as Float32Array | null,
  time: 0,
  generation: 0,
  lastWall: 0,
};

export function prepareVisualFrame(nowWallMs: number): void {
  const wasReady = rendered.poses !== null;
  const raw = engine.latest.poses;
  const epoch = engine.poseEpoch ?? engine.status.runEpoch;
  const changed =
    rendered.source !== engine || rendered.bodies !== engine.bodies || rendered.epoch !== epoch;
  if (changed || (rendered.lastWall && nowWallMs - rendered.lastWall > 500)) {
    rendered.generation++;
  }
  rendered.source = engine;
  rendered.bodies = engine.bodies;
  rendered.epoch = epoch;
  rendered.lastWall = nowWallMs;
  if (!raw || raw.length !== engine.bodies.length * POSE_FLOATS || !raw.length) {
    rendered.poses = null;
    rendered.time = engine.latest.t;
    return;
  }
  if (!rendered.poses || rendered.poses.length !== raw.length)
    rendered.poses = new Float32Array(raw.length);
  const previous = rendered.time;
  if (engine.samplePoses) {
    const time = engine.samplePoses(rendered.poses, nowWallMs);
    if (!Number.isFinite(time)) {
      rendered.poses = null;
      return;
    }
    rendered.time = time;
  } else {
    rendered.poses.set(raw);
    rendered.time = engine.latest.t;
  }
  if (!wasReady) rendered.generation++;
  if (!changed && rendered.time < previous - 0.0001) rendered.generation++;
  // The sampler can discover a discontinuity during this call.
  const sampledEpoch = engine.poseEpoch ?? engine.status.runEpoch;
  if (sampledEpoch !== rendered.epoch) {
    rendered.epoch = sampledEpoch;
    rendered.generation++;
  }
}

export function visualFrame(): Readonly<typeof rendered> {
  return rendered;
}

/**
 * Poses one group per body from `poses` (a frame of exactly these bodies, or null). A group
 * shows only a pose of its own body list: until one arrives it is hidden, never drawn at the
 * origin, and without a frame it keeps the last pose it showed. `placed` remembers that list.
 */
export function placeBodies(
  groups: readonly (THREE.Object3D | null | undefined)[],
  bodies: readonly BodyInfo[],
  poses: Float32Array | null,
  placed: { bodies: readonly BodyInfo[] | null },
): void {
  if (
    !poses ||
    poses.length !== bodies.length * POSE_FLOATS ||
    !validPoseFrame(poses, 0, bodies.length)
  ) {
    if (placed.bodies !== bodies) for (const g of groups) if (g) g.visible = false;
    return;
  }
  for (let i = 0; i < bodies.length; i++) {
    const g = groups[i];
    if (!g) continue;
    const o = i * POSE_FLOATS;
    g.position.set(poses[o] as number, poses[o + 1] as number, poses[o + 2] as number);
    // Poses are w x y z; three.js quaternions are x y z w.
    g.quaternion.set(
      poses[o + 4] as number,
      poses[o + 5] as number,
      poses[o + 6] as number,
      poses[o + 3] as number,
    );
    g.visible = true;
  }
  placed.bodies = bodies;
}

export function visualWheelPosition(out: THREE.Vector3): THREE.Vector3 | null {
  const poses = rendered.source === engine ? rendered.poses : null;
  const body = engine.bodies.findIndex((info) => info.role === "wheel");
  if (!poses || body < 0) return null;
  const offset = body * POSE_FLOATS;
  return out.set(
    poses[offset] as number,
    poses[offset + 2] as number,
    -(poses[offset + 1] as number),
  );
}

/** Ignore a tumble's unstable yaw. The chase camera keeps the last upright road heading. */
export function visualShellHeading(fallback: number): number {
  const poses = rendered.source === engine ? rendered.poses : null;
  const body = engine.bodies.findIndex((info) => info.role === "shell");
  if (!poses || body < 0) return fallback;
  const offset = body * POSE_FLOATS;
  const w = poses[offset + 3] as number;
  const x = poses[offset + 4] as number;
  const y = poses[offset + 5] as number;
  const z = poses[offset + 6] as number;
  const up = 1 - 2 * (x * x + y * y);
  if (up < 0.4) return fallback;
  const forwardX = 1 - 2 * (y * y + z * z);
  const forwardY = 2 * (x * y + w * z);
  return Math.hypot(forwardX, forwardY) < 0.3 ? fallback : Math.atan2(forwardY, forwardX);
}
