# EUC Studio

**Run real electric-unicycle firmware in a 3D simulation with a physical rider. Inspect it, compare it, improve it.**

EUC Studio is a workbench for people who want better and safer EUC controller firmware. It runs the controller's
actual firmware image on an emulated microcontroller. Motor, battery, wheel, rider and city models supply its inputs.
The simulator helps you inspect behavior under stated conditions. It does not establish real riding safety.

![EUC Studio, Ride workspace with the real engine](app/docs/screenshots/ride-live.png)

<sub>The Ride workspace with the real engine: the stock firmware at 20 km/h. The screenshot comes from an earlier
build; the current rider figure, the city world and Free Ride are newer. Some physical parameters are estimates.</sub>

> [!WARNING]
> Custom firmware can make a wheel cut out, injure its rider or destroy its power stage. Read [SAFETY.md](SAFETY.md)
> before you build, flash or ride anything from this repository. Today no build of this repository flashes a real
> wheel ([flasher rules](SAFETY.md#flasher-rules)).

## What it is for

- **See the firmware work.** Watch balance, motor control, tiltback and cutoffs react to a rider in 3D, with scopes,
  an event timeline and the firmware's own variables live.
- **Compare stock and custom firmware.** Run both on the same ride, side by side, and find the first moment they differ.
- **Change one thing at a time.** Our own firmware for the Begode Extreme reproduces the stock behavior, quirks
  included (the compat profile), and then fixes them one by one (the clean profile), each fix shown in a scenario.
- **Ride it yourself.** Free Ride puts you on the simulated wheel in an endless generated city, with the keyboard or a
  gamepad, while the real firmware balances it.
- **Grow to other wheels.** Wheels, boards and processors are described as data, so new models and brands can follow.

## How it works

```
 firmware image ─▶ emulated MCU (ARMv7-M core + peripheral models)
                        │ PWM ▲ currents, halls, voltages, IMU
                        ▼     │
                   inverter ─▶ motor ─▶ battery          (electrical plant, 5 µs steps)
                        │ torque ▲ wheel speed
                        ▼        │
              wheel, tyre, shell, human rider, ground    (MuJoCo, 0.5 ms steps; flat floor,
                        │                                 terrain strip or streamed city)
                        ▼
          EUC Studio app · CLI `euc` · Python bindings
```

Everything runs on one deterministic clock counted in microcontroller cycles, so every run can be replayed exactly.
Our firmware can also run as a host library (SIL) in place of the emulated image, with its quirks switched at run
time. See [docs/architecture.md](docs/architecture.md).

The first wheel is the **Begode Extreme**. Its stock firmware boots unchanged in the Rust engine.
The simulator reproduces the observed interfaces. Unknown hardware values remain explicit estimates.

## Status

Updated 2026-10-23. Open work lives on the public [EUC Studio Roadmap](https://github.com/users/tarikbc/projects/3)
board, grouped by [milestone](https://github.com/tarikbc/euc-studio/milestones).
[docs/project-status.md](docs/project-status.md) has the evidence and the checks for each area.

| Milestone | State | Done | Open |
| --- | --- | --- | --- |
| M0 Foundation | Done | Repository, CI, vendor guard, pinned stock fetch, image gate | Keep the gates for new features |
| M1 Stock boots | Done | Stock firmware boots, sends telemetry and answers app commands faster than real time | Broader hardware calibration |
| M2 Stock rides in 3D | Done | Electrical plant, MuJoCo body, rear suspension linkage, terrain, display, lights, sound, Ride workspace | Measured motor, battery and suspension values ([calibration](https://github.com/tarikbc/euc-studio/milestone/5)) |
| M3 Own CPU core | Done | Own ARMv7-M core, lock-step checks against Unicorn, a selectable per-instruction flash timing model | Exact peripheral timestamps inside a quantum; Tune memory writes and gain edits |
| M4 Human rider | Most done | Articulated rider (the default), opt-in contact feet and lateral rider (L1-L3), 26 scenarios, `.eucrec` recordings, Python bindings, Free Ride | Rider balance without external aids ([lateral rider L4-L6](https://github.com/tarikbc/euc-studio/milestone/3)), measured human response |
| M5 Our firmware | Most done | Compat and clean images boot and ride; the SIL library switches every quirk at run time ([SIL](docs/sil.md)) | The 50 % charge full-effort difference ([#42](https://github.com/tarikbc/euc-studio/issues/42)), [firmware findings](https://github.com/tarikbc/euc-studio/milestone/6), hardware validation |
| M6 Flasher | Simulator only | Gated flasher against a simulated wheel with an emulated bootloader; 320-run fault campaign with no unrecoverable run ([spec](docs/spec/flasher.md)) | Image signing, the emulator ride of gate 3, bootloader facts from the SWD dump, the app wizard; no real flash ([M6](https://github.com/tarikbc/euc-studio/milestone/7)) |
| M7 Release | Started | Desktop source builds, a local macOS package ([guide](docs/local-macos-install.md)), the wheel onboarding guide | Signed bundles, docs site, browser demo, a second wheel ([M7](https://github.com/tarikbc/euc-studio/milestone/8)) |
| M8 Bench | Planned | The safety procedure only | Controlled bench work and trace comparison ([M8](https://github.com/tarikbc/euc-studio/milestone/9)) |

Current work:

| Track | State | Open |
| --- | --- | --- |
| W2 Scenarios in the city | Done (2026-10-23) | The shipped scenarios ride the generated city ([world model](docs/models/world.md), [#11](https://github.com/tarikbc/euc-studio/issues/11)) |
| P1 Physics realism bundle | In progress | Soft tyre, calibrated motor and rolling resistance, kerb absorb, roll stability, then one scenario rerun ([epic #57](https://github.com/tarikbc/euc-studio/issues/57)) |
| Lateral rider L4-L6 | L1-L3 done | The rider balances and turns with its own body instead of external aids ([epic #58](https://github.com/tarikbc/euc-studio/issues/58)) |
| Free Ride polish | Play mode, drive mode, Chase camera, daylight, sound and HUD done | Impact sounds, city look up close, buildings, trees, a minimap ([milestone](https://github.com/tarikbc/euc-studio/milestone/4)) |
| Calibration | Open | Tools and measurements that tie the simulator to real wheels ([epic #59](https://github.com/tarikbc/euc-studio/issues/59)) |

Plans and limits per milestone: [docs/roadmap.md](docs/roadmap.md). All documents: [docs/README.md](docs/README.md).

## Quick start

You need:

- Rust through [rustup](https://rustup.rs). The toolchain version is pinned in `rust-toolchain.toml`; rustup installs it
  on the first `cargo` command.
- Python 3.11 or newer.
- For the desktop app: Node 20.19 or newer, pnpm 10, and the
  [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.
- For our firmware: CMake 3.25 or newer, Ninja, and the Arm GNU Toolchain (see [firmware/README.md](firmware/README.md)).

The engine and the CLI build on macOS, Linux and Windows. The BLE radio features run on macOS only.

```sh
git clone https://github.com/tarikbc/euc-studio && cd euc-studio
git config core.hooksPath tools/hooks          # once per clone: the vendor guard runs before each commit
python3 tools/fetch-mujoco/fetch.py            # once per clone: the pinned MuJoCo library the physics links

cargo run --release -p euc-cli -- fetch begode-extreme   # download and verify the official Begode Extreme build
```

Run a scenario on the stock firmware. Motor, battery, body, rider and city are simulated, and the scenario's checks
are judged at the end:

```sh
cargo run --release -p euc-cli -- sim run --scenario scenarios/ride-20kmh-stop.toml
```

Build our firmware, then ride it and compare it with stock:

```sh
(cd firmware && cmake --workflow --preset target-compat && cmake --workflow --preset target-clean)
cargo run --release -p euc-cli -- sim run --scenario scenarios/ride-20kmh-stop.toml --firmware clean
cargo run --release -p euc-cli -- sim compare --scenario scenarios/ride-20kmh-stop.toml --b compat \
    --tolerance scenarios/compare/compat-vs-stock.toml
```

Start the desktop studio (the first build takes several minutes):

```sh
cd app && pnpm install && pnpm tauri dev
```

More: `cargo run -p euc-cli -- --help` lists every command, [docs/scenarios/writing-scenarios.md](docs/scenarios/writing-scenarios.md)
explains the scenario files, [docs/compare.md](docs/compare.md) the comparison, [docs/python.md](docs/python.md) the
Python API, and [docs/spec/flasher.md](docs/spec/flasher.md) the simulated flasher (`euc flash --sim --synthetic`).
The checks for each area are in [CONTRIBUTING.md](CONTRIBUTING.md#checks-per-area).

## Free Ride

In the desktop app, open the Ride workspace and press **Free ride** above the 3D view. The ride starts at once in the
generated city: the firmware boots (about 1.5 s of simulated time) and the keys work. A crash never pauses; press R
to respawn.

| Key | Gamepad | Action |
| --- | --- | --- |
| W / ↑, S / ↓ | Left stick, RT / LT | Ride forward, brake |
| A / ←, D / → | Left stick | Turn |
| Space (hold) | LB (hold) | Crouch; at speed also a tuck (less drag) |
| X | B | Step off |
| R, Shift+R | X, Back | Respawn here, at the start |
| P (hold), H (hold) | | The wheel's power and handle buttons |
| L, B, M | | Light, horn, pedal mode |
| Esc | Start | Pause, go on |
| C | Y | Camera |

Keys can be rebound in the panel. The full reference, including drive mode and lean mode, is in
[app/src/workspaces/freeRide/README.md](app/src/workspaces/freeRide/README.md).
Free Ride is a simulation for firmware work, not a riding trainer.

## Where to start

- Look at the [roadmap board](https://github.com/users/tarikbc/projects/3) and the
  [good first issues](https://github.com/tarikbc/euc-studio/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22).
  Issues labelled [help wanted](https://github.com/tarikbc/euc-studio/issues?q=is%3Aissue+is%3Aopen+label%3A%22help+wanted%22)
  and [needs: data](https://github.com/tarikbc/euc-studio/issues?q=is%3Aissue+is%3Aopen+label%3A%22needs%3A+data%22)
  are open to anyone with the skills or a wheel to measure.
- Read [CONTRIBUTING.md](CONTRIBUTING.md) for the ground rules, the area map and the checks per area.
- Read [docs/architecture.md](docs/architecture.md), then the guide of the area you work on
  ([docs/README.md](docs/README.md) maps every document).
- Own a wheel? Measurements and ride data help most. Use the
  [real-wheel data](https://github.com/tarikbc/euc-studio/issues/new?template=real-wheel-data.yml) issue form, and
  never post GPS tracks or raw logs that show where you ride.

## Stock firmware is never in this repository

`euc fetch <wheel>` downloads the official image from the manufacturer's own update server to your machine and checks
it against the sha256 pins built into the tool. Nothing from the vendor image is stored here, and a
[guard](tools/vendor-guard/README.md) blocks it on every commit and in CI.

## Repository

| Path | What |
| --- | --- |
| [`crates/`](crates) | Rust engine: CPU core, peripherals, plant, physics, world, recordings, flasher, CLI `euc`, Python bindings |
| [`firmware/`](firmware) | Our controller firmware in C11 (compat and clean profiles, SIL library) |
| [`app/`](app) | The EUC Studio desktop app (Tauri, React, three.js) and its design system |
| [`scenarios/`](scenarios) | Scenario files with their checks, and the compare tolerances |
| [`profiles/`](profiles) | Wheel definitions: board, plant, geometry, probes, stock firmware pins |
| [`python/`](python) | The `euc_studio` Python package and its tests |
| [`docs/`](docs) | Architecture, roadmap, specifications, model notes, findings ([index](docs/README.md)) |
| [`tests/data/`](tests/data) | Real-wheel captures used as test data |
| [`tools/`](tools) | Vendor guard, git hooks, MuJoCo fetch |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). In short: sign off your commits (`git commit -s`), never add vendor material,
and back every safety-relevant change with a scenario. Report a behavior that could hurt a rider privately, as
[SAFETY.md](SAFETY.md#reporting-a-safety-problem) explains.

## License

EUC Studio is **source-available** under the [PolyForm Noncommercial License 1.0.0](LICENSE). You may use, study,
change and share it for any noncommercial purpose. Commercial use is not allowed. See [NOTICE](NOTICE) for
third-party components and trademarks.

EUC Studio is an independent project and is not affiliated with, endorsed by or sponsored by Begode or any other EUC
manufacturer.
