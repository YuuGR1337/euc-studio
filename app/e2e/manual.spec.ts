import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { type Page, expect, test } from "@playwright/test";

// Riding by hand in the Ride workspace with the demo engine: the keys take the rider, Space steps
// off only while riding by hand (it pauses the run otherwise), Esc and M give the rider back, and
// the other shortcuts keep working.

const here = dirname(fileURLToPath(import.meta.url));

async function shot(page: Page, name: string, project: string) {
  const dir = join(here, "..", "test-results", "screens", project);
  mkdirSync(dir, { recursive: true });
  await page.screenshot({ path: join(dir, `${name}.png`), animations: "disabled", caret: "hide" });
}

test.beforeEach(async ({ page, browserName }) => {
  if (process.env.PW_SLOW && browserName === "chromium") {
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Emulation.setCPUThrottlingRate", { rate: Number(process.env.PW_SLOW) || 4 });
  }
  await page.addInitScript(() => {
    if (!sessionStorage.getItem("e2e-cleared")) {
      localStorage.clear();
      sessionStorage.setItem("e2e-cleared", "1");
    }
  });
});

async function openRide(page: Page) {
  await page.goto("/?preroll=24&scene=off#/ride");
  await expect(page.getByTestId("viewport")).toHaveAttribute("data-scene", "off");
  // Focus the page itself, as after a click on the 3D view.
  await page.getByTestId("viewport").click({ position: { x: 400, y: 300 } });
}

const events = (page: Page) =>
  page.getByRole("heading", { name: "Events" }).locator("xpath=ancestor::section[1]");

test("keyboard riding: take, lean and turn, step off, give back", async ({ page }, info) => {
  await openRide(page);
  const hud = page.getByTestId("manual-hud");
  await expect(hud).toHaveCount(0);

  // Holding W and A takes the rider from the scenario and leans forward into a left turn.
  await page.keyboard.down("w");
  await page.keyboard.down("a");
  await expect(hud).toBeVisible();
  await expect(hud).toContainText("Keyboard");
  await expect(hud).toContainText(/fwd \d+ %/);
  await expect(hud).toContainText(/left \d+ %/);
  await expect(events(page).getByText("Manual control")).toBeVisible();
  await page.waitForTimeout(600);
  await shot(page, "ride-manual", info.project.name);
  await page.keyboard.up("w");
  await page.keyboard.up("a");
  await expect(hud).toContainText("Lean0 %");

  // Space steps off while riding by hand, and the run keeps going.
  await page.keyboard.press("Space");
  await expect(hud).toContainText("Stepped off");
  await expect(page.getByText("running", { exact: true })).toBeVisible();
  await expect(events(page).getByText("Rider steps off")).toBeVisible();

  // Other shortcuts still work: C switches the camera.
  await page.keyboard.press("c");
  await expect(page.getByRole("radio", { name: "Orbit" })).toHaveAttribute("aria-checked", "true");

  // Esc gives the rider back; Space pauses the run again.
  await page.keyboard.press("Escape");
  await expect(hud).toHaveCount(0);
  await expect(events(page).getByText("Control back to the scenario")).toBeVisible();
  await page.keyboard.press("Space");
  await expect(page.getByText("paused", { exact: true })).toBeVisible();
});

test("M toggles riding by hand; arrows on a focused control stay with it", async ({ page }) => {
  await openRide(page);
  const hud = page.getByTestId("manual-hud");
  await page.keyboard.press("m");
  await expect(hud).toBeVisible();
  await expect(hud).toContainText("Lean0 %");
  await page.keyboard.press("ArrowDown");
  await expect(hud).toBeVisible();
  await page.keyboard.press("m");
  await expect(hud).toHaveCount(0);

  // An arrow on the focused time-window control moves its selection, not the rider.
  await page.getByRole("radio", { name: "10 s" }).click();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("radio", { name: "30 s" })).toBeFocused();
  await expect(page.getByRole("radio", { name: "30 s" })).toHaveAttribute("aria-checked", "true");
  // The selection follows the focus both ways, wraps around, and Home and End jump to the ends.
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("radio", { name: "5 s" })).toHaveAttribute("aria-checked", "true");
  await page.keyboard.press("End");
  await expect(page.getByRole("radio", { name: "30 s" })).toHaveAttribute("aria-checked", "true");
  await page.keyboard.press("ArrowLeft");
  await expect(page.getByRole("radio", { name: "10 s" })).toHaveAttribute("aria-checked", "true");
  await expect(page.getByRole("radio", { name: "10 s" })).toBeFocused();
  await expect(hud).toHaveCount(0);

  // The palette lists it.
  await page.keyboard.press("ControlOrMeta+k");
  await page.getByPlaceholder("Type a command or search").fill("manual control");
  await page.keyboard.press("Enter");
  await expect(hud).toBeVisible();
});

test("opening Recordings releases manual control and its keys stay in the dialog", async ({
  page,
}) => {
  await openRide(page);
  await page.keyboard.press("w");
  await expect(page.getByTestId("manual-hud")).toBeVisible();
  await page.getByRole("button", { name: "Recordings", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "Recordings" })).toBeVisible();
  await expect(page.getByTestId("manual-hud")).toHaveCount(0);
  await page.keyboard.press("w");
  await expect(page.getByTestId("manual-hud")).toHaveCount(0);
});

test("a gamepad's left stick takes the rider and leans and turns", async ({ page }) => {
  // A stand-in for the Gamepad API: one pad whose stick the test moves.
  await page.addInitScript(() => {
    const pad = { connected: true, axes: [0, 0, 0, 0], buttons: [], id: "test pad", index: 0 };
    Object.assign(window, { __pad: pad });
    Object.defineProperty(navigator, "getGamepads", { value: () => [pad] });
  });
  await openRide(page);
  const hud = page.getByTestId("manual-hud");
  await expect(hud).toHaveCount(0);
  // Up and a little left: full forward lean, a small left turn.
  await page.evaluate(() => {
    (window as unknown as { __pad: { axes: number[] } }).__pad.axes = [-0.4, -1, 0, 0];
  });
  await expect(hud).toBeVisible();
  await expect(hud).toContainText("Gamepad");
  await expect(hud).toContainText("fwd 93 %");
  await expect(hud).toContainText(/left \d+ %/);
  // Back to the centre: still riding by hand, upright.
  await page.evaluate(() => {
    (window as unknown as { __pad: { axes: number[] } }).__pad.axes = [0.05, 0.05, 0, 0];
  });
  await expect(hud).toContainText("Lean0 %");
  await page.keyboard.press("Escape");
  await expect(hud).toHaveCount(0);
});
