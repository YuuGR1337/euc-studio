import * as THREE from "three";
import type { BodyInfo, Geom } from "../../../engine/telemetry.gen.ts";
import { RIDER_BLOCKOUT } from "../../../vendor/euc-thrills/tuning.ts";

// Where the rider figure's joints are, read from the physics bodies (physics world frame: x
// forward, y left, z up). Two riders exist (crates/euc-physics/src/mjcf.rs):
// - articulated: "rider" (pelvis to head, its origin on the hip axis) and per side
//   <side>_thigh, <side>_shank, <side>_foot, each origin on its joint (hip, knee, ankle);
// - rigid: one "rider" body, its origin between the ankles, with the legs, torso and head as
//   fixed shapes. Its knees are placed by two-link IK, softly bent.
// Any other body list (old recordings) has no binding, and the scene draws no rider.

/**
 * The figure's hip joints, from the midline, m: inside the hips of its clothes. The sagittal
 * rider's thighs hang from its pedals' width; a model whose thighs sit nearer the midline (the
 * lateral rider's, at its hip joints) gives its own.
 */
export const FIGURE_HIP_HALF_WIDTH = RIDER_BLOCKOUT.torsoWidth * 0.26;
/** A thigh body this near the midline is on a real hip joint: the figure's hip goes there. */
const HIP_FROM_THIGH_M = 0.12;
/** How far a rigid rider's legs are extended: a soft knee, about 20 degrees of bend. */
const RIGID_LEG_EXTENSION = 0.985;
const SIDES = [-1, 1] as const;

export type RiderBinding =
  | {
      readonly kind: "articulated";
      readonly rider: number;
      /** Body indices per side, right (-1) then left (+1). */
      readonly thigh: readonly [number, number];
      readonly shank: readonly [number, number];
      readonly foot: readonly [number, number];
      /** The physics ankle joint above the pedal tread, m. */
      readonly ankleAbovePedal: number;
      /** The head's top above the pelvis (hip axis), m. */
      readonly headTop: number;
    }
  | {
      readonly kind: "rigid";
      readonly rider: number;
      readonly ankleAbovePedal: number;
      readonly headTop: number;
      /** The hip axis above the rider body's origin (the ankle axis), m. */
      readonly hipHeight: number;
      /** Each foot's distance from the centreline, m. */
      readonly pedalY: number;
    };

const near = (a: number, b: number) => Math.abs(a - b) < 1e-6;
const top = (g: Geom) => g.pos[2] + (g.kind === "sphere" ? g.size[0] : 0);

/** The figure binding of a body list, or null when it is neither rider. */
export function bindRider(bodies: readonly BodyInfo[]): RiderBinding | null {
  const index = (name: string) => bodies.findIndex((b) => b.name === name && b.role === "rider");
  const rider = index("rider");
  if (rider < 0) return null;
  const head = bodies[rider]?.geoms.find((g) => g.kind === "sphere");
  if (!head) return null;
  const sides = (part: string) => {
    const right = index(`right_${part}`);
    const left = index(`left_${part}`);
    return right >= 0 && left >= 0 ? ([right, left] as const) : null;
  };
  const thigh = sides("thigh");
  const shank = sides("shank");
  const foot = sides("foot");
  if (thigh && shank && foot) {
    const sole = bodies[foot[1]]?.geoms.find((g) => g.kind === "box");
    if (!sole) return null;
    // mjcf.rs: the sole box sits at z = 0.03 - ankle_above_pedals.
    return {
      kind: "articulated",
      rider,
      thigh,
      shank,
      foot,
      ankleAbovePedal: 0.03 - sole.pos[2],
      headTop: top(head),
    };
  }
  if (bodies.some((b) => b.role === "rider" && b.name !== "rider")) return null;
  // mjcf.rs rider(): foot boxes at (0.03, ±pedal_y, 0.03 - ankle), the torso capsule from the hip
  // to the shoulders on the centreline, the head sphere above.
  const geoms = bodies[rider]?.geoms ?? [];
  const feet = geoms.filter((g) => g.kind === "box");
  const torso = geoms.find((g) => g.kind === "capsule" && near(g.pos[1], 0));
  const left = feet.find((g) => g.pos[1] > 0);
  if (feet.length !== 2 || !torso || !left) return null;
  const hipHeight = torso.pos[2] - torso.size[1];
  return {
    kind: "rigid",
    rider,
    ankleAbovePedal: 0.03 - left.pos[2],
    headTop: top(head) - hipHeight,
    hipHeight,
    pedalY: left.pos[1],
  };
}

