import { mkdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test, type Locator, type Page, type TestInfo } from "@playwright/test";
import { createServer, type ViteDevServer } from "vite";
import type { EngineState } from "../src/engine/types.ts";

// Production TuneWorkspace, hook, trigger state, scopes, API client, and CSS.
// Only engine identity and the Tauri transport are replaced with controlled fixture replies.
const root = fileURLToPath(new URL("..", import.meta.url));
const fixtureModule = fileURLToPath(new URL("./fixtures/tune-probes.ts", import.meta.url));
const output = fileURLToPath(new URL("../test-results/tune-capture/screens/", import.meta.url));
const html = `<!doctype html><html lang="en"><head><meta charset="UTF-8"><title>Tune capture fixture only</title></head>
<body style="margin:0;height:100vh;overflow:hidden">
<header style="position:fixed;top:12px;left:80px;font-size:12px;color:var(--color-fg-2)">Controlled Tune probe fixture. No native firmware or simulator runs.</header>
<main id="root" style="position:fixed;left:64px;top:56px;width:calc(100vw - 64px);height:calc(100vh - 80px)"></main>
<script type="module">
import React from "react";
import { createRoot } from "react-dom/client";
import { TooltipProvider } from "/src/ui/Tooltip.tsx";
import { TuneWorkspace } from "/src/workspaces/tune/TuneWorkspace.tsx";
import "/src/design/global.css";
createRoot(document.getElementById("root")).render(React.createElement(TooltipProvider, null, React.createElement(TuneWorkspace, { active: true })));
</script></body></html>`;
let server: ViteDevServer;
let url: string;

test.beforeAll(async () => {
  server = await createServer({
    root,
    configFile: `${root}/vite.config.ts`,
    server: { host: "127.0.0.1", port: 0, hmr: false, open: false },
    plugins: [
      {
        name: "tune-controlled-capture-fixture",
        enforce: "pre",
        resolveId(id, importer) {
          if (!importer) return;
          if (
            id === "../../engine/index.ts" &&
            /\/workspaces\/tune\/(TuneWorkspace|useTune)\.tsx?$/.test(importer)
          )
            return fixtureModule;
          if (
            importer.endsWith("/engine/tune.ts") &&
            (id === "./index.ts" || id === "@tauri-apps/api/core")
          )
            return fixtureModule;
        },
        configureServer(vite) {
          vite.middlewares.use((request, response, next) => {
            if (request.url?.split("?")[0] !== "/__tune-capture.html") return next();
            void vite
              .transformIndexHtml(request.url, html)
              .then((text) => {
                response.setHeader("Content-Type", "text/html");
                response.end(text);
              })
              .catch(next);
          });
        },
      },
    ],
  });
  await server.listen();
  const address = server.httpServer?.address();
  if (!address || typeof address === "string") throw new Error("No Tune fixture port");
  url = `http://127.0.0.1:${address.port}/__tune-capture.html`;
  mkdirSync(output, { recursive: true });
});
test.afterAll(async () => {
  await server?.close();
});

const trigger = (page: Page) => page.getByRole("region", { name: "Scope trigger" });
const scope = (page: Page, name = "fixture_signal") =>
  page.getByRole("heading", { name, exact: true }).locator("..").locator("..");
