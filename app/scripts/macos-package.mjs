// Local macOS packaging only. Optional firmware inputs must pass the project's own-image gate.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
    copyFileSync,
    existsSync,
    lstatSync,
    mkdirSync,
    mkdtempSync,
    readFileSync,
    readdirSync,
    readlinkSync,
    realpathSync,
    renameSync,
    rmSync,
    writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const appRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repo = path.resolve(appRoot, "..");
const appId = "com.tarikbc.eucstudio";
const appName = "EUC Studio.app";
const mujocoVersion = "3.9.0"; // Match tools/fetch-mujoco/fetch.py and the Rust binding.
// Keep this build separate from tauri dev and its optional Unicorn dependency.
const target = path.join(appRoot, "src-tauri/target/macos-local");
const args = process.argv.slice(2).filter((arg) => arg !== "--");
const command = args.shift();
const debug = takeFlag("--debug");
const replace = takeFlag("--replace");
const selectedApp = takeValue("--app");
const firmwareDir = takeValue("--firmware-dir");
const builtApp = path.join(target, debug ? "debug" : "release", "bundle/macos", appName);
const firmwareName = "euc-fw-begode-extreme";
const firmwareProfiles = ["compat", "clean"];

function takeFlag(flag) {
    const index = args.indexOf(flag);
    if (index < 0) return false;
    args.splice(index, 1);
    return true;
}

function takeValue(flag) {
    const index = args.indexOf(flag);
    if (index < 0) return undefined;
    const value = args[index + 1];
    if (!value || value.startsWith("--")) throw new Error(`Give a value for ${flag}.`);
    args.splice(index, 2);
    return path.resolve(value);
}

function run(program, values, options = {}) {
    const result = spawnSync(program, values, {
        cwd: appRoot,
        encoding: "utf8",
        stdio: "pipe",
        ...options,
    });
    if (result.error) throw result.error;
    if (result.status !== 0) {
        throw new Error(
            `${program} failed (${result.status}).\n${result.stderr || result.stdout || ""}`,
        );
    }
    return result.stdout?.trim() || "";
}

function plist(bundle, key) {
    return run("/usr/libexec/PlistBuddy", [
        "-c",
        `Print :${key}`,
        path.join(bundle, "Contents/Info.plist"),
    ]);
}

function hash(file) {
    return createHash("sha256").update(readFileSync(file)).digest("hex");
}

function present(file) {
    // Unlike existsSync, this also detects a dangling link at the installation destination.
    return lstatSync(file, { throwIfNoEntry: false }) !== undefined;
}

function inside(root, file) {
    const relative = path.relative(root, file);
    return (
        relative === "" ||
        (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative))
    );
}

function filesUnder(root, allowLinks = true) {
    const files = [];
    for (const entry of readdirSync(root, { withFileTypes: true })) {
        const file = path.join(root, entry.name);
        if (entry.isSymbolicLink()) {
            if (!allowLinks) throw new Error(`Firmware resource links are refused: ${file}`);
            const link = readlinkSync(file);
            if (path.isAbsolute(link) || !inside(realpathSync(root), realpathSync(file))) {
                throw new Error(`External bundle link: ${file}`);
            }
        } else if (entry.isDirectory()) files.push(...filesUnder(file, allowLinks));
        else if (entry.isFile()) files.push(file);
        else throw new Error(`Unsupported file: ${file}`);
    }
    return files;
}

function identity(bundle) {
    if (lstatSync(bundle).isSymbolicLink() || !bundle.endsWith(".app"))
        throw new Error("Use a real .app directory.");
    if (plist(bundle, "CFBundleIdentifier") !== appId)
        throw new Error(`The app ID must be ${appId}.`);
    const executable = plist(bundle, "CFBundleExecutable");
    if (executable !== path.basename(executable))
        throw new Error("Invalid bundle executable name.");
    return path.join(bundle, "Contents/MacOS", executable);
}

