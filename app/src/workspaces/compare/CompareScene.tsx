import { ContactShadows, Environment, Grid, Lightformer } from "@react-three/drei";
import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { useReducedMotion } from "motion/react";
import { Suspense, useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { accents, surface } from "../../design/tokens.ts";
import { compare, useCompareVersion, type CompareRun } from "../../engine/compare.ts";
import { POSE_FLOATS } from "../../engine/telemetry.gen.ts";
import { setLowPower } from "../../lib/power.ts";
import { useUi } from "../../state/ui.ts";
import { BodyPart, SCAN_AXLE_Y, TIRE_R, WheelPart } from "../ride/EucModel.tsx";
import { Ghost } from "../ride/Ghost.tsx";
import { LiveBodies } from "../ride/LiveBodies.tsx";
import {
  LOW_FPS,
  type Quality,
  chooseQuality,
  qualityOverride,
  rendererName,
} from "../ride/quality.ts";
import { TerrainStrip } from "../ride/TerrainStrip.tsx";
import { useWheelModel } from "../ride/wheelModel.ts";

// The Compare workspace's 3D view: run A drawn normally, run B as a translucent ghost over it.
// Where the firmwares agree the ghost hides inside A; where they part, it rides ahead, behind or
// at another angle. The camera looks from the side at the middle of the two and steps back as
// they part, so both stay in the picture.

const DEG = Math.PI / 180;
/** From the side and a little ahead; about twice as far with a rider on board. */
const OFFSET = new THREE.Vector3(0.55, 0.45, 2.4);
const LOOK = new THREE.Vector3(0, 0.34, 0);
/** Width the camera sees per metre of distance (30° field of view, a wide viewport), roughly. */
const WIDTH_PER_M = 0.8;
/** Room around the two wheels (and riders), m. */
const MARGIN_M = 1.6;
/** The camera steps back at most this far (times its usual distance). */
const MAX_PULL = 8;
const at = new THREE.Vector3();
const atB = new THREE.Vector3();
const desired = new THREE.Vector3();
const focus = new THREE.Vector3();

/** A run's wheel position in three.js coordinates (y up), or null without poses. */
function wheelOf(run: CompareRun, out: THREE.Vector3): THREE.Vector3 | null {
  const p = run.latest.poses;
  const i = run.bodies.findIndex((b) => b.role === "wheel");
  if (!p || i < 0) return null;
  const o = i * POSE_FLOATS;
  return out.set(p[o] as number, p[o + 2] as number, -(p[o + 1] as number));
}

/** Where each run's wheel is: from its pose (live), or by distance from A, which stays put (demo). */
function wheels(): void {
  if (!wheelOf(compare.a, at)) at.set(0, 0, 0);
  if (!wheelOf(compare.b, atB))
    atB.set(compare.b.latest.distance - compare.a.latest.distance, 0, 0);
}

const hasRider = () => compare.a.bodies.some((b) => b.role === "rider");

function FollowCamera({ still }: { still: boolean }) {
  const camera = useThree((s) => s.camera);
  const reduce = useReducedMotion();
  const first = useRef(true);
  useFrame((_, dt) => {
    wheels();
    const gap = Number.isFinite(atB.x) ? Math.hypot(atB.x - at.x, atB.z - at.z) : 0;
    focus.copy(at);
    if (gap < 50) focus.lerp(atB, 0.5);
    focus.add(LOOK);
    if (hasRider()) focus.y += 0.5;
    const base = OFFSET.length() * (hasRider() ? 1.9 : 1);
    const fit = (Math.min(gap, 50) + MARGIN_M * (hasRider() ? 1.5 : 1)) / WIDTH_PER_M;
    const pull = Math.min(MAX_PULL, Math.max(1, fit / base));
    desired
      .copy(OFFSET)
      .multiplyScalar((hasRider() ? 1.9 : 1) * pull)
      .add(focus);
    if (reduce || still || first.current) {
      camera.position.copy(desired);
      first.current = false;
    } else {
      camera.position.lerp(desired, 1 - Math.exp(-dt * 3));
    }
    camera.lookAt(focus);
  });
  return null;
}

/** The demo's kinematic wheel for one run, placed by its distance from run A. */
function KinematicWheel({ run, accent }: { run: CompareRun; accent: string }) {
  const model = useWheelModel();
  const root = useRef<THREE.Group>(null);
  const spin = useRef<THREE.Group>(null);
  const body = useRef<THREE.Group>(null);
  useFrame(() => {
    const l = run.latest;
    if (root.current) {
      root.current.position.x = l.distance - compare.a.latest.distance;
      root.current.rotation.x = (Number.isFinite(l.roll) ? l.roll : 0) * DEG;
    }
    if (spin.current) spin.current.rotation.z = -l.wheelAngle;
    if (body.current) body.current.rotation.z = (Number.isFinite(l.pitch) ? l.pitch : 0) * DEG;
  });
  return (
    <group ref={root} position={[0, model ? SCAN_AXLE_Y : TIRE_R, 0]}>
      <group ref={spin}>
        <WheelPart model={model} />
      </group>
      <group ref={body}>
        <BodyPart model={model} accent={accent} />
      </group>
    </group>
  );
}

/** The flat floor: still under the live runs (they move over it), scrolling under the demo's. */
function Floor() {
  const grid = useRef<THREE.Mesh>(null);
  useFrame(() => {
    if (grid.current && compare.kind === "demo") {
      grid.current.position.x = -(compare.a.latest.distance % 1);
    }
  });
  return (
    <>
      <Grid
        ref={grid}
        infiniteGrid
        cellSize={0.25}
        sectionSize={1}
        cellThickness={0.6}
        sectionThickness={1}
        cellColor={surface.raised}
        sectionColor={surface.lift}
        fadeDistance={30}
        fadeStrength={2.2}
        position={[0, 0.0005, 0]}
      />
      <mesh rotation={[-Math.PI / 2, 0, 0]} receiveShadow>
        <planeGeometry args={[60, 60]} />
        <shadowMaterial transparent opacity={0.35} />
      </mesh>
    </>
  );
}

function FollowA({ children }: { children: React.ReactNode }) {
  const group = useRef<THREE.Group>(null);
  useFrame(() => {
    if (group.current && wheelOf(compare.a, at)) group.current.position.set(at.x, 0, at.z);
  });
  return <group ref={group}>{children}</group>;
}

function Lights() {
  return (
    <>
      <hemisphereLight args={["#F2F2F7", "#000000", 0.35]} />
      <FollowA>
        <directionalLight
          position={[2.5, 4, 2.5]}
          intensity={2.8}
          castShadow
          shadow-mapSize={[1024, 1024]}
          shadow-camera-left={-2.5}
          shadow-camera-right={2.5}
          shadow-camera-top={2.5}
          shadow-camera-bottom={-2.5}
          shadow-bias={-0.0004}
        />
      </FollowA>
      <directionalLight position={[-2.5, 1.6, -2.2]} intensity={2.2} color="#FFB27A" />
      <directionalLight position={[-1, 0.8, 3]} intensity={0.5} color="#CDE8FF" />
      <Environment resolution={256} frames={1}>
        <Lightformer
          form="rect"
          intensity={2.4}
          position={[0, 4, 0]}
          rotation-x={Math.PI / 2}
          scale={[5, 5, 1]}
        />
        <Lightformer
          form="rect"
          intensity={1.2}
          position={[-4, 1.2, 2]}
          rotation-y={Math.PI / 2}
          scale={[4, 1.5, 1]}
        />
      </Environment>
    </>
  );
}

/** Low quality: redraw on a fixed clock, and only when a run moved. */
function Ticker({ fps }: { fps: number }) {
  const invalidate = useThree((s) => s.invalidate);
  useEffect(() => {
    let drawnAt = Number.NaN;
    const id = window.setInterval(() => {
      if (compare.a.latest.t === drawnAt) return;
      drawnAt = compare.a.latest.t;
      invalidate();
    }, 1000 / fps);
    return () => window.clearInterval(id);
  }, [fps, invalidate]);
  return null;
}

function FirstFrame({ onFirstFrame }: { onFirstFrame?: () => void }) {
  const done = useRef(false);
  useFrame(() => {
    if (done.current) return;
    done.current = true;
    requestAnimationFrame(() => onFirstFrame?.());
  });
  return null;
}

export interface CompareSceneProps {
  active: boolean;
  onReady?: (quality: Quality) => void;
  onFirstFrame?: () => void;
}

export function CompareScene({ active, onReady, onFirstFrame }: CompareSceneProps) {
  useCompareVersion();
  const accent = useUi((s) => s.accent);
  const [override] = useState(() => qualityOverride(location.search));
  const [quality, setQuality] = useState<Quality>(override ?? "high");
  const high = quality === "high";
  const colour = accents[accent].accent;
  const live = compare.kind === "live";

  return (
    <Canvas
      shadows={high}
      dpr={high ? [1, 2] : 1}
      frameloop={!active ? "never" : high ? "always" : "demand"}
      camera={{ position: OFFSET.toArray(), fov: 30, near: 0.05, far: 80 }}
      gl={{ antialias: true, powerPreference: "high-performance" }}
      onCreated={({ gl }) => {
        gl.toneMapping = THREE.ACESFilmicToneMapping;
        gl.toneMappingExposure = 1.05;
        const chosen = chooseQuality(rendererName(gl.getContext()), override);
        setQuality(chosen);
        setLowPower(chosen === "low");
        onReady?.(chosen);
      }}
      aria-label="3D view of both runs: A solid, B as a ghost"
    >
      <color attach="background" args={[surface.bg]} />
      <fog attach="fog" args={[surface.bg, 10, 45]} />
      <Suspense fallback={null}>
        <Lights />
        {compare.terrain ? (
          <group rotation={[-Math.PI / 2, 0, 0]}>
            <TerrainStrip profile={compare.terrain} />
          </group>
        ) : (
          <Floor />
        )}
        {live ? (
          <>
            <LiveBodies key={`a-${compare.epoch}`} source={compare.a} accent={colour} />
            <LiveBodies key={`b-${compare.epoch}`} source={compare.b} accent={colour} ghost />
          </>
        ) : (
          compare.request && (
            <>
              <KinematicWheel run={compare.a} accent={colour} />
              <Ghost>
                <KinematicWheel run={compare.b} accent={colour} />
              </Ghost>
            </>
          )
        )}
        <FollowA>
          <ContactShadows
            key={quality}
            position={[0, 0.001, 0]}
            opacity={0.7}
            scale={3.5}
            blur={2.2}
            far={0.9}
            resolution={high ? 512 : 256}
            frames={high ? Infinity : 1}
          />
        </FollowA>
      </Suspense>
      <FollowCamera still={!high} />
      <FirstFrame onFirstFrame={onFirstFrame} />
      {active && !high && <Ticker fps={LOW_FPS} />}
    </Canvas>
  );
}
