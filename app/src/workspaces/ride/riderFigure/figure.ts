import * as THREE from "three";
import type { RiderLook } from "../../../vendor/euc-thrills/riderLook.ts";
import { RIDER_BLOCKOUT as B } from "../../../vendor/euc-thrills/tuning.ts";
import { type RiderRig, buildRiderRig, solveChain } from "./rig.ts";
import { STREET_KIT } from "./streetKit.ts";
import type { RiderBinding, SkeletonPose } from "./skeleton.ts";

// The rider figure (rig.ts, in the street kit) posed on the physics skeleton (skeleton.ts). The
// rig's construction is EUC Thrills' (MIT). The physics owns the trunk, thighs, shanks and feet:
// each part of the figure follows its body. Arms, hands and the head move for show only: the
// arms ride bent and loose and balance in a lean (CARRIAGE), and the head keeps part of the
// trunk's pitch off the eyes (the game's headStabilizationFactor).

/** The game's joint frame (+x left, +y up, +z forward) into a physics body frame (x fwd, y left, z up). */
export const GAME_TO_BODY = new THREE.Quaternion(0.5, 0.5, 0.5, 0.5);
/** Speed estimate filter, s: the arms settle over about a second, not on every bump. */
const SPEED_TAU_S = 0.25;
/**
 * The arms' carriage, show only, in the rig's pelvis frame (+x left, +y up from the hip joint,
 * +z forward), metres before the upper body's scale. Bent and loose, the hands a little ahead of
 * the hips with the knuckles out; a little lower and closer at speed. In a lean the high-side
 * arm swings out and up for balance and the low-side hand drops toward its knee.
 */
const CARRIAGE = {
  /** The hands at a standstill: out from the midline, up from the hip joint, forward. */
  slow: { out: 0.25, up: 0.07, forward: 0.11 },
  /** The hands at speed. */
  fast: { out: 0.22, up: 0.02, forward: 0.05 },
  /** The speeds the carriage blends between, m/s. */
  slowBelow: 3,
  fastAbove: 10,
  /** The right hand rides a little further forward: the pair is never a mirror. */
  rightForward: 0.025,
  /** How far the elbows bow out (to the side, against 1 backward). */
  elbowOut: 0.55,
  /** A trunk lean of this much, rad, swings the arms fully. */
  fullLean: 0.25,
  /** At full lean: the high-side hand further out and up, the low-side hand down, in, forward. */
  highOut: 0.16,
  highUp: 0.12,
  lowDown: 0.1,
  lowIn: 0.04,
  lowForward: 0.05,
  /** The lean's filter, s. */
  leanTau: 0.2,
  /** Where the knuckles face: outward, a little forward. */
  knuckles: { out: 1, forward: 0.5 },
} as const;
/** Pelvis motion faster than this between frames is a jump (a reset or a seek), not a speed. */
const JUMP_MPS = 40;

const basis = new THREE.Matrix4();
const axisX = new THREE.Vector3();
const axisY = new THREE.Vector3();
const axisZ = new THREE.Vector3();
const up = new THREE.Vector3();
const ahead = new THREE.Vector3();
const handTarget = new THREE.Vector3();
const bow = new THREE.Vector3();
const knuckles = new THREE.Vector3();
const upper = new THREE.Quaternion();
const lower = new THREE.Quaternion();
const forearm = new THREE.Quaternion();

/**
 * Points a limb group from `from` to `to`: its -y along the bone, its +z as near `forward` as
 * the bone allows. Returns the bone's length.
 */
function aim(
  group: THREE.Object3D,
  from: THREE.Vector3,
  to: THREE.Vector3,
  forward: THREE.Vector3,
) {
  axisY.subVectors(from, to);
  const length = axisY.length();
  if (length < 1e-6) return 0;
  axisY.multiplyScalar(1 / length);
  axisZ.copy(forward).addScaledVector(axisY, -forward.dot(axisY));
  if (axisZ.lengthSq() < 1e-12) axisZ.set(1, 0, 0).addScaledVector(axisY, -axisY.x);
  axisZ.normalize();
  axisX.crossVectors(axisY, axisZ);
  group.position.copy(from);
  group.quaternion.setFromRotationMatrix(basis.makeBasis(axisX, axisY, axisZ));
  return length;
}

export class RiderFigure {
  readonly rig: RiderRig;
  /** Pelvis to helmet top in the look, m: the upper body scales to the physics' head. */
  private readonly lookHeadTop: number;
  private speed = 0;
  /** The trunk's lean to the right, rad, filtered. */
  private lean = 0;
  private readonly last = new THREE.Vector3();
  private hasLast = false;

  constructor(look: RiderLook = STREET_KIT) {
    this.rig = buildRiderRig(look);
    this.lookHeadTop = B.torsoLength + (look.profiles.head.at(-1)?.y ?? 0.34);
  }

  /** Forget the speed and lean history (after a reset or a seek). */
  reset() {
    this.hasLast = false;
    this.speed = 0;
    this.lean = 0;
  }

