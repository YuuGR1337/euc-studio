import { useEffect, useMemo } from "react";
import * as THREE from "three";
import { type HeightMapView, validateMap } from "./mapSurface.ts";

import { buildMapSurface } from "./mapGeometry.ts";

export interface MapSurfaceProps {
  /** Replace this immutable object when the native map revision changes. */
  map: HeightMapView | null;
}

/** One finite mesh and surface-following grid lines. No separate floor exists. */
export function MapSurface({ map }: MapSurfaceProps) {
  const geometry = useMemo(() => {
    if (!map || validateMap(map) !== null) return null;
    return buildMapSurface(map);
  }, [map]);

  useEffect(() => {
    return () => {
      geometry?.surface.dispose();
      geometry?.grid.dispose();
    };
  }, [geometry]);

  // The parent can show validateMap's error. Invalid data never allocates a mesh
  // and never falls back to an invented surface.
  if (!geometry) return null;
  return (
    <group name="generated-map">
      <mesh name="generated-map-surface" geometry={geometry.surface} receiveShadow>
        <meshLambertMaterial
          vertexColors
          flatShading
          side={THREE.FrontSide}
          // The existing studio fog ends at 13 m, inside this 32 m map.
          fog={false}
          emissive="#FFFFFF"
          emissiveIntensity={0.025}
          polygonOffset
          polygonOffsetFactor={1}
          polygonOffsetUnits={1}
        />
      </mesh>
      <lineSegments name="generated-map-grid" geometry={geometry.grid} renderOrder={1}>
        <lineBasicMaterial
          vertexColors
          transparent
          opacity={0.6}
          depthWrite={false}
          toneMapped={false}
          fog={false}
        />
      </lineSegments>
    </group>
  );
}
