import { sampleMap, type HeightMapView, type MapSample } from "./mapSurface.ts";
import { WorldView } from "./worldView.ts";

/** The ground a run draws and samples: the 32 m test pad's grid, or a streaming world. */
export type GroundView = HeightMapView | WorldView;

export const isWorldView = (ground: GroundView | null | undefined): ground is WorldView =>
  ground instanceof WorldView;

/** Ground height and normal under a physics-frame point, or null where the ground is unknown. */
export function sampleGround(
  ground: GroundView | null | undefined,
  x: number,
  y: number,
): MapSample | null {
  if (!ground) return null;
  return ground instanceof WorldView ? ground.sample(x, y) : sampleMap(ground, x, y);
}
