import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { type Locator, type Page, expect, test } from "@playwright/test";

// Quirk switches for Ours-custom (our firmware as the host SIL library) in Compare and in Tune,
// with the demo engine: the switches, the presets, what the next run takes, the "not applied"
// state, steady widths and the keyboard-only focus ring. Screenshots land next to the others in
// test-results/screens/<project>/.

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

/** The width of a box, rounded to the pixel. */
async function width(locator: Locator): Promise<number> {
  return Math.round((await locator.boundingBox())?.width ?? 0);
}

/** The outline a focused control draws (none, or the 2 px accent ring). */
async function outline(locator: Locator): Promise<string> {
  return locator.evaluate((el) => getComputedStyle(el).outlineStyle);
}

test("compare: Ours-custom quirks switch per side and go with the next run", async ({
  page,
}, info) => {
  await page.goto("/?scene=off#/compare");
  await page.getByRole("combobox", { name: "Firmware B" }).click();
  await page.getByRole("option", { name: /Ours-custom/ }).click();
  await page.getByRole("radio", { name: "Max" }).click();
  await page.getByRole("tab", { name: "Quirks" }).click();

  await expect(page.getByText("Switchable", { exact: true })).toBeVisible();
  const presets = page.getByLabel("Quirk presets for run B");
  await expect(presets).toBeVisible();
  await expect(page.getByLabel("Quirk presets for run A")).toHaveCount(0);

  // The whole list, so a switched row stays in view.
  await page.getByRole("switch", { name: /Only where A and B differ/ }).click();
  const table = page.getByRole("table", { name: "Quirks per firmware" });
  const row = table.locator('[data-quirk="KEIL_FTZ"]');
  const b = row.locator('[data-side="b"]').getByRole("switch");
  // A is the stock image: read only. B starts from compat, which keeps KEIL_FTZ.
  await expect(row.locator('[data-side="a"]').getByRole("switch")).toHaveCount(0);
  await expect(row.locator('[data-side="a"]')).toContainText("stock");
  await expect(b).toHaveAttribute("aria-checked", "true");

  await presets.getByRole("button", { name: "Clean" }).click();
  await expect(b).toHaveAttribute("aria-checked", "false");
  await expect(
    table.locator('[data-quirk="WC_DEAD"] [data-side="b"]').getByRole("switch"),
  ).toHaveAttribute("aria-checked", "true");
  const card = page.getByTestId("compare-runs");
  const before = await width(card);
  await b.click();
  await expect(b).toHaveAttribute("aria-checked", "true");
  // A click leaves no focus ring.
  await expect.poll(() => outline(b)).toBe("none");

  // The next run takes the switches: B is our firmware with clean plus KEIL_FTZ.
  await page.getByRole("button", { name: "Run", exact: true }).click();
  await expect(card).toContainText("ours-sil (clean+KEIL_FTZ)");
  await expect(page.getByText("Run again to apply")).toHaveCount(0);
  await b.click();
  await expect(page.getByText("Run again to apply")).toBeVisible();
  await page.waitForTimeout(400);
  await shot(page, "compare-quirk-switches", info.project.name);
  expect(await width(card)).toBe(before);

  // Keyboard: Space switches the focused quirk back, and the ring shows.
  await b.focus();
  await page.keyboard.press("Tab");
  await page.keyboard.press("Shift+Tab");
  await expect.poll(() => outline(b)).not.toBe("none");
  await page.keyboard.press("Space");
  await expect(b).toHaveAttribute("aria-checked", "true");
  await expect(page.getByText("Run again to apply")).toHaveCount(0);
});

test("tune: the Quirks tab restarts the run with Ours-custom and the switched quirks", async ({
  page,
}, info) => {
  await page.goto("/?scene=off#/tune");
  const firmware = page.getByRole("radiogroup", { name: "Firmware" });
  await expect(firmware.getByRole("radio", { name: "Stock" })).toHaveAttribute(
    "aria-checked",
    "true",
  );
  await page.getByRole("tab", { name: "Quirks" }).click();
  const panel = page.getByTestId("tune-quirks");
  await expect(panel).toContainText("The stock image has every quirk.");

  // Start from clean: one change from the running stock firmware per quirk clean fixes.
  await panel.getByRole("button", { name: "Clean" }).click();
  await expect(panel.getByText(/from the running firmware/)).toBeVisible();
  const asin = panel.locator('[data-quirk="ASIN_ACCEL"]').getByRole("switch");
  await expect(asin).toHaveAttribute("aria-checked", "false");
  await asin.click();
  await expect(asin).toHaveAttribute("aria-checked", "true");
  const box = await width(panel);

  const restart = panel.getByRole("button", { name: "Restart with these quirks" });
  await restart.click();
  await expect(firmware.getByRole("radio", { name: "Ours-custom" })).toHaveAttribute(
    "aria-checked",
    "true",
  );
  await expect(panel).toContainText("Ours-custom runs our firmware on the host");
  await expect(panel).toContainText("clean+ASIN_ACCEL");
  // What runs now cannot restart again; a change can.
  await expect(restart).toHaveAttribute("aria-disabled", "true");
  await panel.locator('[data-quirk="KEIL_FTZ"]').getByRole("switch").click();
  await expect(panel.getByText("1 not applied")).toBeVisible();
  await expect(restart).not.toHaveAttribute("aria-disabled", "true");
  expect(await width(panel)).toBe(box);
  await page.waitForTimeout(400);
  await shot(page, "tune-quirks", info.project.name);

  // The run bar's Ours-custom runs the last quirks again after another firmware.
  await restart.click();
  await firmware.getByRole("radio", { name: "Stock" }).click();
  await expect(panel).toContainText("The stock image has every quirk.");
  await firmware.getByRole("radio", { name: "Ours-custom" }).click();
  await expect(panel).toContainText("clean+ASIN_ACCEL+KEIL_FTZ");
});
