import { invoke } from "@tauri-apps/api/core";

/** How often things occur (euc_world::Densities); per 1000 m² of their zone unless noted. */
export interface WorldDensities {
  potholes: number;
  manholes: number;
  cracks: number;
  rough_patches: number;
  wet_patches: number;
  /** Fraction of local street edges with a bump or hump. */
  speed_bumps: number;
  /** Fraction of intersection street ends with a crosswalk. */
  crosswalks: number;
  /** Per 100 m of street side. */
  driveways: number;
  /** Weight of plazas among parcels. */
  plazas: number;
  /** Fraction of furniture and tree slots used. */
  props: number;
}

/** The engine's defaults (euc_world::Densities::default). */
export const DEFAULT_DENSITIES: WorldDensities = {
  potholes: 1.5,
  manholes: 0.6,
  cracks: 12,
  rough_patches: 0.8,
  wet_patches: 1,
  speed_bumps: 0.35,
  crosswalks: 0.7,
  driveways: 1.5,
  plazas: 0.15,
  props: 0.8,
};

/** Upper bounds the engine accepts (euc_world::Densities::validate). */
export const DENSITY_LIMITS: Record<keyof WorldDensities, number> = {
  potholes: 50,
  manholes: 3,
  cracks: 100,
  rough_patches: 6,
  wet_patches: 9,
  speed_bumps: 1,
  crosswalks: 1,
  driveways: 5,
  plazas: 2,
  props: 1,
};

/** The generator revision this studio was built against (euc_world::GENERATOR_REVISION). */
export const WORLD_REVISION = 2;

/** A world: everything it depends on (euc_world::WorldSpec). */
export interface WorldSpec {
  seed: number;
  revision: number;
  biome: "city";
  densities: WorldDensities;
}

/** A world free ride's choice: its seed and densities. */
export interface WorldChoice {
  seed: number;
  densities: WorldDensities;
}

/**
 * Where a run sits in its world: physics = R(-heading) · (world - start), z_physics = z_world - origin_z.
 */
export interface WorldFrame {
  spec: WorldSpec;
  start_xy_m: [number, number];
  start_heading_rad: number;
  origin_z_m: number;
}

const finite = (...values: number[]) => values.every(Number.isFinite);

/** Problems a world choice has before the engine sees it, or null. */
export function worldProblem(world: WorldChoice): string | null {
  if (!Number.isInteger(world.seed) || world.seed < 0 || world.seed > 0xffff_ffff)
    return "Seed must be a whole number from 0 to 4294967295.";
  for (const [key, limit] of Object.entries(DENSITY_LIMITS) as [keyof WorldDensities, number][]) {
    const value = world.densities[key];
    if (!Number.isFinite(value) || value < 0 || value > limit)
      return `Density ${key.replace("_", " ")} must be within 0 to ${limit}.`;
  }
  return null;
}

/** A random seed for "New world". */
export const randomSeed = () => Math.floor(Math.random() * 0xffff_ffff);

export function validFrame(value: unknown): value is WorldFrame {
  if (typeof value !== "object" || value === null) return false;
  const frame = value as Partial<WorldFrame>;
  const spec = frame.spec;
  return (
    !!spec &&
    Number.isInteger(spec.seed) &&
    spec.revision === WORLD_REVISION &&
    spec.biome === "city" &&
    Array.isArray(frame.start_xy_m) &&
    frame.start_xy_m.length === 2 &&
    finite(frame.start_xy_m[0], frame.start_xy_m[1]) &&
    typeof frame.start_heading_rad === "number" &&
    typeof frame.origin_z_m === "number" &&
    finite(frame.start_heading_rad, frame.origin_z_m)
  );
}

export const worldApi = {
  /** Binary chunk payloads (crates/euc-world/src/render.rs documents the layout). */
  async chunks(spec: WorldSpec, keys: [number, number][], lod: number): Promise<ArrayBuffer> {
    return invoke<ArrayBuffer>("world_chunks", { spec, keys, lod });
  },
  /** The world height under a start point (the physics frame's z = 0). */
  async originZ(spec: WorldSpec, start: [number, number]): Promise<number> {
    const z = await invoke<number>("world_origin_z", { spec, startXyM: start });
    if (!Number.isFinite(z)) throw new Error("Invalid world height.");
    return z;
  },
};
