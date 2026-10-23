import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { createServer, type ViteDevServer } from "vite";
import { CURB_M, POTHOLE, syntheticPayload } from "./fixtures/world-payload.ts";

// Production world stream, worker, mesh builder, view sampling and WorldSurface in a real WebGL
// canvas. Only the engine transport is a controlled fixture: chunks are synthetic (a straight
// street, fixtures/world-payload.ts). Set WORLD_PAYLOAD_BIN to euc-world's payload example
// (`cargo build -p euc-world --release --example payload`, then target/release/examples/payload)
// to also draw real generated chunks; that test saves a screenshot and is skipped otherwise.
const root = fileURLToPath(new URL("..", import.meta.url));
const fixture = fileURLToPath(new URL("./fixtures/world-chunks.ts", import.meta.url));
const screens = fileURLToPath(new URL("../test-results/world-view/", import.meta.url));
const REAL_BIN = process.env.WORLD_PAYLOAD_BIN;
const CHUNK_M = 16;
/** WorldStream's rings (worldStream.ts). */
const RINGS = {
  high: [
    { lod: 0, radius: 30 },
    { lod: 1, radius: 104 },
  ],
  low: [
    { lod: 0, radius: 18 },
    { lod: 1, radius: 56 },
  ],
};

/** Chunks per level the stream should draw around a world point. */
function expected(quality: "high" | "low", wx: number, wy: number): [number, number] {
  const rings = RINGS[quality];
  const outer = Math.max(...rings.map((r) => r.radius));
  const out: [number, number] = [0, 0];
  for (let cy = Math.floor((wy - outer) / CHUNK_M); cy <= Math.floor((wy + outer) / CHUNK_M); cy++)
    for (
      let cx = Math.floor((wx - outer) / CHUNK_M);
      cx <= Math.floor((wx + outer) / CHUNK_M);
      cx++
    ) {
      const d = Math.hypot((cx + 0.5) * CHUNK_M - wx, (cy + 0.5) * CHUNK_M - wy);
      const ring = rings.find((r) => d <= r.radius);
      if (ring) out[ring.lod] = (out[ring.lod] ?? 0) + 1;
    }
  return out;
}

