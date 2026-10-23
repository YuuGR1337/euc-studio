/*! EUC Thrills — (c) 2026 VibezZzCoder — MIT — https://github.com/VibezZzCoder/EUC-thrills */
// EUC Studio: the look types only (render/riderLook.ts lines 99-710). None of the game's looks is
// copied: EUC Studio's own look, the street kit, is src/workspaces/ride/riderFigure/streetKit.ts.
// `id` is a plain string here: the game's character roster (data/riders.ts) is not part of this
// copy. The types are unchanged; the comments are shortened, and the game's comments that name or
// describe its other riders are removed.
import type * as THREE from 'three';
import type { LoftProfile, UvRect } from './blockoutKit.ts';

// -- Material slots ----------------------------------------------------------

/**
 * A material, as parameters rather than as an object: a look is a frozen
 * module-level constant, and the rig builds and disposes the GPU material.
 */
export interface RiderMaterialSpec {
  readonly colour: number;
  readonly roughness: number;
  readonly metalness: number;
  readonly emissive?: number;
  readonly emissiveIntensity?: number;
  /** A translucent surface. Opaque materials omit this. */
  readonly opacity?: number;
}

/**
 * The six roles a rider mesh can be painted in. Two roles pointing at the
 * same spec object are one material.
 */
export interface RiderMaterials {
  /** Torso: a jacket, or a dress. */
  readonly body: RiderMaterialSpec;
  /** Arms, legs, neck: sleeves and trousers, or bare skin. */
  readonly limbs: RiderMaterialSpec;
  /** The identity panels. */
  readonly accent: RiderMaterialSpec;
  /** The head shell: a helmet, or a face. */
  readonly head: RiderMaterialSpec;
  /** What sits in the head's aperture: a visor, or eyes. */
  readonly face: RiderMaterialSpec;
  /** Boots, and whatever else is protective gear. */
  readonly gear: RiderMaterialSpec;
}

export type RiderMaterialRole = keyof RiderMaterials;

// -- Panels ------------------------------------------------------------------

/** Where a panel's `u` span is measured from. */
export type PatchAnchor = 'front' | 'back' | 'outboard';

/**
 * One panel lying on a body, in metres and radians. `from`/`to` are heights on
 * the profile, not ring indices, so inserting a ring cannot move a decal.
 */
export interface RiderPatch {
  readonly anchor: PatchAnchor;
  /** Span start, as an offset from the anchor angle. */
  readonly u0: number;
  readonly u1: number;
  /** Multiply both offsets by the side sign (+1 left, -1 right). */
  readonly mirrored?: boolean;
  readonly from: number;
  readonly to: number;
  readonly uSegments: number;
  readonly vSegments: number;
  readonly lift?: number;
  readonly sink?: number;
  readonly taper?: number;
  /** Swell the band's height toward its middle — see `PatchOptions.bulge`. */
  readonly bulge?: number;
  /** Slide the band's centre line at its middle — see `PatchOptions.bow`. */
  readonly bow?: number;
  /** Shear the height span across the angular span, as two heights. */
  readonly skewFrom?: number;
  readonly skewTo?: number;
  readonly shade?: number;
  /**
   * Which page of the look's atlas this panel lands on. A patch with no `art`
   * in a look that has an atlas is mapped to the blank page.
   */
  readonly art?: string;
  /** Restrict the art to one side, +1 left or −1 right. */
  readonly artOn?: number;
  /** The page the other side wears when `artOn` restricts the art. */
  readonly artElse?: string;
}

/** A group of panels drawn as one mesh on one joint. */
export interface RiderPanelGroup {
  readonly patches: readonly RiderPatch[];
  /** A continuous shaped insert, merged with the ordinary patches on the same joint. */
  readonly build?: (side: number) => THREE.BufferGeometry;
  /** A sheet page; per-side pages keep a printed sleeve on one anatomical arm. */
  readonly art?: LoftPage;
  readonly role: RiderMaterialRole;
  /** Casting a shadow means "this carries silhouette". */
  readonly casts: boolean;
  /** Preserve a bulky panel in replay even when its live shadow is unnecessary. */
  readonly ghostSilhouette?: boolean;
}

