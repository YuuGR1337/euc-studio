import { describe, expect, it } from "vitest";
import * as THREE from "three";
import { buildMapSurface } from "./mapGeometry.ts";
import { sampleMap, validateMap, type HeightMapView } from "./mapSurface.ts";

function asymmetricCell(): HeightMapView {
  const heights = new Int16Array(129 * 129);
  // Cell (84,80), world (5,4): a non-planar saddle away from the level spawn.
  heights[80 * 129 + 84] = -100;
  heights[80 * 129 + 85] = 100;
  heights[81 * 129 + 84] = 50;
  heights[81 * 129 + 85] = 0;
  return {
    revision: 1,
    nx: 129,
    ny: 129,
    spacing_m: 0.25,
    origin_xy_m: [-16, -16],
    heights_mm: heights,
  };
}

describe("generated terrain rendering", () => {
  it("renders both collision triangles at their true height with upward winding", () => {
    const map = asymmetricCell();
    expect(validateMap(map)).toBeNull();
    const geometry = buildMapSurface(map);
    const material = new THREE.MeshBasicMaterial({ side: THREE.FrontSide });
    const mesh = new THREE.Mesh(geometry.surface, material);
    try {
      // Independent barycentric results for triangle 00-10-11 and 00-11-01.
      for (const [u, v, height] of [
        [0.72, 0.19, 0.025],
        [0.21, 0.76, 0.0035],
        [0.1, 0.05, -0.085],
      ] as const) {
        const x = 5 + u * 0.25;
        const y = 4 + v * 0.25;
        const ray = new THREE.Raycaster(new THREE.Vector3(x, 20, -y), new THREE.Vector3(0, -1, 0));
        const hits = ray.intersectObject(mesh);
        expect(hits).toHaveLength(1);
        const hit = hits[0];
        const sample = sampleMap(map, x, y);
        if (!hit?.face || !sample) throw new Error("Surface or normal missing");
        expect(hit.point.y).toBeCloseTo(height, 6);
        expect(sample.height_m).toBeCloseTo(height, 6);
        const normal = sample.normal_world;
        expect(hit.face.normal.toArray()).toEqual([
          expect.closeTo(normal[0], 6),
          expect.closeTo(normal[2], 6),
          expect.closeTo(-normal[1], 6),
        ]);
      }
      const outside = new THREE.Raycaster(
        new THREE.Vector3(16.01, 20, 0),
        new THREE.Vector3(0, -1, 0),
      );
      expect(outside.intersectObject(mesh)).toHaveLength(0);
    } finally {
      geometry.surface.dispose();
      geometry.grid.dispose();
      material.dispose();
    }
  });

  it("keeps every metre-grid endpoint on the same finite surface", () => {
    const map = asymmetricCell();
    const geometry = buildMapSurface(map);
    try {
      const positions = geometry.grid.getAttribute("position");
      expect(positions.count).toBe(16896);
      for (let i = 0; i < positions.count; i += 2) {
        expect(
          Math.hypot(
            positions.getX(i + 1) - positions.getX(i),
            positions.getZ(i + 1) - positions.getZ(i),
          ),
        ).toBe(0.25);
      }
      for (let i = 0; i < positions.count; i++) {
        const sample = sampleMap(map, positions.getX(i), -positions.getZ(i));
        if (!sample) throw new Error(`Grid endpoint ${i} is outside the map`);
        expect(positions.getY(i)).toBeCloseTo(sample.height_m, 6);
      }
    } finally {
      geometry.surface.dispose();
      geometry.grid.dispose();
    }
  });
});