let server: ViteDevServer;
let base: string;
test.beforeAll(async () => {
  server = await createServer({
    root,
    configFile: `${root}/vite.config.ts`,
    // Its own dependency cache, so a running dev studio keeps the shared one.
    cacheDir: `${root}/node_modules/.vite-world-view`,
    server: { host: "127.0.0.1", port: 0, hmr: false, open: false },
    plugins: [
      {
        name: "controlled-world-view",
        enforce: "pre",
        resolveId(id, importer) {
          if (id === "@tauri-apps/api/core" && importer?.endsWith("/engine/world.ts"))
            return fixture;
        },
        configureServer(vite) {
          vite.middlewares.use((request, response, next) => {
            const [path, query = ""] = request.url?.split("?") ?? [];
            if (path === "/__world-chunks" && request.method === "POST") {
              const body: Buffer[] = [];
              request.on("data", (part: Buffer) => body.push(part));
              request.on("end", () => {
                try {
                  const { spec, keys, lod } = JSON.parse(Buffer.concat(body).toString()) as {
                    spec: unknown;
                    keys: [number, number][];
                    lod: number;
                  };
                  const real = new URLSearchParams(query).get("source") === "real";
                  if (real && !REAL_BIN) throw new Error("WORLD_PAYLOAD_BIN is not set.");
                  const bytes =
                    real && REAL_BIN
                      ? execFileSync(REAL_BIN, [
                          JSON.stringify(spec),
                          String(lod),
                          ...keys.map(([cx, cy]) => `${cx},${cy}`),
                        ])
                      : syntheticPayload(keys, lod);
                  response.setHeader("Content-Type", "application/octet-stream");
                  response.end(bytes);
                } catch (error) {
                  response.statusCode = 500;
                  response.end(error instanceof Error ? error.message : String(error));
                }
              });
              return;
            }
            if (path !== "/__world-view.html") return next();
            void vite
              .transformIndexHtml(
                request.url ?? "",
                `<!doctype html><html lang="en"><head><meta charset="UTF-8"><title>World view controlled fixture</title></head>
<body style="margin:0;background:#101214"><main id="root" style="width:960px;height:540px"></main>
<script type="module">
import React from "react";
import { createRoot } from "react-dom/client";
import { Canvas } from "@react-three/fiber";
import * as THREE from "three";
import "/e2e/fixtures/world-chunks.ts";
import { DEFAULT_DENSITIES } from "/src/engine/world.ts";
import { WorldCamera, WorldSurface } from "/src/workspaces/ride/WorldSurface.tsx";
import { WorldStream } from "/src/workspaces/ride/worldStream.ts";
import { WorldView } from "/src/workspaces/ride/worldView.ts";
import { Outdoor } from "/src/workspaces/ride/outdoor/Outdoor.tsx";
import { EucModel } from "/src/workspaces/ride/EucModel.tsx";
const q = new URLSearchParams(location.search);
// ?look=outdoor: the studio's world look (Outdoor, Neutral tone mapping at 1.5, shadows) with the
// wheel model, seen from a chase camera 4.2 m behind and 1.95 m up.
const outdoor = q.get("look") === "outdoor";
const quality = q.get("quality") === "low" ? "low" : "high";
const frame = {
  // ?spec= a whole WorldSpec as JSON (densities, incline); else a default city of ?seed=.
  spec: q.get("spec")
    ? JSON.parse(q.get("spec"))
    : { seed: Number(q.get("seed") ?? 1), revision: 2, biome: "city", densities: DEFAULT_DENSITIES },
  start_xy_m: [Number(q.get("x") ?? 0), Number(q.get("y") ?? 0)],
  start_heading_rad: Number(q.get("heading") ?? 0),
  origin_z_m: Number(q.get("z") ?? 0),
};
const view = new WorldView(frame);
const page = { view, scene: null, camera: null, gl: null, errors: [], WorldStream, WorldView, THREE };
window.worldPage = page;
const e = React.createElement;
const plainLook = [
  e("color", { attach: "background", args: ["#A9B8C6"] }),
  e("fog", { attach: "fog", args: ["#A9B8C6", 18, 110] }),
  e("hemisphereLight", { args: ["#DDE6F0", "#3A3530", 1.1] }),
  e("directionalLight", { position: [-20, 30, 10], intensity: 1.6 }),
];
const outdoorLook = [
  e(Outdoor, { world: view, quality }),
  e(React.Suspense, { fallback: null }, e(EucModel, { accent: "#FF7A1A" })),
];
createRoot(document.getElementById("root")).render(
  e(Canvas, {
      shadows: outdoor && quality === "high",
      gl: { preserveDrawingBuffer: true, antialias: true },
      camera: outdoor ? { position: [-4.2, 1.95, 0], fov: 65, near: 0.05 } : { position: [-5, 1.8, 0], fov: 55, near: 0.05 },
      onCreated: (s) => {
        if (outdoor) {
          s.gl.toneMapping = THREE.NeutralToneMapping;
          s.gl.toneMappingExposure = 1.5;
          s.camera.lookAt(3, 1.0, 0);
        } else s.camera.lookAt(14, 0, 0);
        Object.assign(page, { scene: s.scene, camera: s.camera, gl: s.gl });
      },
    },
    ...(outdoor ? outdoorLook : plainLook),
    e(WorldCamera),
    e(WorldSurface, { world: view, quality, onError: (m) => page.errors.push(m) }),
  ),
);
</script></body></html>`,
              )
              .then((html) => {
                response.setHeader("Content-Type", "text/html");
                response.end(html);
              })
              .catch(next);
          });
        },
      },
    ],
  });
  await server.listen();
  const address = server.httpServer?.address();
  if (!address || typeof address === "string") throw new Error("No fixture port.");
  base = `http://127.0.0.1:${address.port}/__world-view.html`;
  mkdirSync(screens, { recursive: true });
});
test.afterAll(async () => {
  await server?.close();
});