const plot = (page: Page, name = "fixture_signal") => scope(page, name).getByRole("img");
async function reads(page: Page, count = 2) {
  const before = await page.evaluate(() => window.tuneProbeFixture.reads);
  await expect
    .poll(() => page.evaluate(() => window.tuneProbeFixture.reads))
    .toBeGreaterThanOrEqual(before + count);
}
async function push(page: Page, t: number, value: number, state: EngineState = "running") {
  await page.evaluate(({ t, value, state }) => window.tuneProbeFixture.push(t, value, state), {
    t,
    value,
    state,
  });
  await expect(page.getByLabel("Simulation time", { exact: true })).toHaveText(`${t.toFixed(3)} s`);
}
async function select(page: Page, name: string, option: string) {
  await page.getByRole("combobox", { name, exact: true }).click();
  await page.getByRole("option", { name: option, exact: true }).click();
}
async function open(page: Page, second = false) {
  await page.addInitScript(() => localStorage.clear());
  await page.goto(url);
  await page.getByRole("checkbox", { name: "Scope fixture_signal", exact: true }).check();
  if (second) await page.getByRole("checkbox", { name: "Scope fixture_aux", exact: true }).check();
  await page.getByRole("spinbutton", { name: "Raw threshold", exact: true }).fill("10");
  await select(page, "Trigger mode", "Rising");
  await expect(
    page.getByRole("combobox", { name: "Capture time after trigger", exact: true }),
  ).toHaveText("5 s");
  await reads(page);
  await expect(trigger(page).getByText("armed", { exact: true })).toBeVisible();
}
async function phase(page: Page, name: "armed" | "acquiring" | "held") {
  await expect(trigger(page).getByText(name, { exact: true })).toBeVisible();
}
async function points(graph: Locator) {
  return graph
    .locator('path[stroke="currentColor"]')
    .evaluate((path) => (path.getAttribute("d")?.match(/[ML]/g) ?? []).length);
}
async function shot(page: Page, info: TestInfo, name: string) {
  const path = `${output}/${name}.png`;
  await page.screenshot({ path, animations: "disabled" });
  await info.attach("Controlled probe fixture only", { path, contentType: "image/png" });
}

test("After trigger defaults to five seconds and offers only supported windows", async ({
  page,
}) => {
  await open(page);
  const after = page.getByRole("combobox", { name: "Capture time after trigger", exact: true });
  await after.click();
  await expect(page.getByRole("option")).toHaveText(["0 s", "1 s", "5 s", "10 s", "15 s"]);
  await page.getByRole("option", { name: "0 s", exact: true }).click();
  await reads(page);
  await push(page, 1, 11);
  await phase(page, "held");
  await expect(scope(page).getByText("1.000 s", { exact: true })).toBeVisible();
});

test("rising crossing acquires five simulation seconds then freezes selected scopes", async ({
  page,
}, info) => {
  await open(page, true);
  await push(page, 1, 11);
  await phase(page, "acquiring");
  await expect(trigger(page).getByText("Crossing at 1.000 s", { exact: true })).toBeVisible();
  await push(page, 3, 13);
  await phase(page, "acquiring");
  expect(await points(plot(page))).toBeGreaterThan(2);
  await shot(page, info, "acquiring-1280x720");
  await push(page, 5.9, 15);
  await phase(page, "acquiring");
  await push(page, 6, 16);
  await phase(page, "held");
  const paths = await Promise.all([
    plot(page).locator('path[stroke="currentColor"]').getAttribute("d"),
    plot(page, "fixture_aux").locator('path[stroke="currentColor"]').getAttribute("d"),
  ]);
  await push(page, 7, 99);
  await phase(page, "held");
  await expect(scope(page).locator(".text-20")).toHaveText("16");
  await expect(scope(page, "fixture_aux").locator(".text-20")).toHaveText("32");
  for (const [index, name] of ["fixture_signal", "fixture_aux"].entries()) {
    await expect(scope(page, name).getByText("6.000 s", { exact: true })).toBeVisible();
    await expect(plot(page, name).locator('path[stroke="currentColor"]')).toHaveAttribute(
      "d",
      paths[index] ?? "",
    );
  }
  await expect(trigger(page).getByText(/Incomplete capture/)).toHaveCount(0);
  await shot(page, info, "complete-1280x720");
});

