import { defineConfig } from "@playwright/test";

// Isolated layout server. No desktop app, native engine, firmware, GPIO, or audio runs here.
export default defineConfig({
  testDir: ".",
  testMatch: "wheel-outputs.spec.ts",
  outputDir: "../test-results/wheel-outputs/artifacts",
  workers: 1,
  timeout: 60_000,
  reporter: "list",
  use: {
    deviceScaleFactor: 1,
    colorScheme: "dark",
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: {
        browserName: "chromium",
        launchOptions: { args: ["--enable-unsafe-swiftshader"] },
      },
    },
    ...(process.env.PW_WEBKIT ? [{ name: "webkit", use: { browserName: "webkit" as const } }] : []),
  ],
});
