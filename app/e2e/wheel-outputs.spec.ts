import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { createServer, type ViteDevServer } from "vite";

// Production UI and CSS, actual exported profile geometry, explicit synthetic output state.
// Import replacement applies only to these three card modules in this isolated test server.
const root = fileURLToPath(new URL("..", import.meta.url));
const layouts = JSON.parse(
  readFileSync(new URL("./fixtures/wheel-output-layouts.json", import.meta.url), "utf8"),
) as { source_hashes: Record<string, string> };
const fixtureModule = fileURLToPath(new URL("./fixtures/wheel-output-engine.ts", import.meta.url));
const screenshots = fileURLToPath(
  new URL("../test-results/wheel-outputs/screens/", import.meta.url),
);
let server: ViteDevServer;
let url: string;

test.beforeAll(async () => {
  for (const [path, hash] of Object.entries(layouts.source_hashes)) {
    expect(
      createHash("sha256")
        .update(readFileSync(`${root}/../${path}`))
        .digest("hex"),
      `Refresh the profile layout fixture after changing ${path}`,
    ).toBe(hash);
  }
  server = await createServer({
    configFile: `${root}/vite.config.ts`,
    root,
    server: { host: "127.0.0.1", port: 0, hmr: false, open: false },
    plugins: [
      {
        name: "wheel-output-layout-fixture",
        enforce: "pre",
        resolveId(id, importer) {
          if (
            id === "../../engine/index.ts" &&
            importer &&
            /\/(WheelOutputs|WheelButtons|useBuzzerSound)\.tsx?$/.test(importer)
          )
            return fixtureModule;
        },
      },
    ],
  });
  await server.listen();
  const address = server.httpServer?.address();
  if (!address || typeof address === "string") throw new Error("Missing fixture server port");
  url = `http://127.0.0.1:${address.port}`;
  mkdirSync(screenshots, { recursive: true });
});
test.afterAll(async () => {
  const bounds = readdirSync(screenshots)
    .filter((file) => file.endsWith("-bounds.json"))
    .sort()
    .map((file) => JSON.parse(readFileSync(`${screenshots}/${file}`, "utf8")) as unknown);
  writeFileSync(`${screenshots}/bounds.json`, JSON.stringify(bounds, null, 2));
  await server?.close();
});

async function open(page: Page, width: number, height: number) {
  await page.setViewportSize({ width, height });
  await page.addInitScript(() => localStorage.clear());
  await page.goto(`${url}/?scene=off#/ride`);
  await expect(page.getByTestId("display-replica")).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(500);
  await page.evaluate(() => {
    const note = document.createElement("p");
    note.textContent =
      "Layout fixture · Profile geometry · All segments on · No native GPIO or audio";
    Object.assign(note.style, {
      position: "fixed",
      top: "260px",
      left: "80px",
      color: "var(--color-fg-2)",
      fontSize: "11px",
      maxWidth: "200px",
      zIndex: "10",
    });
    document.body.append(note);
  });
}

