// Runs the Tauri CLI (`pnpm tauri ...`). Our own Cortex-M3 core runs the firmware in every build.
// `EUC_UNICORN=1 pnpm tauri dev` also builds in the `unicorn` feature: the GPL-2.0 CPU backend
// kept as a development oracle (`EUC_CPU=unicorn` then selects it). It is for development only:
// src-tauri refuses to compile it into any bundle (`tauri build`), so a release can never carry it.

import { spawn } from "node:child_process";

const args = process.argv.slice(2);
const hasFeatures = args.some(
    (a) => a === "--features" || a === "-f" || a.startsWith("--features="),
);
if (args[0] === "dev" && process.env.EUC_UNICORN === "1" && !hasFeatures) {
    args.push("--features", "unicorn");
}

const child = spawn("tauri", args, { stdio: "inherit", shell: process.platform === "win32" });
child.on("exit", (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
