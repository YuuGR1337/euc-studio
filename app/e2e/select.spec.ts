import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";
import { createServer, type ViteDevServer } from "vite";

// Mount the production Select and its real CSS. No native engine or firmware runs here.
const fixture = `<!doctype html>
<html lang="en"><head><meta charset="UTF-8"><title>Select menu test</title></head>
<body style="margin:0"><div id="root"></div><script type="module">
import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { Select } from "/src/ui/Select.tsx";
import "/src/design/global.css";
const options = Array.from({ length: 80 }, (_, index) => ({
  value: "item-" + (index + 1),
  label: "Scenario " + String(index + 1).padStart(2, "0"),
  detail: "Scenario details " + (index + 1),
}));
function Fixture() {
  const [value, setValue] = useState(new URLSearchParams(location.search).get("selected") || "item-1");
  return React.createElement("main", { style: { margin: 16, width: 280 } },
    React.createElement(Select, {
      value, onValueChange: setValue, options,
      "aria-label": "Scenario test", className: "w-full",
    }),
    React.createElement("output", { "aria-label": "Selected scenario" }, value),
  );
}
createRoot(document.getElementById("root")).render(React.createElement(Fixture));
</script></body></html>`;

let server: ViteDevServer;
let url: string;

test.beforeAll(async () => {
  server = await createServer({
    configFile: fileURLToPath(new URL("../vite.config.ts", import.meta.url)),
    root: fileURLToPath(new URL("..", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../test-results/select-vite-cache", import.meta.url)),
    server: { host: "127.0.0.1", port: 0, strictPort: false, hmr: false, open: false },
    plugins: [
      {
        name: "select-browser-fixture",
        configureServer(vite) {
          vite.middlewares.use((request, response, next) => {
            if (request.url?.split("?")[0] !== "/__select-menu.html") return next();
            void vite
              .transformIndexHtml(request.url, fixture)
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
  if (!address || typeof address === "string") throw new Error("No Select test server port");
  url = `http://127.0.0.1:${address.port}/__select-menu.html`;
});

test.afterAll(async () => {
  await server?.close();
});

for (const height of [320, 720]) {
  test(`long Select stays bounded and supports wheel and keyboard at ${height}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 480, height });
    await page.goto(url);
    const trigger = page.getByRole("combobox", { name: "Scenario test" });
    const triggerWidth = (await trigger.boundingBox())?.width ?? Infinity;
    await trigger.click();
    const menu = page.getByRole("listbox");
    await expect(menu).toBeVisible();
    await expect
      .poll(async () =>
        menu.evaluate((element) => {
          const box = element.getBoundingClientRect();
          const available = Number.parseFloat(
            getComputedStyle(element).getPropertyValue("--radix-select-content-available-height"),
          );
          return (
            box.top >= 7 &&
            box.bottom <= innerHeight - 7 &&
            box.height <= Math.min(360, available) + 1
          );
        }),
      )
      .toBe(true);
    // The existing pop animation scales the menu during opening.
    await expect
      .poll(async () => (await menu.boundingBox())?.width ?? 0)
      .toBeGreaterThanOrEqual(triggerWidth - 1);

    const viewport = menu.locator("[data-radix-select-viewport]");
    expect(await viewport.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(
      true,
    );
    await viewport.hover();
    await page.mouse.wheel(0, 10000);
    const last = menu.getByRole("option", { name: /^Scenario 80/ });
    await expect(last).toBeInViewport({ ratio: 1 });
    await last.click();
    await expect(page.getByRole("status", { name: "Selected scenario" })).toHaveText("item-80");
    await expect(trigger).toBeFocused();

    await trigger.press("ArrowDown");
    await expect(last).toBeFocused();
    await expect(last).toBeInViewport({ ratio: 1 });
    await page.keyboard.press("Home");
    const first = menu.getByRole("option", { name: /^Scenario 01/ });
    await expect(first).toBeFocused();
    await expect(first).toBeInViewport({ ratio: 1 });
    await page.keyboard.press("End");
    await expect(last).toBeFocused();
    await expect(last).toBeInViewport({ ratio: 1 });
    await page.keyboard.press("Enter");
    await expect(menu).toBeHidden();
    await expect(trigger).toBeFocused();
  });
}

test("keyboard opening scrolls the selected option into view on a small screen", async ({
  page,
}) => {
  await page.setViewportSize({ width: 480, height: 320 });
  await page.goto(`${url}?selected=item-60`);
  await page.getByRole("combobox", { name: "Scenario test" }).press("ArrowDown");
  const selected = page.getByRole("option", { name: /^Scenario 60/ });
  await expect(selected).toHaveAttribute("aria-selected", "true");
  await expect(selected).toBeFocused();
  await expect(selected).toBeInViewport({ ratio: 1 });
  await page.keyboard.press("Escape");
  await expect(page.getByRole("combobox", { name: "Scenario test" })).toBeFocused();
});
