import { describe, expect, it } from "vitest";
import { HORIZON, HORIZON_HEX, paintSky, sunWorld, worldToScene } from "./sky.ts";

const W = 128;
const H = 64;
const at = (sky: Uint8Array, i: number, j: number) => {
  const o = (j * W + i) * 4;
  return [sky[o] ?? 0, sky[o + 1] ?? 0, sky[o + 2] ?? 0];
};

describe("the painted sky", () => {
  const sky = paintSky(W, H, 42, { clouds: true, ground: false });

  it("meets the fog at the horizon, all the way round", () => {
    expect(HORIZON_HEX.toLowerCase()).toBe(
      `#${HORIZON.map((c) => c.toString(16).padStart(2, "0")).join("")}`,
    );
    // Below the horizon (v < ½) it is the fog colour; the first row above (1.4° up) is close.
    for (let i = 0; i < W; i++) {
      for (const j of [0, H / 4, H / 2 - 1]) {
        const c = at(sky, i, j);
        c.forEach((v, k) => expect(Math.abs(v - (HORIZON[k] ?? 0))).toBeLessThanOrEqual(1));
      }
      const above = at(sky, i, H / 2);
      above.forEach((v, k) => expect(Math.abs(v - (HORIZON[k] ?? 0))).toBeLessThanOrEqual(12));
    }
  });

  it("is bluer overhead and brightest toward the sun", () => {
    const zenith = at(paintSky(W, H, 42, { clouds: false, ground: false }), 0, H - 1);
    expect(zenith[2] ?? 0).toBeGreaterThan((zenith[0] ?? 0) + 60);
    // At the studio's size the sun's core spans texels: the brightest one of a clear sky looks
    // toward the sun (heading 0, world axes in the scene).
    const [w, h] = [1024, 512];
    const clear = paintSky(w, h, 42, { clouds: false, ground: false });
    let best = { i: 0, j: 0, sum: -1 };
    for (let k = 0; k < w * h; k++) {
      const sum = (clear[k * 4] ?? 0) + (clear[k * 4 + 1] ?? 0) + (clear[k * 4 + 2] ?? 0);
      if (sum > best.sum) best = { i: k % w, j: Math.floor(k / w), sum };
    }
    const phi = ((best.j + 0.5) / h - 0.5) * Math.PI;
    const theta = ((best.i + 0.5) / w - 0.5) * 2 * Math.PI;
    const look = [Math.cos(phi) * Math.cos(theta), Math.sin(phi), Math.cos(phi) * Math.sin(theta)];
    const sun = worldToScene(sunWorld(), 0);
    const angle = Math.acos(
      Math.min(
        1,
        look.reduce((a, v, k) => a + v * (sun[k] ?? 0), 0),
      ),
    );
    // Inside the core (0.014 rad), which saturates over a few texels.
    expect(angle).toBeLessThan(0.014);
  });

  it("paints the same clouds for a seed and other clouds for another seed", () => {
    expect(paintSky(W, H, 42, { clouds: true, ground: false })).toEqual(sky);
    expect(paintSky(W, H, 43, { clouds: true, ground: false })).not.toEqual(sky);
    // Without clouds the seed does not matter.
    expect(paintSky(W, H, 1, { clouds: false, ground: false })).toEqual(
      paintSky(W, H, 2, { clouds: false, ground: false }),
    );
  });

  it("gives the environment a ground below the horizon", () => {
    const env = paintSky(W, H, 42, { clouds: false, ground: true });
    const ground = at(env, 0, 0);
    expect(ground[2] ?? 0).toBeLessThan(HORIZON[2] - 60);
  });
});

describe("world directions in the scene", () => {
  const close = (a: number[], b: number[]) => a.forEach((v, k) => expect(v).toBeCloseTo(b[k] ?? 0));

  it("map east, north and up to three.js axes at heading 0", () => {
    close(worldToScene([1, 0, 0], 0), [1, 0, 0]);
    close(worldToScene([0, 1, 0], 0), [0, 0, -1]);
    close(worldToScene([0, 0, 1], 0), [0, 1, 0]);
  });

  it("turn with the start heading: the start's heading becomes the scene's +x", () => {
    // A start heading north: world north is the physics frame's +x, three.js +x.
    close(worldToScene([0, 1, 0], Math.PI / 2), [1, 0, 0]);
    close(worldToScene([1, 0, 0], Math.PI / 2), [0, 0, 1]);
  });

  it("put the sun 42° up in the south-west", () => {
    const [x, y, z] = sunWorld();
    expect(Math.asin(z)).toBeCloseTo((42 * Math.PI) / 180);
    expect(x).toBeLessThan(0);
    expect(y).toBeLessThan(0);
  });
});
