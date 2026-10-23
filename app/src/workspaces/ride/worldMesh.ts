/**
 * Decodes the engine's world chunk payloads (crates/euc-world/src/render.rs) and builds the
 * arrays the view draws: a ground mesh per chunk with a material texture, merged boxes for its
 * faces (curbs, steps, plaza walls, ramps, buildings) and prop instances. Pure; runs in a worker.
 *
 * Coordinates: a chunk's arrays are local to its corner in three.js axes (x east, y up, z south):
 * (x - x0, z, -(y - y0)). The view places the chunk at its corner.
 */

export const CHUNK_M = 16;

export interface ChunkBox {
  kind: number;
  centre: [number, number, number];
  half: [number, number, number];
  /** w, x, y, z in world axes. */
  quat: [number, number, number, number];
}

export interface ChunkProp {
  kind: number;
  /** World position, metres. */
  x: number;
  y: number;
  z: number;
  facing: number;
}

export interface DecodedChunk {
  cx: number;
  cy: number;
  lod: number;
  /** Height samples per side, and their world heights (rows from min y, x fastest). */
  n: number;
  heights: Float32Array;
  /** Material texels per side: code | 0x80 when wet. */
  m: number;
  materials: Uint8Array;
  boxes: ChunkBox[];
  props: ChunkProp[];
}

export interface ChunkMeshArrays {
  cx: number;
  cy: number;
  lod: number;
  n: number;
  heights: Float32Array;
  positions: Float32Array;
  normals: Float32Array;
  uvs: Float32Array;
  indices: Uint32Array;
  /** RGBA texels, m × m. */
  texture: Uint8Array;
  m: number;
  /** Merged face boxes owned by this chunk (centre inside it): positions, normals, colours. */
  boxPositions: Float32Array;
  boxNormals: Float32Array;
  boxColors: Float32Array;
  boxIndices: Uint32Array;
  props: ChunkProp[];
}

/**
 * Material colours (sRGB), by euc_world::Material code: muted mid-values for daylight under the
 * Neutral tone mapping, no pure white even in paint (EUC Thrills' palette discipline, MIT).
 */
const PALETTE: [number, number, number][] = [
  [114, 118, 124], // asphalt
  [134, 130, 122], // worn asphalt
  [178, 174, 164], // concrete
  [158, 122, 104], // paving stones
  [130, 120, 108], // cobblestones
  [164, 150, 124], // gravel
  [84, 116, 70], // grass
  [128, 102, 76], // dirt
  [226, 224, 216], // paint
  [104, 110, 118], // metal
];
const WET: [number, number, number] = [62, 76, 92];
const ASPHALT: [number, number, number] = [114, 118, 124];
const CONCRETE: [number, number, number] = [178, 174, 164];

/** Box colours (linear 0..1 after sRGB decode in the shader: given as sRGB here). */
const BOX_COLOURS: [number, number, number][] = [
  [180, 176, 166], // curb stone
  [172, 168, 158], // step
  [158, 122, 104], // platform (pavers on top; walls the same)
  [170, 166, 156], // ramp
  [178, 170, 152], // building (varied per id below)
  [120, 120, 120], // prop (not sent as boxes)
];
/** Facade colours a building takes by its position (light plaster, sand, clay, stone, brick). */
const FACADES: [number, number, number][] = [
  [189, 181, 160],
  [186, 168, 148],
  [170, 172, 174],
  [152, 158, 168],
  [172, 150, 134],
  [206, 198, 182],
  [132, 112, 100],
];