for (const [width, height] of [
  [1440, 900],
  [1280, 720],
  [1024, 720],
  [960, 640],
  [1440, 720],
] as const) {
  test(`Display layout at ${width}×${height}`, async ({ page }, info) => {
    await open(page, width, height);
    const card = page.getByTestId("wheel-outputs");
    const bell = card.getByRole("button", { name: "Buzzer sound", exact: true });
    await expect(bell).toHaveText("");
    await expect(bell).toHaveAttribute("aria-pressed", "true");
    await expect(card.getByRole("button", { name: "Details", exact: true })).toHaveCount(0);
    await expect(card.getByRole("alert")).toHaveCount(0);
    await expect(card).not.toContainText(/Sent|Restart|runtime|Buttons need|Web Audio/);
    const result = await card.evaluate((element) => {
      const rect = (e: Element) => {
        const r = e.getBoundingClientRect();
        return {
          x: r.x,
          y: r.y,
          width: r.width,
          height: r.height,
          right: r.right,
          bottom: r.bottom,
        };
      };
      const hero = element.querySelector("canvas");
      const front = element.querySelector("svg[data-state]");
      const rear = element.querySelector('[data-testid="tail-light"]');
      const viewport = document.querySelector('[data-testid="viewport"]');
      if (!hero || !front || !rear || !viewport) throw new Error("Missing layout fixture output");
      const pitchLabel = [...viewport.querySelectorAll(".label")].find(
        (e) => e.textContent === "Pitch",
      );
      const pitch = pitchLabel?.parentElement?.parentElement;
      const ride = viewport.querySelector('[data-testid="hud-ride"]');
      if (!pitch || !ride) throw new Error("Missing surrounding HUD");
      const figures = [...element.querySelectorAll("figure")];
      const clipping = [
        ...element.querySelectorAll("button, canvas, figure, figure svg, figcaption"),
      ]
        .filter((child) => {
          const a = child.getBoundingClientRect(),
            b = element.getBoundingClientRect();
          return (
            a.left < b.left - 1 ||
            a.right > b.right + 1 ||
            a.top < b.top - 1 ||
            a.bottom > b.bottom + 1 ||
            child.scrollWidth > child.clientWidth + 1
          );
        })
        .map((e) => e.tagName + ":" + e.getAttribute("aria-label"));
      return {
        card: rect(element),
        viewport: rect(viewport),
        viewportScrollLeft: viewport.scrollLeft,
        pitch: rect(pitch),
        ride: rect(ride),
        hero: rect(hero),
        front: rect(front),
        rear: rect(rear),
        rows: figures.map((f) => {
          const row = f.children[1];
          if (!row) throw new Error("Missing light row");
          return rect(row);
        }),
        circles: [...front.querySelectorAll("circle")].map((c) => ({
          cx: c.getAttribute("cx"),
          cy: c.getAttribute("cy"),
          r: c.getAttribute("r"),
        })),
        clipping,
        text: element.textContent,
      };
    });
    console.log(JSON.stringify({ width, height, ...result }));
    await page.screenshot({
      path: `${screenshots}/${width}x${height}.png`,
      animations: "disabled",
    });
    await info.attach("layout", {
      path: `${screenshots}/${width}x${height}.png`,
      contentType: "image/png",
    });
    if (width === 1440 && height === 900) {
      const file = `${screenshots}/display-card.png`;
      await card.screenshot({ path: file, animations: "disabled" });
      await info.attach("Display card, layout fixture only", {
        path: file,
        contentType: "image/png",
      });
    }
    for (const control of [
      bell,
      card.getByRole("button", { name: "Power button" }),
      card.getByRole("button", { name: "Handle button" }),
      card.locator("figure").nth(0),
      card.locator("figure").nth(1),
    ]) {
      await control.hover();
      await page.waitForTimeout(550);
      await expect(page.getByRole("tooltip")).toHaveCount(0);
    }
    await page.mouse.move(60, 55);
    const afterHover = await page
      .getByTestId("viewport")
      .evaluate((e) => ({ scrollLeft: e.scrollLeft }));
    console.log(JSON.stringify({ width, height, afterHover }));
    writeFileSync(
      `${screenshots}/${width}x${height}-bounds.json`,
      JSON.stringify({ width, height, ...result, afterHover }, null, 2),
    );
    expect(result.circles).toHaveLength(4);
    expect(new Set(result.circles.map((c) => c.cy)).size).toBe(1);
    expect(new Set(result.circles.map((c) => c.cx)).size).toBe(4);
    expect(result.hero.height).toBe(
      result.viewport.width <= 900 || result.viewport.height <= 560 ? 64 : 104,
    );
    expect(result.rows[0]?.height).toBe(result.rows[1]?.height);
    expect(result.rows[0]?.y).toBe(result.rows[1]?.y);
    expect(result.clipping).toEqual([]);
    expect(result.card.right).toBeLessThanOrEqual(result.viewport.right);
    expect(result.card.bottom).toBeLessThanOrEqual(result.viewport.bottom);
    expect(result.card.y).toBeGreaterThanOrEqual(result.pitch.bottom);
    expect(afterHover.scrollLeft).toBe(0);
    if (result.viewport.width <= 760) {
      expect(result.pitch.height).toBeLessThan(40);
      expect(result.card.bottom).toBeLessThanOrEqual(result.ride.y);
      expect(result.ride.right).toBeLessThanOrEqual(result.viewport.right);
    }
  });
}

test("boot boundary disables controls without an error footer or hover help", async ({ page }) => {
  await open(page, 1280, 720);
  await page.evaluate(() => window.wheelOutputFixture.setReady(false));
  const card = page.getByTestId("wheel-outputs");
  for (const name of ["Power button", "Handle button"]) {
    const control = card.getByRole("button", { name });
    await expect(control).toHaveAttribute("aria-disabled", "true");
    // Bypass Playwright's disabled-control wait to check the component's own input guard.
    await control.click({ force: true });
    await control.press("Enter");
    await control.evaluate((element: HTMLButtonElement) => element.click());
    await control.hover();
    await page.waitForTimeout(550);
    await expect(page.getByRole("tooltip")).toHaveCount(0);
  }
  expect(await page.evaluate(() => window.wheelOutputFixture.calls)).toEqual([]);
  await expect(card.getByRole("alert")).toHaveCount(0);
  await page.mouse.move(60, 55);
  await page.screenshot({ path: `${screenshots}/1280x720-not-ready.png`, animations: "disabled" });
});

test("front preview distinguishes off and unknown fixture states", async ({ page }) => {
  await open(page, 1280, 720);
  for (const [duty, state] of [
    [0, "off"],
    [null, "unknown"],
  ] as const) {
    await page.evaluate((value) => window.wheelOutputFixture.setDuty(value), duty);
    const front = page.getByTestId("wheel-outputs").locator("svg[data-state]");
    await expect(front).toHaveAttribute("data-state", state);
    expect(
      await front
        .locator("circle")
        .evaluateAll((elements) => elements.every((e) => e.getAttribute("fill-opacity") === "0")),
    ).toBe(true);
  }
});

