/**
 * Daylight for the streaming world: one sun shared by the painted sky, the shadows and the
 * environment, and a sky whose horizon is exactly the fog colour, so the far ground dissolves
 * into it. Painted once per world on the CPU, from the world seed (the clouds repeat with a
 * recording). The method follows EUC Thrills by VibezZzCoder (MIT; src/render/skyImage.ts and
 * src/render/sky.ts): a horizon-to-zenith ramp shaped by y^0.62, warm scatter low toward the sun,
 * value-noise clouds on a flat plane overhead, a Gaussian sun core and glow. Our own code, palette
 * and noise.
 */

/** sRGB colour, 0-255 per channel. */
export type Rgb = readonly [number, number, number];

/** The fog colour and the sky's colour at the horizon (display sRGB; fog mixes after tone mapping). */
export const HORIZON: Rgb = [188, 212, 236];
export const HORIZON_HEX = "#BCD4EC";
const ZENITH: Rgb = [84, 140, 212];
const SCATTER: Rgb = [246, 222, 196];
const CLOUD: Rgb = [246, 248, 252];
const CLOUD_SHADE: Rgb = [196, 208, 224];
const SUN_CORE: Rgb = [255, 250, 240];
/** The light the ground throws back, for the environment's lower half. */
const GROUND_BOUNCE: Rgb = [138, 134, 124];

/**
 * The sun in world axes (x east, y north, z up): an afternoon sun in the south-west, 42° up.
 * Fixed in the world, so shadows and the painted sun agree with any start heading.
 */
export const SUN_AZIMUTH_DEG = 225;
export const SUN_ELEVATION_DEG = 42;

export function sunWorld(): [number, number, number] {
  const az = (SUN_AZIMUTH_DEG * Math.PI) / 180;
  const el = (SUN_ELEVATION_DEG * Math.PI) / 180;
  return [Math.cos(el) * Math.cos(az), Math.cos(el) * Math.sin(az), Math.sin(el)];
}

/**
 * A world direction in the scene's axes (three.js, y up), where the world is turned by
 * `-headingRad` about y like the world group: world (x, y, z) is three.js (x, z, -y) first.
 */
export function worldToScene(v: readonly [number, number, number], headingRad: number) {
  const [x, y, z] = [v[0], v[2], -v[1]];
  const c = Math.cos(-headingRad);
  const s = Math.sin(-headingRad);
  // Rotation about +y by angle a: x' = x cos a + z sin a, z' = -x sin a + z cos a.
  return [x * c + z * s, y, -x * s + z * c] as [number, number, number];
}

const toLinear = (c: number) => {
  const v = c / 255;
  return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
};
const toSrgb = (v: number) => {
  const c = v <= 0.0031308 ? v * 12.92 : 1.055 * v ** (1 / 2.4) - 0.055;
  return Math.max(0, Math.min(255, Math.round(c * 255)));
};
const linear = (c: Rgb): [number, number, number] => [
  toLinear(c[0]),
  toLinear(c[1]),
  toLinear(c[2]),
];

function hash(x: number, y: number, seed: number) {
  let h = Math.imul(x ^ 0x9e3779b9, 0x85ebca6b) ^ Math.imul(y ^ 0xc2b2ae35, 0x27d4eb2f) ^ seed;
  h ^= h >>> 15;
  h = Math.imul(h, 0x2c1b3c6d);
  return ((h ^ (h >>> 12)) >>> 0) / 0xffffffff;
}

/** Value noise with a quintic fade, in [0, 1). */
function noise(x: number, y: number, seed: number) {
  const ix = Math.floor(x);
  const iy = Math.floor(y);
  const fx = x - ix;
  const fy = y - iy;
  const ux = fx * fx * fx * (fx * (fx * 6 - 15) + 10);
  const uy = fy * fy * fy * (fy * (fy * 6 - 15) + 10);
  const a = hash(ix, iy, seed);
  const b = hash(ix + 1, iy, seed);
  const c = hash(ix, iy + 1, seed);
  const d = hash(ix + 1, iy + 1, seed);
  return a + (b - a) * ux + (c - a) * uy + (a - b - c + d) * ux * uy;
}

function clouds(x: number, y: number, seed: number) {
  let sum = 0;
  let amplitude = 0.55;
  let f = 1;
  for (let octave = 0; octave < 4; octave++) {
    sum += amplitude * noise(x * f, y * f, seed + octave * 0x632be5ab);
    amplitude *= 0.48;
    f *= 2.07;
  }
  return sum;
}

