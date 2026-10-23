// Stills of every scene and a contact sheet (out/stills/), to check the cut before a full render.
// node scripts/stills.mjs  (from video/, after footage.mjs)

import { execFileSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { bundle } from "@remotion/bundler";
import { renderStill, selectComposition } from "@remotion/renderer";

const root = join(import.meta.dirname, "..");
const out = join(root, "out", "stills");
mkdirSync(out, { recursive: true });

const serveUrl = await bundle({ entryPoint: join(root, "src", "index.ts") });
const composition = await selectComposition({ serveUrl, id: "Showcase" });
// Eight moments over the cut: the title, two of each shot, the close.
const picks = [0.05, 0.16, 0.24, 0.37, 0.53, 0.62, 0.74, 0.94].map((at) => [
    `${Math.round(at * 100)}pct`,
    Math.round(at * (composition.durationInFrames - 1)),
]);
const files = [];
for (const [name, frame] of picks) {
    const output = join(out, `${String(frame).padStart(4, "0")}-${name}.png`);
    await renderStill({ composition, serveUrl, output, frame });
    files.push(output);
    console.log(`${name}: frame ${frame}`);
}
const poster = await selectComposition({ serveUrl, id: "Poster" });
await renderStill({ composition: poster, serveUrl, output: join(out, "poster.png") });
files.push(join(out, "poster.png"));
execFileSync("ffmpeg", [
    ...["-v", "error", "-y"],
    ...files.flatMap((f) => ["-i", f]),
    ...[
        "-filter_complex",
        `${files.map((_, i) => `[${i}]scale=640:-1[s${i}]`).join(";")};${files.map((_, i) => `[s${i}]`).join("")}xstack=inputs=${files.length}:layout=${layout(files.length)}[o]`,
    ],
    ...["-map", "[o]", join(out, "contact-sheet.png")],
]);
console.log(`contact sheet: ${join(out, "contact-sheet.png")}`);

/** A 3-wide grid of 640x360 tiles. */
function layout(n) {
    return Array.from({ length: n }, (_, i) => `${(i % 3) * 640}_${Math.floor(i / 3) * 360}`).join(
        "|",
    );
}
