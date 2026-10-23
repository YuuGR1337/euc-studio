import { describe, expect, it } from "vitest";
import * as THREE from "three";
import { CHASE, ChaseRig, type ChaseInput, meshObstruction } from "./chaseRig.ts";

// The feel checks EUC Thrills pins for its chase camera (src/render/chaseCamera.test.ts), with
// this rig's values.

const input = (patch: Partial<ChaseInput> = {}): ChaseInput => ({
  wheel: new THREE.Vector3(0, 0.23, 0),
  ground: 0,
  heading: 0,
  speedMps: 0,
  shellUp: null,
  ...patch,
});
const run = (rig: ChaseRig, seconds: number, at: Partial<ChaseInput> = {}, dt = 1 / 60) => {
  for (let t = 0; t < seconds - 1e-9; t += dt) rig.step(input(at), dt);
};
const fast = CHASE.speedRefMps;

describe("chase camera", () => {
  it("sits behind the wheel at rest, at the arm's length and height, aimed at the hips", () => {
    const rig = new ChaseRig();
    rig.step(input({ heading: Math.PI / 2 }), 1 / 60);
    // Heading +90° points along three.js -z: the camera is behind, at +z.
    expect(rig.position.x).toBeCloseTo(0, 6);
    expect(rig.position.z).toBeCloseTo(CHASE.armRest, 6);
    expect(rig.position.y).toBeCloseTo(CHASE.height, 6);
    expect(rig.aim.y).toBeCloseTo(CHASE.aimHeight, 6);
    expect(rig.fov).toBe(CHASE.fovRest);
  });

  it("stretches the arm and widens the view at speed, the view the slowest", () => {
    const rig = new ChaseRig();
    run(rig, 0.1);
    rig.step(input({ speedMps: fast }), 1 / 60);
    const armShare = (rig.arm - CHASE.armRest) / (CHASE.armFast - CHASE.armRest);
    const fovShare = (rig.fov - CHASE.fovRest) / (CHASE.fovFast - CHASE.fovRest);
    expect(armShare).toBeLessThan(0.05);
    expect(fovShare).toBeLessThan(armShare);
    run(rig, 6, { speedMps: fast });
    expect(rig.arm - CHASE.armRest).toBeGreaterThan(1.5);
    expect(rig.fov - CHASE.fovRest).toBeGreaterThan(9);
  });

  it("leads the aim by 0.22 s of forward speed, never in reverse", () => {
    const rig = new ChaseRig();
    run(rig, 4, { speedMps: 10 });
    expect(rig.lookAhead).toBeCloseTo(2.2, 1);
    run(rig, 4, { speedMps: -3 });
    expect(rig.lookAhead).toBeCloseTo(0, 3);
  });

  it("follows a heading step without overshoot, harder at speed than at walking pace", () => {
    const rig = new ChaseRig();
    run(rig, 0.5, { speedMps: 8 });
    let peak = 0;
    for (let t = 0; t < 0.2 - 1e-9; t += 1 / 60) {
      rig.step(input({ speedMps: 8, heading: 1 }), 1 / 60);
      peak = Math.max(peak, rig.yaw);
    }
    expect(rig.yaw).toBeGreaterThan(0.2);
    expect(rig.yaw).toBeLessThan(0.9);
    for (let t = 0; t < 1.8; t += 1 / 60) {
      rig.step(input({ speedMps: 8, heading: 1 }), 1 / 60);
      peak = Math.max(peak, rig.yaw);
    }
    expect(peak).toBeLessThanOrEqual(1 + 1e-12);
    expect(rig.yaw).toBeGreaterThan(0.99);

    const crawl = new ChaseRig();
    const speedy = new ChaseRig();
    run(crawl, 0.5, { speedMps: 0.4 });
    run(speedy, 0.5, { speedMps: fast });
    run(crawl, 0.2, { speedMps: 0.4, heading: 1 });
    run(speedy, 0.2, { speedMps: fast, heading: 1 });
    expect(speedy.yaw).toBeGreaterThan(1.5 * crawl.yaw);
    expect(crawl.yaw).toBeLessThan(0.5);
  });

  it("takes the short way round across ±π", () => {
    const rig = new ChaseRig();
    run(rig, 0.5, { heading: 3.1 });
    run(rig, 3, { heading: -3.1 });
    expect(rig.yaw).toBeCloseTo(3.1 + (2 * Math.PI - 6.2), 2);
  });

  it("banks a share of the wheel's lean, capped", () => {
    const rig = new ChaseRig();
    const lean = (angle: number) =>
      // Leaning right: the shell's up tilts toward +z when heading along +x.
      new THREE.Vector3(0, Math.cos(angle), Math.sin(angle));
    run(rig, 3, { shellUp: lean(0.3) });
    expect(rig.bank).toBeCloseTo(0.15 * 0.3, 4);
    expect(rig.up.z).toBeGreaterThan(0);
    run(rig, 3, { shellUp: lean(1) });
    expect(rig.bank).toBeCloseTo(CHASE.bankMax, 4);
  });

  it("pulls in fast for a wall and lets out slowly, never nearer than the minimum", () => {
    const rig = new ChaseRig();
    let wall: number | null = null;
    const obstruction = () => wall;
    run(rig, 1, { obstruction });
    wall = 2.5;
    const before = rig.reach;
    run(rig, 0.2, { obstruction });
    const target =
      ((2.5 - CHASE.obstructionMargin) / Math.hypot(CHASE.armRest, 0.7)) * CHASE.armRest;
    expect((before - rig.reach) / (before - target)).toBeGreaterThan(0.95);
    wall = 0.5;
    run(rig, 1, { obstruction });
    expect(rig.reach).toBeCloseTo(CHASE.minArm, 3);
    wall = null;
    const pulled = rig.reach;
    run(rig, 0.2, { obstruction });
    const share = (rig.reach - pulled) / (CHASE.armRest - pulled);
    expect(share).toBeGreaterThan(0.05);
    expect(share).toBeLessThan(0.45);
  });

  it("stops in front of a solid mesh behind the rider", () => {
    // A 2 m cube centred 3.5 m behind the wheel (heading +x): its near face is 2.5 m back.
    const wall = new THREE.Mesh(new THREE.BoxGeometry(2, 2, 2));
    wall.position.set(-3.5, 1.5, 0);
    wall.updateMatrixWorld();
    const rig = new ChaseRig();
    run(rig, 1, { obstruction: meshObstruction([wall]) });
    const behind = -rig.position.x;
    expect(behind).toBeLessThan(2.5 - CHASE.obstructionMargin + 0.05);
    expect(behind).toBeGreaterThan(CHASE.minArm * 0.9);
    const clear = new ChaseRig();
    run(clear, 1, { obstruction: meshObstruction([]) });
    expect(-clear.position.x).toBeCloseTo(CHASE.armRest, 3);
  });

  it("frames a wheel alone at a smaller scale", () => {
    const rig = new ChaseRig();
    rig.step(input({ scale: 0.5 }), 1 / 60);
    expect(-rig.position.x).toBeCloseTo(CHASE.armRest * 0.5, 6);
    expect(rig.position.y).toBeCloseTo(CHASE.height * 0.5, 6);
  });

  it("rides on a filtered ground and never goes under the floor", () => {
    const rig = new ChaseRig();
    run(rig, 1);
    // A 6 cm pothole edge under the wheel: the camera base moves less than half of it in 50 ms.
    run(rig, 0.05, { ground: -0.06 });
    expect(rig.base).toBeGreaterThan(-0.03);
    run(rig, 1, { groundAt: () => 1.8 });
    expect(rig.position.y).toBeGreaterThanOrEqual(1.8 + CHASE.floorClearance - 1e-9);
  });

  it("is the same at 240 and 60 frames per second", () => {
    const a = new ChaseRig();
    const b = new ChaseRig();
    run(a, 0.1);
    run(b, 0.1);
    run(a, 1, { speedMps: 9, heading: 0.8 }, 1 / 240);
    run(b, 1, { speedMps: 9, heading: 0.8 }, 1 / 60);
    expect(a.yaw).toBeCloseTo(b.yaw, 2);
    expect(a.arm).toBeCloseTo(b.arm, 2);
    expect(a.position.distanceTo(b.position)).toBeLessThan(5e-3);
  });

  it("cuts instead of easing under reduced motion", () => {
    const rig = new ChaseRig();
    run(rig, 0.5);
    rig.step(input({ speedMps: fast, heading: 1 }), 1 / 60, true);
    expect(rig.yaw).toBeCloseTo(1, 9);
    expect(rig.arm).toBe(CHASE.armFast);
    expect(rig.bank).toBe(0);
  });
});