function rpaths(file) {
    return [
        ...run("/usr/bin/otool", ["-l", file]).matchAll(
            /cmd LC_RPATH\s+cmdsize \d+\s+path (.*?) \(offset/g,
        ),
    ].map((match) => match[1]);
}

function dependencies(file) {
    return run("/usr/bin/otool", ["-L", file])
        .split("\n")
        .filter((line) => /^\s+\S/.test(line))
        .map((line) => line.trim().split(" (compatibility version")[0]);
}

function macho(file) {
    const magic = readFileSync(file).subarray(0, 4).toString("hex");
    return [
        "cffaedfe",
        "feedfacf",
        "cefaedfe",
        "feedface",
        "cafebabe",
        "bebafeca",
        "cafebabf",
    ].includes(magic);
}

function imageReport(gate, file, profile, format, env) {
    const report = JSON.parse(
        run(
            gate,
            [
                "--json",
                "--profiles",
                path.join(repo, "profiles"),
                "image",
                "validate",
                file,
                "--wheel",
                "begode-extreme",
            ],
            { env },
        ),
    );
    const info = report.identity?.info;
    // The image gate deliberately accepts pinned stock. The packager must not accept it.
    if (
        report.passed !== true ||
        report.identity?.kind !== "ours" ||
        info?.board !== "begode-extreme" ||
        info.profile !== profile ||
        report.format !== format ||
        !/^[a-f0-9]{64}$/.test(report.sha256)
    ) {
        throw new Error(
            `Refused ${profile} ${format}: valid matching .euc_info metadata is required.`,
        );
    }
    return report;
}

function validatePair(gate, directory, profile, env) {
    const bin = path.join(directory, `${firmwareName}.bin`);
    const elf = path.join(directory, `${firmwareName}.elf`);
    const before = { bin: hash(bin), elf: hash(elf) };
    const binReport = imageReport(gate, bin, profile, "raw", env);
    const elfReport = imageReport(gate, elf, profile, "elf", env);
    if (
        binReport.sha256 !== before.bin ||
        elfReport.sha256 !== binReport.sha256 ||
        binReport.size !== elfReport.size ||
        binReport.load_addr !== elfReport.load_addr ||
        hash(bin) !== before.bin ||
        hash(elf) !== before.elf
    ) {
        throw new Error(
            `The ${profile} BIN and ELF differ, or an input changed during validation.`,
        );
    }
    // Both gate reports hash the flash bytes; ELF debug/symbol bytes have their own file digest.
    return { ...before, imageSha256: binReport.sha256, info: binReport.identity.info };
}

function prepareFirmware() {
    if (!firmwareDir) return undefined;
    const root = realpathSync(firmwareDir);
    const inputs = {};
    for (const profile of firmwareProfiles) {
        const candidates = [`target-${profile}`, profile].map((name) => path.join(root, name));
        const directory = candidates.find((candidate) =>
            ["bin", "elf"].every((extension) =>
                existsSync(path.join(candidate, `${firmwareName}.${extension}`)),
            ),
        );
        if (!directory) throw new Error(`Missing ${profile} BIN and ELF below ${root}.`);
        for (const extension of ["bin", "elf"]) {
            const file = path.join(directory, `${firmwareName}.${extension}`);
            if (!inside(root, realpathSync(file)) || !lstatSync(file).isFile()) {
                throw new Error(`Use a regular firmware file inside the selected root: ${file}`);
            }
        }
        inputs[profile] = directory;
    }
    const gateTarget = path.join(target, "image-gate");
    const env = { ...process.env, CARGO_TARGET_DIR: gateTarget };
    // Build the gate from this checkout, not from a potentially stale external CLI.
    run(
        "cargo",
        [
            "build",
            "--locked",
            "--release",
            "--no-default-features",
            "--manifest-path",
            path.join(repo, "Cargo.toml"),
            "--package",
            "euc-cli",
            "--bin",
            "euc",
            "--target-dir",
            gateTarget,
        ],
        { env, stdio: "inherit" },
    );
    const gate = path.join(gateTarget, "release/euc");
    const reports = Object.fromEntries(
        firmwareProfiles.map((profile) => [
            profile,
            validatePair(gate, inputs[profile], profile, env),
        ]),
    );
    mkdirSync(target, { recursive: true });
    const stage = mkdtempSync(path.join(target, ".firmware-"));
    try {
        const profiles = {};
        for (const profile of firmwareProfiles) {
            const directory = path.join(stage, profile);
            mkdirSync(directory);
            for (const extension of ["bin", "elf"]) {
                const name = `${firmwareName}.${extension}`;
                const source = path.join(inputs[profile], name);
                const bytes = readFileSync(source);
                if (
                    createHash("sha256").update(bytes).digest("hex") !== reports[profile][extension]
                ) {
                    throw new Error(`The ${profile} ${extension} changed before staging.`);
                }
                writeFileSync(path.join(directory, name), bytes);
            }
            // Revalidate the exact staged files that the app will receive.
            profiles[profile] = validatePair(gate, directory, profile, env);
            console.log(
                `Own firmware ready: ${profile}, flash SHA-256 ${profiles[profile].imageSha256}`,
            );
        }
        return { stage, profiles };
    } catch (error) {
        rmSync(stage, { recursive: true, force: true });
        throw error;
    }
}

function checkFirmware(bundle, firmware = {}) {
    const profiles = Object.keys(firmware).sort();
    if (profiles.length && profiles.join(",") !== [...firmwareProfiles].sort().join(",")) {
        throw new Error("The firmware manifest must contain both compat and clean.");
    }
    const root = path.join(bundle, "Contents/Resources/firmware");
    if (present(root) && !lstatSync(root).isDirectory()) {
        throw new Error("The firmware resource root must be a real directory.");
    }
    const expected = [];
    for (const profile of profiles) {
        const record = firmware[profile];
        if (
            record.info?.board !== "begode-extreme" ||
            record.info.profile !== profile ||
            record.bin !== record.imageSha256
        ) {
            throw new Error(`Invalid own-firmware metadata: ${profile}`);
        }
        for (const extension of ["bin", "elf"]) {
            const relative = `${profile}/${firmwareName}.${extension}`;
            const file = path.join(root, relative);
            if (!lstatSync(file).isFile() || hash(file) !== record[extension]) {
                throw new Error(`Bundled firmware differs from its validated input: ${relative}`);
            }
            expected.push(relative);
        }
    }
    const actual = existsSync(root)
        ? filesUnder(root, false)
              .map((file) => path.relative(root, file))
              .sort()
        : [];
    if (JSON.stringify(actual) !== JSON.stringify(expected.sort())) {
        throw new Error(
            "Unexpected firmware resource files. Only the four validated BIN/ELF files are allowed.",
        );
    }
}

function check(bundle) {
    const executable = identity(bundle);
    const frameworkRoot = path.join(bundle, "Contents/Frameworks");
    const manifest = JSON.parse(
        readFileSync(path.join(bundle, "Contents/Resources/local-build.json"), "utf8"),
    );
    if (manifest.appId !== appId || manifest.cpu !== "own" || manifest.signing !== "ad-hoc") {
        throw new Error("The local build manifest is invalid.");
    }
    checkFirmware(bundle, manifest.firmware);
    // codesign covers the executable and this manifest. Hashing the signed executable in a
    // signed resource would create a circular dependency through the resource seal.
    const library = path.join(frameworkRoot, manifest.mujocoLibrary);
    if (!inside(frameworkRoot, library) || hash(library) !== manifest.mujocoSha256) {
        throw new Error("The bundled MuJoCo library differs from the packaged build.");
    }
    const binaries = filesUnder(bundle).filter(macho);
    run("/usr/bin/xcrun", ["dyld_info", "-validate_only", ...binaries]);
    for (const file of binaries) {
        for (const dependency of dependencies(file)) {
            if (dependency.startsWith("/System/Library/") || dependency.startsWith("/usr/lib/"))
                continue;
            let resolved;
            if (dependency.startsWith("@rpath/"))
                resolved = path.join(frameworkRoot, dependency.slice(7));
            else if (dependency.startsWith("@loader_path/"))
                resolved = path.resolve(path.dirname(file), dependency.slice(13));
            else if (dependency.startsWith("@executable_path/"))
                resolved = path.resolve(path.dirname(executable), dependency.slice(17));
            if (
                !resolved ||
                !existsSync(resolved) ||
                !inside(realpathSync(bundle), realpathSync(resolved))
            ) {
                throw new Error(`Unbundled dependency in ${file}: ${dependency}`);
            }
        }
        for (const entry of rpaths(file)) {
            if (
                path.isAbsolute(entry) &&
                !entry.startsWith("/System/Library/") &&
                !entry.startsWith("/usr/lib/")
            ) {
                throw new Error(`Nonportable loader path in ${file}: ${entry}`);
            }
        }
    }
    if (!rpaths(executable).includes("@executable_path/../Frameworks"))
        throw new Error("The Frameworks loader path is missing.");
    if (!dependencies(executable).some((entry) => entry === `@rpath/${manifest.mujocoLibrary}`)) {
        throw new Error("The app does not link the bundled MuJoCo library.");
    }
    run("/usr/bin/codesign", ["--verify", "--deep", "--strict", "--verbose=2", bundle]);
    console.log(`Bundle checks pass: ${bundle}`);
    console.log(`CPU: own. Signing: ad-hoc. MuJoCo: @rpath/${manifest.mujocoLibrary}`);
    return manifest;
}

function build(firmware) {
    const libDir = path.resolve(
        process.env.MUJOCO_DYNAMIC_LINK_DIR || path.join(repo, ".cache/mujoco/mujoco-3.9.0/lib"),
    );
    const library = realpathSync(path.join(libDir, "libmujoco.dylib"));
    const framework = path.join(libDir, "mujoco.framework");
    if (!inside(realpathSync(framework), library))
        throw new Error("Use the pinned MuJoCo framework from tools/fetch-mujoco.");
    const installNames = [
        ...run("/usr/bin/otool", ["-D", library]).matchAll(
            /^(@rpath\/mujoco\.framework\/[^\n]+)$/gm,
        ),
    ];
    const installName = installNames[0]?.[1];
    if (
        installName !== `@rpath/mujoco.framework/Versions/A/libmujoco.${mujocoVersion}.dylib` ||
        installNames.some((name) => name[1] !== installName)
    )
        throw new Error("Unexpected MuJoCo install name.");
    const publicRoot = path.join(appRoot, "public");
    const tracked = new Set(
        run("git", ["ls-files", "-z", "--", "app/public"], { cwd: repo }).split("\0"),
    );
    for (const file of filesUnder(publicRoot)) {
        if (!tracked.has(path.relative(repo, file)))
            throw new Error(`Untracked public asset: ${file}`);
    }
    const env = {
        ...process.env,
        CARGO_TARGET_DIR: target,
        MUJOCO_DYNAMIC_LINK_DIR: libDir,
        EUC_NO_UNICORN: "1",
        APPLE_SIGNING_IDENTITY: "-",
    };
    delete env.EUC_CPU;
    // This local command must not use a developer account or submit an app for notarization.
    for (const name of [
        "APPLE_ID",
        "APPLE_PASSWORD",
        "APPLE_TEAM_ID",
        "APPLE_API_KEY",
        "APPLE_API_ISSUER",
        "APPLE_API_KEY_PATH",
        "APPLE_CERTIFICATE",
        "APPLE_CERTIFICATE_PASSWORD",
    ])
        delete env[name];
    // This command intentionally offers no --features passthrough. A bundle also rejects Unicorn in Rust.
    const cli = path.join(appRoot, "node_modules/@tauri-apps/cli/tauri.js");
    run(process.execPath, [cli, "build", "--bundles", "app", ...(debug ? ["--debug"] : [])], {
        env,
        stdio: "inherit",
    });
    const executable = identity(builtApp);
    const frameworks = path.join(builtApp, "Contents/Frameworks");
    mkdirSync(frameworks, { recursive: true });
    run("/usr/bin/ditto", [framework, path.join(frameworks, "mujoco.framework")]);
    // A dependent crate or an external linker flag must not leave a build-machine cache path behind.
    for (const entry of new Set(
        rpaths(executable).filter(
            (entry) =>
                path.isAbsolute(entry) &&
                !entry.startsWith("/System/Library/") &&
                !entry.startsWith("/usr/lib/"),
        ),
    )) {
        run("/usr/bin/install_name_tool", ["-delete_rpath", entry, executable]);
    }
    const resources = path.join(builtApp, "Contents/Resources");
    if (existsSync(path.join(resources, "firmware"))) {
        throw new Error(
            "Remove static firmware resource maps. Use --firmware-dir for validated inputs.",
        );
    }
    if (firmware) {
        for (const profile of firmwareProfiles) {
            const directory = path.join(resources, "firmware", profile);
            mkdirSync(directory, { recursive: true });
            for (const extension of ["bin", "elf"]) {
                const name = `${firmwareName}.${extension}`;
                copyFileSync(path.join(firmware.stage, profile, name), path.join(directory, name));
            }
        }
        checkFirmware(builtApp, firmware.profiles);
    }
    const notices = path.join(resources, "licenses/mujoco");
    mkdirSync(notices, { recursive: true });
    for (const name of ["LICENSE", "THIRD_PARTY_NOTICES"])
        copyFileSync(path.join(libDir, "..", name), path.join(notices, name));
    const models = {};
    for (const file of filesUnder(path.join(publicRoot, "models"))) {
        const relative = path.relative(publicRoot, file);
        if (hash(file) !== hash(path.join(appRoot, "dist", relative)))
            throw new Error(`Missing model asset: ${relative}`);
        models[relative] = hash(file);
    }
    const bundledLibrary = path.join(frameworks, installName.slice(7));
    run("/usr/bin/codesign", ["--force", "--sign", "-", "--timestamp=none", bundledLibrary]);
    run("/usr/bin/codesign", [
        "--force",
        "--sign",
        "-",
        "--timestamp=none",
        path.join(frameworks, "mujoco.framework"),
    ]);
    writeFileSync(
        path.join(resources, "local-build.json"),
        JSON.stringify(
            {
                appId,
                cpu: "own",
                signing: "ad-hoc",
                architecture: process.arch,
                sourceRevision: run("git", ["rev-parse", "HEAD"], { cwd: repo }),
                sourceDirty: Boolean(run("git", ["status", "--porcelain"], { cwd: repo })),
                mujocoLibrary: installName.slice(7),
                mujocoSha256: hash(bundledLibrary),
                models,
                firmware: firmware?.profiles ?? {},
            },
            null,
            2,
        ) + "\n",
    );
    run("/usr/bin/codesign", ["--force", "--sign", "-", "--timestamp=none", builtApp]);
    check(builtApp);
    const quotedApp = `'${builtApp.replaceAll("'", "'\\''")}'`;
    console.log(`Install command: pnpm macos:install --app ${quotedApp}`);
}

function install() {
    if (!selectedApp) throw new Error("Select the reviewed bundle with --app PATH.");
    const source = realpathSync(selectedApp);
    check(source);
    const destination = path.join("/Applications", appName);
    if (inside(destination, source))
        throw new Error("The install source must be outside the destination.");
    if (present(destination)) {
        identity(destination);
        if (!replace) throw new Error("The app already exists. Use --replace to replace it.");
    }
    // Prepare and validate a full copy before changing an existing installation.
    const stage = mkdtempSync(path.join("/Applications", ".euc-studio-install-"));
    const staged = path.join(stage, appName);
    const backup = path.join(stage, "previous.app");
    let moved = false;
    try {
        run("/usr/bin/ditto", [source, staged]);
        check(staged);
        if (present(destination)) {
            identity(destination);
            if (!replace)
                throw new Error("The destination appeared during installation. Use --replace.");
            renameSync(destination, backup);
            moved = true;
        }
        try {
            renameSync(staged, destination);
        } catch (error) {
            if (moved) {
                try {
                    renameSync(backup, destination);
                    moved = false;
                } catch {
                    throw new Error(`Install failed. Recover the previous app from ${backup}.`);
                }
            }
            throw error;
        }
        moved = false;
        console.log(`Installed: ${destination}`);
    } finally {
        // Keep the previous app if rollback itself failed.
        if (!moved) rmSync(stage, { recursive: true, force: true });
    }
}

try {
    if (process.platform !== "darwin") throw new Error("This command requires macOS.");
    const [nodeMajor, nodeMinor] = process.versions.node.split(".").map(Number);
    if (nodeMajor < 20 || (nodeMajor === 20 && nodeMinor < 19))
        throw new Error("Use Node.js 20.19 or newer.");
    if (args.length || !["build", "check", "install", "firmware-check"].includes(command))
        throw new Error(
            "Use build [--firmware-dir ROOT], firmware-check --firmware-dir ROOT, check --app PATH, or install --app PATH [--replace].",
        );
    if (command !== "install" && replace) throw new Error("--replace applies only to install.");
    if (["build", "firmware-check"].includes(command) && selectedApp)
        throw new Error("--app applies only to check or install.");
    if (firmwareDir && !["build", "firmware-check"].includes(command))
        throw new Error("--firmware-dir applies only to build or firmware-check.");
    if (command === "firmware-check" && !firmwareDir)
        throw new Error("Give --firmware-dir for firmware-check.");
    if (["build", "firmware-check"].includes(command)) {
        const firmware = prepareFirmware();
        try {
            if (command === "build") build(firmware);
        } finally {
            if (firmware) rmSync(firmware.stage, { recursive: true, force: true });
        }
    } else if (command === "check") check(selectedApp || builtApp);
    else install();
} catch (error) {
    console.error(`macOS package: ${error.message}`);
    process.exitCode = 1;
}
