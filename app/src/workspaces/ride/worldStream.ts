import * as THREE from "three";
import { worldApi } from "../../engine/world.ts";
import { withContactAo } from "./outdoor/contactAo.ts";
import type { Quality } from "./quality.ts";
import { CHUNK_M, type ChunkMeshArrays, type ChunkProp } from "./worldMesh.ts";
import { WorldView } from "./worldView.ts";
import type { WorkerReply } from "./worldWorker.ts";

/** Rings around the wheel: chunk centres within `radius` metres get level of detail `lod`. */
const RINGS: Record<Quality, { lod: number; radius: number }[]> = {
  high: [
    { lod: 0, radius: 30 },
    { lod: 1, radius: 104 },
  ],
  low: [
    { lod: 0, radius: 18 },
    { lod: 1, radius: 56 },
  ],
};
/** Chunks per request at each level: the near ring fills first and quickly. */
const BATCH = [4, 24];
/** How often the wanted set is recomputed, seconds. */
const REFRESH_S = 0.25;

interface Loaded {
  lod: number;
  ground: THREE.Mesh;
  texture: THREE.DataTexture;
  boxes: THREE.Mesh | null;
  props: ChunkProp[];
}

/** Prop kinds (crates/euc-world/src/render.rs `prop_code`): shapes and colours. */
const PROP_SHAPES: { geometry: THREE.BufferGeometry; colour: string; lift: number }[][] = [
  // Lamp post: a 7 m pole and its head.
  [
    { geometry: new THREE.CylinderGeometry(0.06, 0.08, 7, 8), colour: "#4A4E54", lift: 3.5 },
    { geometry: new THREE.BoxGeometry(0.5, 0.12, 0.22), colour: "#E8E2C8", lift: 6.95 },
  ],
  // Tree: trunk and canopy.
  [
    { geometry: new THREE.CylinderGeometry(0.12, 0.16, 3, 8), colour: "#5A4430", lift: 1.5 },
    { geometry: new THREE.IcosahedronGeometry(1.9, 1), colour: "#3F6436", lift: 4.2 },
  ],
  // Bench, bin, hydrant, bollard.
  [{ geometry: new THREE.BoxGeometry(1.8, 0.45, 0.55), colour: "#5C4330", lift: 0.45 }],
  [{ geometry: new THREE.CylinderGeometry(0.25, 0.25, 0.9, 12), colour: "#3E4247", lift: 0.45 }],
  [{ geometry: new THREE.CylinderGeometry(0.12, 0.14, 0.7, 10), colour: "#A8352B", lift: 0.35 }],
  [{ geometry: new THREE.CylinderGeometry(0.09, 0.09, 0.9, 10), colour: "#2E3034", lift: 0.45 }],
];

/**
 * Streams a world into a three.js group: requests chunks around the wheel from the engine, builds
 * them in a worker, keeps two rings of detail, and drops what falls behind. The group's children
 * are in world axes (x east, y up, z south); the caller places the group in the physics frame.
 */
export class WorldStream {
  readonly view: WorldView;
  private readonly root: THREE.Group;
  private readonly quality: Quality;
  private readonly worker: Worker;
  private readonly loaded = new Map<string, Loaded>();
  /** Requests in flight, per level. */
  private readonly inFlight = [false, false];
  private readonly pending = new Map<number, number>();
  private nextId = 1;
  private sinceRefresh = REFRESH_S;
  private readonly groundTemplate = new THREE.MeshStandardMaterial({
    roughness: 0.93,
    metalness: 0,
  });
  private readonly boxMaterial = new THREE.MeshStandardMaterial({
    vertexColors: true,
    roughness: 0.85,
    metalness: 0,
  });
  private readonly propGroup = new THREE.Group();
  private readonly propMaterials = PROP_SHAPES.map((parts) =>
    parts.map((part) => new THREE.MeshStandardMaterial({ color: part.colour, roughness: 0.8 })),
  );
  private propsDirty = false;
  private disposed = false;
  /** Called when something new is drawn (low quality redraws on demand). */
  onChange: (() => void) | null = null;
  /** The last request error, for the status line; null when streaming works. */
  error: string | null = null;

