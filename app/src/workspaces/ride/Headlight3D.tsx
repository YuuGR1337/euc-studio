import { useFrame } from "@react-three/fiber";
import { useEffect, useRef } from "react";
import * as THREE from "three";
import { isLowPower } from "../../lib/power.ts";

// State comes from the emulated output. Emission gain, beam angle and light strength are
// illustrative rendering choices, not measured optical properties or hardware limits.
const WHITE = "#F2F2F7";
const LENS_TINT = new THREE.Color(WHITE);
const LENS_RADIUS = 0.011; // geometry.toml: measured 22 mm lens diameter.
const LENS_OFFSETS = [-0.045, -0.015, 0.015, 0.045];
const FORWARD: [number, number, number] = [0, 0, -Math.PI / 2];

interface MaterialSlot {
  mesh: THREE.Mesh;
  original: THREE.Material | THREE.Material[];
  owned: THREE.Material | THREE.Material[];
}

/** Four lenses and one inexpensive forward light, all driven by the same observed duty. */
export function Headlight3D({
  readDuty,
  scan,
  fallbackPosition = [0.182, 0.335, 0],
}: {
  readDuty: () => number | null;
  scan?: THREE.Object3D;
  fallbackPosition?: [number, number, number];
}) {
  const lenses = useRef<THREE.MeshStandardMaterial[]>([]);
  const fallbackLenses = useRef<(THREE.MeshStandardMaterial | null)[]>([]);
  const mount = useRef<THREE.Group>(null);
  const light = useRef<THREE.SpotLight>(null);
  const target = useRef<THREE.Object3D>(null);
  const mounted = useRef(!scan);

  useEffect(() => {
    if (!scan) return;
    const node = scan.getObjectByName("headlight_lens");
    mounted.current = false;
    const slots: MaterialSlot[] = [];
    const owned: THREE.MeshStandardMaterial[] = [];
    node?.traverse((child) => {
      if (!(child instanceof THREE.Mesh)) return;
      const original = child.material;
      const copy = (material: THREE.Material) => {
        const cloned = material.clone();
        if (cloned instanceof THREE.MeshStandardMaterial) {
          cloned.color.multiplyScalar(0.25);
          cloned.emissive.setRGB(0, 0, 0);
          cloned.emissiveIntensity = 1;
          owned.push(cloned);
        }
        return cloned;
      };
      const material = Array.isArray(original) ? original.map(copy) : copy(original);
      child.material = material;
      slots.push({ mesh: child, original, owned: material });
    });
    lenses.current = owned;
    if (node && mount.current) {
      // The GLB groups all four lenses under one node. Use their bounds instead of duplicating
      // profile coordinates. Convert the world centre back into the shell's local frame.
      scan.updateWorldMatrix(true, true);
      const bounds = new THREE.Box3().setFromObject(node);
      if (!bounds.isEmpty()) {
        const centre = scan.worldToLocal(bounds.getCenter(new THREE.Vector3()));
        if ([centre.x, centre.y, centre.z].every(Number.isFinite)) {
          mount.current.position.copy(centre);
          mounted.current = true;
        }
      }
    }
    return () => {
      lenses.current = [];
      mounted.current = false;
      for (const slot of slots) {
        if (slot.mesh.material === slot.owned) slot.mesh.material = slot.original;
        for (const material of Array.isArray(slot.owned) ? slot.owned : [slot.owned])
          material.dispose();
      }
    };
  }, [scan]);

  useFrame(() => {
    const duty = readDuty();
    // Missing or invalid telemetry leaves the depiction unlit. The HUD says unavailable.
    const observed = duty !== null && Number.isFinite(duty) && duty >= 0 && duty <= 1 ? duty : 0;
    const materials = scan ? lenses.current : fallbackLenses.current;
    for (const material of materials) {
      if (material) material.emissive.copy(LENS_TINT).multiplyScalar(observed * 4);
    }
    const beam = light.current;
    const aim = target.current;
    if (!beam || !aim) return;
    beam.target = aim;
    // Low power keeps the stateful lens glow and avoids an extra scene light.
    beam.visible = mounted.current && observed > 0 && !isLowPower();
    beam.intensity = observed * 12;
    aim.updateWorldMatrix(true, false);
  });

  return (
    <>
      {!scan && (
        <group position={fallbackPosition}>
          <mesh position={[-0.009, 0, 0]} castShadow>
            <boxGeometry args={[0.018, 0.033, 0.114]} />
            <meshStandardMaterial color="#18181A" roughness={0.5} metalness={0.2} />
          </mesh>
          {LENS_OFFSETS.map((offset, index) => (
            <mesh key={offset} position={[0.001, 0, offset]} rotation={FORWARD}>
              <cylinderGeometry args={[LENS_RADIUS, LENS_RADIUS, 0.002, 24]} />
              <meshStandardMaterial
                ref={(material) => {
                  fallbackLenses.current[index] = material;
                }}
                color="#343A42"
                roughness={0.12}
                metalness={0}
                emissive="#000000"
                emissiveIntensity={1}
              />
            </mesh>
          ))}
        </group>
      )}
      <group ref={mount} position={scan ? undefined : fallbackPosition}>
        <spotLight
          ref={light}
          color={WHITE}
          intensity={0}
          visible={false}
          distance={5}
          angle={Math.PI / 6}
          penumbra={0.6}
          decay={2}
          castShadow={false}
        />
        <object3D ref={target} position={[3, 0, 0]} />
      </group>
    </>
  );
}