test("powered-off fixture darkens outputs and removes the rear indicator", async ({
  page,
}, info) => {
  await open(page, 1280, 720);
  const card = page.getByTestId("wheel-outputs");
  const display = card.getByTestId("display-replica");
  const rear = card.getByTestId("tail-light");
  const lightLevel = () =>
    display.evaluate((element: HTMLCanvasElement) => {
      const ctx = element.getContext("2d");
      if (!ctx) throw new Error("Missing display canvas context");
      const pixels = ctx.getImageData(0, 0, element.width, element.height).data;
      let sum = 0;
      for (let i = 0; i < pixels.length; i += 4)
        sum += (pixels[i] ?? 0) + (pixels[i + 1] ?? 0) + (pixels[i + 2] ?? 0);
      return sum / ((pixels.length / 4) * 3);
    });
  const litLevel = await lightLevel();
  expect(await rear.locator("circle").count()).toBeGreaterThan(0);
  await page.evaluate(() => window.wheelOutputFixture.powerOff());
  await expect(display).toHaveAttribute("aria-label", /, dark$/);
  await expect.poll(lightLevel).toBeLessThan(litLevel / 2);
  const front = card.locator("svg[data-state]");
  await expect(front).toHaveAttribute("data-state", "off");
  await expect(card.locator("figure").nth(0).getByText("Off", { exact: true })).toBeVisible();
  await expect(card.getByText("0/24 lit", { exact: true })).toBeVisible();
  // Every rear circle is either a driven LED hot spot or a BLE indicator. All disappear off.
  await expect(rear.locator("circle")).toHaveCount(0);
  expect(
    await front
      .locator("circle")
      .evaluateAll((circles) => circles.every((c) => c.getAttribute("fill-opacity") === "0")),
  ).toBe(true);
  const file = `${screenshots}/1280x720-powered-off.png`;
  await page.screenshot({ path: file, animations: "disabled" });
  await info.attach("powered-off layout fixture", { path: file, contentType: "image/png" });
  console.log(
    JSON.stringify({
      poweredOffFixture: true,
      litCanvasMean: litLevel,
      darkCanvasMean: await lightLevel(),
    }),
  );
});

test("release errors block new presses until Retry release receives an acknowledgement", async ({
  page,
}, info) => {
  await open(page, 1280, 720);
  const card = page.getByTestId("wheel-outputs");
  const power = card.getByRole("button", { name: "Power button", exact: true });
  await expect(card.getByRole("button", { name: "Retry release", exact: true })).toHaveCount(0);
  await page.evaluate(() => window.wheelOutputFixture.failNextReleases(3));
  await power.hover();
  await page.mouse.down();
  await expect(power).toHaveAttribute("aria-pressed", "true");
  await expect
    .poll(() =>
      page.evaluate(() => window.wheelOutputFixture.calls.filter((c) => c.pressed).length),
    )
    .toBeGreaterThan(0);
  // Let the acknowledged down settle before release, so this path has exactly three up attempts.
  await page.waitForTimeout(30);
  await page.mouse.up();
  await expect
    .poll(() =>
      page.evaluate(() => window.wheelOutputFixture.calls.filter((c) => !c.pressed).length),
    )
    .toBe(3);
  await page.waitForTimeout(150);
  const downCount = await page.evaluate(
    () => window.wheelOutputFixture.calls.filter((c) => c.pressed).length,
  );
  await expect(power).toHaveAttribute("aria-disabled", "true");
  const retry = card.getByRole("button", { name: "Retry release", exact: true });
  await expect(retry).toBeVisible();
  const image = `${screenshots}/1280x720-release-error.png`;
  await page.screenshot({ path: image, animations: "disabled" });
  await info.attach("release error layout fixture", { path: image, contentType: "image/png" });
  // Both pointer and click-only input must respect the persistent release interlock.
  await power.click({ force: true });
  await power.evaluate((element: HTMLButtonElement) => element.click());
  expect(
    await page.evaluate(() => window.wheelOutputFixture.calls.filter((c) => c.pressed).length),
  ).toBe(downCount);
  expect(
    await page.evaluate(() => window.wheelOutputFixture.calls.filter((c) => !c.pressed).length),
  ).toBe(3);
  await retry.click();
  await expect(retry).toHaveCount(0);
  await expect(power).not.toHaveAttribute("aria-disabled", "true");
  expect(
    await page.evaluate(() => window.wheelOutputFixture.calls.filter((c) => !c.pressed).length),
  ).toBe(4);
  await power.hover();
  await page.mouse.down();
  await expect(power).toHaveAttribute("aria-pressed", "true");
  expect(
    await page.evaluate(() => window.wheelOutputFixture.calls.filter((c) => c.pressed).length),
  ).toBeGreaterThan(downCount);
  await page.mouse.up();
  await expect(power).toHaveAttribute("aria-pressed", "false");
  await expect(card.getByRole("button", { name: "Retry release", exact: true })).toHaveCount(0);
  console.log(
    JSON.stringify({
      releaseInterlockFixture: await page.evaluate(() => window.wheelOutputFixture.calls),
    }),
  );
});
