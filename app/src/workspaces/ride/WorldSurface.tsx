import { useFrame, useThree } from "@react-three/fiber";
import { useEffect, useMemo, useRef } from "react";
import * as THREE from "three";
import { engine } from "../../engine/index.ts";
import { visualWheelPosition } from "./poses.ts";
import type { Quality } from "./quality.ts";
import { WorldStream } from "./worldStream.ts";
import type { WorldView } from "./worldView.ts";

const wheelAt = new THREE.Vector3();

/** The world's group transform: world axes (x east, y up, z south) into the physics frame. */
function frameTransform(view: WorldView) {
  const { start_xy_m, start_heading_rad, origin_z_m } = view.frame;
  const angle = -start_heading_rad;
  const position = new THREE.Vector3(-start_xy_m[0], -origin_z_m, start_xy_m[1]).applyAxisAngle(
    new THREE.Vector3(0, 1, 0),
    angle,
  );
  return { position, angle };
}

/**
 * The streaming world around the wheel: ground chunks in two rings of detail, their curbs, steps,
 * plaza walls, ramps and buildings, and the props. Physics frame, like the live bodies.
 */
export function WorldSurface({
  world,
  quality,
  onError,
  follow,
}: {
  world: WorldView;
  quality: Quality;
  /** The stream's last request error, or null once chunks arrive again. */
  onError?: (message: string | null) => void;
  /** The wheel the rings follow, three.js coordinates (Compare's run A); the live run's by default. */
  follow?: (out: THREE.Vector3) => THREE.Vector3 | null;
}) {
  const group = useRef<THREE.Group>(null);
  const reported = useRef<string | null>(null);
  const invalidate = useThree((s) => s.invalidate);
  const transform = useMemo(() => frameTransform(world), [world]);
  const stream = useRef<WorldStream | null>(null);
  useEffect(() => {
    if (!group.current) return;
    const created = new WorldStream(world, group.current, quality);
    created.onChange = invalidate;
    stream.current = created;
    return () => {
      created.dispose();
      stream.current = null;
      if (reported.current !== null) onError?.(null);
      reported.current = null;
    };
  }, [world, quality, invalidate, onError]);
  useFrame((_, dt) => {
    const found = follow
      ? follow(wheelAt)
      : engine.kind !== "demo"
        ? visualWheelPosition(wheelAt)
        : null;
    const wheel = found ? ([found.x, -found.z] as [number, number]) : null;
    // Before the first pose the run starts at the frame's origin.
    stream.current?.update(dt, wheel ?? [0, 0]);
    const error = stream.current?.error ?? null;
    if (error !== reported.current) {
      reported.current = error;
      onError?.(error);
    }
  });
  return (
    <group
      ref={group}
      name="world"
      position={transform.position}
      rotation={[0, transform.angle, 0]}
    />
  );
}

/** A view far enough for a city: the camera sees 300 m, the fog closes from 18 m to 110 m. */
export function WorldCamera() {
  const get = useThree((s) => s.get);
  useEffect(() => {
    const camera = get().camera as THREE.PerspectiveCamera;
    const far = camera.far;
    camera.far = 300;
    camera.updateProjectionMatrix();
    return () => {
      camera.far = far;
      camera.updateProjectionMatrix();
    };
  }, [get]);
  return null;
}
