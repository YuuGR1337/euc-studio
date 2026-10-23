// Captures the showcase's footage: the studio itself, in headless WebKit, replaying the real
// runs the showcase tool exported (app/src-tauri/src/engine/showcase.rs). Ride shots open a
// recording in the studio's own playback; the Compare shot replays a comparison's stream into
// the Compare workspace. Time is Playwright's fake clock, stepped 32 ms per frame (two animation
// frames), so each PNG is the studio exactly 32 ms of simulated time after the last: 31.25 fps.
//
// node capture.mjs <app dir> <runs dir> <frames dir> <shots.json> [capture id]

import { createRequire } from "node:module";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [appDir, runsDir, framesDir, shotsFile, only] = process.argv.slice(2);
const require = createRequire(join(appDir, "package.json"));
const { chromium, webkit } = require("@playwright/test");
const { createServer } = require("vite");

const shots = JSON.parse(readFileSync(shotsFile, "utf8"));
const STEP_MS = 32;
const transport = join(import.meta.dirname, "tauri-core.ts");
const IN_TAURI =
    'const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;';
const DESKTOP = 'return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;';
// The modules whose Tauri calls the transport answers; the rest of the app stays a browser build.
const SWAPPED = [
    "/engine/live.ts",
    "/engine/compareLive.ts",
    "/engine/recording.ts",
    "/engine/world.ts",
];

const server = await createServer({
    root: appDir,
    configFile: join(appDir, "vite.config.ts"),
    cacheDir: join(framesDir, ".vite"),
    logLevel: "warn",
    server: {
        host: "127.0.0.1",
        port: 0,
        hmr: false,
        open: false,
        fs: { allow: [appDir, import.meta.dirname] },
    },
    plugins: [
        {
            name: "showcase-transport",
            enforce: "pre",
            resolveId(id, importer) {
                if (id === "@tauri-apps/api/core" && SWAPPED.some((f) => importer?.endsWith(f)))
                    return transport;
            },
            transform(code, id) {
                // Compare talks to the native comparison (here: its recorded stream); recordings open
                // as on the desktop.
                if (id.endsWith("/src/engine/compare.ts")) {
                    if (!code.includes(IN_TAURI))
                        throw new Error("compare.ts changed its Tauri check");
                    return code.replace(IN_TAURI, "const inTauri = true;");
                }
                if (id.endsWith("/src/engine/recording.ts")) {
                    if (!code.includes(DESKTOP))
                        throw new Error("recording.ts changed its desktop check");
                    return code.replace(DESKTOP, "return true;");
                }
            },
        },
    ],
});
await server.listen();
const base = `http://127.0.0.1:${server.httpServer.address().port}`;

// WebKit, as the desktop app's web view (and its SF Pro Rounded); SHOWCASE_BROWSER=chromium
// draws with Chromium's new headless mode on Metal instead.
const browser =
    process.env.SHOWCASE_BROWSER === "chromium"
        ? await chromium.launch({
              channel: "chromium",
              args: ["--use-angle=metal", "--enable-gpu", "--ignore-gpu-blocklist"],
          })
        : await webkit.launch();

async function openPage(capture, init) {
    const page = await browser.newPage({
        viewport: { width: capture.window?.[0] ?? 1440, height: capture.window?.[1] ?? 810 },
        deviceScaleFactor: capture.scale ?? 4 / 3,
        colorScheme: "dark",
    });
    page.on("pageerror", (e) => console.log(`[${capture.id}] page error`, String(e).slice(0, 300)));
    await page.route("**/__showcase/**", (route) => {
        const name = decodeURIComponent(new URL(route.request().url()).pathname.split("/").pop());
        route.fulfill({ body: readFileSync(join(runsDir, name)) });
    });
    if (init) await page.addInitScript(init.script, init.arg);
    await page.clock.install({ time: 0 });
    return page;
}

/** The world stores every run exported: the view asks for chunks, the transport answers. */
async function stageWorlds(page) {
    const ids = shots.runs.map((r) => r.id);
    await page.evaluate(async (ids) => {
        for (const id of ids) {
            const json = await fetch(`/__showcase/${id}.world.json`).then((r) =>
                r.ok ? r.json() : null,
            );
            if (!json) continue;
            const bin = await fetch(`/__showcase/${id}.world.bin`).then((r) => r.arrayBuffer());
            window.showcase.worlds.push({ ...json, bytes: new Uint8Array(bin) });
        }
    }, ids);
}

async function ready(page) {
    await page.waitForFunction(
        () =>
            document.querySelector('[data-testid="viewport"]')?.getAttribute("data-scene") ===
            "ready",
        null,
        { timeout: 60_000 },
    );
}

/** Steps the paused clock by `frames` frames; with `shoot`, a PNG per frame. */
async function frames(page, capture, count, shoot) {
    const dir = join(framesDir, capture.id);
    const clip = capture.crop ? await page.getByTestId(capture.crop).boundingBox() : undefined;
    for (let i = 0; i < count; i++) {
        await page.clock.runFor(STEP_MS);
        // Worker replies (world chunks) and decoded textures land between steps.
        await page.waitForTimeout(12);
        if (shoot && i === 0) {
            const t0 = await page.evaluate(() => window.capturePlayback.latest.t);
            writeFileSync(join(dir, "meta.json"), JSON.stringify({ t0, fps: 1000 / STEP_MS }));
        }
        if (shoot)
            await page.screenshot({
                path: join(dir, `${String(i).padStart(4, "0")}.png`),
                animations: "disabled",
                caret: "hide",
                clip,
            });
    }
}

