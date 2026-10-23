/*! EUC Thrills — (c) 2026 VibezZzCoder — MIT — https://github.com/VibezZzCoder/EUC-thrills */
// Adapted for EUC Studio from EUC Thrills' src/render/rider.ts (createPlaceholderRider,
// solveChain). The construction of a rider from a look (its
// parts, materials, panels and joint frames) is theirs; the look it builds by default is EUC
// Studio's own street kit (streetKit.ts). What changed:
// - no stance solve: EUC Studio poses the joints from the physics bodies (figure.ts);
// - the legs are three free groups (thigh, shin, foot), not one hip-knee-ankle chain, because
//   each follows its own physics body;
// - no hair sway, cloth drape or joint-surface bands (the street kit uses none of them);
// - no rest pitch on the pelvis: the trunk's pitch is the physics';
// - the arms keep only the rest pose here: figure.ts carries them when riding.
// Joint frames are the game's: +x is the rider's left, +y up, +z forward; limbs hang along -y.
import * as THREE from "three";
import {
  type LoftProfile,
  loftGeometry,
  mapUvInto,
  mergeGeometries,
  patchGeometry,
  vAtHeight,
} from "../../../vendor/euc-thrills/blockoutKit.ts";
import type {
  LoftPage,
  RiderLook,
  RiderMaterialRole,
  RiderMaterialSpec,
  RiderPanelGroup,
  RiderPatch,
} from "../../../vendor/euc-thrills/riderLook.ts";
import { RIDER_BLOCKOUT as B } from "../../../vendor/euc-thrills/tuning.ts";
import { STREET_KIT } from "./streetKit.ts";

/** Where a limb chain starts, bends and ends, in the parent's frame. */
export interface Chain {
  readonly origin: THREE.Vector3;
  readonly target: THREE.Vector3;
  readonly upperLength: number;
  readonly lowerLength: number;
  /** Which way the joint bulges: forward for knees, backward for elbows. */
  readonly bendToward: THREE.Vector3;
}

const DOWN = new THREE.Vector3(0, -1, 0);
const FORWARD = new THREE.Vector3(0, 0, 1);
const SOLVE_TO_TARGET = new THREE.Vector3();
const SOLVE_DIRECTION = new THREE.Vector3();
const SOLVE_PERPENDICULAR = new THREE.Vector3();
const SOLVE_MIDDLE = new THREE.Vector3();
const SOLVE_AIM = new THREE.Vector3();
const SOLVE_INVERSE = new THREE.Quaternion();

/**
 * Two-link inverse kinematics, allocation-free: the upper joint's rotation and the lower joint's
 * (in the upper's frame) that put the chain's end on its target. Past full reach the limb points
 * at the target, straight.
 */
export function solveChain(
  chain: Chain,
  upperOut: THREE.Quaternion,
  lowerOut: THREE.Quaternion,
): void {
  SOLVE_TO_TARGET.copy(chain.target).sub(chain.origin);
  const reach = SOLVE_TO_TARGET.length();
  if (reach < 1e-9) {
    upperOut.identity();
    lowerOut.identity();
    return;
  }
  SOLVE_DIRECTION.copy(SOLVE_TO_TARGET).multiplyScalar(1 / reach);
  // Distance from the origin to the foot of the perpendicular through the middle joint.
  const along = Math.min(
    Math.max(
      (chain.upperLength ** 2 - chain.lowerLength ** 2 + reach ** 2) / (2 * reach),
      -chain.upperLength,
    ),
    chain.upperLength,
  );
  const offset = Math.sqrt(Math.max(0, chain.upperLength ** 2 - along ** 2));
  SOLVE_PERPENDICULAR.copy(chain.bendToward).addScaledVector(
    SOLVE_DIRECTION,
    -chain.bendToward.dot(SOLVE_DIRECTION),
  );
  if (SOLVE_PERPENDICULAR.lengthSq() < 1e-12) SOLVE_PERPENDICULAR.set(0, 0, 1);
  SOLVE_PERPENDICULAR.normalize();
  SOLVE_MIDDLE.copy(chain.origin)
    .addScaledVector(SOLVE_DIRECTION, along)
    .addScaledVector(SOLVE_PERPENDICULAR, offset);
  upperOut.setFromUnitVectors(DOWN, SOLVE_AIM.copy(SOLVE_MIDDLE).sub(chain.origin).normalize());
  // The lower joint is a child of the upper one, so its aim is in the upper joint's frame.
  SOLVE_INVERSE.copy(upperOut).invert();
  lowerOut.setFromUnitVectors(
    DOWN,
    SOLVE_AIM.copy(chain.target).sub(SOLVE_MIDDLE).normalize().applyQuaternion(SOLVE_INVERSE),
  );
}

