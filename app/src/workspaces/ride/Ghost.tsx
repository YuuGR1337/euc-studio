import { useFrame } from "@react-three/fiber";
import { type ReactNode, useRef } from "react";
import * as THREE from "three";
import { semantic } from "../../design/tokens.ts";

// A translucent copy of whatever it wraps: the Compare workspace draws run B as a ghost over run
// A, so the two coincide where the firmwares agree and part where they do not. Every mesh inside
// gets the one ghost material and casts no shadow, including meshes that arrive later (the
// scanned model loads after the first frame).

const GHOST = new THREE.MeshStandardMaterial({
  color: semantic.info,
  transparent: true,
  opacity: 0.24,
  depthWrite: false,
  roughness: 0.55,
  metalness: 0,
});

export function Ghost({ children }: { children: ReactNode }) {
  const group = useRef<THREE.Group>(null);
  useFrame(() => {
    group.current?.traverse((o) => {
      if (o instanceof THREE.Mesh && o.material !== GHOST) {
        o.material = GHOST;
        o.castShadow = false;
        o.receiveShadow = false;
        o.renderOrder = 1;
      }
    });
  });
  return <group ref={group}>{children}</group>;
}
