import { ContactShadows, Environment, Grid, Lightformer } from "@react-three/drei";
import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { Suspense, useEffect, useMemo, useRef, useState } from "react";
import * as THREE from "three";
import { accents, surface } from "../../design/tokens.ts";
import { engine } from "../../engine/index.ts";
import { useUi } from "../../state/ui.ts";
import { setLowPower } from "../../lib/power.ts";
import { EucModel } from "./EucModel.tsx";
import { LiveBodies } from "./LiveBodies.tsx";
import { prepareVisualFrame, visualWheelPosition } from "./poses.ts";
import { CameraRig } from "./CameraRig.tsx";
import { INITIAL_CAMERA } from "./cameraTracking.ts";
import { MapSurface } from "./MapSurface.tsx";
import { type GroundView, isWorldView, sampleGround } from "./ground.ts";
import { WorldCamera, WorldSurface } from "./WorldSurface.tsx";
import { type Slope, slopeAt } from "./terrain.ts";
import { TerrainStrip } from "./TerrainStrip.tsx";
import { LOW_FPS, type Quality, chooseQuality, qualityOverride, rendererName } from "./quality.ts";

const wheelAt = new THREE.Vector3();
const ground: Slope = { z: 0, pitch: 0 };
const worldUp = new THREE.Vector3(0, 1, 0);
const groundNormal = new THREE.Vector3();

function VisualFrame() {
  useFrame(() => prepareVisualFrame(performance.now()), -100);
  return null;
}

/**
 * Keeps its children under the live wheel (floor light, contact shadow), laid on the slope there;
 * still for the demo.
 */
function FollowWheel({ children, map }: { children: React.ReactNode; map?: GroundView | null }) {
  const group = useRef<THREE.Group>(null);
  useFrame(() => {
    if (group.current && engine.kind !== "demo" && visualWheelPosition(wheelAt)) {
      const sampled = sampleGround(map, wheelAt.x, -wheelAt.z);
      if (sampled) {
        group.current.position.set(wheelAt.x, sampled.height_m, wheelAt.z);
        const [x, y, z] = sampled.normal_world;
        group.current.quaternion.setFromUnitVectors(worldUp, groundNormal.set(x, z, -y));
        return;
      }
      slopeAt(engine.terrain, wheelAt.x, ground);
      // Physics x is three.js x and physics z is three.js y: the pitch turns about three.js z.
      group.current.position.set(wheelAt.x, ground.z, wheelAt.z);
      group.current.rotation.z = ground.pitch;
    }
  });
  return <group ref={group}>{children}</group>;
}
/**
 * Ground grid that scrolls under the wheel, so speed reads even though the wheel stays put. Only
 * on flat ground (`flat`): on a scenario's terrain the road strip is the floor, and a flat grid
 * would cover the road where it goes downhill.
 */
function Floor({ flat, map }: { flat: boolean; map?: GroundView | null }) {
  const grid = useRef<THREE.Mesh>(null);
  useFrame(() => {
    // The demo wheel stays put and the floor scrolls; the live wheel moves over a still floor.
    if (grid.current && engine.kind === "demo")
      grid.current.position.x = -(engine.latest.distance % 1);
  });
  return (
    <>
      {flat && (
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
            fadeDistance={11}
            fadeStrength={2.2}
            position={[0, 0.0005, 0]}
          />
          <mesh rotation={[-Math.PI / 2, 0, 0]} receiveShadow>
            <planeGeometry args={[40, 40]} />
            <shadowMaterial transparent opacity={0.35} />
          </mesh>
        </>
      )}
      {engine.kind !== "playback" && engine.status.runMode !== "free_ride" && (
        <FollowWheel map={map}>
          <Spot />
        </FollowWheel>
      )}
    </>
  );
}

/** A soft pool of light on the floor that frames the wheel, like a studio sweep. */
function Spot() {
  const texture = useMemo(() => {
    const c = document.createElement("canvas");
    c.width = c.height = 256;
    const g = c.getContext("2d");
    if (g) {
      const grad = g.createRadialGradient(128, 128, 0, 128, 128, 128);
      grad.addColorStop(0, "rgba(255, 255, 255, 0.09)");
      grad.addColorStop(0.55, "rgba(255, 255, 255, 0.03)");
      grad.addColorStop(1, "rgba(255, 255, 255, 0)");
      g.fillStyle = grad;
      g.fillRect(0, 0, 256, 256);
    }
    const t = new THREE.CanvasTexture(c);
    t.colorSpace = THREE.SRGBColorSpace;
    return t;
  }, []);
  return (
    <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, 0.0003, 0]}>
      <planeGeometry args={[4.5, 4.5]} />
      <meshBasicMaterial map={texture} transparent depthWrite={false} />
    </mesh>
  );
}

const KEY_OFFSET = new THREE.Vector3(2.5, 4, 2.5);

