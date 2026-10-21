import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, searchForWorkspaceRoot } from "vite";
import { realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";
import pkg from "./package.json" with { type: "json" };

const host = process.env.TAURI_DEV_HOST;

// https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
    // Worktrees can share dependencies. Allow the bundled font directory through its real path.
    fs: {
      allow: [
        searchForWorkspaceRoot(fileURLToPath(new URL(".", import.meta.url))),
        realpathSync(
          fileURLToPath(new URL("./node_modules/@fontsource/jetbrains-mono", import.meta.url)),
        ),
      ],
    },
  },
  preview: {
    port: 4173,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    // Tauri ships WebKit on macOS/Linux and WebView2 on Windows.
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome111" : "safari16",
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    chunkSizeWarningLimit: 1600,
  },
});
