import { useFrame } from "@react-three/fiber";
import { type RefObject, useEffect, useMemo, useRef } from "react";
import type * as THREE from "three";
import { RiderFigure } from "./figure.ts";
import { type RiderBinding, poseSkeleton, skeletonPose } from "./skeleton.ts";

/**
 * The rider drawn as the EUC Thrills figure, posed every frame on the live body groups (placed
 * by LiveBodies at priority -50; this runs right after). Physics frame, like the bodies. Hidden
 * on any frame a body it needs has no pose.
 */
export function RiderFigureView({
  binding,
  nodes,
}: {
  binding: RiderBinding;
  /** The body groups, in the order of the body list `binding` was made from. */
  nodes: RefObject<(THREE.Object3D | null)[]>;
}) {
  const group = useRef<THREE.Group>(null);
  const figure = useRef<RiderFigure | null>(null);
  const pose = useMemo(() => skeletonPose(), []);
  useEffect(() => {
    const host = group.current;
    if (!host) return;
    const created = new RiderFigure();
    created.rig.root.visible = false;
    host.add(created.rig.root);
    figure.current = created;
    return () => {
      host.remove(created.rig.root);
      created.dispose();
      figure.current = null;
    };
  }, []);
  useFrame((_, dt) => {
    const current = figure.current;
    if (!current) return;
    const posed = poseSkeleton(binding, nodes.current, pose);
    current.rig.root.visible = posed;
    if (posed) current.pose(binding, pose, dt);
    else current.reset();
  }, -49);
  return <group ref={group} name="rider-figure" />;
}