/** Reads one payload. Throws on a malformed buffer. */
export function decodePayload(buffer: ArrayBuffer): DecodedChunk[] {
  const view = new DataView(buffer);
  const magic = String.fromCharCode(...new Uint8Array(buffer, 0, 4));
  if (magic !== "EUCW") throw new Error("Not a world payload.");
  if (view.getUint32(4, true) !== 1) throw new Error("Unsupported world payload version.");
  const count = view.getUint32(8, true);
  let at = 12;
  const u32 = () => {
    const v = view.getUint32(at, true);
    at += 4;
    return v;
  };
  const f32 = () => {
    const v = view.getFloat32(at, true);
    at += 4;
    return v;
  };
  const chunks: DecodedChunk[] = [];
  for (let c = 0; c < count; c++) {
    const cx = view.getInt32(at, true);
    const cy = view.getInt32(at + 4, true);
    at += 8;
    const lod = u32();
    const n = u32();
    const m = u32();
    const heights = new Float32Array(buffer.slice(at, at + n * n * 4));
    at += n * n * 4;
    const materials = new Uint8Array(buffer.slice(at, at + m * m));
    at += Math.ceil((m * m) / 4) * 4;
    const boxes: ChunkBox[] = [];
    for (let k = u32(); k > 0; k--) {
      const kind = u32();
      at += 8; // identity
      boxes.push({
        kind,
        centre: [f32(), f32(), f32()],
        half: [f32(), f32(), f32()],
        quat: [f32(), f32(), f32(), f32()],
      });
    }
    const props: ChunkProp[] = [];
    for (let k = u32(); k > 0; k--) {
      const kind = u32();
      at += 8;
      props.push({ kind, x: f32(), y: f32(), z: f32(), facing: f32() });
    }
    chunks.push({ cx, cy, lod, n, heights, m, materials, boxes, props });
  }
  if (at !== buffer.byteLength) throw new Error("World payload has trailing bytes.");
  return chunks;
}

const indexCache = new Map<number, Uint32Array>();

/** Two triangles per cell with the (0,0)-(1,1) diagonal, as the engine's height fields. */
function gridIndices(n: number): Uint32Array {
  const cached = indexCache.get(n);
  if (cached) return cached;
  const out = new Uint32Array((n - 1) * (n - 1) * 6);
  let k = 0;
  for (let j = 0; j < n - 1; j++) {
    for (let i = 0; i < n - 1; i++) {
      const a = j * n + i;
      const b = a + 1;
      const c = a + n;
      const d = c + 1;
      // In three.js axes (z = -y) these wind counter-clockwise seen from above.
      out.set([a, b, d, a, d, c], k);
      k += 6;
    }
  }
  indexCache.set(n, out);
  return out;
}

function hash(a: number, b: number) {
  let h = Math.imul(a ^ 0x9e3779b9, 0x85ebca6b) ^ Math.imul(b ^ 0xc2b2ae35, 0x27d4eb2f);
  h ^= h >>> 15;
  h = Math.imul(h, 0x2c1b3c6d);
  return ((h ^ (h >>> 12)) >>> 0) / 0xffffffff;
}

/** Rotates v by the unit quaternion q (w, x, y, z). */
function rotate(q: [number, number, number, number], v: [number, number, number]) {
  const [w, x, y, z] = q;
  const tx = 2 * (y * v[2] - z * v[1]);
  const ty = 2 * (z * v[0] - x * v[2]);
  const tz = 2 * (x * v[1] - y * v[0]);
  return [
    v[0] + w * tx + (y * tz - z * ty),
    v[1] + w * ty + (z * tx - x * tz),
    v[2] + w * tz + (x * ty - y * tx),
  ] as [number, number, number];
}

const FACES: [number, number, number][] = [
  [1, 0, 0],
  [-1, 0, 0],
  [0, 1, 0],
  [0, -1, 0],
  [0, 0, 1],
  [0, 0, -1],
];

