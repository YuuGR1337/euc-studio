# Contributing to EUC Studio

Thank you for helping make EUC firmware better and safer. Please read [SAFETY.md](SAFETY.md) first.

## Ground rules

- **License.** Contributions are accepted under the repository license (PolyForm Noncommercial 1.0.0). Sign off every
  commit with `git commit -s` to certify the [Developer Certificate of Origin](https://developercertificate.org/).
- **No vendor material.** Never commit stock firmware images, decompiled or disassembled vendor code, analysis
  databases, or long byte runs copied from a vendor image. Facts about hardware and behavior, written in your own words
  and code, are welcome. Install the guard once per clone: `git config core.hooksPath tools/hooks`.
- **No copied GPL code.** Protocol facts from other EUC apps are fine; their code is not. Unicorn (GPL-2.0) is a
  development and test oracle only and never part of a release build.
- **Safety-relevant changes need evidence.** A change to balance, motor control, protections, the plant, the rider
  or the flasher needs a simulator scenario that shows its effect, and for the firmware the comparison against stock
  where it applies. See [Safety-relevant changes](#safety-relevant-changes).
- **Estimates stay visible.** A value that nobody measured is marked `ESTIMATE` (or `PRELIMINARY` for a result) in the
  code or the doc that uses it, with where it came from.
- **Privacy.** Never commit GPS tracks, raw ride logs that show where someone rides, photos of people, or body data.
  Derived numbers (a speed, a current, a period) are fine.

## Pick an issue

1. Open the [roadmap board](https://github.com/users/tarikbc/projects/3) or the
   [issue list](https://github.com/tarikbc/euc-studio/issues). Issues are grouped by
   [milestone](https://github.com/tarikbc/euc-studio/milestones), and large topics have an epic (`type: epic`) that
   lists their parts.
2. New here? Start with a
   [good first issue](https://github.com/tarikbc/euc-studio/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22).
3. Labels say where the work is (`area: engine`, `area: physics`, `area: rider`, `area: tyre`, `area: world`,
   `area: firmware`, `area: scenarios`, `area: plant`, `area: studio`, `area: flasher`, `area: sound`,
   `area: calibration`, `area: model`, `area: brands`, `area: release`) and what it needs (`needs: data` for
   measurements or logs, `needs: hardware` for physical access, `type: research` for an investigation before code).
4. Comment on the issue before you start larger work, so two people do not do the same thing. A short plan in the
   comment helps the review.
5. A bug, a feature idea or real-wheel data without an issue yet: use the
   [issue forms](https://github.com/tarikbc/euc-studio/issues/new/choose).

## Area map

| Area | Path | Language | Start with |
| --- | --- | --- | --- |
| CPU core | `crates/euc-armv7m` (our core), `crates/euc-unicorn` (GPL oracle, never shipped) | Rust | [crates/euc-armv7m/README.md](crates/euc-armv7m/README.md), [CPU timing](docs/models/cpu-timing.md) |
| Engine | `crates/euc-core` (clock, bus, traits), `crates/euc-stm32f1` (peripherals), `crates/euc-devices` (IMU, BMS, display, LEDs, buzzer), `crates/euc-sim` (machine, sessions, scenarios, compare, SIL, recordings) | Rust | [Architecture](docs/architecture.md), [SIL](docs/sil.md) |
| Plant | `crates/euc-plant` (inverter, motor, halls, battery, thermal) | Rust | [Electrical model](docs/models/electrical.md) |
| Physics and rider | `crates/euc-physics` (MuJoCo wheel, tyre, suspension, rider, IMU) | Rust | [Body and rider model](docs/models/body.md) |
| City world | `crates/euc-world` (seeded, chunked city generator) | Rust | [World model](docs/models/world.md) |
| Scenarios | `scenarios/`, `crates/euc-sim/src/scenario.rs` | TOML, Rust | [Writing scenarios](docs/scenarios/writing-scenarios.md), [scenario results](docs/models/scenarios.md) |
| Telemetry and recordings | `crates/euc-telemetry` | Rust | [Recordings](docs/recordings.md) |
| Profiles and images | `profiles/`, `crates/euc-profile`, `crates/euc-image`, `crates/euc-fwsource`, `crates/euc-display` | TOML, Rust | [Adding a wheel](docs/adding-a-wheel.md), [image info block](docs/spec/euc-info.md) |
| Protocol and BLE | `crates/euc-proto-begode`, `crates/euc-ble-radio` | Rust | [Begode protocol](docs/protocol/begode.md), [BLE bridge](docs/bridge.md), [BLE radio](docs/ble-radio.md) |
| Flasher | `crates/euc-flasher`, `crates/euc-ble`, `crates/euc-ymodem`, `crates/euc-flash-sim` | Rust | [Flasher spec](docs/spec/flasher.md), [SAFETY.md](SAFETY.md#flasher-rules) |
| CLI | `crates/euc-cli` (`euc`) | Rust | `cargo run -p euc-cli -- --help` |
| Python | `crates/euc-python` (its own cargo workspace), `python/` | Rust, Python | [Python API](docs/python.md) |
| Our firmware | `firmware/` | C11 | [firmware/README.md](firmware/README.md), [cut-offs](firmware/docs/cutoffs.md), [quirks](firmware/docs/quirks.md) |
| Desktop app | `app/` (React, three.js), `app/src-tauri/` (native engine host, its own cargo workspace) | TypeScript, Rust | [app/README.md](app/README.md), [app/DESIGN.md](app/DESIGN.md) |
| Free Ride | `app/src/workspaces/freeRide/`, `app/src-tauri/src/engine/free_ride.rs` | TypeScript, Rust | [Free Ride README](app/src/workspaces/freeRide/README.md), [Free Ride design](docs/design/free-ride.md) |
| 3D model | `app/public/models/begode-extreme/`, `profiles/begode-extreme/geometry.toml` | GLB, TOML | [model README](app/public/models/begode-extreme/README.md), [geometry](docs/hardware/geometry.md) |
| Tools | `tools/vendor-guard`, `tools/fetch-mujoco`, `tools/hooks` | Python, shell | each tool's README |

[docs/README.md](docs/README.md) maps every document.

## Setup

```sh
git config core.hooksPath tools/hooks                    # the vendor guard runs before each commit
python3 tools/fetch-mujoco/fetch.py                      # the pinned MuJoCo library (once per clone)
cargo run --release -p euc-cli -- fetch begode-extreme   # the stock image, for stock runs and compares
```

The firmware needs CMake 3.25 or newer, Ninja and the Arm GNU Toolchain (15.3.rel2 is tested; see
[firmware/README.md](firmware/README.md#build)). The app needs Node 20.19 or newer, pnpm 10 and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

## Checks per area

Run the checks of every area you touched. CI runs the same commands (`.github/workflows/`).

### Every change

```sh
python3 tools/vendor-guard/check.py --all                         # the vendor guard on the whole tree
python3 -m unittest discover -s tools/vendor-guard -p 'test_*.py' # when you change the guard
```

Without the stock images on your machine, the guard skips its deep byte check; CI runs it with the images.

### Rust workspace

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --exclude euc-unicorn --locked
cargo test -p euc-world --locked          # one crate at a time while you work
```

`--all-features` also builds the Unicorn oracle (GPL-2.0, development only), which needs CMake. A plain
`cargo test` runs the default members: every workspace crate but `euc-unicorn` (`euc-python` and `app/src-tauri`
are separate workspaces). `cargo deny check` checks licenses and
advisories against `deny.toml` when you change dependencies.

The SIL tests need the SIL library first:

```sh
(cd firmware && cmake --workflow --preset host-sil)
cargo test -p euc-sim --lib --locked sil::tests -- --include-ignored
```

The stock boot and ride tests need the fetched stock image:
`cargo test --release --locked -p euc-armv7m --test stock_boot --test stock_ride`.

### Python bindings

`crates/euc-python` is its own cargo workspace. [docs/python.md](docs/python.md#build-and-test) has the full build and
test sequence (a virtual environment, maturin, the wheel, pytest). Its Rust checks:

```sh
cargo fmt --manifest-path crates/euc-python/Cargo.toml --check
PYO3_PYTHON="$PWD/python/.venv/bin/python" \
  cargo clippy --manifest-path crates/euc-python/Cargo.toml --all-targets --locked -- -D warnings
```

### Firmware

From `firmware/`:

```sh
cmake --workflow --preset host-debug                      # unit tests with AddressSanitizer and UBSan
cmake --build --preset host-debug --target format-check   # clang-format
cmake --workflow --preset host-sil                        # the SIL library and its tests
cmake --workflow --preset target-compat                   # the images, with their size budget
cmake --workflow --preset target-clean
python3 tools/gen_quirks_doc.py --check                   # docs/quirks.md matches config/quirks.def
cmake --build --preset target-clean --target cppcheck     # MISRA C:2012 addon (cppcheck installed)
```

Linux CI builds the host presets with GCC, which reports `-Wsign-conversion` cases that Apple clang accepts. On macOS,
build the host tests with GCC too before a pull request (Homebrew's `gcc` formula installs it as `gcc-16`; use your
version's name):

```sh
cmake --preset host-debug -B build/host-gcc -DCMAKE_C_COMPILER=gcc-16
cmake --build build/host-gcc && ctest --test-dir build/host-gcc
```

clang-tidy and the probe map have their own commands in [firmware/README.md](firmware/README.md#style-and-static-analysis).

### Desktop app

From `app/`:

```sh
pnpm install --frozen-lockfile
pnpm typecheck && pnpm lint && pnpm format:check
pnpm tokens:check && pnpm contrast
pnpm test                                        # Vitest
pnpm exec playwright install chromium webkit     # once
pnpm e2e                                         # Playwright, the default suite (Chromium)
pnpm exec playwright test -c e2e/free-ride-controls.config.ts   # a separate suite (Chromium and WebKit)
cargo test --manifest-path src-tauri/Cargo.toml --locked        # the native engine host
```

`pnpm format:check` also covers the Markdown files under `app/`. Screenshots and the special suites are described in
[app/README.md](app/README.md#check).

### Scenarios and physics

Run each scenario you changed, on stock and on our firmware:

```sh
cargo run --release -p euc-cli -- sim run --scenario scenarios/<name>.toml
cargo run --release -p euc-cli -- sim run --scenario scenarios/<name>.toml --firmware clean
cargo run --release -p euc-cli -- sim compare --scenario scenarios/<name>.toml --b compat \
    --tolerance scenarios/compare/compat-vs-stock.toml
```

A change to the plant, the physics, the rider or the world can move every verdict. Then run all shipped scenarios
(`scenarios/*.toml`) on stock, compat and clean, and the compare loop of `.github/workflows/compare.yml`, and list the
verdicts before and after in the pull request. Never widen a tolerance or a check to make a run pass; explain a real
change of behavior instead.

### Docs

Keep relative links working and run the app's prettier on Markdown under `app/`. Write short sentences in the active
voice, use the same term for the same thing, and mark planned work as planned.

## Safety-relevant changes

These changes need scenario evidence in the pull request:

- firmware balance, motor drive, cut-offs, alarms, settings storage and service commands (`firmware/core/`,
  `firmware/apps/`, `firmware/config/quirks.def`);
- the plant, the physics, the rider and the world, where they decide a scenario's verdict;
- the flasher, the BLE guard and the image gate (`crates/euc-flasher`, `crates/euc-ble`, `crates/euc-image`).

The evidence is the scenario (new or existing) that shows the effect, its result on stock, compat and clean where
they apply, and the compare result. A firmware change that alters a protection also updates
[firmware/docs/cutoffs.md](firmware/docs/cutoffs.md). Simulation results never authorize a flash or a ride; see
[SAFETY.md](SAFETY.md).

## Commits and pull requests

- One topic per pull request. Keep unrelated formatting out of it.
- Commit subject: `<area>: <what changed>`, in lower case and the present tense, for example
  `physics: the ground ray sees the ground only` or `free ride: sound from the simulation`. The body says why, how you
  verified it, and the numbers that moved. Reference the issue (`Refs #12`, `Closes #12`).
- Sign off every commit: `git commit -s` adds `Signed-off-by: Your Name <you@example.com>`.
- The pre-commit hook runs the vendor guard on the staged files. Never bypass it. If a rule fires on something
  legitimate, add a narrow `[[allow]]` entry with a reason ([tools/vendor-guard/README.md](tools/vendor-guard/README.md)).
- The pull request template asks what changed, why, how you verified it, and for the DCO and vendor guard checks.

## Reporting a safety problem

Report a behavior that could hurt a rider privately through GitHub's
[Report a vulnerability](https://github.com/tarikbc/euc-studio/security/advisories/new) form, not in a public issue.
[SAFETY.md](SAFETY.md#reporting-a-safety-problem) says what to include.
