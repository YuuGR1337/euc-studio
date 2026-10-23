import { describe, expect, it } from "vitest";
import * as THREE from "three";
import {
  ANKLE_ABOVE_PEDAL,
  PEDAL_Y,
  RIDER_HEIGHT,
  SEGMENT,
  articulatedBodies,
  nodesFor,
  rigidBodies,
  standArticulated,
} from "./fixtures.ts";
import { FIGURE_HIP_HALF_WIDTH, bindRider, poseSkeleton, skeletonPose } from "./skeleton.ts";

describe("rider binding", () => {
  it("binds the articulated rider: the ankle height and the head from the body shapes", () => {
    const binding = bindRider(articulatedBodies());
    expect(binding?.kind).toBe("articulated");
    expect(binding?.ankleAbovePedal).toBeCloseTo(ANKLE_ABOVE_PEDAL, 6);
    // mjcf.rs: the head sphere sits at h - ankle - 2L - 0.1 above the hips, radius 0.1.
    expect(binding?.headTop).toBeCloseTo(RIDER_HEIGHT - ANKLE_ABOVE_PEDAL - 2 * SEGMENT, 3);
  });

  it("binds the rigid rider: hips at 0.53 of the height, feet on the pedals", () => {
    const binding = bindRider(rigidBodies());
    if (binding?.kind !== "rigid") throw new Error("not bound as rigid");
    expect(binding.hipHeight).toBeCloseTo(0.53 * RIDER_HEIGHT - ANKLE_ABOVE_PEDAL, 6);
    expect(binding.pedalY).toBeCloseTo(PEDAL_Y, 6);
    expect(binding.ankleAbovePedal).toBeCloseTo(ANKLE_ABOVE_PEDAL, 6);
    expect(binding.headTop).toBeCloseTo(0.4 * RIDER_HEIGHT + 0.1, 6);
  });

  it("binds no other body list (the scene then draws no rider)", () => {
    const old = articulatedBodies().map((b) => (b.name === "rider" ? { ...b, name: "torso" } : b));
    expect(bindRider(old)).toBeNull();
    expect(bindRider(articulatedBodies().filter((b) => b.name !== "left_foot"))).toBeNull();
    expect(bindRider([{ name: "wheel", role: "wheel", geoms: [] }])).toBeNull();
  });
});

describe("skeleton pose", () => {
  it("takes the articulated joints from their bodies, the hips inside the jacket", () => {
    const bodies = articulatedBodies();
    const binding = bindRider(bodies);
    if (!binding) throw new Error("unbound");
    const nodes = nodesFor(bodies);
    const at = new THREE.Vector3(3, -2, 1.2);
    standArticulated(bodies, nodes, { at, kneeBend: 0.4 });
    const pose = skeletonPose();
    expect(poseSkeleton(binding, nodes, pose)).toBe(true);
    expect(pose.pelvis.toArray()).toEqual(at.toArray());
    const [right, left] = pose.legs;
    expect(left.hip.toArray()).toEqual([3, -2 + FIGURE_HIP_HALF_WIDTH, 1.2]);
    expect(right.hip.y).toBeCloseTo(-2 - FIGURE_HIP_HALF_WIDTH, 9);
    const leftShank = nodes[bodies.findIndex((b) => b.name === "left_shank")] as THREE.Object3D;
    expect(left.knee.toArray()).toEqual(leftShank.position.toArray());
    expect(left.knee.x).toBeGreaterThan(at.x);
    expect(left.ankle.distanceTo(left.knee)).toBeCloseTo(SEGMENT, 9);
  });

  it("puts the hips on the thighs' origins when they sit near the midline (the lateral rider)", () => {
    const bodies = articulatedBodies();
    const binding = bindRider(bodies);
    if (!binding) throw new Error("unbound");
    const nodes = nodesFor(bodies);
    const at = new THREE.Vector3(3, -2, 1.2);
    standArticulated(bodies, nodes, { at, hipY: 0.09, pitch: 0.2 });
    const pose = skeletonPose();
    expect(poseSkeleton(binding, nodes, pose)).toBe(true);
    const [right, left] = pose.legs;
    const thigh = (side: string) =>
      (nodes[bodies.findIndex((b) => b.name === `${side}_thigh`)] as THREE.Object3D).position;
    expect(left.hip.toArray()).toEqual(thigh("left").toArray());
    expect(right.hip.toArray()).toEqual(thigh("right").toArray());
    expect(left.hip.y - at.y).toBeCloseTo(0.09, 9);
  });

  it("is not posed while a body it needs has no pose", () => {
    const bodies = articulatedBodies();
    const binding = bindRider(bodies);
    if (!binding) throw new Error("unbound");
    const nodes = nodesFor(bodies);
    standArticulated(bodies, nodes);
    (nodes[bodies.findIndex((b) => b.name === "right_foot")] as THREE.Object3D).visible = false;
    expect(poseSkeleton(binding, nodes, skeletonPose())).toBe(false);
  });

  it("bends the rigid rider's knees forward, softly, its feet on the pedals", () => {
    const bodies = rigidBodies();
    const binding = bindRider(bodies);
    if (binding?.kind !== "rigid") throw new Error("not bound as rigid");
    const nodes = nodesFor(bodies);
    const rider = nodes[2] as THREE.Object3D;
    rider.visible = true;
    rider.position.set(1, 1, 0.3);
    // Heading +90 degrees: the rider's forward is world +y.
    rider.quaternion.setFromAxisAngle(new THREE.Vector3(0, 0, 1), Math.PI / 2);
    const pose = skeletonPose();
    expect(poseSkeleton(binding, nodes, pose)).toBe(true);
    for (const leg of pose.legs) {
      expect(leg.ankle.z).toBeCloseTo(0.3, 9);
      expect(leg.ankle.x).toBeCloseTo(1 - leg.side * PEDAL_Y, 9);
      expect(leg.hip.distanceTo(leg.knee)).toBeCloseTo(leg.knee.distanceTo(leg.ankle), 9);
      // Forward of the straight line from hip to ankle, by a soft bend (< 10 cm).
      const middle = leg.hip.clone().lerp(leg.ankle, 0.5);
      expect(leg.knee.y - middle.y).toBeGreaterThan(0.04);
      expect(leg.knee.y - middle.y).toBeLessThan(0.1);
      expect(leg.thighForward.y).toBeCloseTo(1, 9);
    }
  });
});
