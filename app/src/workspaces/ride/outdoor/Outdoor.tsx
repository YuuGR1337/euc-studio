import { useFrame, useThree } from "@react-three/fiber";
import { useEffect, useMemo, useRef, useState } from "react";
import * as THREE from "three";
import { engine } from "../../../engine/index.ts";
import { POSE_FLOATS } from "../../../engine/telemetry.gen.ts";
import { visualFrame, visualWheelPosition } from "../poses.ts";
import type { Quality } from "../quality.ts";
import type { WorldView } from "../worldView.ts";
import { contactAo } from "./contactAo.ts";
import { HORIZON_HEX, sunWorld, worldToScene } from "./sky.ts";
import type { SkyReply, SkyRequest } from "./skyWorker.ts";

/** Fog in the horizon colour; it ends before the outer ring of chunks does, so no edge shows. */
const FOG: Record<Quality, [number, number]> = { high: [30, 90], low: [12, 44] };
/** The shadow box follows the wheel, pushed ahead along the view, ±HALF metres. */
const SHADOW_HALF_M = 25;
const SHADOW_MAP = 2048;
const SHADOW_AHEAD_M = 9;
/** The sun light sits this far from the box's centre, toward the sun. */
const SUN_DISTANCE_M = 80;
const SUN_INTENSITY = 3.1;
const SUN_COLOUR = "#FFF0DE";
const HEMI_SKY = "#C2CCD6";
const HEMI_GROUND = "#B2AB97";
const HEMI_INTENSITY = 0.6;
const ENV_INTENSITY = 0.2;

function skyTexture({ data, width, height }: SkyReply) {
  const texture = new THREE.DataTexture(data, width, height, THREE.RGBAFormat);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.mapping = THREE.EquirectangularReflectionMapping;
  texture.magFilter = THREE.LinearFilter;
  texture.minFilter = THREE.LinearFilter;
  texture.generateMipmaps = false;
  texture.needsUpdate = true;
  return texture;
}

const wheelAt = new THREE.Vector3();
const centre = new THREE.Vector3();
const forward = new THREE.Vector3();
const lightX = new THREE.Vector3();
const lightY = new THREE.Vector3();
const lightZ = new THREE.Vector3();
const up = new THREE.Vector3(0, 1, 0);

/** The rider's body (the "rider" role) in scene coordinates, or null. */
function bodyPosition(out: THREE.Vector3): THREE.Vector3 | null {
  const poses = visualFrame().poses;
  const index = engine.bodies.findIndex((b) => b.role === "rider");
  if (!poses || index < 0 || poses.length < (index + 1) * POSE_FLOATS) return null;
  const o = index * POSE_FLOATS;
  return out.set(poses[o] as number, poses[o + 2] as number, -(poses[o + 1] as number));
}
const bodyAt = new THREE.Vector3();

interface PaintedSky {
  seed: number;
  sky: THREE.DataTexture;
  environment: THREE.WebGLRenderTarget;
}

/** The sky (background) and its environment for `seed`, painted in a worker; null meanwhile. */
function usePaintedSky(seed: number, gl: THREE.WebGLRenderer): PaintedSky | null {
  const [painted, setPainted] = useState<PaintedSky | null>(null);
  useEffect(() => {
    const worker = new Worker(new URL("./skyWorker.ts", import.meta.url), { type: "module" });
    const replies = new Map<number, SkyReply>();
    worker.addEventListener("message", (event: MessageEvent<SkyReply>) => {
      replies.set(event.data.id, event.data);
      const skyReply = replies.get(0);
      const environmentReply = replies.get(1);
      if (!skyReply || !environmentReply) return;
      worker.terminate();
      const environmentSky = skyTexture(environmentReply);
      const generator = new THREE.PMREMGenerator(gl);
      const environment = generator.fromEquirectangular(environmentSky);
      generator.dispose();
      environmentSky.dispose();
      setPainted({ seed, sky: skyTexture(skyReply), environment });
    });
    const requests: SkyRequest[] = [
      { id: 0, width: 1024, height: 512, seed, clouds: true, ground: false },
      { id: 1, width: 256, height: 128, seed, clouds: false, ground: true },
    ];
    for (const request of requests) worker.postMessage(request);
    return () => worker.terminate();
  }, [seed, gl]);
  useEffect(
    () => () => {
      painted?.sky.dispose();
      painted?.environment.dispose();
    },
    [painted],
  );
  return painted?.seed === seed ? painted : null;
}

/** Positions in three.js coordinates, or null while there is no pose. */
export interface OutdoorSubject {
  wheel: (out: THREE.Vector3) => THREE.Vector3 | null;
  body: (out: THREE.Vector3) => THREE.Vector3 | null;
}

/**
 * World mode's daylight: the painted sky as background, its environment for reflections, fog in
 * the horizon colour, a hemisphere fill and one sun whose shadow box follows the wheel. The sky
 * and the sun turn with the world group (start heading), so the sun stays fixed in the world.
 */
