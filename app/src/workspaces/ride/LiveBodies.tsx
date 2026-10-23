import { useFrame, useThree } from "@react-three/fiber";
import { useEffect, useMemo, useRef } from "react";
import * as THREE from "three";
import { surface, text } from "../../design/tokens.ts";
import { engine, useEngineVersion } from "../../engine/index.ts";
import { useUi } from "../../state/ui.ts";
import { type BodyInfo, type Geom } from "../../engine/telemetry.gen.ts";
import { BodyPart, ShellVisual, WheelPart } from "./EucModel.tsx";
import { Ghost } from "./Ghost.tsx";
import { useWheelModel } from "./wheelModel.ts";
import { placeBodies, visualFrame } from "./poses.ts";
import { RiderFigureView } from "./riderFigure/RiderFigure.tsx";
import { bindRider } from "./riderFigure/skeleton.ts";

// The live engine's bodies, posed every frame from the latest snapshot. Everything sits inside
// one group that turns the physics world (MuJoCo: z up) into three.js (y up), so body poses go
// in unchanged. The wheel and the shell get the scanned model (else the stylized one). The rider
// (one body or many) is drawn as the EUC Thrills figure posed on its bodies (riderFigure/), or,
// with "Physics bodies" on or for a body list the figure cannot bind, from its MJCF geoms like
// any other body. A body without a pose of its list is hidden (placeBodies), never drawn at the
// origin.
//
// The Ride workspace draws the ride engine; the Compare workspace draws each of its two runs
// (`source`), run B as a ghost. A compare run shows the plain shell: the display and tail light
// mirror the ride engine's board, not the run's.

/** Visuals are modelled y up with the axle on z; a body frame has z up with the axle on y. */
const TO_BODY_FRAME: [number, number, number] = [Math.PI / 2, 0, 0];

const riderMaterial = new THREE.MeshStandardMaterial({
  color: "#C7C7CC",
  roughness: 0.62,
  metalness: 0,
});

const shade = (color: string) =>
  new THREE.MeshStandardMaterial({ color, roughness: 0.62, metalness: 0 });

/**
 * The articulated rider (torso, head, upper_arm_*, forearm_*, thigh_*, shank_*, foot_*) as a
 * mannequin in greys: a lighter head, darker legs, dark shoes, so the limbs read against each
 * other. Any other rider body (the rigid rider is one body, "rider") keeps the plain grey.
 */
const SEGMENTS: [prefix: string, material: THREE.Material][] = [
  ["head", shade(text.fg)],
  ["thigh", shade(surface.lift)],
  ["shank", shade(surface.lift)],
  ["foot", shade(surface.track)],
];

const materialFor = (body: string) =>
  SEGMENTS.find(([prefix]) => body.startsWith(prefix))?.[1] ?? riderMaterial;

/** Where the poses come from: the ride engine, or one run of a comparison. */
export interface PoseSource {
  readonly bodies: BodyInfo[];
  readonly latest: { poses: Float32Array | null };
}

function GeomMesh({ geom, material }: { geom: Geom; material: THREE.Material }) {
  const { kind, size, pos, quat } = geom;
  const geometry = useMemo(() => {
    switch (kind) {
      case "capsule":
        return new THREE.CapsuleGeometry(size[0], size[1] * 2, 6, 16);
      case "cylinder":
        return new THREE.CylinderGeometry(size[0], size[0], size[1] * 2, 24);
      case "box":
        return new THREE.BoxGeometry(size[0] * 2, size[1] * 2, size[2] * 2);
      case "sphere":
        return new THREE.SphereGeometry(size[0], 24, 16);
      case "ellipsoid": {
        const g = new THREE.SphereGeometry(1, 24, 16);
        g.scale(size[0], size[1], size[2]);
        return g;
      }
    }
  }, [kind, size]);
  // three.js builds capsules and cylinders along y; MJCF puts them along the geom's z.
  const alongZ = kind === "capsule" || kind === "cylinder";
  return (
    <group position={pos} quaternion={new THREE.Quaternion(quat[1], quat[2], quat[3], quat[0])}>
      <mesh
        geometry={geometry}
        material={material}
        rotation={alongZ ? TO_BODY_FRAME : [0, 0, 0]}
        castShadow
        receiveShadow
      />
    </group>
  );
}

/** The scanned body alone (no display, no tail light), else the stylized shell. */
function PlainBody({ model, accent }: { model: ReturnType<typeof useWheelModel>; accent: string }) {
  const scanned = useMemo(() => model?.body.clone(true), [model]);
  return scanned ? <primitive object={scanned} /> : <ShellVisual accent={accent} />;
}

export function LiveBodies({
  accent,
  source = engine,
  ghost = false,
}: {
  accent: string;
  source?: PoseSource;
  ghost?: boolean;
}) {
  useEngineVersion();
  const model = useWheelModel();
  const bodies = source.bodies;
  const nodes = useRef<(THREE.Group | null)[]>([]);
  const placed = useRef<{ bodies: readonly BodyInfo[] | null }>({ bodies: null });
  const own = source === engine;
  const physicsBodies = useUi((s) => s.physicsBodies);
  const binding = useMemo(
    () => (physicsBodies ? null : bindRider(bodies)),
    [bodies, physicsBodies],
  );

  useFrame(() => {
    // The visual frame is of the engine's bodies now; this render may still hold the last list.
    const frame = own ? visualFrame() : null;
    const poses = frame ? (frame.bodies === bodies ? frame.poses : null) : source.latest.poses;
    placeBodies(nodes.current, bodies, poses, placed.current);
  }, -50);
  // A demand frameloop draws only on request: a new body list with the same names changes no
  // prop, so ask for the frame in which placeBodies hides its stale groups.
  const invalidate = useThree((s) => s.invalidate);
  useEffect(() => invalidate(), [bodies, invalidate]);

  const placedBodies = bodies.map((b, i) => (
    <group
      key={`${i}-${b.name}`}
      name={`body:${b.name}`}
      // Hidden until its first pose (placeBodies): a new group sits at the origin.
      visible={false}
      ref={(g) => {
        nodes.current[i] = g;
      }}
    >
      {b.role === "wheel" ? (
        <group rotation={TO_BODY_FRAME}>
          <WheelPart model={model} />
        </group>
      ) : b.role === "shell" ? (
        <group rotation={TO_BODY_FRAME}>
          {own ? (
            <BodyPart model={model} accent={accent} />
          ) : (
            <PlainBody model={model} accent={accent} />
          )}
        </group>
      ) : b.role === "rider" && binding ? null : (
        b.geoms.map((g, j) => <GeomMesh key={j} geom={g} material={materialFor(b.name)} />)
      )}
    </group>
  ));
  const content = (
    <>
      {placedBodies}
      {binding && <RiderFigureView binding={binding} nodes={nodes} />}
    </>
  );

  return (
    <group rotation={[-Math.PI / 2, 0, 0]}>{ghost ? <Ghost>{content}</Ghost> : content}</group>
  );
}
