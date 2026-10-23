import * as THREE from "three";
import {
  type LoftProfile,
  type LoftRing,
  loftGeometry,
  loftNormal,
  loftPoint,
  loftProfile,
  mergeGeometries,
  patchGeometry,
  shaded,
  tintOver,
  vAtHeight,
} from "../../../vendor/euc-thrills/blockoutKit.ts";
import type {
  RiderLook,
  RiderMaterialSpec,
  RiderPanelGroup,
  RiderPatch,
} from "../../../vendor/euc-thrills/riderLook.ts";
import { RIDER_BLOCKOUT as B } from "../../../vendor/euc-thrills/tuning.ts";

// The street kit: EUC Studio's own rider look, an everyday riding outfit. A black full-face
// helmet with a mirrored visor, a small rear-view mirror on its left and a chin strap; a loose
// black tee with bare forearms; orange gloves with black knuckles; loose light-blue jeans; black
// hard knee and shin guards with a hexagon surface, strapped over the jeans; chunky white
// sneakers. Plain shapes and colours, no marks of any brand.
//
// Built with EUC Thrills' loft kit and look types (MIT, src/vendor/euc-thrills); the profiles,
// panels, extras and colours here are original. Frames are the rig's (rig.ts): +x is the rider's
// left, +y up, +z forward, metres, each part from its joint; sizes are for the rig's reference
// skeleton, and figure.ts stretches them to the physics rider.

/** Colours, sRGB. */
const COLOUR = {
  tee: 0x232325,
  skin: 0xc49279,
  /** Mid-blue denim: the photos' mid-tones, shade taken out against the white shoe. */
  denim: 0x4e6c9c,
  helmet: 0x131416,
  /** The visor and the mirror's glass: a silver mirror; the environment colours it. */
  chrome: 0xd4dae2,
  /** The shoes' white, and the base every other gear colour is painted over. */
  gear: 0xeceae4,
  guard: 0x17181b,
  guardCell: 0x2c2e33,
  glove: 0xe5603f,
  gloveBlack: 0x1b1b1e,
  strap: 0x161618,
  mirrorBack: 0xa4a7ab,
  tab: 0xc23a2e,
  outsole: 0xcfcdc6,
  lace: 0xf6f5f1,
} as const;

/** A gear part painted `target` (gear is one material; vertex colours multiply it). */
const onGear = (target: number) => tintOver(COLOUR.gear, target);
/** The same as one grey multiplier, for patches (their shade is a scalar). */
const greyOnGear = (target: number) => onGear(target)[1];

function paintAll<T extends THREE.BufferGeometry>(geometry: T, target: number): T {
  const colour = geometry.getAttribute("color");
  const [r, g, b] = onGear(target);
  for (let i = 0; i < colour.count; i++) colour.setXYZ(i, r, g, b);
  return geometry;
}

const rows = (
  list: readonly (readonly [number, number, number, number?, number?])[],
  square = 2.35,
): LoftProfile =>
  loftProfile(
    list.map(([y, halfWidth, halfDepth, z = 0, sq = square]) => ({
      y,
      halfWidth,
      halfDepth,
      z,
      square: sq,
    })),
  );

// -- Body ----------------------------------------------------------------------------------------

