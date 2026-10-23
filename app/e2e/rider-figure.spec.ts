import { mkdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { createServer, type ViteDevServer } from "vite";
import {
  BATCH_FLAG_POSES,
  BATCH_HEADER_BYTES,
  BATCH_MAGIC,
  BATCH_VERSION,
  CHANNELS,
  POSE_FLOATS,
} from "../src/engine/telemetry.gen.ts";

// The rider figure (EUC Thrills' Cool Rider, src/workspaces/ride/riderFigure) in the whole studio,
// posed on body lists and frames from real engine recordings (fixtures/rider-poses.json). Only
// the engine transport is the controlled fixture of the live pose checks. Screenshots of every
// frame, figure and physics bodies, Side and Chase cameras, go to test-results/rider-figure/.
const root = fileURLToPath(new URL("..", import.meta.url));
const fixture = fileURLToPath(new URL("./fixtures/live-wire.ts", import.meta.url));
const screens = fileURLToPath(new URL("../test-results/rider-figure/screens/", import.meta.url));
const IN_TAURI =
  'const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;';

interface Rider {
  rider_model: string;
  bodies: { name: string; role: string; geoms: unknown[] }[];
  frames: Record<string, { scenario: string; t_s: number; poses: number[] }>;
}
const riders = JSON.parse(
  readFileSync(new URL("./fixtures/rider-poses.json", import.meta.url), "utf8"),
) as Record<"articulated" | "rigid", Rider>;

let server: ViteDevServer;
let base: string;
test.beforeAll(async () => {
  server = await createServer({
    root,
    configFile: `${root}/vite.config.ts`,
    cacheDir: `${root}/node_modules/.vite-rider-figure`,
    server: { host: "127.0.0.1", port: 0, hmr: false, open: false },
    plugins: [
      {
        name: "controlled-live-engine",
        enforce: "pre",
        resolveId(id, importer) {
          if (
            id === "@tauri-apps/api/core" &&
            (importer?.endsWith("/engine/live.ts") || importer?.endsWith("/engine/compareLive.ts"))
          )
            return fixture;
        },
        transform(code, id) {
          const engine = ["/src/engine/index.ts", "/src/engine/compare.ts"].find((f) =>
            id.endsWith(f),
          );
          if (!engine) return;
          if (!code.includes(IN_TAURI)) throw new Error(`${engine} changed its Tauri check.`);
          return code.replace(IN_TAURI, "const inTauri = true;");
        },
      },
    ],
  });
  await server.listen();
  const address = server.httpServer?.address();
  if (!address || typeof address === "string") throw new Error("No fixture port.");
  base = `http://127.0.0.1:${address.port}/#/ride`;
  mkdirSync(screens, { recursive: true });
});
test.afterAll(async () => {
  await server?.close();
});

function send(page: Page, message: unknown) {
  return page.evaluate((m) => window.liveWire.messages?.(m), message);
}

/** A run of `rider` paused on one of its frames. */
async function show(page: Page, rider: Rider, frame: string, epoch: number) {
  await send(page, { type: "reset", scenario: "ride-20kmh-stop" });
  await send(page, { type: "bodies", bodies: rider.bodies });
  await send(page, {
    type: "status",
    run_mode: "scenario",
    run_epoch: epoch,
    buttons_ready: false,
    power_button_ready: false,
    state: "paused",
    message: "Paused",
    label: "Stock",
    build: "Fixture",
    backend: "fixture",
    scenario: "ride-20kmh-stop",
    firmware: "stock",
    t_s: 1,
    rtf: 0,
    target_rtf: 1,
    playing: false,
  });
  await sendFrame(page, rider, frame, 1);
}

/** One batch of `rider`'s frame at time `t` (the paused scene draws when a batch arrives). */
async function sendFrame(page: Page, rider: Rider, frame: string, t: number) {
  const poses = rider.frames[frame]?.poses;
  if (!poses) throw new Error(`No frame ${frame}.`);
  const layout = {
    header: [BATCH_MAGIC, BATCH_VERSION, BATCH_FLAG_POSES, BATCH_HEADER_BYTES],
    channels: CHANNELS.length,
    bodies: rider.bodies.length,
    poseFloats: POSE_FLOATS,
  };
  await page.evaluate(
    ({ poses, layout, t }) => {
      const [magic, version, flags, headerBytes] = layout.header as [
        number,
        number,
        number,
        number,
      ];
      const size = headerBytes + 8 + 4 * layout.channels + 4 * layout.bodies * layout.poseFloats;
      const buffer = new ArrayBuffer(size);
      const view = new DataView(buffer);
      view.setUint32(0, magic, true);
      view.setUint16(4, version, true);
      view.setUint16(6, flags, true);
      view.setUint32(8, 1, true);
      view.setUint32(12, layout.channels, true);
      view.setUint32(16, layout.bodies, true);
      view.setFloat64(headerBytes, t, true);
      new Float32Array(buffer, headerBytes + 8 + 4 * layout.channels).set(poses);
      window.liveWire.stream?.(buffer);
    },
    { poses, layout, t },
  );
}

/** What the scene draws for the rider: the figure's meshes, and the shapes on the rider bodies. */
function drawn(page: Page) {
  return page.evaluate(() => {
    const get = (window as unknown as { __eucScene?: () => { scene: import("three").Scene } })
      .__eucScene;
    const scene = get?.().scene;
    let figure = 0;
    let shapes = 0;
    scene?.getObjectByName("rider-figure")?.traverseVisible((o) => {
      if ((o as import("three").Mesh).isMesh) figure++;
    });
    scene?.traverse((o) => {
      if (o.name !== "body:rider" && !/^body:(left|right)_/.test(o.name)) return;
      if (!o.visible) return;
      o.traverse((c) => {
        if ((c as import("three").Mesh).isMesh) shapes++;
      });
    });
    return { figure, shapes };
  });
}

async function open(page: Page) {
  // Once (WebKit, 2026-10-22) the first load threw "SyntaxError: Unexpected token ':'" with no
  // usable stack, and React unmounted the studio. WebKit errors carry their source: log it.
  await page.addInitScript(() =>
    addEventListener("error", (event) => {
      const error = event.error as { sourceURL?: string; line?: number } | null;
      if (error?.sourceURL) console.error(`error source: ${error.sourceURL}:${error.line}`);
    }),
  );
  page.on("console", (message) => {
    if (message.text().startsWith("error source:")) console.log(message.text());
  });
  await page.goto(base);
  await expect(page.getByTestId("viewport")).toHaveAttribute("data-scene", "ready", {
    timeout: 60_000,
  });
  await expect.poll(() => page.evaluate(() => !!window.liveWire.messages)).toBe(true);
}

const toggle = (page: Page) => page.getByRole("button", { name: /^Physics bodies/ });
const camera = (page: Page, name: string) => page.getByRole("radio", { name, exact: true });

test("draws the rider figure on the physics bodies, and the shapes with Physics bodies on", async ({
  page,
}) => {
  await open(page);
  await show(page, riders.articulated, "cruise", 1);
  await expect.poll(() => drawn(page)).toEqual({ figure: 26, shapes: 0 });
  await expect(toggle(page)).toHaveAttribute("aria-pressed", "false");

  await toggle(page).click();
  // The articulated rider's shapes: trunk, head, two arms, and a thigh, shank and sole a side.
  await expect.poll(() => drawn(page)).toEqual({ figure: 0, shapes: 10 });
  await page.reload();
  await expect(page.getByTestId("viewport")).toHaveAttribute("data-scene", "ready", {
    timeout: 60_000,
  });
  await expect.poll(() => page.evaluate(() => !!window.liveWire.messages)).toBe(true);
  await show(page, riders.articulated, "cruise", 2);
  await expect(toggle(page)).toHaveAttribute("aria-pressed", "true");
  await expect.poll(() => drawn(page)).toEqual({ figure: 0, shapes: 10 });
  await toggle(page).click();
  await expect.poll(() => drawn(page)).toEqual({ figure: 26, shapes: 0 });

  // The rigid rider (one body) wears the same figure.
  await show(page, riders.rigid, "accelerate", 3);
  await expect.poll(() => drawn(page)).toEqual({ figure: 26, shapes: 0 });
});

test("captures every frame: figure and physics bodies, Side and Chase", async ({ page }) => {
  test.setTimeout(240_000);
  await open(page);
  const viewport = page.getByTestId("viewport");
  let epoch = 10;
  for (const model of ["articulated", "rigid"] as const) {
    const rider = riders[model];
    for (const frame of Object.keys(rider.frames)) {
      await show(page, rider, frame, ++epoch);
      for (const physics of [false, true]) {
        if ((await toggle(page).getAttribute("aria-pressed")) !== String(physics))
          await toggle(page).click();
        await expect.poll(async () => (await drawn(page)).figure > 0).toBe(!physics);
        for (const view of ["Side", "Chase"]) {
          await camera(page, view).click();
          // The cameras ease over about a second; the paused scene draws when a batch comes.
          for (let i = 1; i <= 40; i++) {
            await sendFrame(page, rider, frame, 1 + i / 1000);
            await page.waitForTimeout(40);
          }
          await viewport.screenshot({
            path: `${screens}${model}-${frame}-${view.toLowerCase()}-${physics ? "physics" : "figure"}.png`,
          });
        }
      }
    }
  }
});