type WorldPage = {
  view: import("../src/workspaces/ride/worldView.ts").WorldView;
  scene: import("three").Scene | null;
  camera: import("three").PerspectiveCamera | null;
  gl: import("three").WebGLRenderer | null;
  /** What WorldSurface reported through `onError`, in order. */
  errors: (string | null)[];
  WorldStream: typeof import("../src/workspaces/ride/worldStream.ts").WorldStream;
  WorldView: typeof import("../src/workspaces/ride/worldView.ts").WorldView;
  THREE: typeof import("three");
};
declare global {
  interface Window {
    worldPage: WorldPage;
    testStream?: {
      stream: InstanceType<WorldPage["WorldStream"]>;
      view: InstanceType<WorldPage["WorldView"]>;
      group: import("three").Group;
      wheel: [number, number];
    };
  }
}

/** Ground chunks drawn by WorldSurface, per level (by their vertex count). */
async function drawn(page: Page): Promise<[number, number]> {
  return page.evaluate(() => {
    const out: [number, number] = [0, 0];
    window.worldPage.scene?.traverse((object) => {
      if (!object.name.startsWith("world-chunk-")) return;
      const count = (object as import("three").Mesh).geometry.getAttribute("position").count;
      if (count === 129 * 129) out[0] += 1;
      else if (count === 33 * 33) out[1] += 1;
    });
    return out;
  });
}

/** Coarse colours in the canvas and how far its lower half is from the sky colour. */
async function pixels(page: Page) {
  return page.evaluate(() => {
    const gl = window.worldPage.gl?.getContext();
    if (!gl) throw new Error("No WebGL context.");
    const [w, h] = [gl.drawingBufferWidth, gl.drawingBufferHeight];
    const colours = new Set<number>();
    let ground = 0;
    const pixel = new Uint8Array(4);
    for (let j = 0; j < 18; j++)
      for (let i = 0; i < 32; i++) {
        gl.readPixels(
          Math.floor(((i + 0.5) * w) / 32),
          Math.floor(((j + 0.5) * h) / 18),
          1,
          1,
          gl.RGBA,
          gl.UNSIGNED_BYTE,
          pixel,
        );
        const [r = 0, g = 0, b = 0] = pixel;
        colours.add(((r >> 4) << 8) | ((g >> 4) << 4) | (b >> 4));
        // Rows from the bottom: the lower half is ground.
        if (j < 9) ground += Math.abs(r - 0xa9) + Math.abs(g - 0xb8) + Math.abs(b - 0xc6);
      }
    return { colours: colours.size, groundDistance: ground / (32 * 9) };
  });
}

test("streams two rings of detail around the wheel and draws the street", async ({ page }) => {
  await page.goto(base);
  await expect.poll(() => drawn(page), { timeout: 30_000 }).toEqual(expected("high", 0, 0));
  const requests = await page.evaluate(() => window.worldFixture.requests);
  // The nearest chunks come first, at full detail, four at a time.
  expect(requests[0]?.lod).toBe(0);
  expect(requests[0]?.keys).toHaveLength(4);
  expect(requests.every((r) => r.keys.length <= (r.lod === 0 ? 4 : 24))).toBe(true);
  // The view samples what was drawn, in the physics frame (start (0, 0), heading 0).
  const samples = await page.evaluate(
    ({ x, y }) => [
      window.worldPage.view.sample(0, 0)?.height_m,
      window.worldPage.view.sample(3, -7)?.height_m,
      window.worldPage.view.sample(x, y)?.height_m,
      window.worldPage.camera?.far,
    ],
    { x: POTHOLE.x, y: POTHOLE.y },
  );
  expect(samples[0]).toBeCloseTo(0, 5);
  expect(samples[1]).toBeCloseTo(CURB_M, 5);
  expect(samples[2]).toBeCloseTo(-POTHOLE.depth, 5);
  expect(samples[3]).toBe(300);
  await page.waitForTimeout(300);
  const seen = await pixels(page);
  expect(seen.colours).toBeGreaterThan(8);
  expect(seen.groundDistance).toBeGreaterThan(40);
  await page.locator("canvas").screenshot({ path: `${screens}synthetic-street.png` });
});

