import * as THREE from "three";
import { engine } from "../../engine/index.ts";
import type { CameraMode } from "../../state/ui.ts";
import { type GroundView, sampleGround } from "./ground.ts";
import { visualFrame, visualShellHeading, visualWheelPosition } from "./poses.ts";
import { slopeAt } from "./terrain.ts";

const UP = new THREE.Vector3(0, 1, 0);
const TAU = Math.PI * 2;
export const INITIAL_CAMERA: [number, number, number] = [-3.6, 2.15, 2.2];

/** Exact critically damped spring for a fixed target over this frame. No frame-rate-dependent lerp. */
export function spring(
  value: number,
  velocity: number,
  target: number,
  omega: number,
  dt: number,
): [number, number] {
  const error = value - target;
  const impulse = (velocity + omega * error) * dt;
  const decay = Math.exp(-omega * dt);
  return [target + (error + impulse) * decay, (velocity - omega * impulse) * decay];
}

/** Separate springs for the offset and aim. Translation follows the same visual pose as the model. */
export class ChaseCamera {
  readonly anchor = new THREE.Vector3();
  readonly previousAnchor = new THREE.Vector3();
  readonly aim = new THREE.Vector3();
  readonly aimVelocity = new THREE.Vector3();
  readonly offset = new THREE.Vector3();
  readonly offsetVelocity = new THREE.Vector3();
  readonly wantedOffset = new THREE.Vector3();
  readonly wantedAim = new THREE.Vector3();
  readonly delta = new THREE.Vector3();
  readonly forward = new THREE.Vector3();
  readonly direction = new THREE.Vector3();
  readonly wheel = new THREE.Vector3();
  readonly slope = { z: 0, pitch: 0 };
  yaw = 0;
  yawVelocity = 0;
  speed = 0;
  speedVelocity = 0;
  heightVelocity = 0;
  generation = -1;
  source: typeof engine | null = null;
  mode: CameraMode | null = null;
  framedRider: boolean | null = null;
  initialized = false;

  groundHeight(x: number, z: number, map?: GroundView | null): number | null {
    if (map) return sampleGround(map, x, -z)?.height_m ?? null;
    if (engine.kind === "playback" && !engine.terrain) return null;
    if (engine.status.runMode === "free_ride" && !engine.terrain) return null;
    return slopeAt(engine.terrain, x, this.slope).z;
  }

  target(map?: GroundView | null): boolean {
    const rider = engine.bodies.some((body) => body.role === "rider");
    if (engine.kind === "demo" || !visualWheelPosition(this.wheel)) {
      this.anchor.set(0.02, 0.34, 0);
    } else {
      const floor = this.groundHeight(this.wheel.x, this.wheel.z, map);
      this.anchor.set(
        this.wheel.x,
        (floor ?? this.wheel.y - 0.23) + (rider ? 0.9 : 0.34),
        this.wheel.z,
      );
    }
    return rider;
  }