/** Rider-left is +x, so the outboard side of a limb is +x for the left side. */
const outboardAngle = (side: number) => (side > 0 ? 0 : Math.PI);

export interface RigArm {
  /** +1 is the rider's left; -1 the right. */
  readonly side: number;
  readonly shoulder: THREE.Group;
  readonly elbow: THREE.Group;
  readonly hand: THREE.Object3D;
}

export interface RigLeg {
  /** +1 is the rider's left; -1 the right. */
  readonly side: number;
  /** The hip joint: the thigh hangs `thighLength` along its -y. */
  readonly thigh: THREE.Group;
  /** The knee joint: the shin hangs `shinLength` along its -y. */
  readonly shin: THREE.Group;
  /** The ankle joint; the boot's sole is `ankleAbovePedal` below it. */
  readonly foot: THREE.Group;
  /** The boot, inside `foot`: moved down when the physics' ankle sits higher above the pedal. */
  readonly boot: THREE.Mesh;
}

export interface RiderRig {
  readonly root: THREE.Group;
  /** The hip joint height: everything above the waist hangs off it. */
  readonly pelvis: THREE.Group;
  /** The neck joint, at the shoulders; carries the head. */
  readonly neck: THREE.Group;
  readonly arms: readonly RigArm[];
  readonly legs: readonly RigLeg[];
  dispose(): void;
}

