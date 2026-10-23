// The README files from the Remotion master (out/master.mp4, out/poster.png), into
// docs/assets/video/:
// - showcase.mp4: H.264 in limited (TV) range, the runs' sound at -17 LUFS (two-pass loudnorm,
//   true peak -1.5 dBTP), AAC, faststart;
// - showcase-loop.webp: silent animated WebP, 960 px, every second frame, looping (GitHub plays
//   it inline; it does not play a repository MP4 inline);
// - poster.png: 1280 px.
//
// node scripts/finish.mjs  (from video/, after `remotion render` and `remotion still`)

import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, readdirSync, rmSync, statSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const master = join(root, "out", "master.mp4");
const dest = join(root, "..", "docs", "assets", "video");
mkdirSync(dest, { recursive: true });
const run = (cmd, args) =>
    execFileSync(cmd, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
const ffmpeg = (args) => run("ffmpeg", ["-hide_banner", "-v", "error", "-y", ...args]);
/** What an ffmpeg analysis filter prints (on stderr). */
const analyse = (input, filter) =>
    spawnSync("ffmpeg", ["-hide_banner", "-i", input, "-af", filter, "-f", "null", "-"], {
        encoding: "utf8",
    }).stderr;

// Loudness, first pass: what the mix measures.
const TARGET = "I=-17:TP=-1.5:LRA=11";
const report = analyse(master, `loudnorm=${TARGET}:print_format=json`);
const m = JSON.parse(report.slice(report.lastIndexOf("{"), report.lastIndexOf("}") + 1));
const loudnorm =
    `loudnorm=${TARGET}:measured_I=${m.input_i}:measured_TP=${m.input_tp}:measured_LRA=${m.input_lra}` +
    `:measured_thresh=${m.input_thresh}:offset=${m.target_offset}:linear=true`;

const mp4 = join(dest, "showcase.mp4");
ffmpeg([
    ...["-i", master],
    ...["-vf", "scale=in_range=full:out_range=limited,format=yuv420p", "-color_range", "tv"],
    ...["-c:v", "libx264", "-preset", "slow", "-crf", "24", "-maxrate", "4M", "-bufsize", "8M"],
    ...["-af", `${loudnorm},aresample=48000`, "-c:a", "aac", "-b:a", "128k"],
    ...["-movflags", "+faststart", mp4],
]);

const frames = join(root, "out", "loop-frames");
rmSync(frames, { recursive: true, force: true });
mkdirSync(frames, { recursive: true });
ffmpeg([
    "-i",
    master,
    "-vf",
    "select='not(mod(n\\,2))',scale=960:-2:flags=lanczos",
    "-fps_mode",
    "vfr",
    join(frames, "%04d.png"),
]);
const list = readdirSync(frames)
    .filter((f) => f.endsWith(".png"))
    .sort();
const fps = Number(
    run("ffprobe", [
        "-v",
        "error",
        "-select_streams",
        "v",
        "-show_entries",
        "stream=r_frame_rate",
        "-of",
        "default=nw=1:nk=1",
        master,
    ])
        .trim()
        .split("/")
        .reduce((a, b) => a / b),
);
const loop = join(dest, "showcase-loop.webp");
run("img2webp", [
    "-loop",
    "0",
    "-lossy",
    "-q",
    "52",
    "-m",
    "6",
    "-d",
    String(Math.round((2 * 1000) / fps)),
    ...list.map((f) => join(frames, f)),
    "-o",
    loop,
]);

const poster = join(dest, "poster.png");
ffmpeg(["-i", join(root, "out", "poster.png"), "-vf", "scale=1280:-2:flags=lanczos", poster]);

// What came out: sizes, and the sound's loudness as a player hears it.
const check = analyse(mp4, "ebur128=peak=true");
const summary = check.slice(check.lastIndexOf("Summary:"));
const mb = (f) => `${(statSync(f).size / 1e6).toFixed(1)} MB`;
console.log(
    `showcase.mp4 ${mb(mp4)}, showcase-loop.webp ${mb(loop)} (${list.length} frames), poster.png ${mb(poster)}`,
);
console.log(summary.replace(/\s+/g, " ").trim());