  follow(camera: THREE.Camera, dt: number, reduce: boolean, map?: GroundView | null): boolean {
    const rider = this.target(map);
    const frame = visualFrame();
    const reset =
      !this.initialized || this.source !== engine || this.generation !== frame.generation;
    const entering = this.mode === "orbit";
    const heading = visualShellHeading(this.yaw);
    const speed = Number.isFinite(engine.latest.speed) ? engine.latest.speed : 0;
    const step = Math.min(Math.max(dt, 0), 0.25);
    if (reset || reduce) {
      this.yaw = heading;
      this.yawVelocity = 0;
      this.speed = speed;
      this.speedVelocity = 0;
    } else {
      // Follow the shortest turn across ±pi. A falling wheel keeps its last stable heading.
      const turn = THREE.MathUtils.euclideanModulo(heading - this.yaw + Math.PI, TAU) - Math.PI;
      [this.yaw, this.yawVelocity] = spring(this.yaw, this.yawVelocity, this.yaw + turn, 6, step);
      [this.speed, this.speedVelocity] = spring(this.speed, this.speedVelocity, speed, 4, step);
    }
    this.forward.set(Math.cos(this.yaw), 0, -Math.sin(this.yaw));
    const pace = THREE.MathUtils.smoothstep(Math.abs(this.speed), 0, 65);
    const distance = rider ? 3.6 + 1.1 * pace : 1.9 + 0.7 * pace;
    const side = rider ? 1.8 : 1.05;
    const height = rider ? 1.3 + 0.15 * pace : 0.75 + 0.1 * pace;
    this.wantedOffset.set(-distance, height, side).applyAxisAngle(UP, this.yaw);
    // Look along the road, not along body pitch or roll. Reverse changes the look-ahead, not the camera side.
    const lead = THREE.MathUtils.clamp((this.speed / 3.6) * 0.055, -0.65, rider ? 1.1 : 0.55);
    this.wantedAim.copy(this.anchor).addScaledVector(this.forward, lead);

    if (reset || reduce) {
      this.offset.copy(this.wantedOffset);
      this.offsetVelocity.set(0, 0, 0);
      this.aim.copy(this.wantedAim);
      this.aimVelocity.set(0, 0, 0);
      this.heightVelocity = 0;
      camera.position.copy(this.anchor).add(this.offset);
    } else {
      if (entering) {
        // Start from the user's actual orbit view, including its aim. Switching modes never teleports.
        this.offset.subVectors(camera.position, this.anchor);
        camera.getWorldDirection(this.direction);
        this.aim
          .copy(camera.position)
          .addScaledVector(this.direction, camera.position.distanceTo(this.anchor));
        this.offsetVelocity.set(0, 0, 0);
        this.aimVelocity.set(0, 0, 0);
      } else {
        this.delta.subVectors(this.anchor, this.previousAnchor);
        // Feed horizontal movement forward. A world-position spring would lag metres behind at speed.
        camera.position.x += this.delta.x;
        camera.position.z += this.delta.z;
        this.aim.x += this.delta.x;
        this.aim.z += this.delta.z;
      }
      for (const axis of ["x", "y", "z"] as const) {
        [this.offset[axis], this.offsetVelocity[axis]] = spring(
          this.offset[axis],
          this.offsetVelocity[axis],
          this.wantedOffset[axis],
          6,
          step,
        );
        [this.aim[axis], this.aimVelocity[axis]] = spring(
          this.aim[axis],
          this.aimVelocity[axis],
          this.wantedAim[axis],
          axis === "y" ? 5 : 7,
          step,
        );
      }
      camera.position.x = this.anchor.x + this.offset.x;
      camera.position.z = this.anchor.z + this.offset.z;
      [camera.position.y, this.heightVelocity] = spring(
        camera.position.y,
        this.heightVelocity,
        Math.max(
          this.anchor.y + this.offset.y,
          (this.groundHeight(camera.position.x, camera.position.z, map) ?? -Infinity) + 0.4,
        ),
        5,
        step,
      );
    }
    const cameraFloor = this.groundHeight(camera.position.x, camera.position.z, map);
    const heightGoal = Math.max(this.anchor.y + this.offset.y, (cameraFloor ?? -Infinity) + 0.4);
    if (cameraFloor !== null && camera.position.y < cameraFloor + 0.4) {
      camera.position.y = cameraFloor + 0.4;
      this.heightVelocity = Math.max(0, this.heightVelocity);
    }
    camera.up.copy(UP);
    camera.lookAt(this.aim);
    this.previousAnchor.copy(this.anchor);
    this.source = engine;
    this.generation = frame.generation;
    this.initialized = true;
    this.mode = "follow";
    this.framedRider = rider;
    return (
      this.offset.distanceToSquared(this.wantedOffset) > 0.00001 ||
      this.aim.distanceToSquared(this.wantedAim) > 0.00001 ||
      Math.abs(camera.position.y - heightGoal) > 0.002 ||
      Math.abs(this.yawVelocity) > 0.002 ||
      Math.abs(this.speedVelocity) > 0.02 ||
      frame.time < engine.latest.t - 0.0001
    );
  }
}