/** A loose crew-neck tee, hem to collar; y = 0 is the hip joint. Boxy, untucked over the jeans. */
const TEE = rows(
  [
    [-0.04, 0.172, 0.13, 0.004],
    [-0.034, 0.174, 0.132, 0.004],
    [0, 0.17, 0.128, 0.004],
    [0.06, 0.164, 0.122, 0.006],
    [0.14, 0.162, 0.122, 0.01],
    [0.22, 0.166, 0.126, 0.014],
    [0.3, 0.172, 0.128, 0.015, 2.3],
    [0.37, 0.178, 0.124, 0.012, 2.3],
    [0.42, 0.186, 0.114, 0.006, 2.3],
    [0.455, 0.178, 0.104, 0.002, 2.25],
    [0.48, 0.152, 0.092, -0.003, 2.2],
    [0.505, 0.112, 0.078, -0.005, 2.1],
    [0.527, 0.07, 0.066, -0.004, 2.05],
    [0.538, 0.066, 0.063, -0.004, 2.05],
  ],
  2.25,
);
/** The jeans' seat under the tee's hem. */
const SEAT = rows(
  [
    [-0.085, 0.1, 0.085],
    [-0.07, 0.138, 0.108],
    [-0.04, 0.158, 0.12],
    [-0.005, 0.16, 0.122],
    [0.02, 0.155, 0.118],
  ],
  2.3,
);
/** Baggy jeans, hip to knee: wide all the way down, the cloth bunched in folds over the knee. */
const THIGH = rows(
  [
    [0.03, 0.07, 0.074],
    [0, 0.084, 0.088],
    [-0.06, 0.094, 0.098],
    [-0.14, 0.097, 0.1],
    [-0.22, 0.096, 0.098],
    [-0.3, 0.093, 0.094],
    [-0.35, 0.09, 0.09],
    [-0.37, 0.096, 0.095],
    [-0.39, 0.089, 0.089],
    [-0.41, 0.094, 0.092],
    [-0.44, 0.087, 0.087],
    [-0.465, 0.07, 0.072],
    [-0.48, 0.04, 0.042],
  ],
  2.3,
);
/**
 * The jeans' wide lower leg, knee to a hem that rests on the shoe: a fold behind the knee and
 * the cloth stacked in folds above the shoe.
 */
const SHIN = rows(
  [
    [0.045, 0.03, 0.032],
    [0.03, 0.056, 0.06],
    [0.005, 0.078, 0.082],
    [-0.03, 0.085, 0.086],
    [-0.05, 0.08, 0.081],
    [-0.07, 0.086, 0.085],
    [-0.12, 0.083, 0.083],
    [-0.2, 0.081, 0.081],
    [-0.25, 0.083, 0.084],
    [-0.27, 0.088, 0.089],
    [-0.285, 0.082, 0.084],
    [-0.3, 0.089, 0.091, -0.002],
    [-0.315, 0.083, 0.086, -0.004],
    [-0.33, 0.09, 0.095, -0.006],
    [-0.338, 0.088, 0.093, -0.006],
  ],
  2.3,
);
const UPPER_ARM = rows([
  [-0.004, 0.022, 0.024],
  [-0.02, 0.042, 0.044],
  [-0.045, 0.046, 0.048],
  [-0.08, 0.046, 0.047],
  [-0.16, 0.042, 0.042],
  [-0.24, 0.037, 0.036],
  [-0.29, 0.034, 0.033],
]);
/** The tee's short sleeve, hanging from the shoulder joint. */
const SLEEVE = rows([
  [0.012, 0.05, 0.056],
  [-0.02, 0.062, 0.064],
  [-0.07, 0.066, 0.065],
  [-0.12, 0.065, 0.062],
  [-0.146, 0.063, 0.06],
  [-0.152, 0.059, 0.056],
]);
const FOREARM = rows([
  [0.02, 0.034, 0.033],
  [-0.02, 0.04, 0.037],
  [-0.08, 0.041, 0.038],
  [-0.15, 0.036, 0.032],
  [-0.22, 0.029, 0.025],
  [-0.265, 0.026, 0.022],
]);
const NECK = rows(
  [
    [-0.03, 0.058, 0.056],
    [0.02, 0.054, 0.052],
    [0.07, 0.05, 0.05],
    [0.1, 0.049, 0.049],
  ],
  2.2,
);

// -- Helmet --------------------------------------------------------------------------------------

