import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

// Only export public geometry. No firmware, cache, host, hardware, or user data is read.
const root = fileURLToPath(new URL("../../../", import.meta.url));
const sources = [
    "profiles/begode-extreme/display.toml",
    "profiles/begode-extreme/lights.toml",
    "app/src-tauri/src/engine/display.rs",
    "app/src-tauri/src/engine/lights.rs",
];
const scratch = await mkdtemp(join(tmpdir(), "euc-layout-export-"));
try {
    await mkdir(join(scratch, "src"));
    await writeFile(
        join(scratch, "Cargo.toml"),
        `[package]
name = "wheel-output-layout-export"
version = "0.0.0"
edition = "2024"
[dependencies]
euc-display = { path = ${JSON.stringify(join(root, "crates/euc-display"))} }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
`,
    );
    await writeFile(
        join(scratch, "src/main.rs"),
        `
#[path = ${JSON.stringify(join(root, sources[2]))}]
mod display;
#[path = ${JSON.stringify(join(root, sources[3]))}]
mod lights;
fn main() {
    let root = std::env::args().nth(1).expect("repository root");
    let root = std::path::Path::new(&root);
    let display = display::DisplayLayout::parse(&std::fs::read_to_string(root.join("${sources[0]}")).unwrap()).unwrap();
    let lights = lights::LightsLayout::parse(&std::fs::read_to_string(root.join("${sources[1]}")).unwrap()).unwrap();
    println!("{}", serde_json::json!({ "display": display, "lights": lights }));
}
`,
    );
    const { stdout } = await promisify(execFile)(
        "cargo",
        ["run", "--offline", "--quiet", "--manifest-path", join(scratch, "Cargo.toml"), "--", root],
        {
            env: {
                ...process.env,
                CARGO_TARGET_DIR: join(scratch, "target"),
                CARGO_BUILD_JOBS: "2",
            },
            maxBuffer: 1024 * 1024,
        },
    );
    const sourceHashes = Object.fromEntries(
        await Promise.all(
            sources.map(async (path) => [
                path,
                createHash("sha256")
                    .update(await readFile(join(root, path)))
                    .digest("hex"),
            ]),
        ),
    );
    const fixture = {
        provenance:
            "Layout only. Public profile geometry, exported through the native DisplayLayout::parse and LightsLayout::parse adapters. No firmware capture or private data.",
        source_hashes: sourceHashes,
        ...JSON.parse(stdout),
    };
    await writeFile(
        new URL("./wheel-output-layouts.json", import.meta.url),
        JSON.stringify(fixture, null, 2) + "\n",
    );
} finally {
    await rm(scratch, { recursive: true, force: true });
}