/** A low-quality stream of its own, fed a wheel position every 50 ms. */
async function startStream(page: Page) {
  await page.evaluate(() => {
    const { WorldStream, WorldView, THREE, view: shown, scene } = window.worldPage;
    const view = new WorldView(shown.frame);
    const group = new THREE.Group();
    scene?.add(group);
    const stream = new WorldStream(view, group, "low");
    const state = { stream, view, group, wheel: [0, 0] as [number, number] };
    window.testStream = state;
    setInterval(() => stream.update(0.3, state.wheel), 50);
  });
}
const counts = (page: Page) => page.evaluate(() => window.testStream?.stream.counts());
const moveTo = (page: Page, wheel: [number, number]) =>
  page.evaluate((at) => {
    if (window.testStream) window.testStream.wheel = at;
  }, wheel);
/** The stream's chunks per level, and the drawn chunks outside its outer ring around `at`. */
const around = (page: Page, at: [number, number]) =>
  page.evaluate(([x, y]) => {
    const t = window.testStream;
    if (!t) throw new Error("No stream.");
    const far = t.group.children
      .map((c) => c.name)
      .filter((n) => n.startsWith("world-chunk-"))
      .filter((n) => {
        const [cx = 0, cy = 0] = n.slice("world-chunk-".length).split(",").map(Number);
        return Math.hypot((cx + 0.5) * 16 - x, (cy + 0.5) * 16 - y) > 56;
      });
    return { counts: t.stream.counts(), far };
  }, at);

test("drops chunks that fall behind and loads the ones ahead", async ({ page }) => {
  await page.goto(base);
  await startStream(page);
  await expect
    .poll(() => around(page, [0, 0]), { timeout: 30_000 })
    .toEqual({ counts: expected("low", 0, 0), far: [] });
  await moveTo(page, [160, 0]);
  await expect
    .poll(() => around(page, [160, 0]), { timeout: 30_000 })
    .toEqual({ counts: expected("low", 160, 0), far: [] });
  const after = await page.evaluate(() => {
    const t = window.testStream;
    return {
      here: t?.view.sample(160, -7)?.height_m,
      behind: t?.view.sample(0, 0),
      error: t?.stream.error,
    };
  });
  expect(after.here).toBeCloseTo(CURB_M, 5);
  expect(after.behind).toBeNull();
  expect(after.error).toBeNull();
});

test("WorldSurface reports refused chunks to the view and clears it once chunks arrive", async ({
  page,
}) => {
  await page.goto(`${base}?fail=1`);
  await expect
    .poll(() => page.evaluate(() => window.worldPage.errors))
    .toEqual(["Fixture refused the chunks."]);
  expect(await drawn(page)).toEqual([0, 0]);
  await page.evaluate(() => {
    window.worldFixture.fail = false;
  });
  await expect.poll(() => drawn(page), { timeout: 30_000 }).toEqual(expected("high", 0, 0));
  expect(await page.evaluate(() => window.worldPage.errors)).toEqual([
    "Fixture refused the chunks.",
    null,
  ]);
});

test("keeps what it drew and reports the error when the engine refuses chunks", async ({
  page,
}) => {
  await page.goto(base);
  await startStream(page);
  await expect.poll(() => counts(page), { timeout: 30_000 }).toEqual(expected("low", 0, 0));
  await page.evaluate(() => {
    window.worldFixture.fail = true;
  });
  await moveTo(page, [24, 0]);
  await expect
    .poll(() => page.evaluate(() => window.testStream?.stream.error))
    .toBe("Fixture refused the chunks.");
  // The chunks still in range stay drawn and sampled; nothing new arrived.
  const [near = 0, far = 0] = (await counts(page)) ?? [];
  expect(near + far).toBeGreaterThan(0);
  expect(await page.evaluate(() => window.testStream?.view.sample(0, 0)?.height_m)).toBeCloseTo(
    0,
    5,
  );
});

