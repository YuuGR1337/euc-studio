// Synthetic world chunks in the euc_world::render layout, for the world view check. Not a
// generated world: one straight street along world x (asphalt for |y| < 5 m, a dashed centre
// line), curb stones at |y| = 5 m, sidewalks 13.5 cm up beyond them, a 7 cm pothole at (12, 1.5)
// in a wet patch, a building north of the street in every other chunk, and a lamp post per chunk
// on the south sidewalk.

const LODS = [
  { n: 129, m: 256, small: true, props: true },
  { n: 33, m: 32, small: false, props: true },
  { n: 9, m: 8, small: false, props: false },
];
const CHUNK_M = 16;
export const CURB_M = 0.135;
export const POTHOLE = { x: 12, y: 1.5, depth: 0.07, radius: 0.6 };

/** World height at a point, metres. */
export function syntheticHeight(x: number, y: number): number {
  const r = Math.hypot(x - POTHOLE.x, y - POTHOLE.y);
  if (r < POTHOLE.radius) return -POTHOLE.depth * (1 - (r / POTHOLE.radius) ** 2);
  return Math.abs(y) >= 5 ? CURB_M : 0;
}

function material(x: number, y: number): number {
  if (Math.abs(y) >= 5) return 2; // concrete
  if (Math.hypot(x - POTHOLE.x, y - POTHOLE.y) < 1.4) return 0x80; // wet asphalt
  const dash = ((Math.floor(x / 3) % 2) + 2) % 2 === 0;
  return Math.abs(y) < 0.08 && dash ? 8 : 0; // paint, else asphalt
}

/** A payload for `keys` at `lod`, as `euc_world::render::payload` lays it out. */
export function syntheticPayload(keys: [number, number][], lod: number): Buffer {
  const detail = LODS[Math.min(Math.max(lod, 0), 2)] ?? LODS[2];
  if (!detail) throw new Error("No level of detail.");
  const parts: Buffer[] = [];
  const u32 = (...values: number[]) => {
    const b = Buffer.alloc(4 * values.length);
    values.forEach((v, i) => b.writeUInt32LE(v >>> 0, 4 * i));
    parts.push(b);
  };
  const f32 = (...values: number[]) => {
    const b = Buffer.alloc(4 * values.length);
    values.forEach((v, i) => b.writeFloatLE(v, 4 * i));
    parts.push(b);
  };
  parts.push(Buffer.from("EUCW", "latin1"));
  u32(1, keys.length);
  for (const [cx, cy] of keys) {
    const head = Buffer.alloc(8);
    head.writeInt32LE(cx, 0);
    head.writeInt32LE(cy, 4);
    parts.push(head);
    u32(Math.min(Math.max(lod, 0), 2), detail.n, detail.m);
    const [x0, y0] = [cx * CHUNK_M, cy * CHUNK_M];
    const step = CHUNK_M / (detail.n - 1);
    const heights: number[] = [];
    for (let j = 0; j < detail.n; j++)
      for (let i = 0; i < detail.n; i++)
        heights.push(syntheticHeight(x0 + i * step, y0 + j * step));
    f32(...heights);
    const texel = CHUNK_M / detail.m;
    const materials = Buffer.alloc(Math.ceil((detail.m * detail.m) / 4) * 4);
    for (let j = 0; j < detail.m; j++)
      for (let i = 0; i < detail.m; i++)
        materials[j * detail.m + i] = material(x0 + (i + 0.5) * texel, y0 + (j + 0.5) * texel);
    parts.push(materials);
    // Solids this chunk owns (centre inside it): kind, id, centre, half, quaternion (w, x, y, z).
    const solids: number[][] = [];
    if (detail.small && (cy === 0 || cy === -1)) {
      const side = cy === 0 ? 1 : -1;
      solids.push([
        0,
        cx * 2 + (side > 0 ? 1 : 0),
        x0 + 8,
        side * 5.05,
        CURB_M / 2,
        8,
        0.1,
        CURB_M / 2,
      ]);
    }
    if (cy === 0 && cx % 2 === 0) solids.push([4, 1000 + cx, x0 + 8, 13, CURB_M + 5, 6, 2, 5]);
    u32(solids.length);
    for (const [kind = 0, id = 0, ...box] of solids) {
      u32(kind, id, 0);
      f32(...box, 1, 0, 0, 0);
    }
    const lamp = detail.props && cy === -1;
    u32(lamp ? 1 : 0);
    if (lamp) {
      u32(0, 5000 + cx, 0);
      f32(x0 + 4, -7, CURB_M, Math.PI / 2);
    }
  }
  return Buffer.concat(parts);
}
