/*! EUC Thrills — (c) 2026 VibezZzCoder — MIT — https://github.com/VibezZzCoder/EUC-thrills */
// @ts-nocheck -- EUC Studio: the game's file, unchanged but for the tuning import. It was written
// without noUncheckedIndexedAccess, which this app turns on; riderFigure.test.ts pins its bytes.
import * as THREE from 'three';
import { BLOCKOUT_COLOURS as C } from './tuning.ts';
import type { LoftProfile, UvRect } from './blockoutKit.ts';
import { inkOver, inkSheet, linearFromHex, toSrgbBytes } from './inkKit.ts';

/** Original garment construction, painted in code. No image or font assets. */
export const COOL_ATLAS_SIZE = 1024;
const boxes = {
  jacket: [0, 0, 512, 512], thigh: [512, 0, 768, 512], shin: [768, 0, 1024, 512],
  sleeve: [0, 512, 256, 768], forearm: [256, 512, 512, 768],
  blue: [512, 512, 768, 768], guard: [768, 512, 1024, 768],
  blank: [0, 768, 256, 1024], seat: [256, 768, 512, 1024], pocket: [512, 768, 768, 1024],
} as const;
export type CoolPage = keyof typeof boxes;
export const COOL_REGIONS = Object.freeze(Object.fromEntries(Object.entries(boxes).map(([name, b]) => [
  name, Object.freeze({ u0: (b[0] + 4.5) / 1024, v0: (b[1] + 4.5) / 1024,
    u1: (b[2] - 4.5) / 1024, v1: (b[3] - 4.5) / 1024 }),
]))) as Readonly<Record<CoolPage, UvRect>>;

export interface CoolSheetLayout {
  jacket: LoftProfile; thigh: LoftProfile; shin: LoftProfile;
  sleeve: LoftProfile; forearm: LoftProfile; seat: LoftProfile;
}
const gaussian = (distance: number, width: number): number => Math.exp(-((distance / width) ** 2));

/** `Math.exp(-q)` is +0 for every q above this: the double underflows far
 * below half the smallest subnormal. A pixel gaussian may skip the call there
 * and return that same +0 (checked once, at load). */
const EXP_ZERO_BEYOND = 800;
const EXP_SKIP_EXACT = Math.exp(-EXP_ZERO_BEYOND) === 0;
/** `gaussian`, with the underflowing calls skipped; the same value always. */
const pixelGaussian = (distance: number, width: number): number => {
  const q = (distance / width) ** 2;
  return EXP_SKIP_EXACT && q > EXP_ZERO_BEYOND ? 0 : Math.exp(-q);
};
const SEAMS = [.06, .44, .60, .90] as const;
const JACKET_CREASES = [.085, .185, .288, .395] as const;
const LIMB_FOLDS = [-.065, -.12, -.22, -.30] as const;
const columnsOf = (count: number, value: (index: number) => number): Float64Array => {
  const out = new Float64Array(count);
  for (let index = 0; index < count; index++) out[index] = value(index);
  return out;
};

/**
 * The tailoring sheet. Each pixel evaluates the same arithmetic, in the same
 * order, as the original per-pixel shader closure (2026-10-18, boot CPU: the
 * first paint was ~0.4 s of the renderer's constructor). Every term that reads
 * only the pixel's column (`s`, `u`, `px`) or only its row (`t`, `y`, `py`) is
 * computed once per column or row with the identical expression; mixed terms
 * stay per pixel. The write is `inkField`'s opaque blend. The bytes are pinned
 * against a verbatim copy of the closure painter (`coolRiderAtlas.test.ts`).
 */
