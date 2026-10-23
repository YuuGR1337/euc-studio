import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { createServer, type ViteDevServer } from "vite";

// Free Ride's play mode: the production panel, controller, HUD overlay, API validation and CSS.
// Only the engine transport and the gamepad are controlled fixtures, answering as the play-mode
// engine does. Nothing here clicks before riding.
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
<div id="hud" style="position:fixed;right:24px;top:140px"></div>
<main id="root" style="width:420px;margin:16px"></main>
<script type="module">
import React from "react";
import { createRoot } from "react-dom/client";
import { engine } from "/e2e/fixtures/free-ride-controls.ts";
import { TooltipProvider } from "/src/ui/Tooltip.tsx";
import { FreeRidePanel } from "/src/workspaces/freeRide/FreeRidePanel.tsx";
import { FreeRideOverlay } from "/src/workspaces/freeRide/FreeRideOverlay.tsx";
import "/src/design/global.css";
createRoot(document.getElementById("hud")).render(React.createElement(FreeRideOverlay));
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

const deg = (value: number) => (value * Math.PI) / 180;
const fixtureOf = (page: Page) =>
  page.evaluate(() => {
    const f = window.freeRideFixture;
    return {
      inputs: f.inputs.slice(-1),
      starts: f.starts,
      restarts: f.restarts,
      buttons: f.buttons.slice(-1),
      commands: f.commands,
      pauses: f.pauses,
      status: { ...f.status },
    };
  });
const lastInput = async (page: Page) => (await fixtureOf(page)).inputs[0];
const panelStatus = (page: Page) => page.getByRole("status").filter({ hasText: /./ }).first();
const hud = (page: Page) => page.getByTestId("free-ride-hud");

/** Opens Free Ride and waits until the firmware is ready: no click anywhere. */
async function open(page: Page) {
  await page.goto(url);
  await expect.poll(async () => (await fixtureOf(page)).status.ready).toBe(true);
  await expect(page.getByText("Riding.", { exact: false }).first()).toBeVisible();
}
/** Holds a key until the lean it asks for has built. */
async function rideForward(page: Page) {
  await page.keyboard.down("w");
  await expect.poll(async () => (await lastInput(page))?.desired_lean_rad).toBeCloseTo(deg(10));
}

test("opening Free Ride starts a ride in the city and rides on the first key", async ({ page }) => {
  await page.goto(url);
  await expect.poll(async () => (await fixtureOf(page)).starts.length).toBe(1);
  const { starts } = await fixtureOf(page);
  expect(starts[0]).toMatchObject({ world: { seed: 42 }, rider_model: "articulated_sagittal_v1" });
  await expect(hud(page)).toContainText(/boots|Building/);
  await expect.poll(async () => (await fixtureOf(page)).status.ready).toBe(true);
  await expect(page.getByText("City, seed 42")).toBeVisible();
  await rideForward(page);
  expect((await lastInput(page))?.armed).toBe(true);
  await page.keyboard.down("a");
  await expect
    .poll(async () => (await lastInput(page))?.heading_rate_rad_s ?? 0)
    .toBeGreaterThan(0.5);
  await page.keyboard.up("w");
  await page.keyboard.up("a");
  await expect.poll(async () => (await lastInput(page))?.desired_lean_rad).toBe(0);
  expect((await fixtureOf(page)).pauses).toBe(0);
});

test("crash, R, riding again: no pause and no click", async ({ page }) => {
  await open(page);
  await rideForward(page);
  const before = (await fixtureOf(page)).status.epoch;
  await page.evaluate(() => window.freeRideFixture.crash());
  await expect(hud(page)).toContainText("Off the pedals. R respawns here, Shift R at the start.");
  await page.keyboard.up("w");
  await page.keyboard.press("r");
  await expect
    .poll(async () => (await fixtureOf(page)).restarts)
    .toEqual([{ epoch: before, toStart: false }]);
  await expect.poll(async () => (await fixtureOf(page)).status.ready).toBe(true);
  await rideForward(page);
  const after = await fixtureOf(page);
  expect(after.inputs[0]?.run_epoch).toBe(before + 1);
  expect(after.pauses).toBe(0);
});

test("Shift+R and the HUD's buttons respawn at the start or here", async ({ page }) => {
  await open(page);
  const first = (await fixtureOf(page)).status.epoch;
  await page.keyboard.press("Shift+R");
  await expect
    .poll(async () => (await fixtureOf(page)).restarts.at(-1))
    .toEqual({ epoch: first, toStart: true });
  await expect.poll(async () => (await fixtureOf(page)).status.ready).toBe(true);
  await hud(page)
    .getByRole("button", { name: /Respawn/ })
    .click();
  await expect
    .poll(async () => (await fixtureOf(page)).restarts.at(-1))
    .toEqual({ epoch: first + 1, toStart: false });
  await expect.poll(async () => (await fixtureOf(page)).status.ready).toBe(true);
  // After the click the keys still ride: the focused button does not take them.
  await rideForward(page);
});

