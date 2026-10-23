import { describe, expect, it, vi } from "vitest";
import * as THREE from "three";
import { type BodyInfo, POSE_FLOATS } from "../../engine/telemetry.gen.ts";

vi.mock("../../engine/index.ts", () => ({
  engine: { bodies: [], latest: { t: 0, poses: null }, status: {} },
}));
const { placeBodies } = await import("./poses.ts");

const list = (): BodyInfo[] => [
  { name: "wheel", role: "wheel", geoms: [] },
  { name: "rider", role: "rider", geoms: [] },
];

/** One frame: per body a position and a w x y z quaternion. */
function frame(...bodies: [x: number, y: number, z: number, quat?: number[]][]): Float32Array {
  const poses = new Float32Array(bodies.length * POSE_FLOATS);
  bodies.forEach(([x, y, z, quat = [1, 0, 0, 0]], i) =>
    poses.set([x, y, z, ...quat], i * POSE_FLOATS),
  );
  return poses;
}

/** New groups as the studio mounts them: at the origin, unrotated, visible. */
const fresh = () => [new THREE.Group(), new THREE.Group()];

describe("placeBodies", () => {
  it("hides groups that never had a pose of their bodies, so none shows at the origin", () => {
    const groups = fresh();
    placeBodies(groups, list(), null, { bodies: null });
    expect(groups.map((g) => g.visible)).toEqual([false, false]);
  });

  it("poses every group from its frame and shows it", () => {
    const groups = fresh();
    const placed = { bodies: null as readonly BodyInfo[] | null };
    const bodies = list();
    placeBodies(groups, bodies, frame([1, 2, 0.225], [0, 1.2, 1.17, [0, 0, 0, 1]]), placed);
    expect(groups[0]?.position.toArray()).toEqual([1, 2, 0.225].map(Math.fround));
    expect(groups[1]?.position.toArray()).toEqual([0, 1.2, 1.17].map(Math.fround));
    // Poses are w x y z; three.js keeps x y z w.
    expect(groups[1]?.quaternion.toArray()).toEqual([0, 0, 1, 0]);
    expect(groups.map((g) => g.visible)).toEqual([true, true]);
    expect(placed.bodies).toBe(bodies);
  });

  it("keeps the last pose of the same bodies while no frame comes", () => {
    const groups = fresh();
    const placed = { bodies: null as readonly BodyInfo[] | null };
    const bodies = list();
    placeBodies(groups, bodies, frame([3, 0, 0.225], [3, 0, 1.17]), placed);
    placeBodies(groups, bodies, null, placed);
    expect(groups[1]?.position.x).toBe(3);
    expect(groups.map((g) => g.visible)).toEqual([true, true]);
  });

  it("hides the groups of a new body list until a frame of it comes", () => {
    const groups = fresh();
    const placed = { bodies: null as readonly BodyInfo[] | null };
    placeBodies(groups, list(), frame([3, 0, 0.225], [3, 0, 1.17]), placed);
    const next = list();
    placeBodies(groups, next, null, placed);
    expect(groups.map((g) => g.visible)).toEqual([false, false]);
    placeBodies(groups, next, frame([0, 0, 0.225], [0, 0, 1.17]), placed);
    expect(groups.map((g) => g.visible)).toEqual([true, true]);
    expect(groups[1]?.position.z).toBe(Math.fround(1.17));
  });

  it.each([
    ["short", frame([1, 0, 0.225])],
    ["long", frame([1, 0, 0.225], [1, 0, 1.17], [1, 0, 0.3])],
    ["unrotated", frame([1, 0, 0.225], [1, 0, 1.17, [0, 0, 0, 0]])],
    ["not finite", frame([1, 0, 0.225], [Number.NaN, 0, 1.17])],
  ])("takes a %s frame as none", (_, poses) => {
    const groups = fresh();
    const placed = { bodies: null as readonly BodyInfo[] | null };
    const bodies = list();
    placeBodies(groups, bodies, poses, placed);
    expect(groups.map((g) => g.visible)).toEqual([false, false]);
    placeBodies(groups, bodies, frame([2, 0, 0.225], [2, 0, 1.17]), placed);
    placeBodies(groups, bodies, poses, placed);
    expect(groups.map((g) => g.position.x)).toEqual([2, 2]);
    expect(groups.map((g) => g.visible)).toEqual([true, true]);
  });
});