test("paused duplicate times do not fill an acquiring trace", async ({ page }, info) => {
  await open(page);
  await push(page, 1, 11);
  await phase(page, "acquiring");
  const count = await points(plot(page));
  const path = await plot(page).locator('path[stroke="currentColor"]').getAttribute("d");
  await push(page, 1, 11, "paused");
  await reads(page, 4);
  await phase(page, "acquiring");
  expect(await points(plot(page))).toBe(count);
  await expect(plot(page).locator('path[stroke="currentColor"]')).toHaveAttribute("d", path ?? "");
  await expect(scope(page).getByText("1.000 s", { exact: true })).toBeVisible();
  await shot(page, info, "paused-duplicates-1280x720");
  await push(page, 2, 12);
  await phase(page, "acquiring");
  expect(await points(plot(page))).toBe(count + 1);
  await push(page, 6, 16);
  await phase(page, "held");
});

test("an ended run visibly marks the partial acquisition incomplete", async ({ page }, info) => {
  await open(page);
  await push(page, 1, 11);
  await phase(page, "acquiring");
  await push(page, 2, 12, "completed");
  await phase(page, "held");
  await expect(trigger(page).getByRole("status")).toHaveText(
    "Incomplete capture. The run ended before the requested capture time.",
  );
  await expect(scope(page).getByText("2.000 s", { exact: true })).toBeVisible();
  await push(page, 3, 99, "completed");
  await expect(scope(page).locator(".text-20")).toHaveText("12");
  await expect(scope(page).getByText("2.000 s", { exact: true })).toBeVisible();
  await shot(page, info, "incomplete-run-1280x720");
});

test("manual Hold freezes scopes and Rearm waits for a fresh crossing", async ({ page }, info) => {
  await open(page);
  await push(page, 1, 4);
  await trigger(page).getByRole("button", { name: "Hold", exact: true }).click();
  await phase(page, "held");
  await expect(trigger(page).getByText("Hold at 1.000 s", { exact: true })).toBeVisible();
  await push(page, 2, 12);
  await expect(scope(page).locator(".text-20")).toHaveText("4");
  await trigger(page).getByRole("button", { name: "Rearm", exact: true }).click();
  await phase(page, "armed");
  await reads(page);
  await phase(page, "armed");
  await push(page, 3, 4);
  await phase(page, "armed");
  await push(page, 4, 12);
  await phase(page, "acquiring");
  await expect(trigger(page).getByText("Crossing at 4.000 s", { exact: true })).toBeVisible();
  await trigger(page).getByRole("button", { name: "Hold", exact: true }).click();
  await phase(page, "held");
  await push(page, 5, 99);
  await expect(scope(page).locator(".text-20")).toHaveText("12");
  await expect(scope(page).getByText("4.000 s", { exact: true })).toBeVisible();
  await shot(page, info, "manual-hold-1280x720");
});

test("capture controls and two selected scopes fit at 1280x720", async ({ page }, info) => {
  await open(page, true);
  await select(page, "Capture time after trigger", "15 s");
  await reads(page);
  await push(page, 1, 11);
  await phase(page, "acquiring");
  await page.evaluate(() => document.fonts.ready);
  const bounds = await trigger(page).evaluate((element) => {
    const root = document.querySelector("main");
    if (!root) throw new Error("Missing Tune fixture root");
    const rect = (e: Element) => {
      const r = e.getBoundingClientRect();
      return { x: r.x, y: r.y, width: r.width, height: r.height, right: r.right, bottom: r.bottom };
    };
    return {
      root: rect(root),
      controls: rect(element),
      plots: [...root.querySelectorAll('svg[role="img"]')].map(rect),
      overflow: document.documentElement.scrollWidth > innerWidth,
      children: [...element.querySelectorAll('button, input, [role="combobox"]')].map((e) => ({
        name: e.getAttribute("aria-label") ?? e.textContent,
        ...rect(e),
      })),
    };
  });
  writeFileSync(`${output}/bounds-1280x720.json`, JSON.stringify(bounds, null, 2));
  expect(bounds.overflow).toBe(false);
  expect(bounds.root.height).toBe(640);
  expect(bounds.root.bottom).toBe(696);
  expect(bounds.controls.right).toBeLessThanOrEqual(1280);
  expect(bounds.controls.bottom).toBeLessThan(bounds.root.bottom);
  for (const child of bounds.children) {
    expect(child.x).toBeGreaterThanOrEqual(bounds.controls.x);
    expect(child.right).toBeLessThanOrEqual(bounds.controls.right);
    expect(child.bottom).toBeLessThanOrEqual(bounds.controls.bottom);
  }
  for (const graph of bounds.plots) {
    expect(graph.right).toBeLessThanOrEqual(bounds.root.right);
    expect(graph.bottom).toBeLessThanOrEqual(bounds.root.bottom);
  }
  for (const name of ["fixture_signal", "fixture_aux"])
    await expect(plot(page, name)).toBeVisible();
  await shot(page, info, "layout-1280x720");
});

