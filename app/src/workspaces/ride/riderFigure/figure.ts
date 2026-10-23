import * as THREE from "three";
import { COOL_RIDER_LOOK } from "../../../vendor/euc-thrills/coolRiderLook.ts";
import type { RiderLook } from "../../../vendor/euc-thrills/riderLook.ts";
import { RIDER_BLOCKOUT as B } from "../../../vendor/euc-thrills/tuning.ts";
import { type RiderRig, armEaseForSpeed, buildRiderRig, solveChain } from "./rig.ts";
import type { RiderBinding, SkeletonPose } from "./skeleton.ts";

// The EUC Thrills rider (rig.ts) posed on the physics skeleton (skeleton.ts). The physics owns
// the trunk, thighs, shanks and feet: each part of the figure follows its body. Arms, hands and
// the head move cosmetically, the game's way: the arms hang in its carriage and ease at low
// speed (armEaseForSpeed), and the head keeps part of the trunk's pitch off the eyes
// (headStabilizationFactor).

/** The game's joint frame (+x left, +y up, +z forward) into a physics body frame (x fwd, y left, z up). */
export const GAME_TO_BODY = new THREE.Quaternion(0.5, 0.5, 0.5, 0.5);
/** Speed estimate filter, s: the arms ease over about a second, not on every bump. */
const SPEED_TAU_S = 0.25;
/** Pelvis motion faster than this between frames is a jump (a reset or a seek), not a speed. */
const JUMP_MPS = 40;

const basis = new THREE.Matrix4();
const axisX = new THREE.Vector3();
const axisY = new THREE.Vector3();
const axisZ = new THREE.Vector3();
const up = new THREE.Vector3();
const ahead = new THREE.Vector3();
const handTarget = new THREE.Vector3();
const upper = new THREE.Quaternion();
const lower = new THREE.Quaternion();

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
  private readonly last = new THREE.Vector3();
  private hasLast = false;

  constructor(look: RiderLook = COOL_RIDER_LOOK) {
    this.rig = buildRiderRig(look);
    this.lookHeadTop = B.torsoLength + (look.profiles.head.at(-1)?.y ?? 0.34);
  }

  /** Forget the speed history (after a reset or a seek). */
  reset() {
    this.hasLast = false;
    this.speed = 0;
  }

  /** The filtered horizontal speed the arms ease by, m/s. */
  get speedMps() {
    return this.speed;
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

    this.track(pose.pelvis, dt);
    this.poseArms(armEaseForSpeed(this.speed));

    // The head keeps part of the trunk's pitch off the eyes, as a rider does.
    up.set(0, 0, 1).applyQuaternion(pose.pelvisQuaternion);
    ahead.set(1, 0, 0).applyQuaternion(pose.pelvisQuaternion).setZ(0);
    const pitch = ahead.lengthSq() > 1e-9 ? Math.atan2(up.dot(ahead.normalize()), up.z) : 0;
    rig.neck.rotation.x = -Math.min(
      B.headStabilizationMax,
      Math.max(-B.headStabilizationMax, pitch * B.headStabilizationFactor),
    );
  }

  private track(pelvis: THREE.Vector3, dt: number) {
    if (this.hasLast && dt > 0) {
      const moved = Math.hypot(pelvis.x - this.last.x, pelvis.y - this.last.y) / dt;
      if (moved > JUMP_MPS) this.speed = 0;
      else this.speed += (moved - this.speed) * (1 - Math.exp(-dt / SPEED_TAU_S));
    }
    this.last.copy(pelvis);
    this.hasLast = true;
  }

  /** The game's arm carriage, eased toward the standstill pose by `ease` (0..1). */
  private poseArms(ease: number) {
    for (const arm of this.rig.arms) {
      const sign = Math.sign(arm.side);
      const target = handTarget.set(
        sign * THREE.MathUtils.lerp(arm.baseSplay, arm.easedSplay, ease),
        THREE.MathUtils.lerp(arm.baseTarget.y, arm.easedTarget.y, ease),
        THREE.MathUtils.lerp(arm.baseTarget.z, arm.easedTarget.z, ease),
      );
      // Eased, the elbows bow out and back and the hands turn toward the thighs.
      const bowOut = arm.side < 0 ? B.armEaseElbowOutRight : B.armEaseElbowOut;
      arm.hand.rotation.y = ease > 0 ? sign * B.armEaseHandRoll * ease : 0;
      solveChain(
        {
          origin: arm.shoulder.position,
          target,
          upperLength: B.upperArmLength,
          lowerLength: B.forearmLength,
          bendToward: axisX.set(sign * bowOut * ease, 0, -1).normalize(),
        },
        upper,
        lower,
      );
      arm.shoulder.quaternion.copy(upper);
      arm.elbow.quaternion.copy(lower);
    }
  }

  dispose() {
    this.rig.dispose();
  }
}