/** A full-face shell; y = 0 is the neck joint. Its last ring is the helmet's top. */
const HELMET = loftProfile([
  { y: 0.055, halfWidth: 0.09, halfDepth: 0.11, z: 0.02, square: 2.4 },
  { y: 0.07, halfWidth: 0.108, halfDepth: 0.128, z: 0.024, square: 2.5 },
  { y: 0.1, halfWidth: 0.12, halfDepth: 0.14, z: 0.022, square: 2.5 },
  { y: 0.14, halfWidth: 0.128, halfDepth: 0.146, z: 0.016, square: 2.45 },
  { y: 0.18, halfWidth: 0.132, halfDepth: 0.15, z: 0.01, square: 2.4 },
  { y: 0.22, halfWidth: 0.133, halfDepth: 0.15, z: 0.006, square: 2.35 },
  { y: 0.255, halfWidth: 0.128, halfDepth: 0.145, z: 0.002, square: 2.3 },
  { y: 0.285, halfWidth: 0.115, halfDepth: 0.132, z: -0.002, square: 2.25 },
  { y: 0.31, halfWidth: 0.094, halfDepth: 0.11, z: -0.005, square: 2.2 },
  { y: 0.328, halfWidth: 0.068, halfDepth: 0.08, z: -0.007, square: 2.1 },
  { y: 0.34, halfWidth: 0.038, halfDepth: 0.045, z: -0.008, square: 2.05 },
  { y: 0.346, halfWidth: 0.001, halfDepth: 0.001, z: -0.008, square: 2 },
]);

const patch = (
  values: Partial<RiderPatch> & Pick<RiderPatch, "anchor" | "u0" | "u1" | "from" | "to">,
): RiderPatch => Object.freeze({ uSegments: 24, vSegments: 8, ...values });

/** The chin bar, standing well proud of the face, and the dark seal round the visor. */
const HELMET_PANELS: readonly RiderPatch[] = Object.freeze([
  patch({ anchor: "front", u0: -1.0, u1: 1.0, from: 0.058, to: 0.132, lift: 0.016, taper: 0.4 }),
  patch({
    anchor: "front",
    u0: -1.16,
    u1: 1.16,
    from: 0.124,
    to: 0.272,
    lift: 0.004,
    bulge: 0.2,
    taper: 0.22,
  }),
]);
/** The visor: a wide mirrored eyeport, arched at the brow. */
const VISOR: RiderPanelGroup = Object.freeze({
  role: "face",
  casts: false,
  patches: Object.freeze([
    patch({
      anchor: "front",
      u0: -1.1,
      u1: 1.1,
      from: 0.132,
      to: 0.262,
      lift: 0.008,
      bulge: 0.22,
      taper: 0.26,
      uSegments: 40,
      vSegments: 14,
    }),
  ]),
});

const Y = new THREE.Vector3(0, 1, 0);

/** A tube along `points`, painted `target` on gear. */
function cord(
  points: readonly (readonly [number, number, number])[],
  radius: number,
  target: number,
) {
  const curve = new THREE.CatmullRomCurve3(points.map((p) => new THREE.Vector3(...p)));
  return paintAll(
    shaded(new THREE.TubeGeometry(curve, Math.max(8, points.length * 6), radius, 8, false)),
    target,
  );
}

/** A disc of `radius` and `depth` centred at `at`, its axis along `normal`. */
function disc(at: THREE.Vector3, normal: THREE.Vector3, radius: number, depth: number) {
  const geometry = new THREE.CylinderGeometry(radius, radius, depth, 24);
  geometry.applyQuaternion(new THREE.Quaternion().setFromUnitVectors(Y, normal));
  return shaded(geometry.translate(at.x, at.y, at.z));
}

/** The rear-view mirror on the helmet's left: where it sits, and which way its glass faces. */
const MIRROR = {
  centre: new THREE.Vector3(0.183, 0.178, 0.122),
  // Half way between the rider's eye and straight back: the glass shows what is behind.
  normal: new THREE.Vector3(-0.6, 0.09, -0.79).normalize(),
  radius: 0.024,
};

