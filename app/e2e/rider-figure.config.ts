import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  testMatch: "rider-figure.spec.ts",
  outputDir: "../test-results/rider-figure/artifacts",
  workers: 1,
  timeout: 90_000,
  reporter: "list",
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "webkit", use: { browserName: "webkit" } },
  ],
  use: {
    viewport: { width: 1280, height: 900 },
    colorScheme: "dark",
    trace: "retain-on-failure",
  },
});
