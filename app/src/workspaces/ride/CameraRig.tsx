import { OrbitControls } from "@react-three/drei";
import { useFrame, useThree } from "@react-three/fiber";
import { useReducedMotion } from "motion/react";
import { type ComponentRef, type RefObject, useMemo, useRef } from "react";
import * as THREE from "three";
import { engine } from "../../engine/index.ts";
import type { CameraMode } from "../../state/ui.ts";
import type { HeightMapView } from "./mapSurface.ts";
import { visualFrame } from "./poses.ts";
import { ChaseCamera, spring } from "./cameraTracking.ts";

export function CameraRig({
  mode,
  map,
  moving: movingRef,
}: {
  mode: CameraMode;
  map?: HeightMapView | null;
  moving: RefObject<boolean>;
}) {
  const camera = useThree((state) => state.camera);
  const controls = useRef<ComponentRef<typeof OrbitControls>>(null);
  const reduce = !!useReducedMotion();
  const rigRef = useRef<ChaseCamera | null>(null);
  // Seed Drei before its priority -1 update can aim new controls at the world origin.
  const orbitSeed = useMemo(() => {
    if (mode !== "orbit") return new THREE.Vector3();
    const direction = new THREE.Vector3();
    camera.getWorldDirection(direction);
    return direction.multiplyScalar(4.3).add(camera.position);
  }, [camera, mode]);
  useFrame((_, dt) => {
    const rig = (rigRef.current ??= new ChaseCamera());
    if (engine.kind !== "demo" && !visualFrame().poses) {
      rig.initialized = false;
      movingRef.current = false;
      return;
    }
    if (mode === "follow") {
      movingRef.current = rig.follow(camera, dt, reduce, map);
      return;
    }
    const control = controls.current;
    if (!control) return;
    const rider = rig.target(map);
    const reset = rig.source !== engine || rig.generation !== visualFrame().generation;
    const entering = rig.mode !== "orbit";
    if (reset || rig.framedRider !== rider) {
      rig.delta.subVectors(camera.position, control.target);
      if (rig.delta.lengthSq() < 0.01) rig.delta.set(-3.6, 1.3, 1.8);
      rig.delta.setLength(rider ? 4.3 : 2.3);
      control.target.copy(rig.anchor);
      camera.position.copy(rig.anchor).add(rig.delta);
      rig.aimVelocity.set(0, 0, 0);
    } else {
      if (entering) {
        // Keep the current view. Shift its orbit centre smoothly toward the subject.
        rig.previousAnchor.copy(rig.anchor);
        rig.aimVelocity.set(0, 0, 0);
      }
      rig.delta.subVectors(rig.anchor, rig.previousAnchor);
      control.target.add(rig.delta);
      camera.position.add(rig.delta);
      rig.aim.copy(control.target);
      if (reduce) {
        rig.aim.copy(rig.anchor);
        rig.aimVelocity.set(0, 0, 0);
      } else {
        for (const axis of ["x", "y", "z"] as const) {
          [rig.aim[axis], rig.aimVelocity[axis]] = spring(
            rig.aim[axis],
            rig.aimVelocity[axis],
            rig.anchor[axis],
            7,
            Math.min(dt, 0.25),
          );
        }
      }
      rig.delta.subVectors(rig.aim, control.target);
      control.target.copy(rig.aim);
      camera.position.add(rig.delta);
    }
    control.update();
    rig.source = engine;
    rig.generation = visualFrame().generation;
    rig.framedRider = rider;
    rig.mode = "orbit";
    rig.initialized = true;
    rig.previousAnchor.copy(rig.anchor);
    movingRef.current =
      control.target.distanceToSquared(rig.anchor) > 0.00001 ||
      rig.aimVelocity.lengthSq() > 0.00001 ||
      visualFrame().time < engine.latest.t - 0.0001;
  });
  return mode === "orbit" ? (
    <OrbitControls
      ref={controls}
      makeDefault
      target={orbitSeed}
      enablePan={false}
      enableDamping={!reduce}
      dampingFactor={0.08}
      minDistance={1.2}
      maxDistance={12}
      maxPolarAngle={Math.PI / 2 - 0.05}
    />
  ) : null;
}