const smoothstep = (a: number, b: number, x: number) => {
  const t = Math.max(0, Math.min(1, (x - a) / (b - a)));
  return t * t * (3 - 2 * t);
};

export interface SkyOptions {
  /** Clouds (the background); the environment leaves them out. */
  clouds: boolean;
  /** Below the horizon: the horizon colour (background) or the ground's bounce (environment). */
  ground: boolean;
}

/**
 * Paints an equirectangular sky in the world's axes, as three.js samples one: texel (i, j) looks
 * along azimuth θ = (u − ½)·2π from +x toward +z and elevation φ = (v − ½)·π, with u = (i + ½)/w,
 * v = (j + ½)/h and row 0 at the bottom (a DataTexture is not flipped). RGBA, display sRGB.
 */
export function paintSky(width: number, height: number, seed: number, options: SkyOptions) {
  const out = new Uint8Array(width * height * 4);
  const [sx, sy, sz] = worldToScene(sunWorld(), 0);
  const horizon = linear(HORIZON);
  const zenith = linear(ZENITH);
  const scatter = linear(SCATTER);
  const cloud = linear(CLOUD);
  const shade = linear(CLOUD_SHADE);
  const core = linear(SUN_CORE);
  const bounce = linear(GROUND_BOUNCE);
  const cloudSeed = Math.imul(seed >>> 0, 0x27d4eb2f) ^ 0x165667b1;
  const col = [0, 0, 0];
  for (let j = 0; j < height; j++) {
    const phi = ((j + 0.5) / height - 0.5) * Math.PI;
    const y = Math.sin(phi);
    const cosPhi = Math.cos(phi);
    for (let i = 0; i < width; i++) {
      const theta = ((i + 0.5) / width - 0.5) * 2 * Math.PI;
      const x = cosPhi * Math.cos(theta);
      const z = cosPhi * Math.sin(theta);
      const toSun = x * sx + y * sy + z * sz;
      if (y >= 0) {
        const t = y ** 0.62;
        for (let k = 0; k < 3; k++)
          col[k] = (horizon[k] ?? 0) + ((zenith[k] ?? 0) - (horizon[k] ?? 0)) * t;
        // Warm scatter low toward the sun; none on the horizon line itself (the fog's colour).
        const warm = Math.max(toSun, 0) ** 4 * (1 - y) ** 3 * 0.35 * Math.min(1, y / 0.06);
        for (let k = 0; k < 3; k++)
          col[k] = (col[k] ?? 0) + ((scatter[k] ?? 0) - (col[k] ?? 0)) * warm;
        if (options.clouds && y > 0.02) {
          // A flat cloud deck: plane coordinates grow toward the horizon, where clouds fade out.
          const n = clouds((x / y) * 0.9 + 17.3, (z / y) * 0.9 - 4.1, cloudSeed);
          const cover = smoothstep(0.5, 0.78, n) * smoothstep(0.02, 0.3, y);
          const lit = 0.55 + 0.45 * Math.max(toSun, 0);
          for (let k = 0; k < 3; k++) {
            const c = (shade[k] ?? 0) + ((cloud[k] ?? 0) - (shade[k] ?? 0)) * lit;
            col[k] = (col[k] ?? 0) + (c - (col[k] ?? 0)) * cover * 0.9;
          }
        }
      } else {
        const below = options.ground ? smoothstep(0, 0.12, -y) : 0;
        for (let k = 0; k < 3; k++)
          col[k] = (horizon[k] ?? 0) + ((bounce[k] ?? 0) - (horizon[k] ?? 0)) * below;
      }
      // The sun: a small hot core and two glows.
      const angle = Math.acos(Math.max(-1, Math.min(1, toSun)));
      const sun =
        (y > -0.02 ? 1 : 0) *
        (Math.exp(-((angle / 0.014) ** 2)) * 1.0 +
          Math.exp(-((angle / 0.09) ** 2)) * 0.28 +
          Math.exp(-((angle / 0.45) ** 2)) * 0.08);
      const o = (j * width + i) * 4;
      for (let k = 0; k < 3; k++) {
        const v = (col[k] ?? 0) + ((core[k] ?? 0) - (col[k] ?? 0)) * Math.min(1, sun);
        out[o + k] = toSrgb(v);
      }
      out[o + 3] = 255;
    }
  }
  return out;
}