  constructor(view: WorldView, root: THREE.Group, quality: Quality) {
    this.view = view;
    this.root = root;
    this.quality = quality;
    this.root.add(this.propGroup);
    this.worker = new Worker(new URL("./worldWorker.ts", import.meta.url), { type: "module" });
    this.worker.addEventListener("message", (event: MessageEvent<WorkerReply>) =>
      this.receive(event.data),
    );
  }

  dispose() {
    this.disposed = true;
    this.worker.terminate();
    for (const [key] of this.loaded) this.drop(key);
    this.clearProps();
    this.root.remove(this.propGroup);
    this.groundTemplate.dispose();
    this.boxMaterial.dispose();
    for (const parts of this.propMaterials) for (const m of parts) m.dispose();
  }

  /** Call every frame with the wheel's physics-frame position (null before the first pose). */
  update(dt: number, wheel: [number, number] | null) {
    this.sinceRefresh += dt;
    if (this.propsDirty) this.rebuildProps();
    if (this.sinceRefresh < REFRESH_S || !wheel) return;
    this.sinceRefresh = 0;
    const [wx, wy] = this.view.toWorld(wheel[0], wheel[1]);
    const rings = RINGS[this.quality];
    const outer = Math.max(...rings.map((r) => r.radius));
    const wanted = new Map<string, { cx: number; cy: number; lod: number; distance: number }>();
    const lo = (v: number) => Math.floor((v - outer) / CHUNK_M);
    const hi = (v: number) => Math.floor((v + outer) / CHUNK_M);
    for (let cy = lo(wy); cy <= hi(wy); cy++) {
      for (let cx = lo(wx); cx <= hi(wx); cx++) {
        const distance = Math.hypot((cx + 0.5) * CHUNK_M - wx, (cy + 0.5) * CHUNK_M - wy);
        const ring = rings.find((r) => distance <= r.radius);
        if (ring) wanted.set(WorldView.key(cx, cy), { cx, cy, lod: ring.lod, distance });
      }
    }
    for (const key of [...this.loaded.keys()]) if (!wanted.has(key)) this.drop(key);
    for (let lod = 0; lod < BATCH.length; lod++) {
      if (this.inFlight[lod]) continue;
      const missing = [...wanted.values()]
        .filter((w) => w.lod === lod && this.loaded.get(WorldView.key(w.cx, w.cy))?.lod !== lod)
        .sort((a, b) => a.distance - b.distance)
        .slice(0, BATCH[lod]);
      if (missing.length)
        this.request(
          lod,
          missing.map((w) => [w.cx, w.cy] as [number, number]),
        );
    }
  }

  private request(lod: number, keys: [number, number][]) {
    this.inFlight[lod] = true;
    const id = this.nextId++;
    this.pending.set(id, lod);
    worldApi
      .chunks(this.view.frame.spec, keys, lod)
      .then((buffer) => {
        if (this.disposed) return;
        this.worker.postMessage({ id, buffer }, [buffer]);
      })
      .catch((error: unknown) => {
        this.pending.delete(id);
        this.inFlight[lod] = false;
        this.error = error instanceof Error ? error.message : String(error);
      });
  }

  private receive(reply: WorkerReply) {
    const lod = this.pending.get(reply.id);
    this.pending.delete(reply.id);
    if (lod !== undefined) this.inFlight[lod] = false;
    if (this.disposed) return;
    if ("error" in reply) {
      this.error = reply.error;
      return;
    }
    this.error = null;
    for (const chunk of reply.chunks) this.add(chunk);
    this.onChange?.();
  }

