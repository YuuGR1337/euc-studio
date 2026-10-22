import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { createServer, type ViteDevServer } from "vite";
import {
  BATCH_FLAG_POSES,
  BATCH_HEADER_BYTES,
  BATCH_MAGIC,
  BATCH_VERSION,
  CH,
  CHANNELS,
  POSE_FLOATS,
} from "../src/engine/telemetry.gen.ts";

// The whole studio (LiveEngine, LiveCompare, the visual frame, LiveBodies, the Ride scene) in a
// real WebGL canvas. Only their transport is a controlled fixture (fixtures/live-wire.ts), so the
// spec delivers messages and batches in the order Tauri does. Tauri runs a small batch (under
// 1 KiB) at once but fetches a JSON message of 8 KiB or more: the stock run's body list is about
// 8.5 KiB, so after a reload the one-sample batch of a finished run arrives before its bodies, and
// a comparison's first batches arrive before its reset (both body lists, about 17 KiB).
const root = fileURLToPath(new URL("..", import.meta.url));
const fixture = fileURLToPath(new URL("./fixtures/live-wire.ts", import.meta.url));
const IN_TAURI =
  'const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;';

const BODIES = [
  ["wheel", "wheel"],
  ["shell", "shell"],
  ["rider", "rider"],
  ["left_thigh", "rider"],
  ["left_shank", "rider"],
  ["left_foot", "rider"],
  ["right_thigh", "rider"],
  ["right_shank", "rider"],
  ["right_foot", "rider"],
] as const;
const capsule = (radius: number, half: number, z: number) => ({
  kind: "capsule",
  size: [radius, half, 0],
  pos: [0, 0, z],
  quat: [1, 0, 0, 0],
});
const bodies = BODIES.map(([name, role]) => ({
  name,
  role,
  geoms:
    role !== "rider"
      ? []
      : name === "rider"
        ? [capsule(0.13, 0.2, 0.25), { ...capsule(0.1, 0, 0.62), kind: "sphere" }]
        : [capsule(0.06, 0.18, -0.2)],
}));
/** A finished "Ride to 20 km/h and stop" at 40 s: physics frame (z up), w x y z quaternions. */
const FINAL: Record<string, [number, number, number]> = {
  wheel: [80, 0, 0.225],
  shell: [80, 0, 0.215],
  rider: [80.02, 0, 1.171],
  left_thigh: [80, 0.1, 0.93],
  left_shank: [80, 0.1, 0.56],
  left_foot: [80, 0.1, 0.307],
  right_thigh: [80, -0.1, 0.93],
  right_shank: [80, -0.1, 0.56],
  right_foot: [80, -0.1, 0.307],
};

