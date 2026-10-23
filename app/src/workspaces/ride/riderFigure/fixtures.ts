import * as THREE from "three";
import type { BodyInfo, Geom } from "../../../engine/telemetry.gen.ts";

// Body lists as the engine sends them, for the figure tests. The articulated list is copied from
// a recording of the default rider (1.78 m, ankle 0.08 m above the pedal, Begode Extreme pedals
// 0.1763 m out); the rigid one follows crates/euc-physics/src/mjcf.rs rider() for the same rider.

const geom = (kind: Geom["kind"], size: number[], pos: number[], quat = [1, 0, 0, 0]): Geom => ({
  kind,
  size: size as Geom["size"],
  pos: pos as Geom["pos"],
  quat: quat as Geom["quat"],
});

export const RIDER_HEIGHT = 1.78;
export const ANKLE_ABOVE_PEDAL = 0.08;
export const PEDAL_Y = 0.1763;
/** The articulated legs' segment length (mjcf.rs: 0.245 of the height). */
export const SEGMENT = 0.4361;

const limb = (radius: number) => [geom("capsule", [radius, SEGMENT / 2, 0], [0, 0, -SEGMENT / 2])];
const sole = [geom("box", [0.12, 0.045, 0.03], [0.03, 0, 0.03 - ANKLE_ABOVE_PEDAL])];

export function articulatedBodies(): BodyInfo[] {
  return [
    { name: "wheel", role: "wheel", geoms: [] },
    { name: "shell", role: "shell", geoms: [] },
    {
      name: "rider",
      role: "rider",
      geoms: [
        geom("capsule", [0.13, 0.2848, 0], [0, 0, 0.2848], [6.123234e-17, 1, 0, 0]),
        geom("sphere", [0.1, 0, 0], [0, 0, 0.7278]),
        geom("capsule", [0.045, 0.2581, 0], [0, 0.28208, 0.2493]),
        geom("capsule", [0.045, 0.2581, 0], [0, -0.28208, 0.2493]),
      ],
    },
    { name: "left_thigh", role: "rider", geoms: limb(0.06) },
    { name: "left_shank", role: "rider", geoms: limb(0.045) },
    { name: "left_foot", role: "rider", geoms: sole },
    { name: "right_thigh", role: "rider", geoms: limb(0.06) },
    { name: "right_shank", role: "rider", geoms: limb(0.045) },
    { name: "right_foot", role: "rider", geoms: sole },
  ];
}

export function rigidBodies(): BodyInfo[] {
  const h = RIDER_HEIGHT;
  const a = ANKLE_ABOVE_PEDAL;
  const [hip, shoulder, head] = [0.53 * h - a, 0.82 * h - a, 0.93 * h - a];
  const geoms: Geom[] = [];
  for (const y of [PEDAL_Y, -PEDAL_Y]) {
    geoms.push(geom("box", [0.12, 0.045, 0.03], [0.03, y, 0.03 - a]));
    const leg = new THREE.Vector3(0, -0.4 * y, hip);
    geoms.push(geom("capsule", [0.07, leg.length() / 2, 0], [0, 0.8 * y, hip / 2]));
    geoms.push(
      geom("capsule", [0.045, (shoulder - hip) / 2, 0], [0, 1.6 * y, (hip + shoulder) / 2]),
    );
  }
  geoms.push(geom("capsule", [0.15, (shoulder - hip) / 2, 0], [0, 0, (hip + shoulder) / 2]));
  geoms.push(geom("sphere", [0.1, 0, 0], [0, 0, head]));
  return [
    { name: "wheel", role: "wheel", geoms: [] },
    { name: "shell", role: "shell", geoms: [] },
    { name: "rider", role: "rider", geoms },
  ];
}

/** Body groups as LiveBodies places them: one Object3D per body, visible once posed. */
export function nodesFor(bodies: readonly BodyInfo[]): THREE.Object3D[] {
  return bodies.map((b) => {
    const node = new THREE.Object3D();
    node.name = `body:${b.name}`;
    return node;
  });
}

/**
 * Poses the articulated bodies standing on the pedals: pelvis `pitch` rad forward about the hip
 * axis, legs straight down from the hips (knees and ankles on the physics' own frames), the
 * whole rider at `at` with heading 0.
 */
export function standArticulated(
  bodies: readonly BodyInfo[],
  nodes: THREE.Object3D[],
  { pitch = 0, at = new THREE.Vector3(0, 0, 1.2), kneeBend = 0 } = {},
) {
  const index = (name: string) => bodies.findIndex((b) => b.name === name);
  const trunk = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), pitch);
  const place = (name: string, position: THREE.Vector3, quaternion: THREE.Quaternion) => {
    const node = nodes[index(name)] as THREE.Object3D;
    node.position.copy(position);
    node.quaternion.copy(quaternion);
    node.visible = true;
  };
  place("rider", at, trunk);
  // The thigh swings forward by half the knee bend and the shank back by the other half: the
  // knee leads and the foot stays under the hip.
  const thighQ = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), -kneeBend / 2);
  const shankQ = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), kneeBend / 2);
  for (const [side, y] of [
    ["left", PEDAL_Y],
    ["right", -PEDAL_Y],
  ] as const) {
    const hip = new THREE.Vector3(0, y, 0).add(at);
    const knee = new THREE.Vector3(0, 0, -SEGMENT).applyQuaternion(thighQ).add(hip);
    const ankle = new THREE.Vector3(0, 0, -SEGMENT).applyQuaternion(shankQ).add(knee);
    place(`${side}_thigh`, hip, thighQ);
    place(`${side}_shank`, knee, shankQ);
    place(`${side}_foot`, ankle, new THREE.Quaternion());
  }
}