/** The mirror's short black arm from the chin bar's side and its light grey housing. */
function mirrorHousing(): THREE.BufferGeometry {
  const back = MIRROR.centre.clone().addScaledVector(MIRROR.normal, -0.006);
  return mergeGeometries([
    cord(
      [
        [0.122, 0.128, 0.088],
        [0.152, 0.15, 0.104],
        [back.x, back.y, back.z],
      ],
      0.004,
      COLOUR.strap,
    ),
    paintAll(disc(back, MIRROR.normal, MIRROR.radius + 0.003, 0.01), COLOUR.mirrorBack),
  ]);
}

/** The mirror's glass, a hair proud of its housing. */
function mirrorGlass(): THREE.BufferGeometry {
  return disc(MIRROR.centre, MIRROR.normal, MIRROR.radius, 0.002);
}

/** The chin strap round the throat, and its small red tab on the left. */
function chinStrap(): THREE.BufferGeometry {
  const strap = cord(
    [
      [0.084, 0.086, 0.04],
      [0.06, 0.06, 0.066],
      [0.03, 0.052, 0.08],
      [0, 0.05, 0.084],
      [-0.03, 0.052, 0.08],
      [-0.06, 0.06, 0.066],
      [-0.084, 0.086, 0.04],
    ],
    0.0045,
    COLOUR.strap,
  );
  const tab = paintAll(
    shaded(new THREE.BoxGeometry(0.01, 0.026, 0.004).translate(0.07, 0.05, 0.062)),
    COLOUR.tab,
  );
  return mergeGeometries([strap, tab]);
}

// -- Guards --------------------------------------------------------------------------------------

/**
 * Raised hexagon cells on a guard's shell: a lattice of six-sided studs laid on `profile` at
 * `lift` above it, inside the span (`u` about the front, `y` heights) and its rounded corners.
 */
function hexCells(
  profile: LoftProfile,
  span: { u: number; from: number; to: number; lift: number; taper: number },
): THREE.BufferGeometry[] {
  const cells: THREE.BufferGeometry[] = [];
  const radius = 0.0064;
  const pitch = 0.0142;
  const point = new THREE.Vector3();
  const normal = new THREE.Vector3();
  const middle = (span.from + span.to) / 2;
  const halfHeight = Math.abs(span.to - span.from) / 2;
  // The leg's half width there turns metres across into radians.
  const across = profile[Math.round(vAtHeight(profile, middle))]?.halfWidth ?? 0.07;
  const du = pitch / across;
  for (let row = 0; ; row++) {
    const y = middle + halfHeight - (row + 0.5) * pitch * 0.866;
    if (y < middle - halfHeight) break;
    const shift = row % 2 === 0 ? 0 : du / 2;
    for (let u = -span.u + du / 2 + shift; u <= span.u - du / 2; u += du) {
      const s = (u + span.u) / (2 * span.u);
      const reach = halfHeight * (1 - span.taper * Math.abs(s - 0.5) * 2) * 0.86;
      if (Math.abs(y - middle) > reach) continue;
      const v = vAtHeight(profile, y);
      loftPoint(profile, Math.PI / 2 + u, v, point);
      loftNormal(profile, Math.PI / 2 + u, v, normal);
      const cell = new THREE.CylinderGeometry(radius, radius * 1.1, 0.004, 6);
      cell.applyQuaternion(new THREE.Quaternion().setFromUnitVectors(Y, normal));
      cell.translate(
        point.x + normal.x * (span.lift + 0.0015),
        point.y + normal.y * (span.lift + 0.0015),
        point.z + normal.z * (span.lift + 0.0015),
      );
      cells.push(paintAll(shaded(cell), COLOUR.guardCell));
    }
  }
  return cells;
}

