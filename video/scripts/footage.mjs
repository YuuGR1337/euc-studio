// Turns the captured frames into the clips Remotion cuts (public/footage/<id>.mp4, near
// lossless), cuts each clip's stretch of its run's sound (public/runs/<id>.wav: the firmware's
// beeps, at the simulator's level) and writes src/footage.gen.json: the frame rate, each clip's
// length and its first frame's run time.
//
// node scripts/footage.mjs  (from video/, after capture)

import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const shots = JSON.parse(readFileSync(join(root, "shots.json"), "utf8"));
const runs = join(root, "out", "runs");
const frames = join(root, "out", "frames");
rmSync(join(root, "public", "runs"), { recursive: true, force: true });
mkdirSync(join(root, "public", "footage"), { recursive: true });
mkdirSync(join(root, "public", "runs"), { recursive: true });

const out = { fps: 0, shots: {} };
for (const capture of shots.captures) {
    const dir = join(frames, capture.id);
    const meta = JSON.parse(readFileSync(join(dir, "meta.json"), "utf8"));
    const count = readdirSync(dir).filter((f) => f.endsWith(".png")).length;
    if (out.fps && out.fps !== meta.fps)
        throw new Error(`${capture.id}: ${meta.fps} fps, not ${out.fps}`);
    out.fps = meta.fps;
    const video = `footage/${capture.id}.mp4`;
    execFileSync("ffmpeg", [
        ...["-v", "error", "-y", "-framerate", String(meta.fps), "-i", join(dir, "%04d.png")],
        // A cropped 3D view fills the frame (lanczos up to 1080 rows, the sides trimmed).
        ...[
            "-vf",
            "scale=1920:1080:force_original_aspect_ratio=increase:flags=lanczos,crop=1920:1080",
        ],
        ...["-c:v", "libx264", "-preset", "slow", "-crf", "10", "-pix_fmt", "yuv420p"],
        join(root, "public", video),
    ]);
    // A comparison has no sound of its own (the studio plays none); it takes run A's, recorded
    // on its own as `sound`.
    const run = capture.sound ?? capture.run;
    const data = JSON.parse(readFileSync(join(runs, `${run}.json`), "utf8"));
    // The clip's stretch of the run's sound; the WAV starts at the run's first sample.
    const audio = `runs/${capture.id}.wav`;
    execFileSync("ffmpeg", [
        ...["-v", "error", "-y", "-ss", String(meta.t0 - data.times[0])],
        ...["-t", String(count / meta.fps), "-i", join(runs, `${run}.wav`)],
        join(root, "public", audio),
    ]);
    out.shots[capture.id] = { video, frames: count, t0: meta.t0, audio, audioT0: meta.t0 };
    console.log(`${capture.id}: ${count} frames from t=${meta.t0.toFixed(3)} s, sound of ${run}`);
}
writeFileSync(join(root, "src", "footage.gen.json"), `${JSON.stringify(out, null, 2)}\n`);