/** One pixel of the canvas (rows from the top), display sRGB. */
async function pixel(page: Page, x: number, yFromTop: number) {
  return page.evaluate(
    ([px = 0, py = 0]) => {
      const gl = window.worldPage.gl?.getContext();
      if (!gl) throw new Error("No WebGL context.");
      const out = new Uint8Array(4);
      gl.readPixels(px, gl.drawingBufferHeight - 1 - py, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, out);
      return [out[0] ?? 0, out[1] ?? 0, out[2] ?? 0];
    },
    [x, yFromTop],
  );
}

test("the outdoor look: a blue sky, fog in the horizon colour and a pool under the wheel", async ({
  page,
}) => {
  await page.goto(`${base}?look=outdoor`);
  await expect.poll(() => drawn(page), { timeout: 30_000 }).toEqual(expected("high", 0, 0));
  await page.waitForTimeout(1500);
  const size = await page.evaluate(() => {
    const gl = window.worldPage.gl?.getContext();
    return [gl?.drawingBufferWidth ?? 0, gl?.drawingBufferHeight ?? 0];
  });
  const [w = 0, h = 0] = size;
  // The camera looks 7.5° down with a 65° view: the horizon is 10.3 % of the height above centre.
  const horizon = Math.round(h / 2 - 0.1032 * h);
  const top = await pixel(page, Math.round(w / 2), Math.round(h * 0.03));
  expect(top[2]).toBeGreaterThan((top[0] ?? 0) + 40);
  // Straight down the street, just under the horizon, the ground is past the fog's end: it is
  // the horizon colour, like the sky right above it.
  const fogged = await pixel(page, Math.round(w / 2), horizon + Math.round(h * 0.006));
  [188, 212, 236].forEach((c, k) => expect(Math.abs((fogged[k] ?? 0) - c)).toBeLessThan(8));
  await page.locator("canvas").screenshot({ path: `${screens}outdoor-synthetic.png` });
  // The wheel close up in daylight, for a look at its colours.
  await page.evaluate(() => {
    window.worldPage.camera?.position.set(-1.3, 0.75, 1.0);
    window.worldPage.camera?.lookAt(0, 0.4, 0);
  });
  await page.waitForTimeout(800);
  await page.locator("canvas").screenshot({ path: `${screens}outdoor-wheel.png` });
});

test("the outdoor look on the low tier keeps the sky and the fog colour, without shadows", async ({
  page,
}) => {
  await page.goto(`${base}?look=outdoor&quality=low`);
  await expect.poll(() => drawn(page), { timeout: 30_000 }).toEqual(expected("low", 0, 0));
  await page.waitForTimeout(1500);
  const shadows = await page.evaluate(() => window.worldPage.gl?.shadowMap.enabled);
  expect(shadows).toBe(false);
  const [w = 0, h = 0] = await page.evaluate(() => {
    const gl = window.worldPage.gl?.getContext();
    return [gl?.drawingBufferWidth ?? 0, gl?.drawingBufferHeight ?? 0];
  });
  const top = await pixel(page, Math.round(w / 2), Math.round(h * 0.03));
  expect(top[2]).toBeGreaterThan((top[0] ?? 0) + 40);
  const fogged = await pixel(page, Math.round(w / 2), Math.round(h / 2 - 0.1032 * h) + 3);
  [188, 212, 236].forEach((c, k) => expect(Math.abs((fogged[k] ?? 0) - c)).toBeLessThan(8));
  await page.locator("canvas").screenshot({ path: `${screens}outdoor-low.png` });
});