  /** The filtered horizontal speed the arms ride by, m/s. */
  get speedMps() {
    return this.speed;
  }

  /** The filtered lean the arms balance, rad, + to the rider's right. */
  get leanRad() {
    return this.lean;
  }

  /** Places every part on one frame of the skeleton. `dt` is the frame's time step, s. */
  pose(binding: RiderBinding, pose: SkeletonPose, dt: number) {
    const { rig } = this;
    // Upper body: the game's torso, scaled so its helmet tops out at the physics head.
    const scale = Math.min(1.25, Math.max(0.8, binding.headTop / this.lookHeadTop));
    rig.pelvis.position.copy(pose.pelvis);
    rig.pelvis.quaternion.copy(pose.pelvisQuaternion).multiply(GAME_TO_BODY);
    rig.pelvis.scale.setScalar(scale);

    for (const [i, leg] of rig.legs.entries()) {
      const at = pose.legs[i];
      if (!at) continue;
      const thigh = aim(leg.thigh, at.hip, at.knee, at.thighForward);
      leg.thigh.scale.set(1, thigh / B.thighLength, 1);
      const shin = aim(leg.shin, at.knee, at.ankle, at.shinForward);
      leg.shin.scale.set(1, shin / B.shinLength, 1);
      leg.foot.position.copy(at.ankle);
      leg.foot.quaternion.copy(at.foot).multiply(GAME_TO_BODY);
      // The boot's sole stays on the pedal when the physics ankle sits higher than the game's.
      leg.boot.position.y = B.ankleAbovePedal - binding.ankleAbovePedal;
    }

    this.track(pose.pelvis, pose.pelvisQuaternion, dt);
    this.poseArms();

    // The head keeps part of the trunk's pitch off the eyes, as a rider does.
    up.set(0, 0, 1).applyQuaternion(pose.pelvisQuaternion);
    ahead.set(1, 0, 0).applyQuaternion(pose.pelvisQuaternion).setZ(0);
    const pitch = ahead.lengthSq() > 1e-9 ? Math.atan2(up.dot(ahead.normalize()), up.z) : 0;
    rig.neck.rotation.x = -Math.min(
      B.headStabilizationMax,
      Math.max(-B.headStabilizationMax, pitch * B.headStabilizationFactor),
    );
  }

  private track(pelvis: THREE.Vector3, trunk: THREE.Quaternion, dt: number) {
    // Leaning right lifts the trunk's left (+y) axis.
    const lean = Math.asin(Math.max(-1, Math.min(1, axisX.set(0, 1, 0).applyQuaternion(trunk).z)));
    if (this.hasLast && dt > 0) {
      const moved = Math.hypot(pelvis.x - this.last.x, pelvis.y - this.last.y) / dt;
      if (moved > JUMP_MPS) {
        this.speed = 0;
        this.lean = lean;
      } else {
        this.speed += (moved - this.speed) * (1 - Math.exp(-dt / SPEED_TAU_S));
        this.lean += (lean - this.lean) * (1 - Math.exp(-dt / CARRIAGE.leanTau));
      }
    } else this.lean = lean;
    this.last.copy(pelvis);
    this.hasLast = true;
  }

  /** The carriage (CARRIAGE) at the speed and lean now. */
  private poseArms() {
    const c = CARRIAGE;
    const fast = Math.min(1, Math.max(0, (this.speed - c.slowBelow) / (c.fastAbove - c.slowBelow)));
    const blend = fast * fast * (3 - 2 * fast);
    const lean = Math.max(-1, Math.min(1, this.lean / c.fullLean));
    for (const arm of this.rig.arms) {
      const side = Math.sign(arm.side);
      // Leaning right (lean > 0), the left arm is on the high side.
      const high = Math.max(0, side * lean);
      const low = Math.max(0, -side * lean);
      const target = handTarget.set(
        side *
          (THREE.MathUtils.lerp(c.slow.out, c.fast.out, blend) + c.highOut * high - c.lowIn * low),
        THREE.MathUtils.lerp(c.slow.up, c.fast.up, blend) + c.highUp * high - c.lowDown * low,
        THREE.MathUtils.lerp(c.slow.forward, c.fast.forward, blend) +
          (side < 0 ? c.rightForward : 0) +
          c.lowForward * low,
      );
      solveChain(
        {
          origin: arm.shoulder.position,
          target,
          upperLength: B.upperArmLength,
          lowerLength: B.forearmLength,
          bendToward: bow.set(side * c.elbowOut, 0, -1).normalize(),
        },
        upper,
        lower,
      );
      arm.shoulder.quaternion.copy(upper);
      arm.elbow.quaternion.copy(lower);
      // Turn the hand about the forearm so its knuckles face out and a little forward.
      forearm.copy(upper).multiply(lower).invert();
      knuckles.set(side * c.knuckles.out, 0, c.knuckles.forward).applyQuaternion(forearm);
      arm.hand.rotation.set(0, Math.atan2(knuckles.x, knuckles.z), 0);
    }
  }

  dispose() {
    this.rig.dispose();
  }
}