export function paintCoolAtlas(layout: CoolSheetLayout): Uint8Array {
  const sheet = inkSheet(1024, 1024, [1, 1, 1]), data = sheet.data, width = sheet.width;
  // inkField's blend at coverage 1: keep 0, take 1 (the same multiply-adds).
  const keep = 0, alpha = 1;
  for (const [name, b] of Object.entries(boxes) as [CoolPage, typeof boxes[CoolPage]][]) {
    if (name === 'blank') continue;
    const columns = b[2] - b[0], rows = b[3] - b[1];
    const S = columnsOf(columns, j => Math.max(0, Math.min(1, (b[0] + j + 0.5 - b[0] - 4.5) / (b[2] - b[0] - 9))));
    const S1 = columnsOf(columns, j => 1 - S[j]);
    const WA = columnsOf(columns, j => 0.013 * Math.sin((b[0] + j + 0.5) * Math.PI));
    const P17 = columnsOf(columns, j => (b[0] + j + 0.5) * 1.7);
    const T = columnsOf(rows, k => Math.max(0, Math.min(1, (b[1] + k + 0.5 - b[1] - 4.5) / (b[3] - b[1] - 9))));
    const T1 = columnsOf(rows, k => 1 - T[k]);
    const WC = columnsOf(rows, k => Math.cos((b[1] + k + 0.5) * 2.09));
    const P13 = columnsOf(rows, k => (b[1] + k + 0.5) * 1.3);
    const write = (x: number, y: number, r: number, g: number, bl: number) => {
      const i = (y * width + x) * 3;
      data[i] = data[i]! * keep + r * alpha;
      data[i + 1] = data[i + 1]! * keep + g * alpha;
      data[i + 2] = data[i + 2]! * keep + bl * alpha;
    };
    if (name === 'pocket') {
      const R18 = columnsOf(rows, k => (.55 + .45 * Math.sin((b[1] + k + 0.5) * 1.8) ** 2));
      const PA = columnsOf(columns, j => .23 * gaussian(S[j] - .77, .018));
      const PB = columnsOf(columns, j => .20 * gaussian(S[j] - .78, .004));
      const R210 = columnsOf(rows, k => (.5 + .5 * Math.sin(T[k] * 210) ** 2));
      for (let k = 0; k < rows; k++) for (let j = 0; j < columns; j++) {
        const weave = WA[j] * WC[k] + 0.008 * Math.sin(P17[j] + P13[k]);
        const edge = Math.min(S[j], S1[j], T[k], T1[k]);
        let value = .74 + weave - .15 * pixelGaussian(edge - .03, .012);
        value += .14 * pixelGaussian(edge - .052, .004) * R18[k];
        value -= PA[j];
        value += PB[j] * R210[k];
        write(b[0] + j, b[1] + k, value, value, value);
      }
      continue;
    }
    if (name === 'blue' || name === 'guard') {
      const base = inkOver(linearFromHex(C.coolPrint), linearFromHex(name === 'blue' ? C.riderPanel : C.coolGear));
      const K0 = columnsOf(columns, j => .80 + .18 * Math.sin(Math.PI * S[j]));
      const ribbed = columnsOf(columns, j => name === 'guard' && Math.abs(S[j] - .5) < .28 ? 1 : 0);
      const RIB = columnsOf(rows, k => .11 * Math.sin(T[k] * 24 * Math.PI) ** 12);
      for (let k = 0; k < rows; k++) for (let j = 0; j < columns; j++) {
        const weave = WA[j] * WC[k] + 0.008 * Math.sin(P17[j] + P13[k]);
        const edge = Math.min(S[j], S1[j], T[k], T1[k]);
        const stitch = edge > .025 && edge < .035 ? .16 : 0;
        const rib = ribbed[j] ? RIB[k] : 0;
        const shade = K0[j] - .25 * pixelGaussian(edge, .013) + stitch + weave - rib;
        write(b[0] + j, b[1] + k, base[0] * shade, base[1] * shade, base[2] * shade);
      }
      continue;
    }
    const profile = layout[name];
    const Y = columnsOf(rows, k => {
      const v = T[k] * (profile.length - 1), i = Math.min(profile.length - 2, Math.floor(v));
      return profile[i]!.y + (profile[i + 1]!.y - profile[i]!.y) * (v - i);
    });
    const U = columnsOf(columns, j => S[j] * Math.PI * 2);
    const FRONT = columnsOf(columns, j => Math.max(0, Math.sin(U[j])));
    const BACK = columnsOf(columns, j => Math.max(0, -Math.sin(U[j])));
    // Longitudinal tailoring seams, with a fine lighter stitch on one edge.
    const SA = SEAMS.map(seam => columnsOf(columns, j => .12 * gaussian(S[j] - seam, .004)));
    const SB = SEAMS.map(seam => columnsOf(columns, j => .09 * gaussian(S[j] - seam - .006, .0015)));
    const R1300 = columnsOf(rows, k => (.55 + .45 * Math.sin(Y[k] * 1300) ** 2));
    const shadeRow = (k: number, j: number, light: number): number => {
      for (let m = 0; m < SEAMS.length; m++) {
        light -= SA[m]![j]!;
        light += SB[m]![j]! * R1300[k];
      }
      return light;
    };
    let tail: (k: number, j: number, light: number) => number;
    if (name === 'jacket') {
      // Centre zipper, front creases, back yoke, broad folds, lower-back seam.
      const JA = columnsOf(columns, j => .29 * gaussian(S[j] - .25, .006));
      const JB = columnsOf(columns, j => .35 * gaussian(S[j] - .25, .0014));
      const R1500 = columnsOf(rows, k => (.55 + .45 * Math.sin(Y[k] * 1500) ** 2));
      const FL = columnsOf(columns, j => .12 * Math.abs(S[j] - .25));
      const YH = JACKET_CREASES.map(h => columnsOf(rows, k => Y[k] - h));
      const SW = JACKET_CREASES.map(h => columnsOf(columns, j => .007 * Math.sin(U[j] * 5 + h * 70)));
      const F3 = columnsOf(columns, j => FRONT[j] ** 3);
      const YB = columnsOf(rows, k => Y[k] - .40);
      const CB = columnsOf(columns, j => .016 * Math.cos(U[j] * 2));
      const C4B = columnsOf(columns, j => .075 * Math.cos(U[j] * 4) * BACK[j]);
      const Y024 = columnsOf(rows, k => .18 * gaussian(Y[k] - .024, .003));
      tail = (k, j, light) => {
        light -= JA[j];
        light += JB[j] * R1500[k];
        for (let m = 0; m < JACKET_CREASES.length; m++) {
          const crease = YH[m]![k]! - FL[j] - SW[m]![j]!;
          light -= .10 * pixelGaussian(crease, .006) * F3[j];
          light += .08 * pixelGaussian(crease - .009, .009) * F3[j];
        }
        light -= .11 * pixelGaussian(YB[k] + CB[j], .004) * BACK[j];
        light += C4B[j];
        light -= Y024[k];
        return light;
      };
    } else if (name === 'thigh') {
      // Pocket opening on each side: garment detail, no label or manufacturer.
      const SIDE8 = columnsOf(columns, j => Math.abs(Math.cos(U[j])) ** 8);
      const SU25 = columnsOf(columns, j => .025 * Math.sin(U[j]));
      const SU2 = columnsOf(columns, j => .015 * Math.sin(U[j] * 2));
      const Y155 = columnsOf(rows, k => Y[k] + .155), Y163 = columnsOf(rows, k => Y[k] + .163);
      const Y31 = columnsOf(rows, k => Y[k] + .31);
      tail = (k, j, light) => {
        light -= .19 * pixelGaussian(Y155[k] + SU25[j], .004) * SIDE8[j];
        light += .08 * pixelGaussian(Y163[k] + SU25[j], .003) * SIDE8[j];
        light -= .10 * pixelGaussian(Y31[k] + SU2[j], .008) * FRONT[j];
        return light;
      };
    } else if (name === 'seat') {
      const Y005 = columnsOf(rows, k => .15 * gaussian(Y[k] + .005, .003));
      const SF = columnsOf(columns, j => .18 * gaussian(S[j] - .25, .014) * FRONT[j]);
      tail = (k, j, light) => {
        light -= Y005[k];
        light -= SF[j];
        return light;
      };
    } else {
      // A few cloth compression folds. Their phase varies around the limb,
      // so these never form complete corrugated rings.
      const FH = LIMB_FOLDS.map(h => columnsOf(rows, k => Y[k] - h));
      const FS = LIMB_FOLDS.map(h => columnsOf(columns, j => .014 * Math.sin(U[j] * 2 + h * 19)));
      const MASK = LIMB_FOLDS.map(h => columnsOf(columns, j => .2 + .8 * Math.max(0, Math.cos(U[j] + h * 10)) ** 3));
      tail = (k, j, light) => {
        for (let m = 0; m < LIMB_FOLDS.length; m++) {
          const fold = FH[m]![k]! + FS[m]![j]!, mask = MASK[m]![j]!;
          light -= .10 * pixelGaussian(fold, .005) * mask;
          light += .09 * pixelGaussian(fold - .008, .008) * mask;
        }
        return light;
      };
    }
    for (let k = 0; k < rows; k++) for (let j = 0; j < columns; j++) {
      const weave = WA[j] * WC[k] + 0.008 * Math.sin(P17[j] + P13[k]);
      const light = tail(k, j, shadeRow(k, j, .79 + weave));
      write(b[0] + j, b[1] + k, light, light, light);
    }
  }
  return toSrgbBytes(sheet);
}

const cached = new WeakMap<CoolSheetLayout, Uint8Array>();
export function createCoolAtlas(layout: CoolSheetLayout): THREE.DataTexture {
  let pixels = cached.get(layout);
  if (!pixels) { pixels = paintCoolAtlas(layout); cached.set(layout, pixels); }
  const texture = new THREE.DataTexture(pixels, 1024, 1024, THREE.RGBAFormat);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.magFilter = THREE.LinearFilter;
  texture.minFilter = THREE.LinearMipmapLinearFilter;
  texture.generateMipmaps = true;
  texture.anisotropy = 4;
  texture.needsUpdate = true;
  texture.name = 'cool-rider-tailoring';
  return texture;
}
