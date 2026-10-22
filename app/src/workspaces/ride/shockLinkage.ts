// The Begode Extreme's rear linkage in its plane (the model's frame: x forward, y up, metres): the
// current one from the owner's photos; the scan shows an older system. Per side, the carriage
// carries the bottom shaft (the shock's bottom eye and the link's lower hole), the link joins it
// to the rocker pin, and the rocker turns about its frame pivot and carries the shock's top eye.
// docs/hardware/geometry.md ("Rear linkage") has the evidence (the pivot is an ESTIMATE); the
// positions are geometry.toml's `[suspension] linkage_pivots_xz_m` and the GLB's node origins.

import * as THREE from "three";

const DEG = Math.PI / 180;
/** The slider rods lean 0.13° with the top to the rear (geometry.toml `[suspension]`). */
const ROD_LEAN = -0.13 * DEG;

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
  /** Bottom cross shaft, on the carriage: shock bottom eye and link lower hole. */
  bottomEye: P2;
}

export interface LinkagePose {
  /** Rocker turn from rest about +z (counter-clockwise from x toward y), radians. */
  rocker: number;
  rockerPin: P2;
  topEye: P2;
  bottomEye: P2;
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
 * The linkage with the axle `travel` metres up the rods from full extension: the rocker pin is
 * where the circle about the rocker pivot meets the circle of the link's length about the moved
 * bottom shaft, on the same side as at rest. Null when the link cannot reach.
 */
export function poseLinkage(rest: LinkageRest, travel: number): LinkagePose | null {
  const { pivot, rockerPin, topEye, bottomEye } = rest;
  const bottom: P2 = [
    bottomEye[0] + travel * Math.sin(ROD_LEAN),
    bottomEye[1] + travel * Math.cos(ROD_LEAN),
  ];
  const r1 = distance(pivot, rockerPin);
  const r2 = distance(bottomEye, rockerPin);
  const d = distance(pivot, bottom);
  if (d === 0 || d > r1 + r2 || d < Math.abs(r1 - r2)) return null;
  const a = (r1 * r1 - r2 * r2 + d * d) / (2 * d);
  const h = Math.sqrt(Math.max(r1 * r1 - a * a, 0));
  const [ux, uy] = [(bottom[0] - pivot[0]) / d, (bottom[1] - pivot[1]) / d];
  const [mx, my] = [pivot[0] + a * ux, pivot[1] + a * uy];
  // Which side of the pivot-to-shaft line the pin sits on at rest (left is positive).
  const side =
    (bottomEye[0] - pivot[0]) * (rockerPin[1] - pivot[1]) -
    (bottomEye[1] - pivot[1]) * (rockerPin[0] - pivot[0]);
  const pin: P2 = side >= 0 ? [mx - h * uy, my + h * ux] : [mx + h * uy, my - h * ux];
  const rocker = angle(pivot, pin) - angle(pivot, rockerPin);
  const eye = turn(topEye, pivot, rocker);
  return {
    rocker,
    rockerPin: pin,
    topEye: eye,
    bottomEye: bottom,
    link: angle(pin, bottom) - angle(rockerPin, bottomEye),
    shock: angle(bottom, eye) - angle(bottomEye, topEye),
    length: distance(bottom, eye),
  };
}

const Z = new THREE.Vector3(0, 0, 1);
const spin = new THREE.Quaternion();

/**
 * The model's linkage parts, all children of "body". The contract with the GLB (its README):
 * "shock" has its origin on the top eye and its local +y from the bottom eye to the top eye;
 * "shock_mount" (the carriage's rear arm) has its origin on the bottom eye, "rocker" on its frame
 * pivot and "link" on the rocker pin. Their rest positions are the linkage at full extension.
 */
export interface ShockRig {
  root: THREE.Object3D;
  shock: THREE.Object3D;
  mount: THREE.Object3D;
  rocker: THREE.Object3D;
  link: THREE.Object3D;
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
  const at = (o: THREE.Object3D): P2 => [o.position.x, o.position.y];
  const rest = { pivot: at(rocker), rockerPin: at(link), topEye: at(shock), bottomEye: at(mount) };
  return {
    root,
    shock,
    mount,
    rocker,
    link,
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
 * up the rods, the rocker turns, the link follows, and the shock spans its two eyes. Its length is
 * the drawn linkage's; it matches the engine's shock channel only once the profile's ratio_curve
 * is this linkage's curve (until then they differ by up to 17 mm). Travel the link cannot reach
 * keeps the last pose.
 */
export function poseShock(rig: ShockRig, travelM: number) {
  const pose = poseLinkage(rig.rest, travelM);
  if (!pose) return;
  const { mount, rocker, link, shock, rest } = rig;
  mount.position.set(
    rig.mountRest.x + pose.bottomEye[0] - rest.bottomEye[0],
    rig.mountRest.y + pose.bottomEye[1] - rest.bottomEye[1],
    rig.mountRest.z,
  );
  rocker.quaternion.copy(spin.setFromAxisAngle(Z, pose.rocker)).multiply(rig.rockerRest);
  link.position.set(pose.rockerPin[0], pose.rockerPin[1], link.position.z);
  link.quaternion.copy(spin.setFromAxisAngle(Z, pose.link)).multiply(rig.linkRest);
  shock.position.set(pose.topEye[0], pose.topEye[1], shock.position.z);
  shock.quaternion.copy(spin.setFromAxisAngle(Z, pose.shock)).multiply(rig.shockRest);
  shock.scale.y = pose.length / rig.length;
}
