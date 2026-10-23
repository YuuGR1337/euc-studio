// The Begode Extreme's rear linkage in its plane (the model's frame: x forward, y up, metres): the
// current one from the owner's photos; the scan shows an older system. The carriage carries the
// shock's bottom eye; the rocker turns about its frame pivot and carries the shock's top eye; a
// link joins the rocker pin to a lower pin. docs/hardware/suspension-linkage.md keeps two readings
// open until a seat-press video: the lower pin rides on the carriage and the rocker turns (a), or
// it is on the frame and the rocker is a fixed shock bracket (b). The positions are geometry.toml's
// `[suspension] linkage_pivots_xz_m` and the GLB's node origins.

import * as THREE from "three";

const DEG = Math.PI / 180;
/** The slider rods lean 0.13° with the top to the rear (geometry.toml `[suspension]`). */
const ROD_LEAN = -0.13 * DEG;

/**
 * Which reading the drawing follows: "turning" (a: the link's lower pin rides on the carriage, the
 * rocker turns) or "fixed" (b: the lower pin is on the frame, only the shock's bottom eye moves).
 * Switch here once the seat-press video decides.
 */
export type LinkageReading = "turning" | "fixed";
export const LINKAGE_READING: LinkageReading = "turning";

/** A point in the linkage plane: x forward, y up, metres. */
export type P2 = readonly [number, number];

/** The linkage at full extension. */
export interface LinkageRest {
  /** Rocker pivot on the frame. */
  pivot: P2;
  /** Link-to-rocker pin. */
  rockerPin: P2;
  /** Shock top eye, on the rocker. */
  topEye: P2;
  /** Shock bottom eye, on the carriage. */
  bottomEye: P2;
  /** Link lower pin: on the carriage in the turning reading, on the frame in the fixed one. */
  linkBottom: P2;
}

export interface LinkagePose {
  /** Rocker turn from rest about +z (counter-clockwise from x toward y), radians. */
  rocker: number;
  rockerPin: P2;
  topEye: P2;
  bottomEye: P2;
  linkBottom: P2;
  /** Link turn from rest, radians. */
  link: number;
  /** Shock turn from rest, radians. */
  shock: number;
  /** Eye to eye, metres. */
  length: number;
}

const angle = (a: P2, b: P2) => Math.atan2(b[1] - a[1], b[0] - a[0]);
export const distance = (a: P2, b: P2) => Math.hypot(b[0] - a[0], b[1] - a[1]);

function turn(p: P2, about: P2, by: number): P2 {
  const [c, s] = [Math.cos(by), Math.sin(by)];
  const [x, y] = [p[0] - about[0], p[1] - about[1]];
  return [about[0] + c * x - s * y, about[1] + s * x + c * y];
}

/**
 * The linkage with the axle `travel` metres up the rods from full extension. Turning: the rocker
 * pin is where the circle about the rocker pivot meets the circle of the link's length about the
 * moved link pin, on the same side as at rest; null when the link cannot reach. Fixed: the rocker
 * and the link stay, and the shock spans the fixed top eye and the moved bottom eye.
 */
export function poseLinkage(
  rest: LinkageRest,
  travel: number,
  reading: LinkageReading = LINKAGE_READING,
): LinkagePose | null {
  const { pivot, rockerPin, topEye, bottomEye, linkBottom } = rest;
  const up = (p: P2): P2 => [
    p[0] + travel * Math.sin(ROD_LEAN),
    p[1] + travel * Math.cos(ROD_LEAN),
  ];
  const bottom = up(bottomEye);
  if (reading === "fixed") {
    return {
      rocker: 0,
      rockerPin,
      topEye,
      bottomEye: bottom,
      linkBottom,
      link: 0,
      shock: angle(bottom, topEye) - angle(bottomEye, topEye),
      length: distance(bottom, topEye),
    };
  }
  const lower = up(linkBottom);
  const r1 = distance(pivot, rockerPin);
  const r2 = distance(linkBottom, rockerPin);
  const d = distance(pivot, lower);
  if (d === 0 || d > r1 + r2 || d < Math.abs(r1 - r2)) return null;
  const a = (r1 * r1 - r2 * r2 + d * d) / (2 * d);
  const h = Math.sqrt(Math.max(r1 * r1 - a * a, 0));
  const [ux, uy] = [(lower[0] - pivot[0]) / d, (lower[1] - pivot[1]) / d];
  const [mx, my] = [pivot[0] + a * ux, pivot[1] + a * uy];
  // Which side of the pivot-to-link-pin line the rocker pin sits on at rest (left is positive).
  const side =
    (linkBottom[0] - pivot[0]) * (rockerPin[1] - pivot[1]) -
    (linkBottom[1] - pivot[1]) * (rockerPin[0] - pivot[0]);
  const pin: P2 = side >= 0 ? [mx - h * uy, my + h * ux] : [mx + h * uy, my - h * ux];
  const rocker = angle(pivot, pin) - angle(pivot, rockerPin);
  const eye = turn(topEye, pivot, rocker);
  return {
    rocker,
    rockerPin: pin,
    topEye: eye,
    bottomEye: bottom,
    linkBottom: lower,
    link: angle(pin, lower) - angle(rockerPin, linkBottom),
    shock: angle(bottom, eye) - angle(bottomEye, topEye),
    length: distance(bottom, eye),
  };
}

