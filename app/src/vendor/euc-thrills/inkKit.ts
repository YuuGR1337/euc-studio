/*! EUC Thrills — (c) 2026 VibezZzCoder — MIT — https://github.com/VibezZzCoder/EUC-thrills */
// EUC Studio: trimmed to the parts coolRiderAtlas.ts uses (Rgb, InkSheet, linearFromHex,
// linearFromByte, inkOver, inkSheet, toSrgbBytes); each kept block is unchanged.

/** Linear-light RGB, 0..1 per channel. Not sRGB — see the file comment. */
export type Rgb = readonly [number, number, number];

/**
 * A sheet being printed: linear RGB, three floats a pixel, no alpha.
 *
 * No alpha channel because the sheet is opaque by construction — it starts as
 * a colour and every operation composites onto it. Alpha exists only as an
 * operation's own coverage.
 */
export interface InkSheet {
  readonly width: number;
  readonly height: number;
  readonly data: Float32Array;
}

/** Decode an sRGB hex the way `THREE.Color` does, into linear light. */
export function linearFromHex(hex: number): Rgb {
  return [
    linearFromByte((hex >> 16) & 0xff),
    linearFromByte((hex >> 8) & 0xff),
    linearFromByte(hex & 0xff),
  ];
}

/**
 * One sRGB byte, decoded to linear light.
 *
 * Named and shared because `inkRaster` decodes a quarter of a million of them
 * off a real image and a hex literal decodes three: two curves that are meant
 * to be the same curve are one edit away from not being, and the thing that
 * would drift is the colour of somebody's logo.
 */
export function linearFromByte(channel: number): number {
  const value = channel / 255;
  return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
}

/**
 * The multiplier that turns `base` into `target`, per channel, clamped to 1.
 *
 * `blockoutKit.tintOver` is this function's vertex-colour twin, and the pair is
 * the whole colour discipline of a painted rider: a vertex tint may lift a
 * near-black suit to a near-white printing ground because it is a float, and
 * this one may only ever *darken*, because a texel is eight bits and 1.0 is as
 * bright as it goes. That asymmetry is why the printed field's ground is
 * painted by the mesh and its ink by the sheet, and not the other way round.
 */
export function inkOver(base: Rgb, target: Rgb): Rgb {
  return [
    Math.min(1, target[0] / Math.max(1e-4, base[0])),
    Math.min(1, target[1] / Math.max(1e-4, base[1])),
    Math.min(1, target[2] / Math.max(1e-4, base[2])),
  ];
}

/** A blank sheet, flooded with one colour. */
export function inkSheet(width: number, height: number, base: Rgb): InkSheet {
  const data = new Float32Array(width * height * 3);
  for (let i = 0; i < data.length; i += 3) {
    data[i] = base[0];
    data[i + 1] = base[1];
    data[i + 2] = base[2];
  }
  return { width, height, data };
}

/**
 * Encode the sheet as sRGB RGBA bytes, ready for a `THREE.DataTexture`.
 *
 * Opaque alpha throughout. The sheet is a multiplier over surfaces that are
 * already solid, so transparency here would mean "let the sky through a
 * rider's chest", and every place the art needs to disappear it disappears by
 * being 1.0 — the value that multiplies nothing.
 */
export function toSrgbBytes(sheet: InkSheet): Uint8Array {
  const encode = (channel: number): number => {
    const clamped = Math.min(1, Math.max(0, channel));
    const srgb = clamped <= 0.0031308 ? clamped * 12.92 : 1.055 * clamped ** (1 / 2.4) - 0.055;
    return Math.round(srgb * 255);
  };
  const bytes = new Uint8Array(sheet.width * sheet.height * 4);
  for (let i = 0, j = 0; i < sheet.data.length; i += 3, j += 4) {
    const r = sheet.data[i]!, g = sheet.data[i + 1]!, b = sheet.data[i + 2]!;
    // A grey pixel encodes one value three times (equal floats encode equally;
    // ±0 clamp to the same 0 and NaN never compares equal), so encode it once.
    if (r === g && g === b) {
      bytes[j] = bytes[j + 1] = bytes[j + 2] = encode(r);
    } else {
      bytes[j] = encode(r);
      bytes[j + 1] = encode(g);
      bytes[j + 2] = encode(b);
    }
    bytes[j + 3] = 255;
  }
  return bytes;
}
