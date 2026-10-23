import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { createServer, type ViteDevServer } from "vite";

// Each run mode keeps its own camera: Ride (scenarios) starts on Side, Free Ride on Chase, and C
// cycles in both. The whole studio runs with the live engine; only its transport is the
// controlled fixture of the live pose checks (fixtures/live-wire.ts).
const root = fileURLToPath(new URL("..", import.meta.url));
const fixture = fileURLToPath(new URL("./fixtures/live-wire.ts", import.meta.url));
const IN_TAURI =
  'const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;';

let server: ViteDevServer;
let base: string;
test.beforeAll(async () => {
  server = await createServer({
    root,
    configFile: `${root}/vite.config.ts`,
    cacheDir: `${root}/node_modules/.vite-camera-modes`,
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
});
test.afterAll(async () => {
  await server?.close();
});

function runMode(page: Page, run_mode: "scenario" | "free_ride", run_epoch: number) {
  return page.evaluate((m) => window.liveWire.messages?.(m), {
    type: "status",
    run_mode,
    run_epoch,
    buttons_ready: false,
    power_button_ready: false,
    state: "paused",
    message: "Paused",
    label: "Stock",
    build: "Fixture",
    backend: "fixture",
    scenario: "ride-20kmh-stop",
    firmware: "stock",
    t_s: 0,
    rtf: 0,
    target_rtf: 1,
    playing: false,
  });
}
const camera = (page: Page, name: string) => page.getByRole("radio", { name, exact: true });

test("Ride starts on Side, Free Ride on Chase; C cycles each and each is kept", async ({
  page,
}) => {
  await page.goto(base);
  await expect.poll(() => page.evaluate(() => !!window.liveWire.messages)).toBe(true);
  await runMode(page, "scenario", 1);
  await expect(camera(page, "Side")).toHaveAttribute("aria-checked", "true");
  await page.locator("body").press("c");
  await expect(camera(page, "Orbit")).toHaveAttribute("aria-checked", "true");

  await runMode(page, "free_ride", 2);
  await expect(camera(page, "Chase")).toHaveAttribute("aria-checked", "true");
  await page.locator("body").press("c");
  await expect(camera(page, "Orbit")).toHaveAttribute("aria-checked", "true");
  await page.locator("body").press("c");
  await expect(camera(page, "Side")).toHaveAttribute("aria-checked", "true");

  await runMode(page, "scenario", 3);
  await expect(camera(page, "Orbit")).toHaveAttribute("aria-checked", "true");
  await page.reload();
  await expect.poll(() => page.evaluate(() => !!window.liveWire.messages)).toBe(true);
  await runMode(page, "free_ride", 4);
  await expect(camera(page, "Side")).toHaveAttribute("aria-checked", "true");
});
