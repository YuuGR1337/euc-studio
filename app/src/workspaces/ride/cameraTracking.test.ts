import { beforeEach, describe, expect, it, vi } from "vitest";
import { PerspectiveCamera } from "three";

const data = vi.hoisted(() => ({
  engine: {
    kind: "live",
    bodies: [{ role: "wheel" }, { role: "shell" }, { role: "rider" }],
    latest: { speed: 0, t: 0 },
    status: { runMode: "scenario" },
    terrain: null,
  },
  frame: { time: 0, generation: 1 },
  wheel: { x: 0, y: 0.23, z: 0 },
  heading: 0,
}));
vi.mock("../../engine/index.ts", () => ({ engine: data.engine }));
vi.mock("./poses.ts", () => ({
  visualFrame: () => data.frame,
  visualShellHeading: () => data.heading,
  visualWheelPosition: (out: { set: (x: number, y: number, z: number) => unknown }) =>
    out.set(data.wheel.x, data.wheel.y, data.wheel.z),
}));
import { ChaseCamera, spring } from "./cameraTracking.ts";

describe("chase camera tracking", () => {
  beforeEach(() => {
    data.wheel = { x: 0, y: 0.23, z: 0 };
    data.heading = 0;
    data.frame.time = 0;
    data.frame.generation = 1;
    data.engine.latest.speed = 0;
    data.engine.latest.t = 0;
  });

  it("has the same spring response at 30, 60, and 144 frames per second", () => {
    const sample = (hz: number) => {
      let x = 0,
        velocity = 0;
      for (let i = 0; i < hz; i++) [x, velocity] = spring(x, velocity, 10, 6, 1 / hz);
      return [x, velocity];
    };
    const at60 = sample(60);
    for (const hz of [30, 144]) {
      const values = sample(hz);
      expect(values[0]).toBeCloseTo(at60[0] as number, 11);
      expect(values[1]).toBeCloseTo(at60[1] as number, 11);
    }
  });

  it("keeps a stable relative view at constant high speed", () => {
    const rig = new ChaseCamera();
    const camera = new PerspectiveCamera();
    data.engine.latest.speed = 72;
    rig.follow(camera, 1 / 60, false);
    const offset = camera.position.x - data.wheel.x;
    const aimOffset = rig.aim.x - data.wheel.x;
    for (let i = 1; i <= 180; i++) {
      data.wheel.x = (20 * i) / 60;
      data.engine.latest.t = data.frame.time = i / 60;
      rig.follow(camera, 1 / 60, false);
      expect(camera.position.x - data.wheel.x).toBeCloseTo(offset, 8);
      expect(rig.aim.x - data.wheel.x).toBeCloseTo(aimOffset, 8);
    }
  });

  it("turns across the pi boundary by the short route", () => {
    const rig = new ChaseCamera();
    const camera = new PerspectiveCamera();
    data.heading = Math.PI - 0.02;
    rig.follow(camera, 1 / 60, false);
    data.heading = -Math.PI + 0.02;
    for (let i = 0; i < 120; i++) rig.follow(camera, 1 / 60, false);
    expect(Math.abs(rig.yaw - (Math.PI - 0.02))).toBeLessThan(0.041);
    expect(rig.yaw).toBeGreaterThan(Math.PI);
  });

  it("keeps a level horizon through turn updates", () => {
    const rig = new ChaseCamera();
    const camera = new PerspectiveCamera();
    rig.follow(camera, 1 / 60, false);
    data.heading = Math.PI / 2;
    for (let i = 0; i < 120; i++) rig.follow(camera, 1 / 60, false);
    expect(camera.up.toArray()).toEqual([0, 1, 0]);
    expect(camera.position.toArray().every(Number.isFinite)).toBe(true);
    expect(rig.aim.toArray().every(Number.isFinite)).toBe(true);
  });

  it("settles above terrain clearance instead of redrawing forever", () => {
    const rig = new ChaseCamera();
    const camera = new PerspectiveCamera();
    rig.groundHeight = (x) => (x < -1 ? 10 : 0);
    let moving = rig.follow(camera, 1 / 60, false);
    for (let i = 0; i < 240; i++) moving = rig.follow(camera, 1 / 60, false);
    expect(camera.position.y).toBeGreaterThanOrEqual(10.4);
    expect(moving).toBe(false);
  });

  it("cuts to a new generation instead of flying across the map", () => {
    const rig = new ChaseCamera();
    const camera = new PerspectiveCamera();
    rig.follow(camera, 1 / 60, false);
    data.wheel.x = 1000;
    data.frame.generation++;
    rig.follow(camera, 1 / 60, false);
    expect(Math.abs(camera.position.x - data.wheel.x)).toBeLessThan(5);
    expect(Math.abs(rig.aim.x - data.wheel.x)).toBeLessThan(2);
  });

  it("preserves the orbit view on its first follow frame", () => {
    const rig = new ChaseCamera();
    const camera = new PerspectiveCamera();
    rig.follow(camera, 1 / 60, false);
    rig.mode = "orbit";
    camera.position.set(8, 4, -3);
    camera.lookAt(rig.anchor);
    const position = camera.position.clone();
    rig.follow(camera, 0, false);
    expect(camera.position.distanceTo(position)).toBeLessThan(1e-10);
  });

  it("uses direct tracking when reduced motion is enabled", () => {
    const rig = new ChaseCamera();
    const camera = new PerspectiveCamera();
    rig.follow(camera, 1 / 60, true);
    data.wheel.x = 50;
    data.heading = Math.PI / 2;
    rig.follow(camera, 1 / 60, true);
    expect(rig.yaw).toBe(Math.PI / 2);
    expect(Math.abs(camera.position.x - data.wheel.x)).toBeLessThan(3);
  });
});
