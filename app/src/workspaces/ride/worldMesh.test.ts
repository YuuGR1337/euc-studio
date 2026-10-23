import { describe, expect, it } from "vitest";
import { DEFAULT_DENSITIES } from "../../engine/world.ts";
import { buildChunk, CHUNK_M, decodePayload } from "./worldMesh.ts";
import { WorldView } from "./worldView.ts";

/** A payload as crates/euc-world/src/render.rs writes it: one chunk, one box, one prop. */
function payload(n: number, m: number, height: (i: number, j: number) => number): ArrayBuffer {
  const bytes: number[] = [];
  const u32 = (v: number) => {
    const b = new DataView(new ArrayBuffer(4));
    b.setUint32(0, v, true);
    bytes.push(...new Uint8Array(b.buffer));
  };
  const i32 = (v: number) => {
    const b = new DataView(new ArrayBuffer(4));
    b.setInt32(0, v, true);
    bytes.push(...new Uint8Array(b.buffer));
  };
  const f32 = (v: number) => {
    const b = new DataView(new ArrayBuffer(4));
    b.setFloat32(0, v, true);
    bytes.push(...new Uint8Array(b.buffer));
  };
  bytes.push(..."EUCW".split("").map((c) => c.charCodeAt(0)));
  u32(1);
  u32(1);
  i32(1);
  i32(-1);
  u32(0);
  u32(n);
  u32(m);
  for (let j = 0; j < n; j++) for (let i = 0; i < n; i++) f32(height(i, j));
  for (let k = 0; k < m * m; k++) bytes.push(k === 0 ? 0x80 | 8 : 0);
  while (bytes.length % 4) bytes.push(0);
  u32(1); // one box: a curb stone at the chunk's centre, axis-aligned
  u32(0);
  u32(1);
  u32(0);
  [24, -8, 0.05, 1, 0.075, 0.1, 1, 0, 0, 0].forEach(f32);
  u32(1); // one prop: a lamp post
  u32(0);
  u32(2);
  u32(0);
  [20, -10, 0, 0.5].forEach(f32);
  return new Uint8Array(bytes).buffer;
}

describe("world chunk payloads", () => {
  it("decode into heights, materials, boxes and props", () => {
    const [chunk] = decodePayload(payload(3, 2, (i, j) => i + 10 * j));
    expect(chunk).toMatchObject({ cx: 1, cy: -1, lod: 0, n: 3, m: 2 });
    expect(Array.from(chunk?.heights ?? [])).toEqual([0, 1, 2, 10, 11, 12, 20, 21, 22]);
    expect(chunk?.materials[0]).toBe(0x88);
    expect(chunk?.boxes[0]?.centre).toEqual([24, -8, 0.05000000074505806]);
    expect(chunk?.props[0]).toMatchObject({ kind: 0, x: 20, y: -10, z: 0 });
  });

  it("refuse a buffer that is not a world payload or has trailing bytes", () => {
    expect(() => decodePayload(new ArrayBuffer(12))).toThrow();
    const good = new Uint8Array(payload(2, 1, () => 0));
    const longer = new Uint8Array(good.length + 4);
    longer.set(good);
    expect(() => decodePayload(longer.buffer)).toThrow(/trailing/);
  });

  it("build an upward ground and outward box faces in three.js axes", () => {
    const [decoded] = decodePayload(payload(3, 2, () => 0));
    if (!decoded) throw new Error("no chunk");
    const mesh = buildChunk(decoded);
    // Flat ground: every normal points up (three.js +y).
    for (let k = 0; k < 9; k++) expect(mesh.normals[k * 3 + 1]).toBeCloseTo(1);
    // Corner 2 (i = 2, j = 0) sits 16 m east, at the chunk's south edge (z = 0).
    expect(Array.from(mesh.positions.slice(6, 9))).toEqual([CHUNK_M, 0, -0]);
    // Each box triangle winds counter-clockwise about its outward normal.
    for (let t = 0; t < mesh.boxIndices.length; t += 3) {
      const [a = 0, b = 0, c = 0] = [0, 1, 2].map((o) => mesh.boxIndices[t + o] ?? 0);
      const p = (i: number) => [0, 1, 2].map((k) => mesh.boxPositions[i * 3 + k] ?? 0);
      const [pa, pb, pc] = [p(a), p(b), p(c)];
      const u = [0, 1, 2].map((k) => (pb[k] ?? 0) - (pa[k] ?? 0));
      const v = [0, 1, 2].map((k) => (pc[k] ?? 0) - (pa[k] ?? 0));
      const cross = [
        (u[1] ?? 0) * (v[2] ?? 0) - (u[2] ?? 0) * (v[1] ?? 0),
        (u[2] ?? 0) * (v[0] ?? 0) - (u[0] ?? 0) * (v[2] ?? 0),
        (u[0] ?? 0) * (v[1] ?? 0) - (u[1] ?? 0) * (v[0] ?? 0),
      ];
      const normal = [0, 1, 2].map((k) => mesh.boxNormals[a * 3 + k] ?? 0);
      expect(cross.reduce((sum, c, k) => sum + c * (normal[k] ?? 0), 0)).toBeGreaterThan(0);
    }
  });
});

describe("world view", () => {
  const frame = {
    spec: { seed: 1, revision: 2, biome: "city" as const, densities: DEFAULT_DENSITIES },
    start_xy_m: [20, -12] as [number, number],
    start_heading_rad: Math.PI / 2,
    origin_z_m: 3,
  };

  it("turns physics points into world points and back", () => {
    const view = new WorldView(frame);
    const [wx, wy] = view.toWorld(1, 0);
    expect(wx).toBeCloseTo(20);
    expect(wy).toBeCloseTo(-11);
    const [px, py] = view.toPhysics(wx, wy);
    expect(px).toBeCloseTo(1);
    expect(py).toBeCloseTo(0);
  });

  it("samples loaded heights in the physics frame, with the slope turned with it", () => {
    const view = new WorldView(frame);
    // Chunk (1, -1): a 10 % rise toward world +x.
    const n = 3;
    view.setGrid(
      1,
      -1,
      0,
      n,
      Float32Array.from({ length: n * n }, (_, k) => 3 + (k % n) * 0.8),
    );
    const at = view.sample(0, 0);
    expect(at?.height_m).toBeCloseTo((20 - 16) * 0.1);
    // World +x is physics -y for a start heading of +90°: the normal leans toward physics +y.
    expect(at?.normal_world[1]).toBeGreaterThan(0.09);
    expect(view.sample(400, 0)).toBeNull();
  });
});
