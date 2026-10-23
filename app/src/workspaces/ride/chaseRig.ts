// The Chase camera: behind the rider, a little above, aimed ahead of the wheel. It reads only the
// wheel's position, heading, speed and lean, and the ground, so it never touches the physics.
//
// Method and starting values from EUC Thrills by VibezZzCoder (MIT,
// https://github.com/VibezZzCoder/EUC-thrills, src/render/chaseCamera.ts and CAMERA in
// src/data/tuning.ts): every value follows its target with a first-order lag (no springs, no
// overshoot); the arm stretches and the field of view widens with speed; the yaw lags the heading
// more at walking pace than at speed; the aim leads along the true heading; the camera banks a
// little with the wheel; an obstruction pulls the camera in fast and lets it out slowly. Our own
// addition: the ground height under the wheel is filtered, so potholes and curbs do not shake the
// view (a physics wheel bounces where a game wheel glides). Pure: the rig is tested on its own.

import * as THREE from "three";
import { approach } from "../../lib/rideControls.ts";

export const CHASE = {
  /** Speed at which the arm and the field of view reach their "fast" values, m/s (60 km/h). */
  speedRefMps: 60 / 3.6,
  /** Camera distance behind the wheel, m, at rest and at speed; lag, s. */
  armRest: 4.2,
  armFast: 6.0,
  armTau: 0.55,
  /** Camera height above the ground under the wheel, m. */
  height: 1.95,
  /** Aim height above the ground, m: the rider's hips (about 0.9 m) and 0.35 m. */
  aimHeight: 1.25,
  /** Vertical field of view, degrees, at rest and at speed; lag, s (the slowest). */
  fovRest: 65,
  fovFast: 78,
  fovTau: 0.7,
  /** Yaw lag behind the heading, s, at rest and at speed. */
  yawLagRest: 0.42,
  yawLagFast: 0.14,
  /** The aim leads by this many seconds of forward speed, at most lookAheadMax m; lag, s. */
  lookAheadS: 0.22,
  lookAheadMax: 3.6,
  lookAheadTau: 0.35,
  /** Bank: this share of the wheel's lean, at most bankMax rad; lag, s. */
  bankFactor: 0.15,
  bankMax: 0.1,
  bankTau: 0.18,
  /** Lag of the ground height the camera rides on, s. */
  groundTau: 0.12,
  /** Obstruction: the camera stays this far in front of a hit, m, and never nearer than minArm. */
  obstructionMargin: 0.35,
  minArm: 1.6,
  /** Pull-in and let-out lags, s. */
  pullInTau: 0.05,
  letOutTau: 0.55,
  /** The camera never goes nearer than this to the ground under it, m. */
  floorClearance: 0.4,
  /** Wheel radius, m: the ground under the wheel when no ground can be sampled. */
  wheelRadius: 0.23,
} as const;

export interface ChaseInput {
  /** Wheel centre, three.js coordinates (y up). */
  wheel: THREE.Vector3;
  /** Ground height under the wheel, or null (then wheel height minus the wheel radius). */
  ground: number | null;
  /** Shell heading, rad: 0 along three.js +x, left (toward -z) positive. */
  heading: number;
  /** Wheel speed, m/s, forward positive. */
  speedMps: number;
  /** The shell's up vector in three.js coordinates, or null (no bank). */
  shellUp: THREE.Vector3 | null;
  /**
   * Subject size: 1 frames a rider on the wheel; a wheel alone (the demo, a riderless run) gets
   * about half the arm and height. Default 1.
   */
  scale?: number;
  /** Ground height at a point, for the floor under the camera; absent: no floor. */
  groundAt?: (x: number, z: number) => number | null;
  /**
   * The free distance from `from` toward `to` before a solid, m, or null when the way is clear.
   * Absent: nothing obstructs.
   */
  obstruction?: (from: THREE.Vector3, to: THREE.Vector3) => number | null;
}

const TAU = Math.PI * 2;
const UP = new THREE.Vector3(0, 1, 0);
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
const shortest = (angle: number) => THREE.MathUtils.euclideanModulo(angle + Math.PI, TAU) - Math.PI;

/**
 * An obstruction test against meshes (the world's solids): the distance from `from` toward `to`
 * to the first face, or null when the way is clear. One ray per call.
 */
export function meshObstruction(
  objects: readonly THREE.Object3D[],
): (from: THREE.Vector3, to: THREE.Vector3) => number | null {
  const ray = new THREE.Raycaster();
  const direction = new THREE.Vector3();
  return (from, to) => {
    if (!objects.length) return null;
    direction.subVectors(to, from);
    const length = direction.length();
    if (length < 1e-6) return null;
    ray.set(from, direction.divideScalar(length));
    ray.far = length;
    return ray.intersectObjects(objects as THREE.Object3D[], false)[0]?.distance ?? null;
  };
}

export class ChaseRig {
  /** Camera yaw, rad, unwrapped (it can run past ±π). */
  yaw = 0;
  arm: number = CHASE.armRest;
  fov: number = CHASE.fovRest;
  lookAhead = 0;
  bank = 0;
  base = 0;
  /** The arm length after obstructions, m. */
  reach: number = CHASE.armRest;
  initialized = false;
  readonly position = new THREE.Vector3();
  readonly aim = new THREE.Vector3();
  readonly up = new THREE.Vector3(0, 1, 0);
  private readonly pivot = new THREE.Vector3();
  private readonly wanted = new THREE.Vector3();
  private readonly offset = new THREE.Vector3();
  private readonly right = new THREE.Vector3();

  /** Forces the next step to start from the targets (a new run, a restart, a mode change). */
  reset(): void {
    this.initialized = false;
  }