const guard = greyOnGear(COLOUR.guard);
/** The knee as the cup sees it, in the shin's frame: the leg's width carried over the joint. */
const KNEE_FORM = rows([
  [-0.15, 0.085, 0.086],
  [-0.08, 0.088, 0.088],
  [0, 0.09, 0.091],
  [0.07, 0.086, 0.088],
]);
/** The thigh under the guard's upper cup, without the jeans' folds. */
const THIGH_FORM = rows([
  [-0.47, 0.094, 0.094],
  [-0.4, 0.098, 0.097],
  [-0.3, 0.1, 0.099],
  [-0.24, 0.1, 0.099],
]);
/**
 * How far the legs reach toward the wheel, m from the leg's axis: the lateral rider's contact
 * shapes (a 5 cm knee ball, 3.5 cm shin and 5 cm calf capsules) touch the side pads at about
 * 5 cm, so the jeans and the guards are flattened on that side to sit there, not in the pads.
 * The outside stays baggy, as cloth pressed on the pads does.
 */
const INNER_REACH = { jeans: 0.056, guards: 0.062 };

/**
 * Flattens the wheel-side (inner) face of a leg part to `reach`: the inner side is -x on the left
 * leg (`side` +1) and +x on the right, in the leg's frame. A smooth squeeze (tanh), so a fold or a
 * cell keeps its shape near the axis and nothing passes `reach`.
 */
function hugInner<T extends THREE.BufferGeometry>(geometry: T, side: number, reach: number): T {
  const position = geometry.getAttribute("position");
  for (let i = 0; i < position.count; i++) {
    const inner = -side * position.getX(i);
    if (inner > 0) position.setX(i, -side * reach * Math.tanh(inner / reach));
  }
  position.needsUpdate = true;
  geometry.computeVertexNormals();
  return geometry;
}

/** A band on `profile` between two heights, across `u0`..`u1`, in the guard's black. */
function band(
  profile: LoftProfile,
  options: { from: number; to: number; u0: number; u1: number; lift: number } & Partial<
    Parameters<typeof patchGeometry>[1]
  >,
): THREE.BufferGeometry {
  const { from, to, ...rest } = options;
  return patchGeometry(profile, {
    uSegments: 24,
    vSegments: 2,
    shade: guard,
    ...rest,
    v0: vAtHeight(profile, from),
    v1: vAtHeight(profile, to),
  });
}
/** A strap all the way round the leg. */
const strapRound = (profile: LoftProfile, at: number, height = 0.024) =>
  band(profile, {
    from: at - height / 2,
    to: at + height / 2,
    u0: -Math.PI * 1.5,
    u1: Math.PI / 2,
    lift: 0.005,
    uSegments: 36,
  });
const KNEE_CUP = { u: 0.95, from: 0.04, to: -0.13, lift: 0.016, taper: 0.28 };

/** A guard's hard cup: a shell over the front of `profile`, with its hexagon cells. */
function cup(profile: LoftProfile, span: typeof KNEE_CUP): THREE.BufferGeometry {
  const shell = patchGeometry(profile, {
    u0: Math.PI / 2 - span.u,
    u1: Math.PI / 2 + span.u,
    v0: vAtHeight(profile, Math.min(span.from, span.to)),
    v1: vAtHeight(profile, Math.max(span.from, span.to)),
    uSegments: 28,
    vSegments: 12,
    lift: span.lift,
    taper: span.taper,
    bulge: 0.08,
    shade: guard,
  });
  return mergeGeometries([shell, ...hexCells(profile, span)]);
}
const UPPER_CUP = { u: 0.95, from: -0.305, to: -0.43, lift: 0.014, taper: 0.3 };

/**
 * The knee guard's lower half (the cup over the knee), the shin plate and two straps, on the
 * shin; their wheel side flattened like the jeans'.
 */