/** The axle travel the cable's morph target displaces its low part by (the GLB's "travel_100mm"). */
export const CABLE_MORPH_TRAVEL = 0.1;

const Z = new THREE.Vector3(0, 0, 1);
const spin = new THREE.Quaternion();

/**
 * The model's linkage parts, all children of "body". The contract with the GLB (its README):
 * "shock" has its origin on the top eye and its local +y from the bottom eye to the top eye;
 * "shock_mount" (the carriage's rear arm) has its origin on the bottom eye, "rocker" on its frame
 * pivot, "link" on the rocker pin and "link_bottom" (the lower pin and its plates) on the link's
 * lower pin (a model without it has the link on the bottom eye's shaft). Their rest positions are
 * the linkage at full extension. Optional: "fender" (the internal fender over the tyre) and "axle"
 * (the axle stubs) ride with the carriage; "cable" (the cable bundle over the tyre) has a morph
 * target that moves its low part with the carriage for CABLE_MORPH_TRAVEL of travel.
 */
export interface ShockRig {
  root: THREE.Object3D;
  shock: THREE.Object3D;
  mount: THREE.Object3D;
  rocker: THREE.Object3D;
  link: THREE.Object3D;
  linkBottom: THREE.Object3D | undefined;
  /** Parts that ride on the carriage with the axle (the fender, the axle stubs) and their rests. */
  carriage: { node: THREE.Object3D; rest: THREE.Vector3 }[];
  /** The cable bundle's meshes with a travel morph target. */
  cable: THREE.Mesh[];
  rest: LinkageRest;
  mountRest: THREE.Vector3;
  shockRest: THREE.Quaternion;
  rockerRest: THREE.Quaternion;
  linkRest: THREE.Quaternion;
  /** Eye to eye at rest, m. */
  length: number;
}

/** The linkage parts under `root`, or null when the model has none (an older model). */
export function shockRig(root: THREE.Object3D): ShockRig | null {
  const [shock, mount, rocker, link] = ["shock", "shock_mount", "rocker", "link"].map((name) =>
    root.getObjectByName(name),
  );
  if (!shock || !mount || !rocker || !link) return null;
  const linkBottom = root.getObjectByName("link_bottom");
  const carriage = ["fender", "axle"].flatMap((name) => {
    const node = root.getObjectByName(name);
    return node ? [{ node, rest: node.position.clone() }] : [];
  });
  const cable: THREE.Mesh[] = [];
  root.getObjectByName("cable")?.traverse((o) => {
    if (o instanceof THREE.Mesh && o.morphTargetInfluences?.length) cable.push(o);
  });
  const at = (o: THREE.Object3D): P2 => [o.position.x, o.position.y];
  const rest = {
    pivot: at(rocker),
    rockerPin: at(link),
    topEye: at(shock),
    bottomEye: at(mount),
    linkBottom: at(linkBottom ?? mount),
  };
  return {
    root,
    shock,
    mount,
    rocker,
    link,
    linkBottom,
    carriage,
    cable,
    rest,
    mountRest: mount.position.clone(),
    shockRest: shock.quaternion.clone(),
    rockerRest: rocker.quaternion.clone(),
    linkRest: link.quaternion.clone(),
    length: distance(rest.topEye, rest.bottomEye),
  };
}

/**
 * Poses the linkage for the axle travel (metres from full extension): the carriage's clevis slides
 * up the rods, the rocker turns (turning reading), the link follows, and the shock spans its two
 * eyes. Its length is the drawn linkage's; it matches the engine's shock channel only once the
 * profile's ratio_curve is this linkage's curve (until then they differ by up to 9 mm in the
 * turning reading). Travel the link cannot reach keeps the last pose.
 */
export function poseShock(
  rig: ShockRig,
  travelM: number,
  reading: LinkageReading = LINKAGE_READING,
) {
  const pose = poseLinkage(rig.rest, travelM, reading);
  if (!pose) return;
  const { mount, rocker, link, shock, rest } = rig;
  const [dx, dy] = [pose.bottomEye[0] - rest.bottomEye[0], pose.bottomEye[1] - rest.bottomEye[1]];
  mount.position.set(rig.mountRest.x + dx, rig.mountRest.y + dy, rig.mountRest.z);
  rig.linkBottom?.position.set(pose.linkBottom[0], pose.linkBottom[1], rig.linkBottom.position.z);
  for (const { node, rest: at } of rig.carriage) node.position.set(at.x + dx, at.y + dy, at.z);
  for (const m of rig.cable) {
    const influences = m.morphTargetInfluences;
    if (influences) influences[0] = Math.max(travelM, 0) / CABLE_MORPH_TRAVEL;
  }
  rocker.quaternion.copy(spin.setFromAxisAngle(Z, pose.rocker)).multiply(rig.rockerRest);
  link.position.set(pose.rockerPin[0], pose.rockerPin[1], link.position.z);
  link.quaternion.copy(spin.setFromAxisAngle(Z, pose.link)).multiply(rig.linkRest);
  shock.position.set(pose.topEye[0], pose.topEye[1], shock.position.z);
  shock.quaternion.copy(spin.setFromAxisAngle(Z, pose.shock)).multiply(rig.shockRest);
  shock.scale.y = pose.length / rig.length;
}