test("the wheel's pad: power off and on, handle, light, horn, pedal mode", async ({ page }) => {
  await open(page);
  await page.keyboard.down("p");
  await expect
    .poll(async () => (await fixtureOf(page)).buttons[0])
    .toMatchObject({ button: "power", pressed: true });
  await page.keyboard.up("p");
  await expect
    .poll(async () => (await fixtureOf(page)).buttons[0])
    .toMatchObject({ button: "power", pressed: false });
  // A long press switched the wheel off: the run goes on and says how to switch it on.
  await page.evaluate(() => window.freeRideFixture.power(false));
  await expect(hud(page)).toContainText("The wheel is off. P powers it on.");
  await page.keyboard.down("p");
  await page.keyboard.up("p");
  await expect(hud(page)).not.toContainText("The wheel is off");
  await page.keyboard.down("h");
  await expect
    .poll(async () => (await fixtureOf(page)).buttons[0])
    .toMatchObject({ button: "handle", pressed: true });
  await page.keyboard.up("h");
  await page.keyboard.press("l");
  await page.keyboard.press("b");
  await page.keyboard.press("m");
  await page.getByRole("radio", { name: "Strobe", exact: true }).click();
  await page.getByRole("radio", { name: "Hard", exact: true }).click();
  await page.getByRole("button", { name: /^Horn/ }).click();
  await expect
    .poll(async () => (await fixtureOf(page)).commands)
    .toEqual([
      { id: "light", value: 1 },
      { id: "horn" },
      { id: "pedal_mode", value: 0 },
      { id: "light", value: 2 },
      { id: "pedal_mode", value: 2 },
      { id: "horn" },
    ]);
  await expect(page.getByRole("radio", { name: "Strobe", exact: true })).toHaveAttribute(
    "aria-checked",
    "true",
  );
  expect((await fixtureOf(page)).pauses).toBe(0);
});

test("Escape pauses and a riding key goes on; a text field keeps its keys", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Escape");
  await expect(panelStatus(page)).toContainText("Paused");
  await page.keyboard.down("w");
  await expect(panelStatus(page)).toContainText("Riding");
  await page.keyboard.up("w");
  await page.getByLabel("Outside text field").focus();
  await page.keyboard.type("wasd");
  await expect(page.getByLabel("Outside text field")).toHaveValue("wasd");
  await page.waitForTimeout(300);
  expect((await lastInput(page))?.desired_lean_rad).toBe(0);
  await page.getByLabel("Outside text field").blur();
  await rideForward(page);
});

test("New world starts a run from a new seed", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "New world", exact: true }).click();
  await expect.poll(async () => (await fixtureOf(page)).starts.length).toBe(2);
  const seed = (await fixtureOf(page)).starts[1]?.world?.seed;
  expect(seed).not.toBe(42);
  await expect(page.getByText(`City, seed ${seed}`)).toBeVisible();
});

test("a gamepad rides when its stick moves; X respawns, Back at the start", async ({ page }) => {
  await open(page);
  await page.evaluate(() => window.freeRideFixture.gamepad([0, -1, 0, 0]));
  await expect
    .poll(async () => (await lastInput(page))?.desired_lean_rad ?? 0)
    .toBeGreaterThan(deg(5));
  expect((await lastInput(page))?.source).toBe("gamepad");
  const epoch = (await fixtureOf(page)).status.epoch;
  await page.evaluate(() => window.freeRideFixture.gamepad([0, 0, 0, 0], [2]));
  await expect
    .poll(async () => (await fixtureOf(page)).restarts.at(-1))
    .toEqual({ epoch, toStart: false });
  await page.evaluate(() => window.freeRideFixture.gamepad([0, 0, 0, 0]));
  await expect.poll(async () => (await fixtureOf(page)).status.ready).toBe(true);
  await page.evaluate(() => window.freeRideFixture.gamepad([0, 0, 0, 0], [8]));
  await expect
    .poll(async () => (await fixtureOf(page)).restarts.at(-1))
    .toEqual({ epoch: epoch + 1, toStart: true });
});

test("a rebound key rides; the studio's keys are refused", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "Keys", exact: true }).click();
  await page.getByRole("button", { name: "Change Lean forward", exact: true }).click();
  await page.keyboard.press("c");
  await expect(page.getByRole("alert")).toContainText("C is kept for the studio");
  await page.keyboard.press("i");
  await expect(page.getByLabel("Lean forward keys", { exact: true })).toContainText("I");
  await page.locator("body").click({ position: { x: 600, y: 600 } });
  await page.keyboard.down("i");
  await expect.poll(async () => (await lastInput(page))?.desired_lean_rad).toBeCloseTo(deg(10));
  await page.keyboard.up("i");
});

test("a refused input shows why and the ride goes on", async ({ page }) => {
  await open(page);
  await page.evaluate(() => window.freeRideFixture.refuseInput(true));
  await expect(page.getByRole("alert")).toContainText("Fixture refuses the input.");
  await page.evaluate(() => window.freeRideFixture.refuseInput(false));
  await expect(page.getByRole("alert")).toHaveCount(0);
  expect((await fixtureOf(page)).pauses).toBe(0);
});

test("the panel keeps only the play controls", async ({ page }) => {
  await open(page);
  for (const gone of [
    "Test pad 32 m",
    "Rider model for next run",
    "Precise input",
    "Input settings",
    "Ride details",
    "World seed",
  ])
    await expect(page.getByText(gone, { exact: true })).toHaveCount(0);
  for (const kept of ["Respawn", "At the start", "New world", "Horn", "Keys"])
    await expect(page.getByRole("button", { name: new RegExp(`^${kept}`) }).first()).toBeVisible();
});