  /**
   * One frame. With `reduce` (reduced motion) the camera cuts to its targets instead of easing.
   * Returns true while anything still moves toward its target (the low-quality ticker redraws).
   */
  step(input: ChaseInput, dt: number, reduce = false): boolean {
    const s = clamp(Number.isFinite(dt) ? dt : 0, 0, 0.25);
    const v = Number.isFinite(input.speedMps) ? input.speedMps : 0;
    const pace = clamp(Math.abs(v) / CHASE.speedRefMps, 0, 1);
    const heading = Number.isFinite(input.heading) ? input.heading : this.yaw;
    const groundY = input.ground ?? input.wheel.y - CHASE.wheelRadius;
    const size = clamp(input.scale ?? 1, 0.2, 2);
    const armTarget = lerp(CHASE.armRest, CHASE.armFast, pace) * size;
    const fovTarget = lerp(CHASE.fovRest, CHASE.fovFast, pace);
    const leadTarget = v > 0 ? Math.min(CHASE.lookAheadMax, CHASE.lookAheadS * v) : 0;
    this.right.set(Math.sin(heading), 0, Math.cos(heading));
    let roll = 0;
    if (input.shellUp && input.shellUp.lengthSq() > 1e-6)
      roll = Math.atan2(input.shellUp.dot(this.right), input.shellUp.dot(UP));
    const bankTarget = clamp(CHASE.bankFactor * roll, -CHASE.bankMax, CHASE.bankMax);
    const cut = !this.initialized || reduce;
    if (cut) {
      this.yaw = this.initialized ? this.yaw + shortest(heading - this.yaw) : heading;
      this.arm = armTarget;
      this.fov = fovTarget;
      this.lookAhead = leadTarget;
      this.bank = reduce ? 0 : bankTarget;
      this.base = groundY;
    } else {
      // Slow pivots must not whip the camera: a long lag at walking pace, a short one at speed.
      const lag = lerp(CHASE.yawLagRest, CHASE.yawLagFast, pace);
      this.yaw += shortest(heading - this.yaw) * (1 - Math.exp(-s / lag));
      this.arm = approach(this.arm, armTarget, CHASE.armTau, Infinity, s);
      this.fov = approach(this.fov, fovTarget, CHASE.fovTau, Infinity, s);
      this.lookAhead = approach(this.lookAhead, leadTarget, CHASE.lookAheadTau, Infinity, s);
      this.bank = approach(this.bank, bankTarget, CHASE.bankTau, Infinity, s);
      this.base = approach(this.base, groundY, CHASE.groundTau, Infinity, s);
    }
    // The aim leads along the true heading; the camera hangs behind on the lagged one, so in a
    // turn the view already looks into it.
    const fx = Math.cos(this.yaw);
    const fz = -Math.sin(this.yaw);
    this.pivot.set(input.wheel.x, this.base + CHASE.aimHeight * size, input.wheel.z);
    this.aim
      .copy(this.pivot)
      .add(
        this.offset.set(Math.cos(heading), 0, -Math.sin(heading)).multiplyScalar(this.lookAhead),
      );
    this.wanted.set(
      input.wheel.x - fx * this.arm,
      this.base + CHASE.height * size,
      input.wheel.z - fz * this.arm,
    );
    // Obstruction: slide along the arm toward the pivot, fast in, slow out.
    const free = input.obstruction?.(this.pivot, this.wanted) ?? null;
    const length = this.pivot.distanceTo(this.wanted);
    const reachTarget =
      free === null
        ? this.arm
        : clamp(
            ((free - CHASE.obstructionMargin) / length) * this.arm,
            Math.min(CHASE.minArm * size, this.arm),
            this.arm,
          );
    if (cut) this.reach = reachTarget;
    else
      this.reach = approach(
        this.reach,
        reachTarget,
        reachTarget < this.reach ? CHASE.pullInTau : CHASE.letOutTau,
        Infinity,
        s,
      );
    this.reach = Math.min(this.reach, this.arm);
    this.offset.subVectors(this.wanted, this.pivot).multiplyScalar(this.reach / this.arm);
    this.position.copy(this.pivot).add(this.offset);
    const floor = input.groundAt?.(this.position.x, this.position.z) ?? null;
    if (floor !== null && this.position.y < floor + CHASE.floorClearance)
      this.position.y = floor + CHASE.floorClearance;
    // Bank: the camera's up tilts toward the side the wheel leans to.
    this.right.set(Math.sin(this.yaw), 0, Math.cos(this.yaw));
    this.up
      .copy(UP)
      .multiplyScalar(Math.cos(this.bank))
      .addScaledVector(this.right, Math.sin(this.bank));
    this.initialized = true;
    return (
      Math.abs(shortest(heading - this.yaw)) > 1e-3 ||
      Math.abs(this.arm - armTarget) > 1e-3 ||
      Math.abs(this.fov - fovTarget) > 1e-2 ||
      Math.abs(this.lookAhead - leadTarget) > 1e-3 ||
      Math.abs(this.bank - bankTarget) > 1e-4 ||
      Math.abs(this.base - groundY) > 1e-3 ||
      Math.abs(this.reach - reachTarget) > 1e-3
    );
  }

  /** Places a camera: position, up, aim and the field of view (degrees). */
  apply(camera: THREE.PerspectiveCamera): void {
    camera.position.copy(this.position);
    camera.up.copy(this.up);
    camera.lookAt(this.aim);
    if (Math.abs(camera.fov - this.fov) > 1e-3) {
      camera.fov = this.fov;
      camera.updateProjectionMatrix();
    }
  }
}