let server: ViteDevServer;
let base: string;
test.beforeAll(async () => {
  server = await createServer({
    root,
    configFile: `${root}/vite.config.ts`,
    // Its own dependency cache, so a running dev studio keeps the shared one.
    cacheDir: `${root}/node_modules/.vite-live-poses`,
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
          // The live engines, as in the desktop app; the rest of the app stays a browser build.
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
});
test.afterAll(async () => {
  await server?.close();
});

async function open(page: Page) {
  await page.goto(base);
  await expect(page.getByTestId("viewport")).toHaveAttribute("data-scene", "ready", {
    timeout: 60_000,
  });
  await expect.poll(() => page.evaluate(() => !!window.liveWire.messages)).toBe(true);
}

function send(page: Page, message: unknown) {
  return page.evaluate((m) => window.liveWire.messages?.(m), message);
}

/**
 * One sample at `t` with the poses of FINAL and `speed` (other channels 0), as
 * crates/euc-telemetry lays out a batch: to the ride's stream, or to a comparison's run A or B.
 */
function sendBatch(
  page: Page,
  t: number,
  { to = "stream", speed = 0 }: { to?: "stream" | "a" | "b"; speed?: number } = {},
) {
  const poses = BODIES.flatMap(([name]) => [...(FINAL[name] ?? [0, 0, 0]), 1, 0, 0, 0]);
  const layout = {
    header: [BATCH_MAGIC, BATCH_VERSION, BATCH_FLAG_POSES, BATCH_HEADER_BYTES],
    channels: CHANNELS.length,
    speedLane: CH.speed_kmh,
    bodies: BODIES.length,
    poseFloats: POSE_FLOATS,
  };
  return page.evaluate(
    ({ t, poses, layout, to, speed }) => {
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
      view.setUint32(20, 0, true);
      view.setUint32(24, 0, true);
      view.setFloat64(headerBytes, t, true);
      const lanes = new Float32Array(buffer, headerBytes + 8, layout.channels).fill(0);
      lanes[layout.speedLane] = speed;
      new Float32Array(buffer, headerBytes + 8 + 4 * layout.channels).set(poses);
      if (to === "stream") window.liveWire.stream?.(buffer);
      else window.liveWire.compare?.[to](buffer);
    },
    { t, poses, layout, to, speed },
  );
}

function status(page: Page, state: string, t: number) {
  return send(page, {
    type: "status",
    run_mode: "scenario",
    run_epoch: 1,
    buttons_ready: false,
    power_button_ready: false,
    state,
    message: state === "completed" ? "Scenario complete. Restart for another run" : "Paused",
    label: "Stock",
    build: "Fixture",
    backend: "fixture",
    scenario: "ride-20kmh-stop",
    firmware: "stock",
    t_s: t,
    rtf: 0,
    target_rtf: 1,
    playing: false,
  });
}

/** Each live body group: shown or not, and where (its local frame is the physics frame). */
function drawnBodies(page: Page) {
  return page.evaluate(
    (names) => {
      const get = (window as unknown as { __eucScene?: () => { scene: import("three").Scene } })
        .__eucScene;
      const scene = get?.().scene;
      return names.map((name) => {
        const group = scene?.getObjectByName(`body:${name}`);
        return group
          ? { name, visible: group.visible, at: group.position.toArray().map((v) => +v.toFixed(3)) }
          : { name, visible: null, at: null };
      });
    },
    BODIES.map(([name]) => name) as string[],
  );
}

test("draws a finished run's last frame after a reload, though the frame came before its bodies", async ({
  page,
}) => {
  await open(page);
  // Announce on a finished run: reset (small, at once), the one-sample batch (380 bytes, at
  // once), then the bodies (fetched), then the status.
  await send(page, { type: "reset", scenario: "ride-20kmh-stop" });
  await sendBatch(page, 40);
  await send(page, { type: "bodies", bodies });
  await status(page, "completed", 40);
  await expect.poll(async () => (await drawnBodies(page)).every((b) => b.at)).toBe(true);
  // No more batches come for a finished run: the last frame stays.
  await page.waitForTimeout(1000);
  expect(await drawnBodies(page)).toEqual(
    BODIES.map(([name]) => ({
      name,
      visible: true,
      at: (FINAL[name] ?? []).map((v) => +Math.fround(v).toFixed(3)),
    })),
  );
});

test("hides bodies until a frame of them arrives, never at the origin", async ({ page }) => {
  await open(page);
  await send(page, { type: "reset", scenario: "ride-20kmh-stop" });
  await send(page, { type: "bodies", bodies });
  await status(page, "paused", 0);
  await expect.poll(async () => (await drawnBodies(page)).every((b) => b.at)).toBe(true);
  await page.waitForTimeout(500);
  expect((await drawnBodies(page)).map((b) => b.visible)).toEqual(BODIES.map(() => false));
  await sendBatch(page, 40);
  await expect
    .poll(async () => (await drawnBodies(page)).map((b) => b.visible))
    .toEqual(BODIES.map(() => true));
  expect((await drawnBodies(page)).find((b) => b.name === "left_foot")?.at).toEqual([
    80, 0.1, 0.307,
  ]);
});

/** What a comparison's runs hold: sample times, speeds and the latest frame's float count. */
function compareRuns(page: Page) {
  return page.evaluate(async (url) => {
    type Ring = { toArray(): Float64Array };
    type Run = { time: Ring; channels: { speed: Ring }; latest: { poses: Float32Array | null } };
    // The app's own module instance: the dev server serves it once under this URL.
    const { compare } = (await import(/* @vite-ignore */ url)) as { compare: { a: Run; b: Run } };
    const read = (run: Run) => ({
      t: Array.from(run.time.toArray()),
      speed: Array.from(run.channels.speed.toArray()),
      poses: run.latest.poses?.length ?? 0,
    });
    return { a: read(compare.a), b: read(compare.b) };
  }, "/src/engine/compare.ts");
}

test("a comparison keeps the samples that came before its reset", async ({ page }) => {
  await page.goto(base.replace("#/ride", "#/compare"));
  await expect(page.getByTestId("compare-viewport")).toHaveAttribute("data-scene", "ready", {
    timeout: 60_000,
  });
  await page.getByRole("button", { name: "Run", exact: true }).click();
  await expect.poll(() => page.evaluate(() => !!window.liveWire.compare)).toBe(true);
  // In real time a batch holds one sample (about 380 bytes) and runs at once; the reset, with
  // both body lists, is fetched and comes after the first batches.
  await sendBatch(page, 0.02, { to: "a", speed: 1 });
  await sendBatch(page, 0.02, { to: "b", speed: 2 });
  await sendBatch(page, 0.04, { to: "a", speed: 3 });
  await page.evaluate((reset) => window.liveWire.compare?.messages(reset), {
    type: "reset",
    scenario: "ride-20kmh-stop",
    duration_s: 40,
    stream_hz: 50,
    a: { spec: "stock", label: "Stock", sha256: "fixture-a", checks_skipped: [] },
    b: { spec: "ours-compat", label: "Ours-compat", sha256: "fixture-b", checks_skipped: [] },
    bodies_a: bodies,
    bodies_b: bodies,
    terrain: null,
    tolerance_file: "fixture.toml",
    tolerance_note: null,
  });
  await sendBatch(page, 0.06, { to: "a", speed: 5 });
  await sendBatch(page, 0.04, { to: "b", speed: 4 });
  const frame = BODIES.length * POSE_FLOATS;
  expect(await compareRuns(page)).toEqual({
    a: { t: [0.02, 0.04, 0.06], speed: [1, 3, 5], poses: frame },
    b: { t: [0.02, 0.04], speed: [2, 4], poses: frame },
  });
});