// -- Extras ------------------------------------------------------------------

/**
 * A mesh a look adds that the rig has no slot for. The geometry must carry
 * normals and a `color` attribute.
 */
export interface RiderExtra {
  readonly name: string;
  /** Which joint it hangs from. `neck` is where the head is. */
  readonly joint: 'neck' | 'pelvis';
  readonly role: RiderMaterialRole;
  /** A distinct surface, such as clear helmet glazing, owned by this rig. */
  readonly material?: RiderMaterialSpec;
  readonly casts: boolean;
  /** Its page of the look's atlas, as on `RiderPatch.art`. */
  readonly art?: string;
  /** Hang it from a node that keeps hanging under gravity instead of from the joint. */
  readonly sways?: boolean;
  build(): THREE.BufferGeometry;
}

// -- The look ----------------------------------------------------------------

/** A loft's page: one name, or one per side for a limb whose two sides differ. */
export type LoftPage = string | ((side: number) => string);

/** A look's own printed sheet: a texture on chosen material roles. */
export interface RiderAtlas {
  /** One texture per rig, over pixels the atlas module memoises. */
  build(): THREE.Texture;
  /** Which materials carry the map. Anything else in the rig is unmapped. */
  readonly roles: readonly RiderMaterialRole[];
  /** Where a named page sits on the sheet; an unknown name returns the blank page. */
  region(art: string | undefined): UvRect;
  /** The page each body loft lands on, when its material samples the sheet. */
  readonly lofts?: {
    readonly torso?: string;
    readonly seat?: string;
    /** A limb's page may differ by side (+1 left, −1 right). */
    readonly upperArm?: LoftPage;
    readonly forearm?: LoftPage;
    readonly thigh?: LoftPage;
    readonly shin?: LoftPage;
    readonly neck?: string;
    readonly head?: string;
    readonly hand?: LoftPage;
  };
}

export interface RiderLook {
  readonly id: string;
  readonly materials: RiderMaterials;

  readonly profiles: {
    /** The torso garment, hem to collar. `y = 0` is the hip joint. */
    readonly torso: LoftProfile;
    /** What closes the torso's open hem. */
    readonly seat: LoftProfile;
    readonly thigh: LoftProfile;
    readonly shin: LoftProfile;
    readonly upperArm: LoftProfile;
    readonly forearm: LoftProfile;
    readonly neck: LoftProfile;
    /** A helmet, or a head. `y = 0` is the neck joint. */
    readonly head: LoftProfile;
    readonly boot: LoftProfile;
    readonly bootSole: LoftProfile;
    /** A glove, or a bare hand. */
    readonly hand: LoftProfile;
    /** A whole garment sleeve hanging from the shoulder joint, in the body material. */
    readonly sleeve?: LoftProfile;
  };

  /** Open, matching elbow/knee rings receive a short render-only bend band. */
  readonly continuousJoints?: boolean;
  /** Untucked cloth encloses the posed upper trouser volumes. */
  readonly drapedHem?: boolean;
  /** Conservative clothed upper-thigh envelope, metres; omitted keeps the original loose-cargo envelope. */
  readonly hemDrapeEnvelope?: { readonly x: number; readonly z: number; readonly cap: number };

  /** How many sections each lofted part is built from; omitted parts keep the rig's defaults. */
  readonly density?: {
    readonly limb?: number;
    readonly torso?: number;
    readonly head?: number;
    readonly boot?: number;
    readonly hand?: number;
    readonly neck?: number;
  };

  /** Extra lofts merged into a part's own mesh, built per side (+1 is the rider's left). */
  readonly build?: {
    readonly hand?: readonly ((side: number) => THREE.BufferGeometry)[];
    /** Optional complete shell, for helmets with a real opening instead of a closed loft. */
    readonly head?: () => THREE.BufferGeometry;
    /** Detail merged into the existing boot, in ankle-local coordinates. */
    readonly boot?: readonly ((side: number) => THREE.BufferGeometry)[];
  };

  /** The look's printed sheet, if it has one. See `RiderAtlas`. */
  readonly atlas?: RiderAtlas;