test("sample-limited acquisition visibly reports an incomplete capture", async ({ page }, info) => {
  // Virtual wall time drives the unchanged 250 ms production poll. Probe times stay explicit.
  await page.clock.install();
  await open(page);
  await page.clock.pauseAt(await page.evaluate(() => Date.now() + 1000));
  await page.evaluate(() => window.tuneProbeFixture.push(1, 11));
  await page.clock.runFor(250);
  await phase(page, "acquiring");
  const initial = await points(plot(page));
  const additional = 240 - initial;
  for (let index = 1; index <= additional; index++) {
    await page.evaluate((t) => window.tuneProbeFixture.push(t, 12), 1 + index / 1000);
    await page.clock.runFor(250);
  }
  await phase(page, "held");
  expect(await points(plot(page))).toBe(240);
  await expect(trigger(page).getByRole("status")).toHaveText(
    "Incomplete capture. The 240-sample limit was reached.",
  );
  await expect(
    scope(page).getByText(`${(1 + additional / 1000).toFixed(3)} s`, { exact: true }),
  ).toBeVisible();
  await shot(page, info, "incomplete-sample-limit-1280x720");
});

async function pendingReply(page: Page, kind: "catalog" | "snapshot", session: number, t?: number) {
  await expect
    .poll(() => page.evaluate(() => window.tuneProbeFixture.pending))
    .toContainEqual(
      t === undefined ? expect.objectContaining({ kind, session }) : { kind, session, t },
    );
}

test("Rearm discards a deferred crossing and uses the first fresh reply as its baseline", async ({
  page,
}, info) => {
  await open(page);
  await page.evaluate(() => {
    window.tuneProbeFixture.deferNext("snapshot");
    window.tuneProbeFixture.push(1, 11);
  });
  await pendingReply(page, "snapshot", 42, 1);
  await trigger(page).getByRole("button", { name: "Rearm", exact: true }).click();
  await phase(page, "armed");
  await page.evaluate(() => {
    window.tuneProbeFixture.deferNext("snapshot");
    window.tuneProbeFixture.push(2, 12);
    window.tuneProbeFixture.resolveNext();
  });
  await pendingReply(page, "snapshot", 42, 2);
  await phase(page, "armed");
  await expect(page.getByLabel("Simulation time", { exact: true })).toHaveText("0.000 s");
  await expect(trigger(page).getByText(/Crossing at/)).toHaveCount(0);
  await page.evaluate(() => window.tuneProbeFixture.resolveNext());
  await expect(page.getByLabel("Simulation time", { exact: true })).toHaveText("2.000 s");
  await phase(page, "armed");
  await push(page, 3, 4);
  await phase(page, "armed");
  await push(page, 4, 12);
  await phase(page, "acquiring");
  await expect(trigger(page).getByText("Crossing at 4.000 s", { exact: true })).toBeVisible();
  await shot(page, info, "deferred-rearm-1280x720");
});