const KNEE_GUARD: RiderPanelGroup = Object.freeze({
  role: "gear",
  casts: true,
  patches: Object.freeze([]),
  build: (side: number) =>
    hugInner(
      mergeGeometries([
        cup(KNEE_FORM, KNEE_CUP),
        band(SHIN, {
          from: -0.235,
          to: -0.095,
          u0: Math.PI / 2 - 0.85,
          u1: Math.PI / 2 + 0.85,
          lift: 0.007,
          taper: 0.45,
          bulge: 0.06,
          vSegments: 14,
        }),
        strapRound(SHIN, -0.13),
        strapRound(SHIN, -0.22),
      ]),
      side,
      INNER_REACH.guards,
    ),
});
/** The knee guard's upper half, above the knee, with its strap round the thigh. */
const THIGH_GUARD: RiderPanelGroup = Object.freeze({
  role: "gear",
  casts: true,
  patches: Object.freeze([]),
  build: (side: number) =>
    hugInner(
      mergeGeometries([cup(THIGH_FORM, UPPER_CUP), strapRound(THIGH, -0.315)]),
      side,
      INNER_REACH.guards,
    ),
});

// -- Hands ---------------------------------------------------------------------------------------

/** The glove's palm, cuff to knuckles, hanging from the wrist. */
const GLOVE = rows(
  [
    [0.01, 0.03, 0.022],
    [-0.01, 0.035, 0.026],
    [-0.03, 0.036, 0.024],
    [-0.05, 0.042, 0.022],
    [-0.075, 0.045, 0.021],
    [-0.09, 0.043, 0.019],
  ],
  2.6,
);

/** Four fingers curling a little forward, and the thumb on the inside. */
function gloveFingers(side: number): THREE.BufferGeometry {
  const parts: THREE.BufferGeometry[] = [];
  const lengths = [0.07, 0.078, 0.074, 0.06];
  for (const [n, length] of lengths.entries()) {
    const x = (n - 1.5) * 0.0175;
    const ring = (y: number, w: number, z: number): LoftRing => ({
      y,
      halfWidth: w,
      halfDepth: w * 1.05,
      x,
      z,
      square: 2.3,
    });
    parts.push(
      loftGeometry(
        loftProfile([
          ring(-0.084, 0.0085, 0),
          ring(-0.084 - length * 0.45, 0.0082, 0.004),
          ring(-0.084 - length * 0.85, 0.0074, 0.011),
          ring(-0.084 - length, 0.0055, 0.016),
          ring(-0.087 - length, 0, 0.018),
        ]),
        { radialSegments: 10 },
      ),
    );
  }
  parts.push(
    loftGeometry(
      loftProfile([
        { y: -0.025, halfWidth: 0.011, halfDepth: 0.012, x: -side * 0.03, z: 0.006 },
        { y: -0.045, halfWidth: 0.011, halfDepth: 0.012, x: -side * 0.04, z: 0.012 },
        { y: -0.065, halfWidth: 0.0095, halfDepth: 0.01, x: -side * 0.042, z: 0.02 },
        { y: -0.072, halfWidth: 0, halfDepth: 0, x: -side * 0.042, z: 0.023 },
      ]),
      { radialSegments: 10 },
    ),
  );
  // A hard bar over the knuckles.
  parts.push(
    shaded(
      new THREE.SphereGeometry(1, 16, 8).scale(0.036, 0.008, 0.007).translate(0, -0.088, 0.02),
    ),
  );
  return mergeGeometries(parts);
}

/** Orange, with a black cuff, knuckle bar and finger backs. */
function paintGlove(geometry: THREE.BufferGeometry): void {
  const position = geometry.getAttribute("position");
  const colour = geometry.getAttribute("color");
  const orange = onGear(COLOUR.glove);
  const black = onGear(COLOUR.gloveBlack);
  for (let i = 0; i < position.count; i++) {
    const y = position.getY(i);
    const z = position.getZ(i);
    const cuff = y > -0.028;
    const knuckles = y < -0.08 && y > -0.096 && z > 0.012;
    const fingerBack = y < -0.104 && y > -0.124 && z > 0.009;
    const [r, g, b] = cuff || knuckles || fingerBack ? black : orange;
    colour.setXYZ(i, r, g, b);
  }
}

// -- Shoes ---------------------------------------------------------------------------------------

/**
 * A chunky sneaker's upper, heel to toe along y; each ring's half depth is half its height (the
 * rig stands the loft up: its bottom flat on the sole, `0.047 - halfDepth` keeps it so).
 */
