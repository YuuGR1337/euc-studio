/** The native grid in physics coordinates: x forward, y left, z up.
 * Treat each accepted map revision as immutable. Native number[] grids and cached
 * Int16Array grids both fit this interface without an engine dependency.
 */
export interface HeightMapView {
  readonly revision: number;
  readonly nx: number;
  readonly ny: number;
  readonly spacing_m: number;
  readonly origin_xy_m: readonly [number, number];
  readonly heights_mm: readonly number[] | Int16Array;
}

/** One triangle's height and upward unit normal, in physics world coordinates. */
export interface MapSample {
  height_m: number;
  normal_world: [number, number, number];
}

/** Inclusive horizontal world bounds, in metres. */
export interface MapBounds {
  min: [number, number];
  max: [number, number];
}

/** Warning and pause distances from the finite map edge, in metres. */
export const MAP_WARNING_M = 2;
export const MAP_PAUSE_M = 1;

const SAMPLES = 129;
const SPACING_M = 0.25;
const ORIGIN_M = -16;
const MAX_HEIGHT_MM = 12_000;
const MAX_REVISION = 0xffff_ffff;
/** Squared rise across a 0.25 m cell at 150% grade, in square millimetres. */
const MAX_RISE_SQUARED_MM = 375 * 375;

function validLayout(value: unknown): value is HeightMapView {
  if (typeof value !== "object" || value === null) return false;
  const map = value as Partial<HeightMapView>;
  return (
    Number.isInteger(map.revision) &&
    (map.revision as number) >= 0 &&
    (map.revision as number) <= MAX_REVISION &&
    map.nx === SAMPLES &&
    map.ny === SAMPLES &&
    map.spacing_m === SPACING_M &&
    Array.isArray(map.origin_xy_m) &&
    map.origin_xy_m.length === 2 &&
    map.origin_xy_m[0] === ORIGIN_M &&
    map.origin_xy_m[1] === ORIGIN_M &&
    (Array.isArray(map.heights_mm) || map.heights_mm instanceof Int16Array) &&
    map.heights_mm.length === SAMPLES * SAMPLES
  );
}

function validHeight(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && Math.abs(value) <= MAX_HEIGHT_MM;
}

/**
 * Check an incoming map before geometry allocation. Return a visible error for
 * the parent UI, or null on success. This matches the native revision-1 bounds,
 * level spawn cells, and both triangle grade limits; it does not regenerate data.
 */
export function validateMap(value: unknown): string | null {
  if (!validLayout(value)) {
    return "Invalid map layout. Use 129 × 129 samples, 0.25 m spacing, and origin [-16, -16].";
  }
  for (let iy = 0; iy < value.ny; iy++) {
    for (let ix = 0; ix < value.nx; ix++) {
      const height = value.heights_mm[iy * value.nx + ix];
      if (!validHeight(height)) {
        return `Invalid map height at (${ix}, ${iy}). Use integer millimetres within ±12000.`;
      }
      const x = value.origin_xy_m[0] + ix * value.spacing_m;
      const y = value.origin_xy_m[1] + iy * value.spacing_m;
      // Flatten complete cells touching the 1 m spawn disk, as the native generator does.
      if (
        Math.hypot(Math.max(0, Math.abs(x) - SPACING_M), Math.max(0, Math.abs(y) - SPACING_M)) <=
          1 &&
        height !== 0
      ) {
        return `Map sample (${ix}, ${iy}) changes the level spawn patch.`;
      }
    }
  }
  for (let iy = 0; iy < value.ny - 1; iy++) {
    for (let ix = 0; ix < value.nx - 1; ix++) {
      const index = iy * value.nx + ix;
      // The preceding pass checks every index before these reads.
      const h00 = value.heights_mm[index] as number;
      const h10 = value.heights_mm[index + 1] as number;
      const h01 = value.heights_mm[index + value.nx] as number;
      const h11 = value.heights_mm[index + value.nx + 1] as number;
      const first = (h10 - h00) ** 2 + (h11 - h10) ** 2;
      const second = (h11 - h01) ** 2 + (h01 - h00) ** 2;
      if (first > MAX_RISE_SQUARED_MM || second > MAX_RISE_SQUARED_MM) {
        return `Map cell (${ix}, ${iy}) exceeds the 150% grade limit.`;
      }
    }
  }
  return null;
}

/** Layout bounds only. Validate the complete map once before using it. */
export function mapBounds(map: HeightMapView | null | undefined): MapBounds | null {
  if (!validLayout(map)) return null;
  return {
    min: [map.origin_xy_m[0], map.origin_xy_m[1]],
    max: [
      map.origin_xy_m[0] + (map.nx - 1) * map.spacing_m,
      map.origin_xy_m[1] + (map.ny - 1) * map.spacing_m,
    ],
  };
}

/**
 * Sample the authoritative surface using physics x/y, never Three.js x/z.
 * Call validateMap once per immutable map before this frame-loop helper.
 * Outside and nonfinite coordinates return null; there is no clamping or floor.
 *
 * MuJoCo 3.9.0 splits each cell 00→11. For cell fractions u/v, v <= u
 * selects (00, 10, 11); otherwise (00, 11, 01). The first triangle supplies
 * the diagonal normal. Interior grid lines choose the positive-side cell;
 * maximum edges choose the final cell. The optional output avoids frame allocations.
 */
export function sampleMap(
  map: HeightMapView | null | undefined,
  x_m: number,
  y_m: number,
  out?: MapSample,
): MapSample | null {
  if (!validLayout(map) || !Number.isFinite(x_m) || !Number.isFinite(y_m)) return null;
  const maxX = map.origin_xy_m[0] + (map.nx - 1) * map.spacing_m;
  const maxY = map.origin_xy_m[1] + (map.ny - 1) * map.spacing_m;
  if (x_m < map.origin_xy_m[0] || y_m < map.origin_xy_m[1] || x_m > maxX || y_m > maxY) {
    return null;
  }
  const fx = (x_m - map.origin_xy_m[0]) / map.spacing_m;
  const fy = (y_m - map.origin_xy_m[1]) / map.spacing_m;
  const ix = Math.min(Math.floor(fx), map.nx - 2);
  const iy = Math.min(Math.floor(fy), map.ny - 2);
  const u = fx - ix;
  const v = fy - iy;
  const index = iy * map.nx + ix;
  const mm00 = map.heights_mm[index];
  const mm10 = map.heights_mm[index + 1];
  const mm01 = map.heights_mm[index + map.nx];
  const mm11 = map.heights_mm[index + map.nx + 1];
  if (!validHeight(mm00) || !validHeight(mm10) || !validHeight(mm01) || !validHeight(mm11)) {
    return null;
  }
  const h00 = mm00 / 1000;
  const h10 = mm10 / 1000;
  const h01 = mm01 / 1000;
  const h11 = mm11 / 1000;
  const first = v <= u;
  const dx = (first ? h10 - h00 : h11 - h01) / map.spacing_m;
  const dy = (first ? h11 - h10 : h01 - h00) / map.spacing_m;
  const height = first
    ? h00 + u * (h10 - h00) + v * (h11 - h10)
    : h00 + u * (h11 - h01) + v * (h01 - h00);
  // Match the native normalization, including operation order.
  const norm = Math.hypot(Math.hypot(dx, dy), 1);
  const sample = out ?? { height_m: 0, normal_world: [0, 0, 1] };
  sample.height_m = height;
  sample.normal_world[0] = -dx / norm;
  sample.normal_world[1] = -dy / norm;
  sample.normal_world[2] = 1 / norm;
  return sample;
}
