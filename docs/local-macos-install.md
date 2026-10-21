# Local macOS app

This command builds a local `.app` with the own CPU core and bundled MuJoCo.
The app uses ad-hoc signing.
It has no Developer ID signature or notarization.
It does not install automatically.

## Build

Install the app dependencies with the pinned pnpm version.
Prepare the pinned MuJoCo cache with the repository fetch tool.

```sh
python3 tools/fetch-mujoco/fetch.py
cd app
pnpm install --frozen-lockfile
pnpm macos:package
```

Set `MUJOCO_DYNAMIC_LINK_DIR` to an existing cache's `lib` directory to reuse that cache.
The packager reads that cache without changing it.
The packager uses `app/node_modules` for the Tauri CLI and frontend build.
Xcode Command Line Tools must supply `codesign`, `otool`, and `install_name_tool`.
The repository supplies the app icons and model assets.
Use Node.js 20.19 or newer, as the app manifest requires.

The release artifact is:

```text
app/src-tauri/target/macos-local/release/bundle/macos/EUC Studio.app
```

`pnpm macos:package --debug` creates a debug bundle under `macos-local/debug`.
Both commands use the own CPU core.
Neither command enables Unicorn or runs tests or scenarios.
The frontend build includes the current tracked model assets.
The packager refuses untracked public assets and external asset links.
It checks model hashes against the frontend build output.

## Check and install

Use the exact artifact path from the build output.
Check the reviewed artifact before installation.

```sh
cd app
pnpm macos:check --app '/absolute/path/EUC Studio.app'
pnpm macos:install --app '/absolute/path/EUC Studio.app'
```

Installation refuses an existing `/Applications/EUC Studio.app` by default.
Use the explicit replacement flag only after review:

```sh
pnpm macos:install --app '/absolute/path/EUC Studio.app' --replace
```

The installer checks the source app ID, build manifest, hashes, portable linkage, and code signature.
It also checks the destination app ID before replacement.
The expected app ID is `com.tarikbc.eucstudio`.
It prepares and checks a copy before it replaces an existing app.
It restores the old app if the final rename fails.
It does not launch the app or request administrator rights.
The user needs write access to `/Applications`.
Quit the existing app before replacement.

## Runtime files and limits

MuJoCo resides at `Contents/Frameworks/mujoco.framework`.
The executable uses `@executable_path/../Frameworks` as its loader path.
Its MuJoCo dependency uses the framework's existing `@rpath` install name.
The packager removes absolute development-cache loader paths from the executable.
It signs the copied framework and app after packaging.
MuJoCo license files reside under `Contents/Resources/licenses/mujoco`.
`Contents/Resources/local-build.json` records the artifact hashes and source revision.

The linkage check accepts only system libraries or dependencies inside the bundle.
The signature check uses `codesign --verify --deep --strict`.
`dyld_info -validate_only` checks the Mach-O files without launching the app.
These checks do not establish UI behavior, simulation accuracy, or Gatekeeper approval.
The build does not launch the engine.

The app does not contain stock firmware, the firmware cache, or private research files.
The existing runtime cache lookup and stock-image fetch behavior remain in the engine.
Without `--firmware-dir`, the packager also omits own firmware images.

## Optional own firmware

Select an existing own-firmware build root:

```sh
cd app
pnpm macos:package --firmware-dir /absolute/path/to/firmware/build
```

The root must contain both `target-compat` and `target-clean`, or `compat` and `clean` subdirectories.
Each subdirectory must contain `euc-fw-begode-extreme.bin` and its matching `.elf`.
The packager selects these four names only.
It never copies the input directory as a resource.
Files must resolve inside the selected root.
Individual input files must be regular files.

Check the inputs without building or launching the app:

```sh
node scripts/macos-package.mjs firmware-check --firmware-dir /absolute/path/to/firmware/build
```

This command builds the current checkout's `euc-cli` with `cargo build --locked`.
It uses a separate target directory under `app/src-tauri/target/macos-local/image-gate`.
It runs `euc image validate` against this checkout's wheel profile for each BIN and ELF.
It requires a passed gate and own-image metadata for the correct board and profile.
The gate's acceptance of pinned stock does not permit stock packaging.
The packager rejects stock identity and missing `.euc_info` metadata.
It also rejects unknown images and a profile mismatch.
The current gate accepts unsealed development images.

The BIN digest must equal the gate's digest of flattened ELF flash bytes.
Image lengths and load addresses must also match.
The packager checks complete ELF file hashes separately, including symbols and debug data.
It detects input changes during validation and staging.
It validates the staged copies again before the app build.
Those copies remain fixed while other agents change source or build files.
The packager removes the temporary copies after completion or failure.

The output includes only the validated four files:

```text
Resources/firmware/compat/euc-fw-begode-extreme.bin
Resources/firmware/compat/euc-fw-begode-extreme.elf
Resources/firmware/clean/euc-fw-begode-extreme.bin
Resources/firmware/clean/euc-fw-begode-extreme.elf
```

The signed `local-build.json` records each firmware file hash, flash digest, and own-image metadata.
`macos:check` also checks those files and refuses extra firmware resource files.
The installer performs the same checks before installation.
Do not add a static firmware resource map to `tauri.conf.json`.
The optional packaging command supplies the validated files after Tauri builds the app.

The desktop uses `app.path().resource_dir()?.join("firmware")` as the bundled firmware root.
`images.rs::set_resource_directory` sets this path from the app's resource directory.
`EUC_FIRMWARE_DIR` takes priority during lookup.
The runtime also checks BIN/ELF equality and the current probe layout.
The packaging gate does not replace the runtime probe check.

Parent must freeze the integrated source before the final package build.
Parent must review that new artifact before installation.