test("an epoch change masks the old held capture while an old snapshot remains pending", async ({
  page,
}, info) => {
  await open(page);
  await select(page, "Capture time after trigger", "0 s");
  await reads(page);
  await push(page, 1, 11);
  await phase(page, "held");
  await page.evaluate(() => {
    window.tuneProbeFixture.deferNext("snapshot");
    window.tuneProbeFixture.push(2, 99);
  });
  await pendingReply(page, "snapshot", 42, 2);
  await page.evaluate(() => {
    window.tuneProbeFixture.deferNext("catalog");
    window.tuneProbeFixture.reset(43);
  });
  await expect(page.getByRole("heading", { name: "fixture_signal", exact: true })).toHaveCount(0);
  await expect(page.getByLabel("Simulation time", { exact: true })).toHaveCount(0);
  await expect(trigger(page).getByText(/Crossing at/)).toHaveCount(0);
  await page.evaluate(() => window.tuneProbeFixture.resolveNext());
  await pendingReply(page, "catalog", 43);
  await expect(page.getByRole("heading", { name: "fixture_signal", exact: true })).toHaveCount(0);
  await page.evaluate(() => window.tuneProbeFixture.resolveNext());
  await expect(page.getByLabel("Simulation time", { exact: true })).toHaveText("0.000 s");
  await expect(scope(page).locator(".text-20")).toHaveText("0");
  await phase(page, "armed");
  await expect(plot(page).locator('path[aria-label^="Trigger at"]')).toHaveCount(0);
  await shot(page, info, "epoch-reset-1280x720");
});

test("a quick reset that ends between polls cannot retain the previous capture", async ({
  page,
}, info) => {
  await open(page);
  await select(page, "Capture time after trigger", "0 s");
  await reads(page);
  await push(page, 1, 11);
  await phase(page, "held");
  const reset = await page.evaluate(() => {
    window.tuneProbeFixture.deferNext("catalog");
    return window.tuneProbeFixture.quickReset(43);
  });
  expect(reset.beforeReads).toBe(reset.afterReads);
  expect(reset.epoch).toBe(43);
  expect(reset.state).toBe("completed");
  await pendingReply(page, "catalog", 43);
  await expect(page.getByRole("heading", { name: "fixture_signal", exact: true })).toHaveCount(0);
  await expect(page.getByLabel("Simulation time", { exact: true })).toHaveCount(0);
  await expect(trigger(page).getByText(/Crossing at/)).toHaveCount(0);
  await page.evaluate(() => window.tuneProbeFixture.resolveNext());
  await expect(page.getByLabel("Simulation time", { exact: true })).toHaveText("0.000 s");
  await expect(scope(page).locator(".text-20")).toHaveText("0");
  await phase(page, "armed");
  await expect(plot(page).locator('path[aria-label^="Trigger at"]')).toHaveCount(0);
  await shot(page, info, "quick-completed-reset-1280x720");
});

test("a delayed rejection after Hold cannot clear the current held capture", async ({
  page,
}, info) => {
  await open(page);
  await push(page, 1, 4);
  await page.evaluate(() => {
    window.tuneProbeFixture.deferNext("snapshot");
    window.tuneProbeFixture.push(2, 99);
  });
  await pendingReply(page, "snapshot", 42, 2);
  await trigger(page).getByRole("button", { name: "Hold", exact: true }).click();
  await phase(page, "held");
  await expect(trigger(page).getByText("Hold at 1.000 s", { exact: true })).toBeVisible();
  await page.evaluate(() => {
    window.tuneProbeFixture.deferNext("snapshot");
    window.tuneProbeFixture.push(3, 77);
    window.tuneProbeFixture.rejectNext();
  });
  await pendingReply(page, "snapshot", 42, 3);
  await phase(page, "held");
  await expect(scope(page).locator(".text-20")).toHaveText("4");
  await expect(
    page.getByRole("heading", { name: "Firmware probes are unavailable", exact: true }),
  ).toHaveCount(0);
  await page.evaluate(() => window.tuneProbeFixture.resolveNext());
  await expect(page.getByLabel("Simulation time", { exact: true })).toHaveText("3.000 s");
  await phase(page, "held");
  await expect(scope(page).locator(".text-20")).toHaveText("4");
  await expect(scope(page).getByText("1.000 s", { exact: true })).toBeVisible();
  await shot(page, info, "delayed-error-held-1280x720");
});