  private add(chunk: ChunkMeshArrays) {
    const key = WorldView.key(chunk.cx, chunk.cy);
    this.drop(key);
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(chunk.positions, 3));
    geometry.setAttribute("normal", new THREE.BufferAttribute(chunk.normals, 3));
    geometry.setAttribute("uv", new THREE.BufferAttribute(chunk.uvs, 2));
    geometry.setIndex(new THREE.BufferAttribute(chunk.indices, 1));
    geometry.computeBoundingSphere();
    const texture = new THREE.DataTexture(chunk.texture, chunk.m, chunk.m, THREE.RGBAFormat);
    texture.colorSpace = THREE.SRGBColorSpace;
    texture.magFilter = chunk.lod === 0 ? THREE.NearestFilter : THREE.LinearFilter;
    texture.minFilter = THREE.LinearMipmapLinearFilter;
    texture.generateMipmaps = true;
    texture.anisotropy = 4;
    texture.needsUpdate = true;
    const material = withContactAo(this.groundTemplate.clone());
    material.map = texture;
    const ground = new THREE.Mesh(geometry, material);
    ground.name = `world-chunk-${key}`;
    ground.position.set(chunk.cx * CHUNK_M, 0, -chunk.cy * CHUNK_M);
    ground.receiveShadow = true;
    this.root.add(ground);
    let boxes: THREE.Mesh | null = null;
    if (chunk.boxIndices.length) {
      const g = new THREE.BufferGeometry();
      g.setAttribute("position", new THREE.BufferAttribute(chunk.boxPositions, 3));
      g.setAttribute("normal", new THREE.BufferAttribute(chunk.boxNormals, 3));
      g.setAttribute("color", new THREE.BufferAttribute(chunk.boxColors, 3));
      g.setIndex(new THREE.BufferAttribute(chunk.boxIndices, 1));
      g.computeBoundingSphere();
      boxes = new THREE.Mesh(g, this.boxMaterial);
      boxes.name = `world-solids-${key}`;
      boxes.position.copy(ground.position);
      boxes.receiveShadow = true;
      boxes.castShadow = true;
      this.root.add(boxes);
    }
    this.loaded.set(key, { lod: chunk.lod, ground, texture, boxes, props: chunk.props });
    this.view.setGrid(chunk.cx, chunk.cy, chunk.lod, chunk.n, chunk.heights);
    this.propsDirty = true;
  }

  private drop(key: string) {
    const entry = this.loaded.get(key);
    if (!entry) return;
    this.root.remove(entry.ground);
    entry.ground.geometry.dispose();
    (entry.ground.material as THREE.Material).dispose();
    entry.texture.dispose();
    if (entry.boxes) {
      this.root.remove(entry.boxes);
      entry.boxes.geometry.dispose();
    }
    this.loaded.delete(key);
    const [cx = 0, cy = 0] = key.split(",").map(Number);
    this.view.dropGrid(cx, cy);
    this.propsDirty = true;
  }

  private clearProps() {
    for (const child of [...this.propGroup.children]) {
      this.propGroup.remove(child);
      (child as THREE.InstancedMesh).dispose();
    }
  }

  /** One instanced mesh per prop part, over every loaded chunk. */
  private rebuildProps() {
    this.propsDirty = false;
    this.clearProps();
    const byKind: ChunkProp[][] = PROP_SHAPES.map(() => []);
    for (const entry of this.loaded.values()) for (const p of entry.props) byKind[p.kind]?.push(p);
    const matrix = new THREE.Matrix4();
    const position = new THREE.Vector3();
    const rotation = new THREE.Quaternion();
    const up = new THREE.Vector3(0, 1, 0);
    const scale = new THREE.Vector3(1, 1, 1);
    PROP_SHAPES.forEach((parts, kind) => {
      const props = byKind[kind] ?? [];
      if (!props.length) return;
      parts.forEach((part, index) => {
        const material = this.propMaterials[kind]?.[index];
        if (!material) return;
        const mesh = new THREE.InstancedMesh(part.geometry, material, props.length);
        props.forEach((p, i) => {
          rotation.setFromAxisAngle(up, p.facing);
          matrix.compose(position.set(p.x, p.z + part.lift, -p.y), rotation, scale);
          mesh.setMatrixAt(i, matrix);
        });
        mesh.instanceMatrix.needsUpdate = true;
        mesh.computeBoundingSphere();
        mesh.castShadow = true;
        mesh.name = `world-props-${kind}-${index}`;
        this.propGroup.add(mesh);
      });
    });
    this.onChange?.();
  }

  /** Chunks drawn now, per level (for tests and the status line). */
  counts(): [number, number] {
    const out: [number, number] = [0, 0];
    for (const entry of this.loaded.values()) out[entry.lod] = (out[entry.lod] ?? 0) + 1;
    return out;
  }
}
