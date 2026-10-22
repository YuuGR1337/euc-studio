import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { createServer, type ViteDevServer } from "vite";

// Production panel, controller, map editor, API validation, and CSS.
// Only the engine transport and gamepad device are controlled fixtures.
const root = fileURLToPath(new URL("..", import.meta.url));
const fixture = fileURLToPath(new URL("./fixtures/free-ride-controls.ts", import.meta.url));
let server: ViteDevServer;
let url: string;
test.beforeAll(async () => {
  server = await createServer({
    root,
    configFile: `${root}/vite.config.ts`,
    server: { host: "127.0.0.1", port: 0, hmr: false, open: false },
    plugins: [
      {
        name: "controlled-free-ride-ui",
        enforce: "pre",
        resolveId(id, importer) {
          if (id === "@tauri-apps/api/core" && importer?.endsWith("/engine/freeRide.ts"))
            return fixture;
        },
        configureServer(vite) {
          vite.middlewares.use((request, response, next) => {
            if (request.url?.split("?")[0] !== "/__free-ride-controls.html") return next();
            void vite
              .transformIndexHtml(
                request.url,
                `<!doctype html><html lang="en"><head><meta charset="UTF-8"><title>Free ride controlled fixture</title></head><body>
<header style="padding:16px">Controlled transport. No firmware or physical simulation runs.</header>
<label style="position:fixed;right:24px;top:80px">Outside text field<input id="outside" aria-label="Outside text field"></label>
<main id="root" style="width:420px;margin:16px"></main>
<script type="module">
import React from "react";
import { createRoot } from "react-dom/client";
import { engine } from "/e2e/fixtures/free-ride-controls.ts";
import { TooltipProvider } from "/src/ui/Tooltip.tsx";
import { FreeRidePanel } from "/src/workspaces/freeRide/FreeRidePanel.tsx";
import "/src/design/global.css";
createRoot(document.getElementById("root")).render(React.createElement(TooltipProvider, null, React.createElement(FreeRidePanel, {engine, active:true})));
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
  url = `http://127.0.0.1:${address.port}/__free-ride-controls.html`;
});
test.afterAll(async () => {
  await server?.close();
});

async function open(page: Page) {
  await page.goto(url);
  await page.getByLabel("Free ride control surface", { exact: true }).focus();
  await expect(page.getByRole("button", { name: "Arm and ride", exact: true })).toBeEnabled();
}
async function arm(page: Page) {
  await expect(page.getByRole("button", { name: "Arm and ride", exact: true })).toBeEnabled();
  await page.getByRole("button", { name: "Arm and ride", exact: true }).click();
  await expect(page.getByRole("button", { name: "Pause ride", exact: true })).toBeVisible();
}
async function lastInput(page: Page) {
  return page.evaluate(() => window.freeRideFixture.inputs.at(-1));
}
async function select(page: Page, name: string, option: string) {
  await page.getByRole("combobox", { name, exact: true }).click();
  await page.getByRole("option", { name: option, exact: true }).click();
}

test("held keys send live lean and turn; release sends neutral with ordered packets", async ({
  page,
}) => {
  await open(page);
  await arm(page);
  await page.keyboard.down("w");
  await page.keyboard.down("a");
  await expect
    .poll(async () => (await lastInput(page))?.desired_lean_rad)
    .toBeCloseTo((10 * Math.PI) / 180);
  await expect
    .poll(async () => (await lastInput(page))?.heading_rate_rad_s)
    .toBeCloseTo((30 * Math.PI) / 180);
  await page.keyboard.up("w");
  await page.keyboard.up("a");
  await expect.poll(async () => (await lastInput(page))?.desired_lean_rad).toBe(0);
  await expect.poll(async () => (await lastInput(page))?.heading_rate_rad_s).toBe(0);
  const packets = await page.evaluate(() => window.freeRideFixture.inputs);
  expect(packets.length).toBeGreaterThan(5);
  expect(packets.every((p, i) => i === 0 || p.seq > (packets[i - 1]?.seq ?? Infinity))).toBe(true);
});

test("outside text focus pauses and a held key cannot resume on refocus", async ({ page }) => {
  await open(page);
  await arm(page);
  await page.keyboard.down("w");
  await expect.poll(async () => (await lastInput(page))?.desired_lean_rad).toBeGreaterThan(0);
  await page.getByLabel("Outside text field", { exact: true }).focus();
  await expect(page.getByRole("button", { name: "Arm and ride", exact: true })).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.freeRideFixture.pauses)).toBeGreaterThan(0);
  await page.getByLabel("Free ride control surface", { exact: true }).focus();
  const refocusedAt = await page.evaluate(() => performance.now());
  // Keep W held for twice the neutral dwell. A transient disabled state cannot prove this guard.
  await expect
    .poll(() => page.evaluate((start) => performance.now() - start, refocusedAt))
    .toBeGreaterThan(500);
  await expect(page.getByRole("button", { name: "Arm and ride", exact: true })).toBeDisabled();
  await page.keyboard.up("w");
  await arm(page);
  await expect.poll(async () => (await lastInput(page))?.desired_lean_rad).toBe(0);
});

test("keyboard slider sends motion only while its key stays down", async ({ page }) => {
  await open(page);
  await arm(page);
  await page.getByRole("slider", { name: "Desired lean", exact: true }).focus();
  await page.keyboard.down("ArrowRight");
  await expect.poll(async () => (await lastInput(page))?.desired_lean_rad).toBeGreaterThan(0);
  await page.keyboard.up("ArrowRight");
  await expect.poll(async () => (await lastInput(page))?.desired_lean_rad).toBe(0);
  await expect(page.getByRole("button", { name: "Pause ride", exact: true })).toBeVisible();
});

test("gamepad axes drive live packets and disconnect pauses", async ({ page }) => {
  await open(page);
  await page.evaluate(() => window.freeRideFixture.gamepad([0, 0]));
  await select(page, "Input source", "Gamepad");
  await select(page, "Gamepad", "Controlled standard gamepad");
  await arm(page);
  await page.evaluate(() => window.freeRideFixture.gamepad([0.5, -0.6]));
  await expect.poll(async () => (await lastInput(page))?.desired_lean_rad).toBeGreaterThan(0);
  await expect.poll(async () => (await lastInput(page))?.heading_rate_rad_s).toBeLessThan(0);
  await page.evaluate(() => window.freeRideFixture.gamepad(null));
  await expect(page.getByRole("button", { name: "Arm and ride", exact: true })).toBeDisabled();
  await expect.poll(() => page.evaluate(() => window.freeRideFixture.status.armed)).toBe(false);
});

test("lost input replies pause instead of retaining motion", async ({ page }) => {
  await open(page);
  await arm(page);
  await page.keyboard.down("w");
  await expect.poll(async () => (await lastInput(page))?.desired_lean_rad).toBeGreaterThan(0);
  await page.evaluate(() => window.freeRideFixture.delayInput(true));
  await expect(page.getByRole("button", { name: "Arm and ride", exact: true })).toBeVisible();
  await expect(
    page.getByText("Input reply timed out. Controls are paused.", { exact: true }),
  ).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.freeRideFixture.status.armed)).toBe(false);
  await page.keyboard.up("w");
  const beforeDrain = await page.evaluate(() => window.freeRideFixture.inputs.length);
  await page.evaluate(() => window.freeRideFixture.delayInput(false));
  await expect
    .poll(() => page.evaluate(() => window.freeRideFixture.inputs.length))
    .toBeGreaterThan(beforeDrain + 2);
  expect(await page.evaluate(() => window.freeRideFixture.status.armed)).toBe(false);
  const afterDrain = await page.evaluate(
    (start) => window.freeRideFixture.inputs.slice(start),
    beforeDrain,
  );
  expect(
    afterDrain.every(
      (packet) => !packet.armed && packet.desired_lean_rad === 0 && packet.heading_rate_rad_s === 0,
    ),
  ).toBe(true);
});

test("map editing pauses and sends a new run specification", async ({ page }) => {
  await open(page);
  await arm(page);
  await page.getByRole("tab", { name: "World", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.freeRideFixture.status.armed)).toBe(false);
  await select(page, "Map preset", "Gentle slope");
  await page.getByRole("button", { name: "Apply map and reset", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.freeRideFixture.starts.length)).toBe(1);
  const spec = await page.evaluate(() => window.freeRideFixture.starts[0]);
  expect(spec?.map?.grade_pct).toEqual([5, 0]);
  expect(spec?.rider_model).toBe("rigid_ankle_v1");
  await expect.poll(() => page.evaluate(() => window.freeRideFixture.status.epoch)).toBe(2);
  await expect(page.getByRole("button", { name: "Arm and ride", exact: true })).toBeVisible();
});

test("pointer release outside the slider track sends neutral and keeps the ride armed", async ({
  page,
}) => {
  await open(page);
  await arm(page);
  const thumb = page.getByRole("slider", { name: "Desired lean", exact: true });
  const bounds = await thumb.boundingBox();
  if (!bounds) throw new Error("Slider has no bounds.");
  await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);
  await page.mouse.down();
  await page.mouse.move(bounds.x + 70, bounds.y + bounds.height / 2);
  await expect.poll(async () => (await lastInput(page))?.desired_lean_rad).toBeGreaterThan(0);
  await page.mouse.move(bounds.x + 90, bounds.y + 100);
  await page.mouse.up();
  await expect.poll(async () => (await lastInput(page))?.desired_lean_rad).toBe(0);
  await expect(page.getByRole("button", { name: "Pause ride", exact: true })).toBeVisible();
  expect(await page.evaluate(() => window.freeRideFixture.status.armed)).toBe(true);
});

test("native map rejection retains the current epoch and reports the error", async ({ page }) => {
  await open(page);
  await page.evaluate(() => window.freeRideFixture.rejectMap(true));
  await page.getByRole("tab", { name: "World", exact: true }).click();
  await select(page, "Map preset", "Gentle slope");
  await page.getByRole("button", { name: "Apply map and reset", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveText("Fixture native terrain rejection.");
  expect(await page.evaluate(() => window.freeRideFixture.status.epoch)).toBe(1);
  expect(await page.evaluate(() => window.freeRideFixture.status.armed)).toBe(false);
});
