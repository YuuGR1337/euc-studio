import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  testMatch: "tune-capture.spec.ts",
  outputDir: "../test-results/tune-capture/artifacts",
  workers: 1,
  timeout: 60_000,
  reporter: "list",
  use: {
    browserName: "chromium",
    viewport: { width: 1280, height: 720 },
    deviceScaleFactor: 1,
    colorScheme: "dark",
    trace: "retain-on-failure",
  },
});