const SNEAKER = loftProfile(
  (
    [
      [-0.11, 0.018, 0.02],
      [-0.104, 0.032, 0.038],
      [-0.094, 0.042, 0.05],
      [-0.075, 0.047, 0.057],
      [-0.05, 0.049, 0.06],
      [-0.025, 0.051, 0.058],
      [0, 0.054, 0.052],
      [0.035, 0.057, 0.044],
      [0.07, 0.058, 0.036],
      [0.1, 0.056, 0.03],
      [0.124, 0.051, 0.026],
      [0.142, 0.042, 0.022],
      [0.155, 0.03, 0.017],
      [0.163, 0.016, 0.011],
      [0.166, 0.001, 0.001],
    ] as const
  ).map(([y, halfWidth, halfDepth]) => ({
    y,
    halfWidth,
    halfDepth,
    z: 0.047 - halfDepth,
    square: 2.3,
  })),
);
/** A thick sole, 3.4 cm, with a rounded edge, standing a little up the upper's sides. */
const SNEAKER_SOLE = loftProfile([
  { y: -0.018, halfWidth: 0.056, halfDepth: 0.134, z: 0.01, square: 2.8 },
  { y: -0.014, halfWidth: 0.062, halfDepth: 0.142, z: 0.01, square: 3 },
  { y: 0, halfWidth: 0.065, halfDepth: 0.146, z: 0.01, square: 3 },
  { y: 0.012, halfWidth: 0.064, halfDepth: 0.144, z: 0.01, square: 3 },
  { y: 0.016, halfWidth: 0.058, halfDepth: 0.137, z: 0.01, square: 2.8 },
]);
/** Where the rig's boot sole meets the upper, in the ankle frame (rig.ts). */
const SOLE_TOP = -B.ankleAbovePedal + 0.018;

/** A point on the sneaker's upper, in the ankle frame: `along` the foot, at angle `u`. */
function onUpper(u: number, along: number, lift: number): THREE.Vector3 {
  const v = vAtHeight(SNEAKER, along);
  const p = loftPoint(SNEAKER, u, v, new THREE.Vector3());
  const n = loftNormal(SNEAKER, u, v, new THREE.Vector3());
  p.addScaledVector(n, lift);
  // The rig turns the loft a quarter about x and stands it on the sole.
  return new THREE.Vector3(p.x, -p.z + SOLE_TOP + 0.047, p.y);
}

/** Crossed laces over the instep. */
function laces(): THREE.BufferGeometry {
  const parts: THREE.BufferGeometry[] = [];
  const top = -Math.PI / 2;
  for (let n = 0; n < 5; n++) {
    const along = -0.02 + n * 0.017;
    const a = onUpper(top - 0.55, along, 0.002);
    const b = onUpper(top + 0.55, along + 0.009, 0.002);
    const c = onUpper(top, along + 0.0045, 0.004);
    parts.push(cord([a.toArray(), c.toArray(), b.toArray()], 0.0022, COLOUR.lace));
  }
  return mergeGeometries(parts);
}

/** A light grey outsole band under the white midsole. */
function paintSneaker(geometry: THREE.BufferGeometry): void {
  const position = geometry.getAttribute("position");
  const colour = geometry.getAttribute("color");
  const [r, g, b] = onGear(COLOUR.outsole);
  for (let i = 0; i < position.count; i++) {
    if (position.getY(i) < SOLE_TOP - 0.013) colour.setXYZ(i, r, g, b);
  }
}

// -- Jeans ---------------------------------------------------------------------------------------

/**
 * Worn denim: lighter over the front of the thigh and the knee, darker down the inner seam, and a
 * faint mottle so the cloth is not one flat blue.
 */