/** Build a rider in a look (the street kit by default), in its rest pose. */
export function buildRiderRig(look: RiderLook = STREET_KIT): RiderRig {
  const root = new THREE.Group();
  root.name = "rider-figure-rig";
  const geometries: THREE.BufferGeometry[] = [];
  const materials: THREE.Material[] = [];
  const textures: THREE.Texture[] = [];
  const track = <T extends THREE.BufferGeometry>(geometry: T): T => {
    geometries.push(geometry);
    return geometry;
  };

  // Every material sets vertexColors, and every geometry carries a `color` attribute (a
  // multiplier: 1 is the authored colour). One material per distinct spec, by identity. The
  // look's printed sheet, if it has one, is one texture per rig, freed with it.
  const atlas = look.atlas;
  const sheet = atlas === undefined ? null : atlas.build();
  if (sheet !== null) textures.push(sheet);
  const mapsRole = (role: RiderMaterialRole): boolean =>
    atlas !== undefined &&
    atlas.roles.some((mapped) => look.materials[mapped] === look.materials[role]);
  /** Fold a part's own texture square onto its page of the sheet (a no-op without one). */
  const paged = (
    geometry: THREE.BufferGeometry,
    role: RiderMaterialRole,
    art?: string,
  ): THREE.BufferGeometry =>
    atlas !== undefined && mapsRole(role) ? mapUvInto(geometry, atlas.region(art)) : geometry;

  const built = new Map<RiderMaterialSpec, THREE.MeshStandardMaterial>();
  const materialFor = (
    role: RiderMaterialRole,
    override?: RiderMaterialSpec,
  ): THREE.MeshStandardMaterial => {
    const spec = override ?? look.materials[role];
    const existing = built.get(spec);
    if (existing !== undefined) return existing;
    const material = new THREE.MeshStandardMaterial({
      color: spec.colour,
      roughness: spec.roughness,
      metalness: spec.metalness,
      emissive: spec.emissive ?? 0x000000,
      emissiveIntensity: spec.emissiveIntensity ?? 1,
      opacity: spec.opacity ?? 1,
      transparent: spec.opacity !== undefined && spec.opacity < 1,
      depthWrite: spec.opacity === undefined || spec.opacity >= 1,
      vertexColors: true,
      map: sheet !== null && override === undefined && mapsRole(role) ? sheet : null,
    });
    materials.push(material);
    built.set(spec, material);
    return material;
  };

  const bodyMaterial = materialFor("body");
  const headMaterial = materialFor("head");
  const gearMaterial = materialFor("gear");
  const handMaterial = materialFor(look.parts.hands);
  const { profiles, shades, panels } = look;
  const density = {
    limb: look.density?.limb ?? 14,
    torso: look.density?.torso ?? 24,
    head: look.density?.head ?? 20,
    boot: look.density?.boot ?? 12,
    hand: look.density?.hand ?? 10,
    neck: look.density?.neck ?? 12,
  };

  /** Casting a shadow means "this carries silhouette"; flat panels do not cast. */
  const shadowed = (mesh: THREE.Mesh): THREE.Mesh => {
    mesh.castShadow = true;
    return mesh;
  };

  /** A limb segment hanging from its joint, in the joint's own frame. */
  const limb = (
    profile: LoftProfile,
    role: RiderMaterialRole,
    shade: number,
    paintwork?: (geometry: THREE.BufferGeometry, side: number) => void,
    side = 1,
    page?: LoftPage,
  ): THREE.Mesh => {
    const geometry = track(
      paged(
        loftGeometry(profile, { radialSegments: density.limb, shade, splitSeam: mapsRole(role) }),
        role,
        typeof page === "function" ? page(side) : page,
      ),
    );
    paintwork?.(geometry, side);
    return shadowed(new THREE.Mesh(geometry, materialFor(role)));
  };

  /** A panel spanning two heights on a profile, rather than two ring indices. */
  const patchBetween = (
    profile: LoftProfile,
    options: Omit<Parameters<typeof patchGeometry>[1], "v0" | "v1"> & { from: number; to: number },
  ): THREE.BufferGeometry => {
    const { from, to, ...rest } = options;
    return patchGeometry(profile, {
      ...rest,
      v0: vAtHeight(profile, from),
      v1: vAtHeight(profile, to),
    });
  };
  const anchorAngle = (patch: RiderPatch, side: number): number => {
    if (patch.anchor === "outboard") return outboardAngle(side);
    return patch.anchor === "front" ? Math.PI / 2 : -Math.PI / 2;
  };
  /** One authored patch as geometry on a profile, its span resolved against its landmark. */
  const patchOn = (profile: LoftProfile, patch: RiderPatch, side: number) => {
    const centre = anchorAngle(patch, side);
    const sign = patch.mirrored === true ? side : 1;
    return patchBetween(profile, {
      u0: centre + sign * patch.u0,
      u1: centre + sign * patch.u1,
      from: patch.from,
      to: patch.to,
      uSegments: patch.uSegments,
      vSegments: patch.vSegments,
      lift: patch.lift,
      sink: patch.sink,
      taper: patch.taper,
      bulge: patch.bulge,
      bow: patch.bow,
      skew:
        patch.skewFrom !== undefined && patch.skewTo !== undefined
          ? vAtHeight(profile, patch.skewFrom) - vAtHeight(profile, patch.skewTo)
          : undefined,
      shade: patch.shade,
    });
  };
  /**
   * One mesh from the panels that share a joint. `side` is null for a group on the pelvis, where a
   * mirrored or outboard patch expands into a left/right pair inside the same buffer.
   */
  const panelMesh = (
    name: string,
    group: RiderPanelGroup,
    profile: LoftProfile,
    side: number | null,
  ): THREE.Mesh => {
    const parts: THREE.BufferGeometry[] = [];
    const pageFor = (patch: RiderPatch, patchSide: number): string | undefined =>
      patch.artOn !== undefined && Math.sign(patch.artOn) !== Math.sign(patchSide)
        ? patch.artElse
        : patch.art;
    const add = (patch: RiderPatch, patchSide: number): void => {
      parts.push(paged(patchOn(profile, patch, patchSide), group.role, pageFor(patch, patchSide)));
    };
    for (const patch of group.patches) {
      if (side !== null) add(patch, side);
      else if (patch.mirrored === true || patch.anchor === "outboard")
        for (const pairSide of [-1, 1]) add(patch, pairSide);
      else add(patch, 1);
    }
    if (group.build) {
      for (const pairSide of side === null ? [-1, 1] : [side]) {
        const page = typeof group.art === "function" ? group.art(pairSide) : group.art;
        parts.push(paged(group.build(pairSide), group.role, page));
      }
    }
    const mesh = new THREE.Mesh(track(mergeGeometries(parts)), materialFor(group.role));
    mesh.name = name;
    if (group.casts) shadowed(mesh);
    return mesh;
  };

  // -- Legs ---------------------------------------------------------------------------------------
  // Rest pose: the game's stance (hips at its hip height, boots on its pedals), so the rig reads
  // right before the first physics pose.
  const hipHalfWidth = B.torsoWidth * 0.26;
  const ankleY = B.ankleAbovePedal + 0.16;
  const hipY = 0.92;
  const solvedUpper = new THREE.Quaternion();
  const solvedLower = new THREE.Quaternion();
  const legs: RigLeg[] = [];
  for (const side of [-1, 1]) {
    const sideName = side > 0 ? "left" : "right";
    const thigh = new THREE.Group();
    thigh.name = `rider-hip-${sideName}`;
    thigh.position.set(side * hipHalfWidth, hipY, 0);
    const ankleTarget = new THREE.Vector3(side * B.stanceHalfWidth, ankleY, 0);
    solveChain(
      {
        origin: thigh.position,
        target: ankleTarget,
        upperLength: B.thighLength,
        lowerLength: B.shinLength,
        bendToward: FORWARD,
      },
      solvedUpper,
      solvedLower,
    );
    thigh.quaternion.copy(solvedUpper);
    thigh.add(
      limb(
        profiles.thigh,
        look.parts.legs,
        shades.legs,
        look.paint?.thigh,
        side,
        atlas?.lofts?.thigh,
      ),
    );
    if (panels.thighPad)
      thigh.add(panelMesh(`rider-thigh-pad-${sideName}`, panels.thighPad, profiles.thigh, side));

    const shin = new THREE.Group();
    shin.name = `rider-knee-${sideName}`;
    shin.position.set(0, -B.thighLength, 0).applyQuaternion(solvedUpper).add(thigh.position);
    shin.quaternion.copy(solvedUpper).multiply(solvedLower);
    shin.add(
      limb(profiles.shin, look.parts.legs, shades.legs, look.paint?.shin, side, atlas?.lofts?.shin),
    );
    if (panels.kneePad)
      shin.add(panelMesh(`rider-knee-pad-${sideName}`, panels.kneePad, profiles.shin, side));

    const foot = new THREE.Group();
    foot.name = `rider-ankle-${sideName}`;
    foot.position.copy(ankleTarget);
    // The boot: a lofted upper stood up onto a flat sole, merged into one mesh, its sole on the
    // pedal tread `ankleAbovePedal` below the ankle.
    const soleTop = -B.ankleAbovePedal + 0.018;
    const upper = loftGeometry(profiles.boot, { radialSegments: density.boot })
      .rotateX(Math.PI / 2)
      .translate(0, soleTop + 0.047, 0);
    const sole = loftGeometry(profiles.bootSole, {
      radialSegments: density.boot + 4,
      shade: shades.sole,
    }).translate(0, soleTop, 0.018);
    const bootGeometry = track(
      mergeGeometries([upper, sole, ...(look.build?.boot ?? []).map((build) => build(side))]),
    );
    look.paint?.boot?.(bootGeometry, side);
    const boot = shadowed(new THREE.Mesh(bootGeometry, gearMaterial));
    boot.name = `rider-boot-${sideName}`;
    foot.add(boot);

    root.add(thigh, shin, foot);
    legs.push({ side, thigh, shin, foot, boot });
  }

  // -- Pelvis, and everything above it ------------------------------------------------------------
  const pelvis = new THREE.Group();
  pelvis.name = "rider-pelvis";
  pelvis.position.y = hipY;
  root.add(pelvis);
  // The garment, whatever is under its hem, and the collar if there is one: one mesh.
  const lofts = atlas?.lofts;
  const torsoParts = [
    paged(
      loftGeometry(profiles.torso, { radialSegments: density.torso, splitSeam: mapsRole("body") }),
      "body",
      lofts?.torso,
    ),
  ];
  if (look.parts.seat === "body") {
    torsoParts.push(
      paged(
        loftGeometry(profiles.seat, {
          radialSegments: density.torso,
          shade: shades.seat,
          splitSeam: mapsRole("body"),
        }),
        "body",
        lofts?.seat,
      ),
    );
  }
  if (panels.collar)
    torsoParts.push(paged(patchOn(profiles.torso, panels.collar, 1), "body", panels.collar.art));
  const torsoGeometry = track(mergeGeometries(torsoParts));
  look.paint?.torso?.(torsoGeometry);
  pelvis.add(shadowed(new THREE.Mesh(torsoGeometry, bodyMaterial)));
  if (look.parts.seat !== "body") {
    const seat = new THREE.Mesh(
      track(
        paged(
          loftGeometry(profiles.seat, {
            radialSegments: density.torso,
            shade: shades.seat,
            splitSeam: mapsRole(look.parts.seat),
          }),
          look.parts.seat,
          lofts?.seat,
        ),
      ),
      materialFor(look.parts.seat),
    );
    seat.name = "rider-seat";
    pelvis.add(seat);
  }
  if (panels.waist) pelvis.add(panelMesh("rider-belt", panels.waist, profiles.torso, null));
  if (panels.shoulders)
    pelvis.add(panelMesh("rider-shoulder-panels", panels.shoulders, profiles.torso, null));
  if (panels.torso)
    pelvis.add(panelMesh("rider-jacket-panels", panels.torso, profiles.torso, null));

  // -- Arms ---------------------------------------------------------------------------------------
  // Arms low and loose, hands around waist height and slightly forward; never a mirrored pair:
  // the right hand sits a little wider and farther forward than the left.
  const shoulderY = B.torsoLength;
  const arms: RigArm[] = [];
  for (const side of [-1, 1]) {
    const sideName = side > 0 ? "left" : "right";
    const shoulder = new THREE.Group();
    shoulder.name = `rider-shoulder-${sideName}`;
    shoulder.position.set(side * B.shoulderHalfWidth, shoulderY, 0);
    const rightArm = side < 0;
    const reach = B.upperArmLength + B.forearmLength;
    const baseSplay =
      B.shoulderHalfWidth +
      B.armSplay +
      (rightArm ? B.armAsymmetrySplay : 0) +
      look.armCarriage.splay;
    const baseTarget = new THREE.Vector3(
      side * baseSplay,
      shoulderY - reach * B.armHangFraction + look.armCarriage.rise,
      B.handForward + (rightArm ? B.armAsymmetryForward : 0),
    );
    solveChain(
      {
        origin: shoulder.position,
        target: baseTarget,
        upperLength: B.upperArmLength,
        lowerLength: B.forearmLength,
        // Elbows behind, so the arms read as hanging rather than as reaching.
        bendToward: new THREE.Vector3(0, 0, -1),
      },
      solvedUpper,
      solvedLower,
    );
    shoulder.quaternion.copy(solvedUpper);
    shoulder.add(
      limb(profiles.upperArm, "limbs", 1, look.paint?.upperArm, side, atlas?.lofts?.upperArm),
    );
    // The accent runs down the outer sleeve, not around the whole arm.
    if (panels.sleeve)
      shoulder.add(panelMesh(`rider-sleeve-${sideName}`, panels.sleeve, profiles.upperArm, side));
    if (profiles.sleeve) {
      const sleeve = new THREE.Mesh(
        track(loftGeometry(profiles.sleeve, { radialSegments: density.limb })),
        bodyMaterial,
      );
      sleeve.name = `rider-cap-sleeve-${sideName}`;
      shoulder.add(sleeve);
    }

    const elbow = new THREE.Group();
    elbow.name = `rider-elbow-${sideName}`;
    elbow.position.y = -B.upperArmLength;
    elbow.quaternion.copy(solvedLower);
    elbow.add(limb(profiles.forearm, "limbs", 1, look.paint?.forearm, side, atlas?.lofts?.forearm));
    if (panels.elbowPad)
      elbow.add(panelMesh(`rider-elbow-pad-${sideName}`, panels.elbowPad, profiles.forearm, side));
    shoulder.add(elbow);

    // A hand may be several lofts (palm, thumb, fingers) merged into one mesh.
    const handParts = [loftGeometry(profiles.hand, { radialSegments: density.hand })];
    for (const build of look.build?.hand ?? []) handParts.push(build(side));
    const handPage = atlas?.lofts?.hand;
    const handGeometry = track(
      paged(
        mergeGeometries(handParts),
        look.parts.hands,
        typeof handPage === "function" ? handPage(side) : handPage,
      ),
    );
    look.paint?.hand?.(handGeometry, side);
    const hand = shadowed(new THREE.Mesh(handGeometry, handMaterial));
    hand.name = `rider-hand-${sideName}`;
    // Set back into the sleeve's rounded end, so a cuff overlaps a wrist.
    hand.position.y = -B.forearmLength + 0.012;
    elbow.add(hand);

    pelvis.add(shoulder);
    arms.push({ side, shoulder, elbow, hand });
  }

  // -- Head ---------------------------------------------------------------------------------------
  const neck = new THREE.Group();
  neck.name = "rider-neck";
  neck.position.y = shoulderY;
  pelvis.add(neck);
  const neckRole = look.parts.neck ?? "limbs";
  neck.add(
    shadowed(
      new THREE.Mesh(
        track(
          paged(
            loftGeometry(profiles.neck, {
              radialSegments: density.neck,
              shade: shades.neck,
              splitSeam: mapsRole(neckRole),
            }),
            neckRole,
            atlas?.lofts?.neck,
          ),
        ),
        materialFor(neckRole),
      ),
    ),
  );
  const headParts = [
    paged(
      look.build?.head?.() ??
        loftGeometry(profiles.head, { radialSegments: density.head, splitSeam: mapsRole("head") }),
      "head",
      atlas?.lofts?.head,
    ),
  ];
  for (const patch of panels.head)
    headParts.push(paged(patchOn(profiles.head, patch, 1), "head", patch.art));
  neck.add(shadowed(new THREE.Mesh(track(mergeGeometries(headParts)), headMaterial)));
  if (panels.face) neck.add(panelMesh("rider-face", panels.face, profiles.head, null));
  // What the look adds that the rig has no slot for: the street kit's mirror and chin strap.
  for (const extra of look.extras) {
    const mesh = new THREE.Mesh(
      track(extra.material ? extra.build() : paged(extra.build(), extra.role, extra.art)),
      materialFor(extra.role, extra.material),
    );
    mesh.name = extra.name;
    if (extra.casts) shadowed(mesh);
    (extra.joint === "neck" ? neck : pelvis).add(mesh);
  }

  return {
    root,
    pelvis,
    neck,
    arms,
    legs,
    dispose: () => {
      for (const geometry of geometries) geometry.dispose();
      for (const material of materials) material.dispose();
      for (const texture of textures) texture.dispose();
    },
  };
}