  /** Vertex-colour multipliers on shared materials. 1 is the authored colour. */
  readonly shades: {
    /** The lower garment inside the torso mesh. */
    readonly seat: number;
    /** Thighs and shins. */
    readonly legs: number;
    readonly collar: number;
    readonly sole: number;
    readonly neck: number;
  };

  /** Which material role each ambiguous part is painted in. */
  readonly parts: {
    readonly hands: RiderMaterialRole;
    /** The neck. Defaults to `limbs`. */
    readonly neck?: RiderMaterialRole;
    readonly kneePad: RiderMaterialRole;
    /** Thighs and shins. */
    readonly legs: RiderMaterialRole;
    /** What closes the torso's hem: `'body'` merges it into the torso mesh. */
    readonly seat: RiderMaterialRole;
  };

  readonly panels: {
    /** Merged into the torso mesh. Closes the loft's neck opening. */
    readonly collar?: RiderPatch;
    readonly shoulders?: RiderPanelGroup;
    readonly torso?: RiderPanelGroup;
    /** On the torso, at the waist. One mesh, full wrap. */
    readonly waist?: RiderPanelGroup;
    readonly sleeve?: RiderPanelGroup;
    readonly elbowPad?: RiderPanelGroup;
    /** On the thigh: the half of a knee guard above the knee. */
    readonly thighPad?: RiderPanelGroup;
    readonly kneePad?: RiderPanelGroup;
    /** Merged into the head mesh: chin bar, brow, spoiler, rim — or none. */
    readonly head: readonly RiderPatch[];
    /** The aperture: a visor, or eyes. */
    readonly face?: RiderPanelGroup;
  };

  readonly extras: readonly RiderExtra[];

  /**
   * Optional vertex repaints, run once at build. Each limb painter is handed
   * the side it is building (+1 left, −1 right); the torso painter reads the
   * sign of x off each vertex.
   */
  readonly paint?: {
    readonly thigh?: (geometry: THREE.BufferGeometry, side: number) => void;
    readonly shin?: (geometry: THREE.BufferGeometry, side: number) => void;
    readonly boot?: (geometry: THREE.BufferGeometry, side: number) => void;
    readonly hand?: (geometry: THREE.BufferGeometry, side: number) => void;
    readonly upperArm?: (geometry: THREE.BufferGeometry, side: number) => void;
    readonly forearm?: (geometry: THREE.BufferGeometry, side: number) => void;
    readonly torso?: (geometry: THREE.BufferGeometry) => void;
  };

  /** Static additions to the relaxed hand target, in metres. */
  readonly armCarriage: {
    readonly splay: number;
    readonly rise: number;
  };

  /** How this rider moves when the ride style moves them; absent means `MOTION_STILL`. */
  readonly motion?: RiderMotion;
}

/** The amplitudes a ride style's channels are spent at, in metres and radians. */
export interface RiderMotion {
  /** The sway's pelvis roll into the weave, rad per unit of sway. */
  readonly swayPelvisRoll: number;
  /** The head's tilt with it, rad per unit of sway. */
  readonly swayHeadTilt: number;
  /** The low-side arm out and the high-side arm in, metres per unit of sway. */
  readonly swayArmSplay: number;
  /** A little fore-aft alternation of the hands with the sway, metres. */
  readonly swayArmSwing: number;
  /** Multipliers on the arms' and hips' wobble reactions. 0 is the default bracing. */
  readonly staggerArms: number;
  readonly staggerHips: number;
  /** The head lolling into the wobble's swing, rad at full fight. */
  readonly staggerHeadLoll: number;
  /** The head tilted back at rest, rad. */
  readonly sipNeckPitch: number;
  /** 0..1: how much of the upper body's counter-roll in a carve is forgotten. */
  readonly overLean: number;
}

/** No motion at all. */
export const MOTION_STILL: RiderMotion = Object.freeze({
  swayPelvisRoll: 0,
  swayHeadTilt: 0,
  swayArmSplay: 0,
  swayArmSwing: 0,
  staggerArms: 0,
  staggerHips: 0,
  staggerHeadLoll: 0,
  sipNeckPitch: 0,
  overLean: 0,
});
