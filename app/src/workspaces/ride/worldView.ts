import type { WorldFrame } from "../../engine/world.ts";
import type { MapSample } from "./mapSurface.ts";
import { CHUNK_M } from "./worldMesh.ts";

interface Grid {
  lod: number;
  n: number;
  heights: Float32Array;
}

/**
 * A run's world as the view sees it: the frame between world and physics coordinates, and the
 * height grids of the chunks drawn so far (for the camera, the contact shadow and the grade).
 * One object per run; replace it when the run changes.
 */
export class WorldView {
  readonly frame: WorldFrame;
  private readonly cos: number;
  private readonly sin: number;
  private readonly grids = new Map<string, Grid>();

  constructor(frame: WorldFrame) {
    this.frame = frame;
    this.cos = Math.cos(frame.start_heading_rad);
    this.sin = Math.sin(frame.start_heading_rad);
  }

  static key(cx: number, cy: number) {
    return `${cx},${cy}`;
  }

  /** A physics-frame point in world coordinates. */
  toWorld(px: number, py: number): [number, number] {
    const [sx, sy] = this.frame.start_xy_m;
    return [sx + this.cos * px - this.sin * py, sy + this.sin * px + this.cos * py];
  }

  /** A world point in the physics frame. */
  toPhysics(wx: number, wy: number): [number, number] {
    const dx = wx - this.frame.start_xy_m[0];
    const dy = wy - this.frame.start_xy_m[1];
    return [this.cos * dx + this.sin * dy, -this.sin * dx + this.cos * dy];
  }

  setGrid(cx: number, cy: number, lod: number, n: number, heights: Float32Array) {
    this.grids.set(WorldView.key(cx, cy), { lod, n, heights });
  }

  dropGrid(cx: number, cy: number) {
    this.grids.delete(WorldView.key(cx, cy));
  }

  /** The drawn ground under a physics-frame point, in the physics frame; null where none is loaded. */
  sample(px: number, py: number): MapSample | null {
    const [wx, wy] = this.toWorld(px, py);
    const cx = Math.floor(wx / CHUNK_M);
    const cy = Math.floor(wy / CHUNK_M);
    const grid = this.grids.get(WorldView.key(cx, cy));
    if (!grid) return null;
    const step = CHUNK_M / (grid.n - 1);
    const fx = (wx - cx * CHUNK_M) / step;
    const fy = (wy - cy * CHUNK_M) / step;
    const i = Math.min(grid.n - 2, Math.max(0, Math.floor(fx)));
    const j = Math.min(grid.n - 2, Math.max(0, Math.floor(fy)));
    const u = fx - i;
    const v = fy - j;
    const h = (di: number, dj: number) => grid.heights[(j + dj) * grid.n + i + di] ?? 0;
    const [h00, h10, h01, h11] = [h(0, 0), h(1, 0), h(0, 1), h(1, 1)];
    // The two triangles of the engine's height fields: (00, 10, 11) when v <= u, else (00, 11, 01).
    const [height, gx, gy] =
      v <= u
        ? [h00 + u * (h10 - h00) + v * (h11 - h10), (h10 - h00) / step, (h11 - h10) / step]
        : [h00 + u * (h11 - h01) + v * (h01 - h00), (h11 - h01) / step, (h01 - h00) / step];
    // The world-frame gradient turned into the physics frame.
    const px1 = this.cos * gx + this.sin * gy;
    const py1 = -this.sin * gx + this.cos * gy;
    const norm = Math.hypot(px1, py1, 1);
    return {
      height_m: height - this.frame.origin_z_m,
      normal_world: [-px1 / norm, -py1 / norm, 1 / norm],
    };
  }
}
