import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import * as THREE from "three";
import {
  type LinkageRest,
  type P2,
  distance,
  poseLinkage,
  poseShock,
  shockRig,
} from "./shockLinkage.ts";

const root = new URL("../../../../", import.meta.url);
const read = (path: string) => readFileSync(new URL(path, root), "utf8");

/** geometry.toml's current linkage at full extension, x and z in metres. */
function profileLinkage(): LinkageRest {
  const line = /^linkage_pivots_xz_m = \{(.*)\}$/m.exec(
    read("profiles/begode-extreme/geometry.toml"),
  );
  const point = (key: string): P2 => {
    const m = new RegExp(`${key} = \\[(-?[\\d.]+), (-?[\\d.]+)\\]`).exec(line?.[1] ?? "");
    if (!m) throw new Error(`geometry.toml: no ${key} in linkage_pivots_xz_m`);
    return [Number(m[1]), Number(m[2])];
  };
  return {
    pivot: point("frame_pivot"),
    rockerPin: point("rocker_pin"),
    topEye: point("shock_top"),
    bottomEye: point("shock_bottom"),
    linkBottom: point("link_bottom"),
  };
}

/** plant.toml's suspension: the constant leverage and the shock's stroke. */
function profileSuspension(): { ratio: number; strokeMm: number; curve: string } {
  const plant = read("profiles/begode-extreme/plant.toml");
  const number = (key: string) => {
    const m = new RegExp(`^${key} = ([\\d.]+)`, "m").exec(plant);
    if (!m) throw new Error(`plant.toml: no ${key}`);
    return Number(m[1]);
  };
  const curve = /^ratio_curve = (.*?)\s+(#.*)?$/m.exec(plant)?.[1] ?? "";
  return { ratio: number("linkage_ratio"), strokeMm: number("stroke_mm"), curve };
}

interface GlbNode {
  name?: string;
  translation?: number[];
  rotation?: number[];
}

/** Named nodes of a model level of detail (glTF: x forward, y up). */
function glbNodes(lod: number): Map<string, GlbNode> {
  const bytes = readFileSync(
    new URL(`app/public/models/begode-extreme/begode-extreme-lod${lod}.glb`, root),
  );
  const json = JSON.parse(bytes.subarray(20, 20 + bytes.readUInt32LE(12)).toString("utf8")) as {
    nodes: GlbNode[];
  };
  return new Map(json.nodes.flatMap((n) => (n.name ? [[n.name, n]] : [])));
}

/** The linkage nodes as three.js objects under one body, as the GLB loader builds them. */
function linkageBody(lod: number): THREE.Object3D {
  const body = new THREE.Object3D();
  for (const [name, node] of glbNodes(lod)) {
    if (!["shock", "shock_mount", "link_bottom", "rocker", "link"].includes(name)) continue;
    const o = new THREE.Object3D();
    o.name = name;
    const [x = 0, y = 0, z = 0] = node.translation ?? [];
    const [qx = 0, qy = 0, qz = 0, qw = 1] = node.rotation ?? [];
    o.position.set(x, y, z);
    o.quaternion.set(qx, qy, qz, qw);
    body.add(o);
  }
  body.updateMatrixWorld(true);
  return body;
}

function pose(rest: LinkageRest, travel: number) {
  const p = poseLinkage(rest, travel);
  if (!p) throw new Error(`the link cannot reach at ${travel} m`);
  return p;
}

describe("rear linkage", () => {
  const rest = profileLinkage();

  it("is at rest at full extension", () => {
    const p = pose(rest, 0);
    expect(p.rocker).toBeCloseTo(0, 9);
    expect(p.link).toBeCloseTo(0, 9);
    expect(p.shock).toBeCloseTo(0, 9);
    expect(p.length).toBeCloseTo(0.2, 3);
    expect(p.rockerPin[0]).toBeCloseTo(rest.rockerPin[0], 9);
    expect(p.rockerPin[1]).toBeCloseTo(rest.rockerPin[1], 9);
  });

  it("keeps the link and the rocker rigid along the travel", () => {
    const link = distance(rest.rockerPin, rest.linkBottom);
    const eyeToPivot = distance(rest.topEye, rest.pivot);
    const pinToEye = distance(rest.rockerPin, rest.topEye);
    for (const travel of [0.02, 0.05, 0.1]) {
      const p = pose(rest, travel);
      expect(distance(p.rockerPin, p.linkBottom)).toBeCloseTo(link, 9);
      expect(distance(p.topEye, rest.pivot)).toBeCloseTo(eyeToPivot, 9);
      expect(distance(p.rockerPin, p.topEye)).toBeCloseTo(pinToEye, 9);
      expect(p.bottomEye[1] - rest.bottomEye[1]).toBeCloseTo(travel, 5);
      expect(p.linkBottom[1] - rest.linkBottom[1]).toBeCloseTo(travel, 5);
    }
  });

  it("compresses the drawn shock within its stroke over the physics travel", () => {
    // The physics keeps a constant leverage while the drawing follows the measured linkage
    // (docs/hardware/geometry.md "Rear linkage"); the drawing then differs from the shock channel
    // by up to 8.4 mm. When plant.toml gets the linkage's ratio_curve, this test should compare
    // the two exactly instead.
    const { ratio, strokeMm, curve } = profileSuspension();
    expect(curve).toBe("[]");
    const travelMm = strokeMm * ratio;
    let last = 0;
    for (let axleMm = 5; axleMm <= travelMm; axleMm += 5) {
      const drawn = (0.2 - pose(rest, axleMm / 1000).length) * 1000;
      expect(drawn, `at ${axleMm} mm`).toBeGreaterThan(last);
      expect(drawn, `at ${axleMm} mm`).toBeLessThan(strokeMm);
      expect(Math.abs(drawn - axleMm / ratio), `at ${axleMm} mm`).toBeLessThan(9);
      last = drawn;
    }
  });

  it("matches the model's rest pivots", () => {
    for (const lod of [0, 1, 2]) {
      const nodes = glbNodes(lod);
      const near = (name: string, p: P2) => {
        const [x = Number.NaN, y = Number.NaN] = nodes.get(name)?.translation ?? [];
        expect(Math.abs(x - p[0]), `${name} x, LOD${lod}`).toBeLessThan(1e-4);
        expect(Math.abs(y - p[1]), `${name} y, LOD${lod}`).toBeLessThan(1e-4);
      };
      near("rocker", rest.pivot);
      near("link", rest.rockerPin);
      near("shock", rest.topEye);
      near("shock_mount", rest.bottomEye);
      near("link_bottom", rest.linkBottom);
    }
  });

  it("keeps the model's parts joined as it poses them", () => {
    const body = linkageBody(1);
    const rig = shockRig(body);
    if (!rig) throw new Error("the model has no linkage nodes");
    // Rest points in each part's own frame: the link's lower hole, the rocker's top eye and pin.
    const local = (o: THREE.Object3D, p: P2) => o.worldToLocal(new THREE.Vector3(p[0], p[1], 0));
    const linkLower = local(rig.link, rig.rest.linkBottom);
    const mountPin = local(rig.mount, rig.rest.linkBottom);
    const rockerEye = local(rig.rocker, rig.rest.topEye);
    const rockerPin = local(rig.rocker, rig.rest.rockerPin);
    const close = (a: THREE.Vector3, b: THREE.Vector3, what: string) =>
      expect(Math.hypot(a.x - b.x, a.y - b.y), what).toBeLessThan(1e-6);
    for (const travel of [0, 0.03, 0.06, 0.1]) {
      poseShock(rig, travel);
      body.updateMatrixWorld(true);
      const bottomEye = rig.mount.getWorldPosition(new THREE.Vector3());
      // The shock's bottom eye, its top eye on the rocker, the link on the pin and the shaft.
      const shockBottom = rig.shock.localToWorld(new THREE.Vector3(0, -rig.length, 0));
      close(shockBottom, bottomEye, `bottom eye at ${travel}`);
      close(rig.rocker.localToWorld(rockerEye.clone()), rig.shock.position, `top eye at ${travel}`);
      close(
        rig.rocker.localToWorld(rockerPin.clone()),
        rig.link.position,
        `rocker pin at ${travel}`,
      );
      // The link's lower hole on the carriage's pin, and the marker on it.
      const pin = rig.mount.localToWorld(mountPin.clone());
      close(rig.link.localToWorld(linkLower.clone()), pin, `link lower hole at ${travel}`);
      close(
        rig.linkBottom?.getWorldPosition(new THREE.Vector3()) ?? pin,
        pin,
        `link pin at ${travel}`,
      );
      expect(bottomEye.y - rest.bottomEye[1]).toBeCloseTo(travel, 5);
    }
  });
});