export function buildChunk(chunk: DecodedChunk): ChunkMeshArrays {
  const { n, m, heights } = chunk;
  // Every index below stays inside its array; the reads say so.
  const h = (index: number) => heights[index] ?? 0;
  const x0 = chunk.cx * CHUNK_M;
  const y0 = chunk.cy * CHUNK_M;
  const step = CHUNK_M / (n - 1);
  const positions = new Float32Array(n * n * 3);
  const normals = new Float32Array(n * n * 3);
  const uvs = new Float32Array(n * n * 2);
  for (let j = 0; j < n; j++) {
    for (let i = 0; i < n; i++) {
      const k = j * n + i;
      positions[k * 3] = i * step;
      positions[k * 3 + 1] = h(k);
      positions[k * 3 + 2] = -j * step;
      const hx = h(j * n + Math.min(n - 1, i + 1)) - h(j * n + Math.max(0, i - 1));
      const hy = h(Math.min(n - 1, j + 1) * n + i) - h(Math.max(0, j - 1) * n + i);
      const span = (i > 0 && i < n - 1 ? 2 : 1) * step;
      const spanY = (j > 0 && j < n - 1 ? 2 : 1) * step;
      // Physics normal (-dz/dx, -dz/dy, 1) in three.js axes (x, z, -y).
      const nx = -hx / span;
      const ny = -hy / spanY;
      const length = Math.hypot(nx, ny, 1);
      normals[k * 3] = nx / length;
      normals[k * 3 + 1] = 1 / length;
      normals[k * 3 + 2] = -ny / length;
      uvs[k * 2] = i / (n - 1);
      uvs[k * 2 + 1] = j / (n - 1);
    }
  }
  const texture = new Uint8Array(m * m * 4);
  for (let j = 0; j < m; j++) {
    for (let i = 0; i < m; i++) {
      const code = chunk.materials[j * m + i] ?? 0;
      const base = PALETTE[code & 0x7f] ?? ASPHALT;
      const wet = (code & 0x80) !== 0;
      // A little grain, fixed to the world so it never swims.
      const grain = 0.94 + 0.12 * hash(chunk.cx * m + i, chunk.cy * m + j);
      const k = (j * m + i) * 4;
      for (let c = 0; c < 3; c++) {
        const value = wet ? (base[c] ?? 0) * 0.55 + (WET[c] ?? 0) * 0.45 : (base[c] ?? 0);
        texture[k + c] = Math.max(0, Math.min(255, Math.round(value * grain)));
      }
      texture[k + 3] = 255;
    }
  }
  // Boxes this chunk owns (centre inside it): 24 vertices, 36 indices each.
  const owned = chunk.boxes.filter(
    (b) =>
      b.centre[0] >= x0 &&
      b.centre[0] < x0 + CHUNK_M &&
      b.centre[1] >= y0 &&
      b.centre[1] < y0 + CHUNK_M,
  );
  const boxPositions = new Float32Array(owned.length * 24 * 3);
  const boxNormals = new Float32Array(owned.length * 24 * 3);
  const boxColors = new Float32Array(owned.length * 24 * 3);
  const boxIndices = new Uint32Array(owned.length * 36);
  owned.forEach((box, b) => {
    let colour = BOX_COLOURS[box.kind] ?? CONCRETE;
    if (box.kind === 4) {
      const t = hash(Math.round(box.centre[0] * 10), Math.round(box.centre[1] * 10));
      const base = FACADES[Math.floor(t * FACADES.length)] ?? CONCRETE;
      const shade =
        0.92 + 0.16 * hash(Math.round(box.centre[1] * 10), Math.round(box.centre[0] * 10));
      colour = [base[0] * shade, base[1] * shade, base[2] * shade];
    }
    FACES.forEach((face, f) => {
      const [u, v] = face[0] !== 0 ? [1, 2] : face[1] !== 0 ? [0, 2] : [0, 1];
      const normal = rotate(box.quat, face);
      const corners: [number, number][] = [
        [-1, -1],
        [1, -1],
        [1, 1],
        [-1, 1],
      ];
      corners.forEach(([su, sv], c) => {
        const local: [number, number, number] = [
          face[0] * box.half[0],
          face[1] * box.half[1],
          face[2] * box.half[2],
        ];
        local[u] = su * (box.half[u] ?? 0);
        local[v] = sv * (box.half[v] ?? 0);
        const world = rotate(box.quat, local);
        const k = (b * 24 + f * 4 + c) * 3;
        boxPositions[k] = box.centre[0] + world[0] - x0;
        boxPositions[k + 1] = box.centre[2] + world[2];
        boxPositions[k + 2] = -(box.centre[1] + world[1] - y0);
        boxNormals[k] = normal[0];
        boxNormals[k + 1] = normal[2];
        boxNormals[k + 2] = -normal[1];
        boxColors[k] = (colour[0] / 255) ** 2.2;
        boxColors[k + 1] = (colour[1] / 255) ** 2.2;
        boxColors[k + 2] = (colour[2] / 255) ** 2.2;
      });
      // Winding: the corners run counter-clockwise about u × v, which is +x, -y and +z for the
      // three face pairs; the other faces reverse. The map to three.js axes keeps handedness.
      const base = b * 24 + f * 4;
      const flip = face[0] < 0 || face[1] > 0 || face[2] < 0;
      boxIndices.set(
        flip
          ? [base, base + 2, base + 1, base, base + 3, base + 2]
          : [base, base + 1, base + 2, base, base + 2, base + 3],
        b * 36 + f * 6,
      );
    });
  });
  return {
    cx: chunk.cx,
    cy: chunk.cy,
    lod: chunk.lod,
    n,
    heights,
    positions,
    normals,
    uvs,
    indices: gridIndices(n),
    texture,
    m,
    boxPositions,
    boxNormals,
    boxColors,
    boxIndices,
    props: chunk.props,
  };
}
