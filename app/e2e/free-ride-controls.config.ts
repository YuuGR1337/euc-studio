import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  testMatch: "free-ride-controls.spec.ts",
  outputDir: "../test-results/free-ride-controls/artifacts",
  workers: 1,
  timeout: 30_000,
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