/** Real chunks in the outdoor look, from a chase camera, for side-by-side review. */
/** euc_world::Densities::default(), for specs written here (the page has DEFAULT_DENSITIES). */
const DEFAULT_DENSITIES_FOR_SPEC = {
  potholes: 1.5,
  manholes: 0.6,
  cracks: 12,
  rough_patches: 0.8,
  wet_patches: 1,
  speed_bumps: 0.35,
  crosswalks: 0.7,
  driveways: 1.5,
  plazas: 0.15,
  props: 0.8,
};
const OUTDOOR_VIEWS: {
  name: string;
  x: number;
  y: number;
  heading: number;
  spec?: unknown;
}[] = [
  { name: "spawn-east", x: 0, y: 0, heading: 0 },
  { name: "pothole-street", x: 70, y: 3.05, heading: 0 },
  { name: "stairs-plaza", x: 124.4, y: -47.62, heading: -95.35 },
  { name: "ramp-plaza", x: 117.9, y: -55.75, heading: 8.86 },
  // W2: the hill-climb scenario's city, graded 20 % east from 40 m, seen from 30 m out.
  {
    name: "hill-climb-20pct",
    x: 0,
    y: 0,
    heading: 0,
    spec: {
      seed: 42,
      revision: 2,
      biome: "city",
      densities: {
        ...DEFAULT_DENSITIES_FOR_SPEC,
        hills: 0,
        potholes: 0,
        manholes: 0,
        cracks: 0,
        rough_patches: 0,
        wet_patches: 0,
      },
      incline: { origin: [0, 0], direction: [1, 0], slope_pct: 20, start_m: 40, ramp_m: 10 },
    },
  },
];
for (const v of OUTDOOR_VIEWS) {
  test(`renders real chunks in the outdoor look: ${v.name}`, async ({ page }) => {
    test.skip(!REAL_BIN, "Set WORLD_PAYLOAD_BIN to euc-world's payload example.");
    // Real chunks with shadows on a software renderer can take minutes on a busy machine.
    test.setTimeout(300_000);
    const heading = (v.heading * Math.PI) / 180;
    const world = "spec" in v ? `spec=${encodeURIComponent(JSON.stringify(v.spec))}` : "seed=42";
    const url = `${base}?look=outdoor&source=real&${world}&x=${v.x}&y=${v.y}&heading=${heading}`;
    // First the height under the start (the physics frame's z = 0), then the view on it.
    await page.goto(url);
    await expect.poll(() => drawn(page), { timeout: 60_000 }).not.toEqual([0, 0]);
    await expect
      .poll(() => page.evaluate(() => window.worldPage.view.sample(0, 0)?.height_m ?? null), {
        timeout: 60_000,
      })
      .not.toBeNull();
    const z = await page.evaluate(() => window.worldPage.view.sample(0, 0)?.height_m ?? 0);
    await page.goto(`${url}&z=${z}`);
    await expect.poll(() => drawn(page), { timeout: 60_000 }).toEqual(expected("high", v.x, v.y));
    await page.waitForTimeout(1500);
    await page.locator("canvas").screenshot({ path: `${screens}outdoor-${v.name}.png` });
    // Draw calls, triangles and the frame rate of this browser, for the phase's budget table.
    const numbers = await page.evaluate(async () => {
      const gl = window.worldPage.gl;
      const start = performance.now();
      let frames = 0;
      await new Promise<void>((done) => {
        const tick = () => {
          frames += 1;
          if (performance.now() - start < 2000) requestAnimationFrame(tick);
          else done();
        };
        requestAnimationFrame(tick);
      });
      return {
        fps: Math.round((frames * 1000) / (performance.now() - start)),
        calls: gl?.info.render.calls,
        triangles: gl?.info.render.triangles,
        textures: gl?.info.memory.textures,
        geometries: gl?.info.memory.geometries,
      };
    });
    writeFileSync(`${screens}outdoor-${v.name}.json`, JSON.stringify(numbers));
  });
}

test("draws real generated chunks", async ({ page }) => {
  test.skip(!REAL_BIN, "Set WORLD_PAYLOAD_BIN to euc-world's payload example.");
  // The pothole scenario's street (scenarios/world/world-pothole-15kmh.toml): looking east.
  await page.goto(`${base}?source=real&seed=42&x=70&y=3.05&heading=0`);
  await expect.poll(() => drawn(page), { timeout: 60_000 }).toEqual(expected("high", 70, 3.05));
  await page.evaluate(() => {
    const { camera, view } = window.worldPage;
    const z = view.sample(0, 0)?.height_m ?? 0;
    camera?.position.set(-5, z + 1.8, 0);
    camera?.lookAt(14, z, 0);
  });
  await page.waitForTimeout(500);
  const seen = await pixels(page);
  expect(seen.colours).toBeGreaterThan(8);
  await page.locator("canvas").screenshot({ path: `${screens}real-street.png` });
});