export function Outdoor({
  world,
  quality,
  follow,
}: {
  world: WorldView;
  quality: Quality;
  /** The wheel and the body the shadows and contact pools follow (Compare's run A); the live run's by default. */
  follow?: OutdoorSubject;
}) {
  const gl = useThree((s) => s.gl);
  const get = useThree((s) => s.get);
  const heading = world.frame.start_heading_rad;
  const seed = world.frame.spec.seed;
  const high = quality === "high";
  const painted = usePaintedSky(seed, gl);
  useEffect(() => {
    const scene = get().scene;
    scene.backgroundRotation.set(0, -heading, 0);
    scene.environmentRotation.set(0, -heading, 0);
    scene.environmentIntensity = ENV_INTENSITY;
    return () => {
      scene.backgroundRotation.set(0, 0, 0);
      scene.environmentRotation.set(0, 0, 0);
      scene.environmentIntensity = 1;
    };
  }, [get, heading]);

  // The sun's direction (toward it) in the scene, and the light's basis for texel snapping.
  const sun = useMemo(() => new THREE.Vector3(...worldToScene(sunWorld(), heading)), [heading]);
  useEffect(() => {
    lightZ.copy(sun);
    lightX.crossVectors(up, lightZ).normalize();
    lightY.crossVectors(lightZ, lightX);
    // The body's pool stretches along the shadow, which falls away from the sun.
    contactAo.axis.value.set(-sun.x, -sun.z, 1.6);
    const len = Math.hypot(sun.x, sun.z) || 1;
    contactAo.axis.value.x /= len;
    contactAo.axis.value.y /= len;
    return () => {
      contactAo.wheel.value.w = 0;
      contactAo.body.value.w = 0;
    };
  }, [sun]);

  const light = useRef<THREE.DirectionalLight>(null);
  useFrame(({ camera }) => {
    const wheel = follow
      ? follow.wheel(wheelAt)
      : engine.kind !== "demo"
        ? visualWheelPosition(wheelAt)
        : wheelAt.set(0, 0, 0);
    const key = light.current;
    if (wheel) {
      contactAo.wheel.value.set(wheel.x, wheel.z, 0.6, 0.55);
      const body = follow ? follow.body(bodyAt) : bodyPosition(bodyAt);
      // The body's pool lies a little down-shadow of the body, as its soft shadow would.
      if (body)
        contactAo.body.value.set(
          body.x + contactAo.axis.value.x * 0.35,
          body.z + contactAo.axis.value.y * 0.35,
          1.05,
          0.3,
        );
      else contactAo.body.value.w = 0;
    } else {
      contactAo.wheel.value.w = 0;
      contactAo.body.value.w = 0;
    }
    if (!key) return;
    camera.getWorldDirection(forward);
    forward.y = 0;
    if (forward.lengthSq() > 1e-6) forward.normalize();
    centre.copy(wheel ?? wheelAt.set(0, 0, 0)).addScaledVector(forward, SHADOW_AHEAD_M);
    // Snap the box's centre to whole shadow texels in the light's frame, so edges do not crawl.
    const texel = (2 * SHADOW_HALF_M) / SHADOW_MAP;
    const a = Math.round(centre.dot(lightX) / texel) * texel;
    const b = Math.round(centre.dot(lightY) / texel) * texel;
    const c = centre.dot(lightZ);
    centre.copy(lightX).multiplyScalar(a).addScaledVector(lightY, b).addScaledVector(lightZ, c);
    key.target.position.copy(centre);
    key.position.copy(centre).addScaledVector(sun, SUN_DISTANCE_M);
    key.target.updateMatrixWorld();
  });

  const [near, far] = FOG[quality];
  return (
    <>
      {/* Until the worker has painted the sky, the background is its horizon colour. */}
      {painted ? (
        <primitive object={painted.sky} attach="background" />
      ) : (
        <color attach="background" args={[HORIZON_HEX]} />
      )}
      {painted && <primitive object={painted.environment.texture} attach="environment" />}
      <fog attach="fog" args={[HORIZON_HEX, near, far]} />
      <hemisphereLight args={[HEMI_SKY, HEMI_GROUND, HEMI_INTENSITY]} />
      <directionalLight
        ref={light}
        color={SUN_COLOUR}
        intensity={SUN_INTENSITY}
        castShadow={high}
        shadow-mapSize={[SHADOW_MAP, SHADOW_MAP]}
        shadow-camera-left={-SHADOW_HALF_M}
        shadow-camera-right={SHADOW_HALF_M}
        shadow-camera-top={SHADOW_HALF_M}
        shadow-camera-bottom={-SHADOW_HALF_M}
        shadow-camera-near={1}
        shadow-camera-far={SUN_DISTANCE_M + 60}
        shadow-bias={-0.0003}
        shadow-normalBias={0.03}
      />
    </>
  );
}
