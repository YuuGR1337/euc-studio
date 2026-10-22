import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { type Page, expect, test } from "@playwright/test";

// The Compare workspace with the demo engine: stock against ours-clean on the demo's 20 km/h ride
// (ours-clean starts its speed tiltback at 18 km/h there, so the runs part ways), and a run from
// the setup bar. Screenshots land next to the others in test-results/screens/<project>/.

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

test("compare workspace: setup, ghost view, scopes, summary and quirks", async ({ page }, info) => {
  // The whole 26 s demo ride, compared before the page draws.
  await page.goto("/?compare=ride-20,30#/compare");
  await expect(page.getByTestId("compare-viewport")).toHaveAttribute("data-scene", "ready", {
    timeout: 60_000,
  });
  await expect(page.getByRole("combobox", { name: "Firmware A" })).toContainText("Stock");
  await expect(page.getByRole("combobox", { name: "Firmware B" })).toContainText("Ours-clean");
  await expect(page.getByRole("button", { name: "Run", exact: true })).toBeVisible();

  // Both runs, one lane per channel, A solid and B dashed.
  for (const lane of ["speed", "pitch", "pwm", "current", "voltage"]) {
    await expect(page.getByTestId(`compare-lane-${lane}`)).toBeVisible();
  }
  const runs = page.getByTestId("compare-runs");
  await expect(runs).toContainText("Stock");
  await expect(runs).toContainText("Ours-clean");
  await expect(page.getByTestId("compare-gap")).toContainText("behind");

  // The summary: where they part, and the channel over its limit.
  await expect(page.getByTestId("first-divergence")).toContainText("First divergence");
  await expect(page.getByTestId("first-divergence")).toContainText(/\d+\.\d{3} s/);
  await expect(page.getByText("Exceeds the tolerance").first()).toBeVisible();
  const table = page.getByRole("table", { name: "Differences per channel" });
  const speed = table.getByRole("row").filter({ hasText: "speed_kmh" });
  await expect(speed).toContainText("≤ 0.5 km/h");
  await expect(speed).toContainText("Over");
  await page.waitForTimeout(600);
  await shot(page, "compare", info.project.name);

  // The cards keep their widths: the HUD does not follow the numbers.
  const width = async () => Math.round((await runs.boundingBox())?.width ?? 0);
  const before = await width();

  // Quirks: read only; stock against clean differ on most of them.
  await page.getByRole("tab", { name: "Quirks" }).click();
  const quirks = page.getByRole("table", { name: "Quirks per firmware" });
  await expect(page.getByText("Read only")).toBeVisible();
  await expect(quirks.getByText("KEIL_FTZ")).toBeVisible();
  // WC_DEAD is kept by clean, so A and B agree on it and the filter hides it.
  await expect(quirks.getByText("WC_DEAD")).toHaveCount(0);
  await page.getByRole("switch").click();
  await expect(quirks.getByText("WC_DEAD")).toBeVisible();
  await page.waitForTimeout(400);
  await shot(page, "compare-quirks", info.project.name);
  expect(await width()).toBe(before);
});

test("a comparison runs from the setup bar", async ({ page }) => {
  await page.goto("/?scene=off#/compare");
  await expect(
    page.getByRole("heading", { name: "Find where two firmwares part ways" }),
  ).toBeVisible();

  // Stock against ours-compat: the demo's compat rides exactly like stock.
  await page.getByRole("combobox", { name: "Firmware B" }).click();
  await page.getByRole("option", { name: /Ours-compat/ }).click();
  await page.getByRole("radio", { name: "Max" }).click();
  // The demo cannot load a file: the option says why.
  await page.getByRole("combobox", { name: "Firmware A" }).click();
  await expect(page.getByRole("option", { name: /File/ })).toHaveAttribute("data-disabled", "");
  await page.keyboard.press("Escape");

  await page.getByRole("button", { name: "Run", exact: true }).click();
  await expect(page.getByRole("button", { name: "Stop" })).toBeVisible();
  await expect(page.getByText("Within tolerance", { exact: true }).first()).toBeVisible({
    timeout: 30_000,
  });
  await expect(page.getByTestId("first-divergence")).toContainText("No divergence");
  await expect(page.getByRole("button", { name: "Run", exact: true })).toBeVisible();
});

test("the run bar steps aside in Compare, and the ride waits while a comparison runs", async ({
  page,
}) => {
  // The status bar names the ride engine's state: "running" or "paused".
  const ride = (state: string) => page.getByText(state, { exact: true });
  await page.goto("/?scene=off#/ride");
  await expect(ride("running")).toBeVisible();
  await page.keyboard.press("ControlOrMeta+3");
  await expect(page.getByTestId("runbar-compare-note")).toHaveText("Comparison controls are below");
  await expect(page.getByRole("radiogroup", { name: "Firmware" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Pause simulation" })).toHaveCount(0);

  // Starting a comparison pauses the ride (1×: it runs for 26 s, long enough to look around).
  await page.getByRole("button", { name: "Run", exact: true }).click();
  await expect(page.getByRole("button", { name: "Stop" })).toBeVisible();
  await expect(ride("paused")).toBeVisible();

  // In Ride the run bar is back, and Space cannot start the ride while the comparison runs.
  await page.keyboard.press("ControlOrMeta+1");
  await expect(page.getByRole("radiogroup", { name: "Firmware" })).toBeVisible();
  await page.getByTestId("viewport").click({ position: { x: 400, y: 300 } });
  await page.keyboard.press("Space");
  await expect(page.getByText("The ride waits for the comparison").first()).toBeVisible();
  await expect(ride("paused")).toBeVisible();

  // Stopped in Compare, the ride plays again once Compare is left, as it was before.
  await page.keyboard.press("ControlOrMeta+3");
  await page.getByRole("button", { name: "Stop" }).click();
  await expect(ride("paused")).toBeVisible();
  await page.keyboard.press("ControlOrMeta+1");
  await expect(ride("running")).toBeVisible();
});