function fade(top: number, bottom: number, strength: number) {
  return (geometry: THREE.BufferGeometry, side: number): void => {
    const position = geometry.getAttribute("position");
    const colour = geometry.getAttribute("color");
    for (let i = 0; i < position.count; i++) {
      const x = position.getX(i);
      const y = position.getY(i);
      const z = position.getZ(i);
      const front = Math.max(0, z) / 0.1;
      const t = (y - bottom) / (top - bottom);
      const along = t > 0 && t < 1 ? Math.sin(Math.PI * t) : 0;
      // The inner seam faces the other leg: -x on the left leg, +x on the right.
      const inner = Math.max(0, -side * x) / 0.1;
      const mottle = Math.sin(x * 91 + y * 57) * Math.sin(z * 73 - y * 39);
      const k =
        colour.getX(i) * (1 + strength * front * along - 0.12 * inner * inner + 0.04 * mottle);
      colour.setXYZ(i, k, k, k);
    }
  };
}

const material = (colour: number, roughness: number, metalness = 0): RiderMaterialSpec =>
  Object.freeze({ colour, roughness, metalness });

export const STREET_KIT: RiderLook = Object.freeze({
  id: "street-kit",
  materials: Object.freeze({
    body: material(COLOUR.tee, 0.95),
    limbs: material(COLOUR.skin, 0.62),
    accent: material(COLOUR.denim, 0.92),
    head: material(COLOUR.helmet, 0.22),
    face: material(COLOUR.chrome, 0.05, 1),
    gear: material(COLOUR.gear, 0.62),
  }),
  profiles: Object.freeze({
    torso: TEE,
    seat: SEAT,
    thigh: THIGH,
    shin: SHIN,
    upperArm: UPPER_ARM,
    forearm: FOREARM,
    neck: NECK,
    head: HELMET,
    boot: SNEAKER,
    bootSole: SNEAKER_SOLE,
    hand: GLOVE,
    sleeve: SLEEVE,
  }),
  density: Object.freeze({ torso: 40, limb: 28, head: 56, boot: 28, hand: 20, neck: 20 }),
  shades: Object.freeze({ seat: 0.9, legs: 1, collar: 1, sole: 1, neck: 1 }),
  parts: Object.freeze({
    hands: "gear",
    neck: "limbs",
    kneePad: "gear",
    legs: "accent",
    seat: "accent",
  }),
  build: Object.freeze({
    hand: Object.freeze([gloveFingers]),
    boot: Object.freeze([laces]),
  }),
  paint: Object.freeze({
    thigh: (geometry: THREE.BufferGeometry, side: number) => {
      fade(-0.02, -0.4, 0.1)(geometry, side);
      hugInner(geometry, side, INNER_REACH.jeans);
    },
    shin: (geometry: THREE.BufferGeometry, side: number) => {
      fade(0.04, -0.12, 0.08)(geometry, side);
      hugInner(geometry, side, INNER_REACH.jeans);
    },
    hand: paintGlove,
    boot: paintSneaker,
  }),
  panels: Object.freeze({
    // The tee's ribbed crew neck.
    collar: patch({
      anchor: "back",
      u0: -Math.PI,
      u1: Math.PI,
      from: 0.516,
      to: 0.538,
      lift: 0.003,
      uSegments: 40,
      vSegments: 2,
      shade: 0.8,
    }),
    kneePad: KNEE_GUARD,
    thighPad: THIGH_GUARD,
    head: HELMET_PANELS,
    face: VISOR,
  }),
  extras: Object.freeze([
    {
      name: "rider-mirror",
      joint: "neck" as const,
      role: "gear" as const,
      casts: true,
      build: mirrorHousing,
    },
    {
      name: "rider-mirror-glass",
      joint: "neck" as const,
      role: "face" as const,
      casts: false,
      build: mirrorGlass,
    },
    {
      name: "rider-chin-strap",
      joint: "neck" as const,
      role: "gear" as const,
      casts: false,
      build: chinStrap,
    },
  ]),
  // The rest pose's hands a little nearer the thighs; figure.ts carries the arms when riding.
  armCarriage: Object.freeze({ splay: -0.04, rise: 0 }),
});