async function playback(capture) {
    const page = await openPage(capture);
    await page.goto(`${base}/?quality=high#/ride`);
    await ready(page);
    await stageWorlds(page);
    await page.evaluate(async (run) => {
        window.showcase.recording = await fetch(`/__showcase/${run}.json`).then((r) => r.json());
        const bytes = await fetch(`/__showcase/${run}.eucrec`).then((r) => r.arrayBuffer());
        const { openRecording } = await import("/src/engine/recording.ts");
        const { activatePlayback } = await import("/src/engine/index.ts");
        const { PlaybackEngine } = await import("/src/engine/playback.ts");
        const { useUi } = await import("/src/state/ui.ts");
        // As the Recordings panel opens a file (Shell.tsx).
        const playback = new PlaybackEngine(
            await openRecording(new File([bytes], `${run}.eucrec`)),
        );
        activatePlayback(playback);
        useUi.getState().navigate("ride");
        window.capturePlayback = playback;
    }, capture.run);
    await page.getByRole("radio", { name: capture.camera }).click();
    if (capture.events)
        await page
            .getByRole("tablist", { name: "Filter events" })
            .getByRole("tab", { name: new RegExp(`^${capture.events}`) })
            .click();
    const preroll = capture.preroll_s ?? 1.5;
    await page.evaluate((t) => window.capturePlayback.seek(t), capture.from_s - preroll);
    // Let the view load the world around the start in real time, then stop the clock.
    await page.waitForTimeout(2500);
    await page.clock.pauseAt((await page.evaluate(() => Date.now())) + 1000);
    await page.getByRole("button", { name: "Run simulation" }).click();
    await frames(page, capture, Math.round((preroll * 1000) / STEP_MS), false);
    await frames(page, capture, Math.round((capture.seconds * 1000) / STEP_MS), true);
    const misses = await page.evaluate(() => window.showcase.misses);
    const t = await page.evaluate(() => window.capturePlayback.latest.t);
    await page.close();
    return { misses, t };
}

async function comparison(capture) {
    const recorded = JSON.parse(
        readFileSync(join(runsDir, `${capture.compare}.compare.json`), "utf8"),
    );
    const page = await openPage(capture, {
        script: (info) => {
            window.showcaseInit = { compareInfo: info };
        },
        arg: recorded.info,
    });
    await page.goto(`${base}/?quality=high#/compare`);
    await page.waitForFunction(
        () =>
            document
                .querySelector('[data-testid="compare-viewport"]')
                ?.getAttribute("data-scene") === "ready",
        null,
        { timeout: 60_000 },
    );
    await stageWorlds(page);
    const spec = recorded.spec;
    const choose = async (name, label) => {
        await page.getByRole("combobox", { name }).click();
        await page.getByRole("option", { name: label, exact: true }).click();
    };
    const label = (id) => recorded.info.firmwares.find((f) => f.id === id).label;
    const scenario = recorded.info.scenarios.find((s) => s.id === spec.scenario).label;
    await choose("Compare scenario", scenario);
    await choose("Firmware A", label(spec.a.kind));
    await choose("Firmware B", label(spec.b.kind));
    await page.clock.pauseAt((await page.evaluate(() => Date.now())) + 1000);
    await page.getByRole("button", { name: "Run", exact: true }).click();
    await page.waitForFunction(() => !!window.showcase.compare);
    // The stream as the pump sent it: everything up to `t` (a batch's last sample time; a message
    // goes with the batch before it).
    let next = 0;
    let last = 0;
    const deliver = async (t) => {
        const due = [];
        while (next < recorded.items.length) {
            const item = recorded.items[next];
            if (item.t_s !== undefined && item.t_s !== null && item.t_s > t) break;
            if (item.t_s != null) last = item.t_s;
            due.push(item);
            next++;
        }
        if (due.length)
            await page.evaluate((items) => {
                for (const item of items) {
                    if (item.message) window.showcase.compare.messages(item.message);
                    else window.showcase.compare[item.side](new Uint8Array(item.bytes).buffer);
                }
            }, due);
    };
    const preroll = capture.preroll_s ?? 1.5;
    await deliver(capture.from_s - preroll);
    await page.waitForTimeout(2500);
    const dir = join(framesDir, capture.id);
    const total = Math.round(((capture.seconds + preroll) * 1000) / STEP_MS);
    const skip = Math.round((preroll * 1000) / STEP_MS);
    const clip = capture.crop ? await page.getByTestId(capture.crop).boundingBox() : undefined;
    for (let i = 0; i < total; i++) {
        await deliver(capture.from_s - preroll + ((i + 1) * STEP_MS) / 1000);
        await page.clock.runFor(STEP_MS);
        await page.waitForTimeout(12);
        if (i === skip)
            writeFileSync(
                join(dir, "meta.json"),
                JSON.stringify({
                    t0: capture.from_s - preroll + ((i + 1) * STEP_MS) / 1000,
                    fps: 1000 / STEP_MS,
                }),
            );
        if (i >= skip)
            await page.screenshot({
                path: join(dir, `${String(i - skip).padStart(4, "0")}.png`),
                animations: "disabled",
                caret: "hide",
                clip,
            });
    }
    const misses = await page.evaluate(() => window.showcase.misses);
    await page.close();
    return { misses, t: last };
}

for (const capture of shots.captures) {
    if (only && capture.id !== only) continue;
    const dir = join(framesDir, capture.id);
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    const started = Date.now();
    const result = capture.compare ? await comparison(capture) : await playback(capture);
    console.log(
        `${capture.id}: ${Math.round((capture.seconds * 1000) / STEP_MS)} frames, ended at t=${result.t.toFixed(2)} s, ` +
            `${((Date.now() - started) / 1000).toFixed(0)} s, misses ${JSON.stringify(result.misses)}`,
    );
    if (result.misses.length) process.exitCode = 1;
}
await browser.close();
await server.close();