function Lights() {
  // The key light (and its shadow) travels with the live wheel.
  const key = useRef<THREE.DirectionalLight>(null);
  useFrame(() => {
    const light = key.current;
    if (!light || engine.kind === "demo" || !visualWheelPosition(wheelAt)) return;
    light.position.copy(wheelAt).add(KEY_OFFSET);
    light.target.position.copy(wheelAt);
    light.target.updateMatrixWorld();
  });
  return (
    <>
      <hemisphereLight args={["#F2F2F7", "#000000", 0.35]} />
      <directionalLight
        ref={key}
        position={KEY_OFFSET.toArray()}
        intensity={2.8}
        castShadow
        shadow-mapSize={[1024, 1024]}
        shadow-camera-left={-2}
        shadow-camera-right={2}
        shadow-camera-top={2.2}
        shadow-camera-bottom={-2}
        shadow-bias={-0.0004}
      />
      {/* Rim from behind separates the dark shell from the black background; neutral, so the
          model keeps the colours of the owner's photos. */}
      <directionalLight position={[-2.5, 1.6, -2.2]} intensity={2.2} color="#FFFFFF" />
      <directionalLight position={[-1, 0.8, 3]} intensity={0.5} color="#CDE8FF" />
      {/* Studio softboxes built in-scene: no HDR download, works offline. */}
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
          intensity={1.4}
          position={[-4, 1.2, 2]}
          rotation-y={Math.PI / 2}
          scale={[4, 1.5, 1]}
        />
        <Lightformer
          form="rect"
          intensity={1}
          position={[4, 1.2, 2]}
          rotation-y={-Math.PI / 2}
          scale={[4, 1.5, 1]}
        />
        <Lightformer form="ring" intensity={0.8} color="#FFFFFF" position={[0, 1, -5]} scale={2} />
      </Environment>
    </>
  );
}

/**
 * In low quality the scene redraws on a fixed clock, and only when the simulation moved. Orbit
 * drags still redraw at once (the controls invalidate on change).
 */
function Ticker({ fps, moving }: { fps: number; moving: React.RefObject<boolean> }) {
  const invalidate = useThree((s) => s.invalidate);
  useEffect(() => {
    let drawnAt = Number.NaN;
    const id = window.setInterval(() => {
      if (engine.latest.t === drawnAt && !moving.current) return;
      drawnAt = engine.latest.t;
      invalidate();
    }, 1000 / fps);
    return () => window.clearInterval(id);
  }, [fps, invalidate, moving]);
  return null;
}

export interface SceneProps {
  active: boolean;
  /** The 32 m test pad or a streaming world (Free ride and its playback). */
  map?: GroundView | null;
  onReady?: (info: { quality: Quality; renderer: string }) => void;
  /** Called once, after the scene has drawn its first frame (tests wait on it). */
  onFirstFrame?: () => void;
  /** The streaming world's last chunk error, or null when it streams. */
  onWorldError?: (message: string | null) => void;
}

/** Reports the first drawn frame (from the frame after it, so the first one has reached the screen). */
function FirstFrame({ onFirstFrame }: { onFirstFrame?: () => void }) {
  const done = useRef(false);
  useFrame(() => {
    if (done.current) return;
    done.current = true;
    requestAnimationFrame(() => onFirstFrame?.());
  });
  return null;
}

export function Scene({ active, map, onReady, onFirstFrame, onWorldError }: SceneProps) {
  const camera = useUi((s) => s.camera);
  const accent = useUi((s) => s.accent);
  const [override] = useState(() => qualityOverride(location.search));
  const [quality, setQuality] = useState<Quality>(override ?? "high");
  const high = quality === "high";
  const cameraMoving = useRef(false);

  return (
    <Canvas
      shadows={high}
      dpr={high ? [1, 2] : 1}
      frameloop={!active ? "never" : high ? "always" : "demand"}
      camera={{ position: INITIAL_CAMERA, fov: 42, near: 0.05, far: 60 }}
      gl={{ antialias: true, powerPreference: "high-performance" }}
      onCreated={({ gl, get }) => {
        // Dev builds: scripts and agents reach the scene (camera, controls) without clicks.
        if (import.meta.env.DEV) Object.assign(window, { __eucScene: get });
        // Neutral keeps hues: ACES turned the orange handle red (measured against the owner's photos).
        gl.toneMapping = THREE.NeutralToneMapping;
        gl.toneMappingExposure = 1.5;
        const renderer = rendererName(gl.getContext());
        const chosen = chooseQuality(renderer, override);
        setQuality(chosen);
        setLowPower(chosen === "low");
        onReady?.({ quality: chosen, renderer });
      }}
      aria-label="3D view of the wheel"
    >
      <VisualFrame />
      <color attach="background" args={[surface.bg]} />
      {/* A world shows its streets out to the fog; the studio sweep keeps the close fog. */}
      <fog attach="fog" args={isWorldView(map) ? [surface.bg, 18, 110] : [surface.bg, 5, 13]} />
      <Suspense fallback={null}>
        <Lights />
        <Floor
          flat={
            !map &&
            engine.status.runMode !== "free_ride" &&
            !engine.terrain &&
            engine.kind !== "playback"
          }
          map={map}
        />
        {map && !isWorldView(map) && <MapSurface map={map} />}
        {isWorldView(map) && (
          <>
            <WorldSurface world={map} quality={quality} onError={onWorldError} />
            <WorldCamera />
          </>
        )}
        {engine.terrain && (
          // The physics world (z up) in three.js (y up), as the live bodies.
          <group rotation={[-Math.PI / 2, 0, 0]}>
            <TerrainStrip profile={engine.terrain} />
          </group>
        )}
        {engine.kind !== "demo" ? (
          <LiveBodies accent={accents[accent].accent} />
        ) : (
          <EucModel accent={accents[accent].accent} />
        )}
        {engine.kind !== "playback" && (engine.status.runMode !== "free_ride" || !!map) && (
          <FollowWheel map={map}>
            <ContactShadows
              // The key: a new component when quality changes, so `frames` applies from the start.
              key={quality}
              position={[0, 0.001, 0]}
              opacity={0.75}
              scale={3}
              blur={2.2}
              far={0.9}
              resolution={high ? 512 : 256}
              frames={high ? Infinity : 1}
            />
          </FollowWheel>
        )}
      </Suspense>
      <CameraRig mode={camera} map={map} moving={cameraMoving} />
      <FirstFrame onFirstFrame={onFirstFrame} />
      {active && !high && <Ticker fps={LOW_FPS} moving={cameraMoving} />}
    </Canvas>
  );
}
