// Renders src-tauri/app-icon.svg to src-tauri/app-icon.png (1024 x 1024, transparent corners)
// with headless Chromium. Then `pnpm tauri icon src-tauri/app-icon.png` makes every size.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const svgPath = fileURLToPath(new URL("../src-tauri/app-icon.svg", import.meta.url));
const pngPath = fileURLToPath(new URL("../src-tauri/app-icon.png", import.meta.url));

const browser = await chromium.launch();
const page = await browser.newPage({
  viewport: { width: 1024, height: 1024 },
  deviceScaleFactor: 1,
});
await page.setContent(
  `<html><body style="margin:0;background:transparent">${readFileSync(svgPath, "utf8")}</body></html>`,
);
await page.locator("svg").screenshot({ path: pngPath, omitBackground: true });
await browser.close();
console.log(`wrote ${pngPath}`);
