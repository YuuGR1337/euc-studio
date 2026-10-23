import { createHash } from "node:crypto";
import { describe, expect, it } from "vitest";
import * as THREE from "three";
import { paintCoolAtlas } from "../../../vendor/euc-thrills/coolRiderAtlas.ts";
import { COOL_SHEET_LAYOUT } from "../../../vendor/euc-thrills/coolRiderLook.ts";
import { RIDER_BLOCKOUT as B } from "../../../vendor/euc-thrills/tuning.ts";
import { GAME_TO_BODY, RiderFigure } from "./figure.ts";
import { ANKLE_ABOVE_PEDAL, articulatedBodies, nodesFor, standArticulated } from "./fixtures.ts";
import { armEaseForSpeed, buildRiderRig } from "./rig.ts";
import { type RiderBinding, bindRider, poseSkeleton, skeletonPose } from "./skeleton.ts";

const world = (object: THREE.Object3D, local: THREE.Vector3) => {
  object.updateWorldMatrix(true, false);
  return local.clone().applyMatrix4(object.matrixWorld);
};

function posed(options: Parameters<typeof standArticulated>[2] = {}) {
  const bodies = articulatedBodies();
  const binding = bindRider(bodies) as RiderBinding;
  const nodes = nodesFor(bodies);
  standArticulated(bodies, nodes, options);
  const pose = skeletonPose();
  expect(poseSkeleton(binding, nodes, pose)).toBe(true);
  const figure = new RiderFigure();
  figure.pose(binding, pose, 1 / 60);
  figure.rig.root.updateMatrixWorld(true);
  return { binding, pose, figure };
}

describe("rider figure", () => {
  it("paints the game's tailoring sheet byte for byte", () => {
    // The hash of the bytes EUC Thrills' own modules paint (commit a66dabd).
    const bytes = paintCoolAtlas(COOL_SHEET_LAYOUT);
    expect(createHash("sha256").update(bytes).digest("hex")).toBe(
      "49b253cbe935aed826eeea96f69cb0f19b24b5871096d658fac0c53b2d7fefe3",
    );
  });

  it("builds every part with vertex colours and normals, and frees it all", () => {
    const rig = buildRiderRig();
    const meshes: THREE.Mesh[] = [];
    rig.root.traverse((o) => {
      if (o instanceof THREE.Mesh) meshes.push(o);
    });
    expect(meshes).toHaveLength(26);
    for (const mesh of meshes) {
      expect(mesh.geometry.getAttribute("color")).toBeDefined();
      expect(mesh.geometry.getAttribute("normal")).toBeDefined();
      expect((mesh.material as THREE.MeshStandardMaterial).vertexColors).toBe(true);
    }
    let freed = 0;
    for (const mesh of meshes) mesh.geometry.addEventListener("dispose", () => freed++);
    rig.dispose();
    expect(freed).toBe(meshes.length);
  });

  it("turns the game's joint frame into the physics body frame", () => {
    const along = (x: number, y: number, z: number) =>
      new THREE.Vector3(x, y, z)
        .applyQuaternion(GAME_TO_BODY)
        .toArray()
        .map((v) => +v.toFixed(9));
    expect(along(1, 0, 0)).toEqual([0, 1, 0]); // the rider's left
    expect(along(0, 1, 0)).toEqual([0, 0, 1]); // up
    expect(along(0, 0, 1)).toEqual([1, 0, 0]); // forward
  });

  it("puts each leg part on its physics bodies: thigh hip to knee, shin knee to ankle", () => {
    const { pose, figure } = posed({ kneeBend: 0.5 });
    for (const [i, leg] of figure.rig.legs.entries()) {
      const at = pose.legs[i];
      if (!at) throw new Error("no leg");
      expect(
        world(leg.thigh, new THREE.Vector3(0, -B.thighLength, 0)).distanceTo(at.knee),
      ).toBeLessThan(1e-6);
      expect(
        world(leg.shin, new THREE.Vector3(0, -B.shinLength, 0)).distanceTo(at.ankle),
      ).toBeLessThan(1e-6);
      // The knee faces forward (physics +x) and the boot's toe too.
      expect(world(leg.thigh, new THREE.Vector3(0, -B.thighLength / 2, 1)).x).toBeGreaterThan(
        at.knee.x,
      );
      expect(world(leg.foot, new THREE.Vector3(0, 0, 1)).x).toBeGreaterThan(at.ankle.x + 0.99);
    }
  });

  it("stands the boots' soles on the pedal tread, below the physics ankle", () => {
    const { pose, figure } = posed();
    for (const [i, leg] of figure.rig.legs.entries()) {
      const box = new THREE.Box3().setFromObject(leg.boot);
      const ankle = pose.legs[i]?.ankle as THREE.Vector3;
      expect(box.min.z).toBeCloseTo(ankle.z - ANKLE_ABOVE_PEDAL, 2);
    }
  });

  it("tops the helmet at the physics head, and keeps part of a forward lean off the head", () => {
    const { binding, figure } = posed();
    const box = new THREE.Box3().setFromObject(figure.rig.neck);
    expect(box.max.z - 1.2).toBeCloseTo(binding.headTop, 2);
    expect(figure.rig.neck.rotation.x).toBeCloseTo(0, 9);
    const leaning = posed({ pitch: 0.3 }).figure;
    expect(leaning.rig.neck.rotation.x).toBeCloseTo(-0.3 * B.headStabilizationFactor, 6);
    const folded = posed({ pitch: 1.2 }).figure;
    expect(folded.rig.neck.rotation.x).toBeCloseTo(-B.headStabilizationMax, 6);
  });

  it("eases the arms at a standstill and carries them at speed", () => {
    const bodies = articulatedBodies();
    const binding = bindRider(bodies) as RiderBinding;
    const nodes = nodesFor(bodies);
    const pose = skeletonPose();
    const figure = new RiderFigure();
    /** The right hand, from the pelvis. */
    const hand = () => {
      figure.rig.root.updateMatrixWorld(true);
      const arm = figure.rig.arms[0];
      if (!arm) throw new Error("no arm");
      return arm.hand.getWorldPosition(new THREE.Vector3()).sub(pose.pelvis);
    };
    const ride = (speed: number, seconds: number) => {
      for (let t = 0; t < seconds; t += 1 / 60) {
        standArticulated(bodies, nodes, { at: new THREE.Vector3(speed * t, 0, 1.2) });
        poseSkeleton(binding, nodes, pose);
        figure.pose(binding, pose, 1 / 60);
      }
    };
    ride(0, 1);
    expect(figure.speedMps).toBeCloseTo(0, 9);
    const still = hand();
    ride(20, 3);
    expect(figure.speedMps).toBeCloseTo(20, 1);
    expect(armEaseForSpeed(figure.speedMps)).toBe(0);
    const riding = hand();
    // Eased at a standstill, the right hand sits nearer the body (the game's eased carriage).
    expect(Math.abs(riding.y)).toBeGreaterThan(Math.abs(still.y) + 0.01);
    expect(figure.rig.arms[0]?.hand.rotation.y).toBe(0);
  });

  it("ignores a jump (a reset or a seek) as a speed", () => {
    const { binding, pose, figure } = posed();
    pose.pelvis.x += 50;
    figure.pose(binding, pose, 1 / 60);
    expect(figure.speedMps).toBe(0);
  });
});