export interface LegPose {
  /** -1 right, +1 left. */
  readonly side: number;
  /** The figure's hip joint, world. */
  readonly hip: THREE.Vector3;
  readonly knee: THREE.Vector3;
  readonly ankle: THREE.Vector3;
  /** The foot's frame (sole flat when level), world. */
  readonly foot: THREE.Quaternion;
  /** Forward (+x) of the thigh and of the shin, world: the side a knee bends to. */
  readonly thighForward: THREE.Vector3;
  readonly shinForward: THREE.Vector3;
}

/** One frame of the skeleton, preallocated and filled in place by `poseSkeleton`. */
export interface SkeletonPose {
  /** The pelvis frame: origin on the hip axis, x forward, y left, z up. */
  readonly pelvis: THREE.Vector3;
  readonly pelvisQuaternion: THREE.Quaternion;
  /** Right (-1) then left (+1). */
  readonly legs: readonly [LegPose, LegPose];
}

export function skeletonPose(): SkeletonPose {
  const leg = (side: number): LegPose => ({
    side,
    hip: new THREE.Vector3(),
    knee: new THREE.Vector3(),
    ankle: new THREE.Vector3(),
    foot: new THREE.Quaternion(),
    thighForward: new THREE.Vector3(1, 0, 0),
    shinForward: new THREE.Vector3(1, 0, 0),
  });
  return {
    pelvis: new THREE.Vector3(),
    pelvisQuaternion: new THREE.Quaternion(),
    legs: [leg(-1), leg(1)],
  };
}

const X = new THREE.Vector3(1, 0, 0);
const local = new THREE.Vector3();
const inverse = new THREE.Quaternion();
const reach = new THREE.Vector3();

/** A body's group, when it is placed (visible) this frame. */
type Nodes = readonly (THREE.Object3D | null | undefined)[];
const placed = (nodes: Nodes, i: number) => {
  const node = nodes[i];
  return node && node.visible ? node : null;
};

/**
 * Fills `out` from the placed body groups (each holds its body's world pose). False when a body
 * the binding needs has no pose this frame; `out` is then partly written and must not be drawn.
 */
export function poseSkeleton(binding: RiderBinding, nodes: Nodes, out: SkeletonPose): boolean {
  const rider = placed(nodes, binding.rider);
  if (!rider) return false;
  const q = rider.quaternion;
  if (binding.kind === "articulated") {
    out.pelvis.copy(rider.position);
    out.pelvisQuaternion.copy(q);
    for (let i = 0; i < 2; i++) {
      const thigh = placed(nodes, binding.thigh[i] as number);
      const shank = placed(nodes, binding.shank[i] as number);
      const foot = placed(nodes, binding.foot[i] as number);
      const leg = out.legs[i] as LegPose;
      if (!thigh || !shank || !foot) return false;
      local.copy(thigh.position).sub(rider.position).applyQuaternion(inverse.copy(q).invert());
      if (Math.abs(local.y) <= HIP_FROM_THIGH_M) leg.hip.copy(thigh.position);
      else
        leg.hip
          .set(0, leg.side * FIGURE_HIP_HALF_WIDTH, 0)
          .applyQuaternion(q)
          .add(rider.position);
      leg.knee.copy(shank.position);
      leg.ankle.copy(foot.position);
      leg.foot.copy(foot.quaternion);
      leg.thighForward.copy(X).applyQuaternion(thigh.quaternion);
      leg.shinForward.copy(X).applyQuaternion(shank.quaternion);
    }
    return true;
  }
  // Rigid: everything in the rider body's frame, then into the world.
  out.pelvis.set(0, 0, binding.hipHeight).applyQuaternion(q).add(rider.position);
  out.pelvisQuaternion.copy(q);
  for (const [i, side] of SIDES.entries()) {
    const leg = out.legs[i] as LegPose;
    const hip = local.set(0, side * FIGURE_HIP_HALF_WIDTH, binding.hipHeight);
    reach.set(0, side * binding.pedalY, 0).sub(hip);
    const length = reach.length();
    const segment = length / (2 * RIGID_LEG_EXTENSION);
    // Equal segments: the knee sits over the leg's middle, pushed forward (+x, across the leg).
    const bulge = Math.sqrt(Math.max(0, segment * segment - (length / 2) ** 2));
    leg.knee
      .copy(hip)
      .addScaledVector(reach, 0.5)
      .add(reach.set(bulge, 0, 0))
      .applyQuaternion(q)
      .add(rider.position);
    leg.hip.copy(hip).applyQuaternion(q).add(rider.position);
    leg.ankle
      .set(0, side * binding.pedalY, 0)
      .applyQuaternion(q)
      .add(rider.position);
    leg.foot.copy(q);
    leg.thighForward.copy(X).applyQuaternion(q);
    leg.shinForward.copy(X).applyQuaternion(q);
  }
  return true;
}
