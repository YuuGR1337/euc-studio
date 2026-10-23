# Adding a wheel

This guide describes the M7 path for a second wheel.
It does not establish support for a second wheel.
`begode-extreme` is the only embedded wheel profile in this checkout.
All paths below start at the repository root.

Read [SAFETY.md](../SAFETY.md) and [CONTRIBUTING.md](../CONTRIBUTING.md) first.
Use the [roadmap](roadmap.md) for milestone boundaries.
Complete simulator work before any hardware work.
Keep hardware access, flashing, and riding outside this procedure.

## 1. Define the support boundary

Record the wheel ID, board revision, MCU, instruction set, memory map, and protocol.
Mark each fact as documented, observed, inferred, estimated, or unknown.
Keep a source reference and units with each value.
Do not choose a wheel from the examples in this guide.

### Capability checklist

- [ ] Identify the exact board revision.
- [ ] Identify the MCU core and required instructions.
- [ ] Identify the app slot, vector table, RAM, and storage pages.
- [ ] Identify clocks, interrupts, DMA routes, sensors, and output wiring.
- [ ] Identify the firmware source and protocol.
- [ ] List missing models and unsupported behavior.

The profile's `board.mcu` and `board.core` fields contain text.
They do not select a CPU backend or add instructions.
The loader can accept text for a core that the simulator cannot run.

| Target | Current implementation | Required work |
|---|---|---|
| Cortex-M3 with modeled STM32F1 behavior | `crates/euc-armv7m/` and `crates/euc-stm32f1/` | Check instructions, registers, clocks, wiring, and device behavior. |
| Another Cortex-M3 MCU or clone | No general compatibility guarantee | Model register and timing differences. Check exception and flash behavior. |
| Cortex-M4F | Unsupported as a complete target | Add required DSP/FPU behavior, FP registers, exception state, MCU models, and firmware port. |
| RISC-V | Unsupported | Add CPU, trap, interrupt, memory, image, startup, and toolchain support. |
| Other wheel protocols | Begode code supplies the current protocol behavior | Add protocol code and connect each caller. A profile string does not supply a plugin. |
| Several CPUs in one wheel | `Machine` owns one CPU | Implement node scheduling and communication before claiming this support. |
| OTA and real flashing | `euc flash` runs the gated flasher against a simulated wheel and the offline gates only ([flasher](spec/flasher.md)); no real flash | A new wheel needs its own bootloader facts, recovery image and gates; complete M6 first. |

`crates/euc-sim/src/cpu.rs` selects `own` or optional `unicorn`.
Unicorn is a development oracle under the `unicorn` feature.
Release builds must use the project's CPU implementation.
Unicorn does not establish support for another MCU.

## 2. Keep research clean

Publish hardware facts and behavior descriptions in your own words.
Write original implementation code from those facts.
Keep stock images in a private cache outside the repository.
Keep source references separate from vendor code extracts.

### Clean research checklist

- [ ] Exclude vendor source, decompiled code, disassembly listings, and analysis databases.
- [ ] Exclude stock binaries and copied byte sequences, including hex arrays.
- [ ] Exclude private research files and extracts.
- [ ] Exclude user photos, recordings, weight, and other personal data.
- [ ] Use synthetic inputs and generic rider settings for public examples.
- [ ] Record unknown values without replacing them with personal measurements.
- [ ] Review every new public file for prohibited material.

Do not copy the complete Begode profile folder as a new wheel.
Its files include assumptions and wheel-specific observations.
Use schema types and clean facts to create each new file.

See [the vendor guard](../tools/vendor-guard/README.md).
The guard's deep byte check requires known stock images.
A guard result without that check does not establish that all copied bytes are absent.
New stock hashes also need guard coverage in `tools/vendor-guard/denylist.toml` through the integration owner.
Do not weaken guard rules or add broad exceptions.

## 3. Create the profile metadata

Create `profiles/<wheel-id>/wheel.toml`.
Use lowercase letters, digits, and dashes for IDs.
Match `id` to the folder name.
Use a board ID of at most 15 ASCII characters for the current C firmware block.
The Rust profile schema permits 16 bytes, but the C field also needs its terminating zero.

`crates/euc-profile/src/lib.rs` defines `WheelProfile`, `Board`, `AppLayout`, and `Ram`.
`crates/euc-profile/src/validate.rs` checks their constraints.
Unknown fields cause errors.

This complete metadata example uses an illustrative Cortex-M3 app layout.
It is not a definition of a real second wheel.
Replace all identity and memory values with reviewed facts.

```toml
schema = 1
id = "example-wheel"
name = "Example Wheel"
brand = "Example"

[board]
id = "example-wheel"
mcu = "STM32F103RC-class"
core = "Cortex-M3"
hclk_hz = 72_000_000

[board.app]
base = 0x0800_3000
max_size = 49_152
vectors = 76
info_offset = 0x140
storage_pages = [0x0800_F000]

[board.ram]
base = 0x2000_0000
size = 0xC000
```

`firmware` defaults to an empty list.
`storage_pages` defaults to an empty list, but real profiles must identify all protected storage pages.
All other fields in this example are required.
This file alone supports metadata loading and image checks.
It cannot supply a complete simulation.

### Layout checklist

- [ ] Set a nonzero clock from reviewed clock facts.
- [ ] Keep the app slot outside the bootloader and all protected storage.
- [ ] Provide at least 16 vector entries.
- [ ] Align the app base to the vector table size rounded up to a power of two.
- [ ] Use at least 128-byte alignment for that base.
- [ ] Place the 128-byte info block after the vector table.
- [ ] Align `info_offset` to four bytes.
- [ ] Keep the info block inside the app slot.
- [ ] Define a nonempty RAM region without address overflow.

The loader checks storage page start addresses against the app slot.
It does not prove erase-page sizes or the complete bootloader protection policy.
Review those boundaries against the actual flash organization.

### Profile loading and embedding checklist

`ProfileDir` reads a named folder in `crates/euc-profile/src/dir.rs`.
`ProfileSource` records the source in `crates/euc-profile/src/source.rs`.
The CLI defaults to `ProfileSource::Builtin`.
`--profiles DIR` or `EUC_PROFILES` replaces the embedded set with that folder.
An override changes the trusted pins and produces an origin warning.

- [ ] Use `--profiles profiles` explicitly during profile development.
- [ ] Pass the parent `profiles/` folder, not the wheel folder.
- [ ] Add every public profile file to `FILES` in `crates/euc-profile/src/builtin.rs` before release integration.
- [ ] Keep `FILES` sorted by wheel ID and file name.
- [ ] Rebuild each binary that embeds the profiles.
- [ ] Confirm the reported profile origin in saved evidence.

The existing embedding checks compare the explicit list with the profile files on disk.
They do not add files automatically.
Use `--wheel <wheel-id>` explicitly with image validation after a second profile exists.

## 4. Describe the simulator board

`WheelFiles::load` in `crates/euc-sim/src/wheel.rs` requires four files.
It reads `wheel.toml`, `board.toml`, `stockmap.toml`, and `plant.toml`.

| File | Type or reader | Required content |
|---|---|---|
| `board.toml` | `BoardFile` in `crates/euc-sim/src/config.rs` | `schema`, `mcu`, `memory`, and `board`; component and wire lists as needed. |
| `stockmap.toml` | `StockMap` in the same file | `schema`, exact stock `build` label; optional `entry` and `ram` lists. |
| `plant.toml` | `PlantParams` and `WheelBody` | Electrical sections and required body sections. See section 5. |
| `settings.toml` | `crates/euc-sim/src/settings.rs` | Optional presets; current encoding uses Begode settings and STM32F1 flash seeds. |
| `probes.toml` | `crates/euc-sim/src/probes.rs` | Optional own-firmware variables with ELF symbols, offsets, types, and layout ID. |
| `geometry.toml` | `collision_shapes` in `wheel.rs` | Optional collision shapes; the physics uses generic shapes when absent. |
| `ble.toml` | `crates/euc-ble-radio/src/profile.rs` | BLE advertisement, GATT, protocol, and module behavior for BLE callers. |
| `display.toml`, `lights.toml` | `crates/euc-display/src/` | Optional desktop output layouts; missing files leave raw digit and LED views. |

### Board checklist

- [ ] Set `schema = 1`.
- [ ] Set `mcu.name`, `irqs`, `priority_bits`, and `irq_names`.
- [ ] Define `memory.flash`, `code_end`, `sram`, `sram_bitband`, `periph`, `periph_bitband`, and `ppb`.
- [ ] Set `board.button_active_low` and `release_after_latch_s`.
- [ ] Give every component a unique name and registered `kind`.
- [ ] Specify bus `base`, optional `size`, role-based `irq`, and constructor parameters.
- [ ] Describe DMA requests with `request` and `channel`.
- [ ] Describe signal connections with `from` and `to`.
- [ ] Map static ADC signals with `analog` entries.
- [ ] Review component order because it controls event dispatch order.
- [ ] Reconcile the profile, simulator memory, linker script, and flash-array model.

The Begode files use different RAM extents in metadata, simulation, and the linker script.
Do not copy those differences without a reason.

This fragment shows actual component and wire syntax.
It does not replace the required board tables or other components.
The `RCC`, `USART1`, and simulator `board` endpoints must exist.

```toml
[[instance]]
name = "USART1"
kind = "stm32f1.usart"
base = 0x4001_3800
clock = "RCC"
bus = "apb2"
irq = { irq = 37 }

[[wire]]
from = "USART1.tx"
to = "board.ble_rx"

[[wire]]
from = "board.ble_tx"
to = "USART1.rx"
```

`Machine` adds the `board` and `plant` components automatically.
Do not add duplicate instances for them.
`board` exposes `ble_rx`, `ble_tx`, `latch`, and `button`.
`plant` accepts `leg_a`, `leg_b`, and `leg_c` PWM signals.
It emits `hall_a`, `hall_b`, and `hall_c`.

### Unimplemented extension points: peripherals and cores

`crates/euc-core/src/periph.rs` defines `Peripheral`, `Ctx`, `Signal`, and `PlantView`.
A model implements reset, register access, events, signal input, and named ports as needed.
Use `Ctx` for scheduled events, interrupts, DMA, bus access, and plant sampling.
Keep all timing on the simulator clock.

`crates/euc-core/src/registry.rs` maps each kind to a constructor.
`Build::params`, `irq`, and `id_of` resolve its configuration.
Register new kinds through `crates/euc-sim/src/lib.rs` and the relevant model crate.
Existing kinds come from `euc-stm32f1`, `euc-devices`, and the simulator's Cortex-M3 DWT model.

Unknown registers can retain written values without real behavior.
`stm32f1.plain` provides no hardware behavior.
Neither behavior proves that a peripheral works.
Resolve each required status transition, interrupt, DMA request, and reset effect.

`Machine` uses the Cortex-M exception controller in `crates/euc-core/src/nvic.rs`.
Its default-handler detection currently scans vector entries 16 through 75.
Review `crates/euc-sim/src/machine.rs` for another vector count.
CPU timing uses fixed CPI and estimated exception costs by default; `--timing flash` selects the per-instruction
flash timing model of our core. See [CPU timing](models/cpu-timing.md) before making timing claims.

## 5. Describe the plant and protocol

Use `crates/euc-plant/src/plant.rs` for `PlantParams`.
Use `crates/euc-physics/src/params.rs` for `WheelBody`.
`split_plant` requires `[body]` and validates the electrical parameters.

### Plant and protocol checklist

- [ ] Define `schema`, `motor`, `inverter`, `halls`, `sensors`, `battery`, and `thermal`.
- [ ] Define `body.tire`, `body.wheel`, and `body.shell`.
- [ ] Add `body.suspension` only when the wheel model requires it.
- [ ] Set explicit `grid_us` and `seed` for repeatable evidence.
- [ ] Review motor direction, phase order, hall order, and electrical angle offsets together.
- [ ] Review sensor channel numbers, scales, signs, offsets, and temperature conversion.
- [ ] Distinguish static `board.toml` analog inputs from live `plant.toml` sensor inputs.
- [ ] Document battery and mechanical estimates with units and uncertainty.
- [ ] Describe protocol commands and telemetry without copying implementation code.
- [ ] Check settings storage and calibration behavior separately for the new wheel.

See [electrical models](models/electrical.md) and [body models](models/body.md) for parameter meanings.
Use generic rider inputs without personal data.

### Unimplemented extension points: protocol and desktop

`crates/euc-proto-begode/` supplies current frame and command handling.
`crates/euc-ble-radio/src/protocol.rs` connects protocol behavior for BLE replay.
`crates/euc-sim/src/compare.rs` uses Begode frame comparisons.
`crates/euc-sim/src/session.rs` assumes named tiltback variables and a Begode PWM scale.
Another protocol needs explicit caller changes and evidence.

The desktop also uses Begode decoders in `app/src-tauri/src/engine/phone.rs` and `host.rs`.
Review output device names and types in that directory.
`app/src-tauri/src/engine/firmware.rs` loads embedded profiles and output layouts.
`app/src-tauri/src/engine/scenarios.rs` embeds every shipped root scenario and scans root scenario files in development builds.
A nested scenario folder does not enter that scan.
Add release scenario registration separately when desktop support enters scope.

## 6. Pin and validate images

`FirmwareSource` supports `begode-api`, `manual`, and `local` entries in `wheel.toml`.
Each entry starts with `[[firmware]]` and a `kind` field.

| Kind | Required fields | Optional fields and limits |
|---|---|---|
| `begode-api` | `file`, `sha256`, `label` | `channel` defaults to `current`; current builds also require `type_code`. `date` is optional. |
| `manual` | `sha256`, `label` | Optional `date` and `hint`; the user supplies the file. |
| `local` | `path`, `label` | Repository-relative build path; no stock pin. |

Use a 64-digit SHA-256 digest of the exact app image.
Use a TOML date such as `2026-10-16` when the release date is known.
Do not use a fabricated digest to pass profile review.

### Image pin and gate checklist

- [ ] Confirm the official source and exact board revision before adding a pin.
- [ ] Compute the digest locally without publishing the image.
- [ ] Record the source, release label, date if known, and image boundaries.
- [ ] Add the reviewed pin to the correct wheel profile.
- [ ] Keep the downloaded or imported image outside the repository.
- [ ] Validate the image against the explicit wheel ID and profile source.
- [ ] Save every gate result and the flattened image digest.
- [ ] Refuse unknown images or unresolved layout failures.

The following commands illustrate the manual import path.
`euc` means the built CLI binary.
Replace `<wheel-id>` and private paths before use.
These commands do not access hardware.

```sh
euc --profiles profiles firmware import <wheel-id> /absolute/private/stock.bin
euc --profiles profiles --json image validate /absolute/private/stock.bin --wheel <wheel-id>
```

`euc fetch <wheel-id>` handles Begode API sources only.
`--archive` also requests archived builds.
`--accept-unpinned` only keeps unverified files; it does not authorize them.
The simulator's `stock` alias selects a current `begode-api` entry.
For a manual source, pass the image path through `--firmware`, `--a`, or `--b`.
The `local` entry does not automatically select a simulator image.

`crates/euc-image/src/gate.rs` checks load address, size, stack pointer, vectors, info block, and provenance.
ELF input must contain 32-bit little-endian ARM code.
Raw input assumes the profile's app base.
The stack pointer must have eight-byte alignment and lie above RAM base, at or below RAM end.
Reset must be nonzero.
Every nonzero handler must point inside the image with the Thumb bit set.

Our image needs a valid `.euc_info` block for the board.
Stock images need a matching pin when that block is absent.
A sealed block also needs matching length and CRC.
The gate does not authenticate signatures.
`euc image seal` fills the length and CRC of an unsigned development BIN image.
See [the image specification](spec/euc-info.md) for the format and planned signature field.

The simulator file path does not automatically run the complete image gate.
Run image validation explicitly before simulation evidence collection.
A successful gate establishes image structure and identity under the selected profile.
It does not establish safe control behavior or a working update path.

## 7. Port our firmware after stock simulation

First establish stock boot, telemetry, commands, and plant behavior in the simulator.
Then add our board and application code within the existing C architecture.

### Firmware port checklist

- [ ] Create `firmware/boards/<wheel-id>/board.h`, `link_app.ld`, and `CMakeLists.txt`.
- [ ] Create `firmware/apps/<wheel-id>/` with its application sources and `CMakeLists.txt`.
- [ ] Match `BOARD_ID`, board directory, image metadata, and profile board ID.
- [ ] Define startup vectors, linker assertions, storage protection, and stack boundaries.
- [ ] Connect the board target to the correct MCU port.
- [ ] Build separate `compat` and `clean` images.
- [ ] Keep ELF, raw image, map, and size report together outside public source files.
- [ ] Provide valid own-firmware probes before claiming variable comparisons.

`firmware/CMakeLists.txt` selects `EUC_BOARD`, but it always adds `ports/stm32f1` today.
`firmware/cmake/toolchain-arm-none-eabi.cmake` selects Cortex-M3 and software floating point.
`firmware/cmake/image.cmake` supplies `euc_add_image` and size checks.
Review `firmware/size-budget.toml` when adding a board.

This build example applies only after the new board and application files exist.
It uses the current Cortex-M3 toolchain.
Run it from the repository root.

```sh
cmake -S firmware -B firmware/build/example-wheel-compat -G Ninja \
  -DCMAKE_TOOLCHAIN_FILE="$PWD/firmware/cmake/toolchain-arm-none-eabi.cmake" \
  -DCMAKE_BUILD_TYPE=MinSizeRel -DCMAKE_INTERPROCEDURAL_OPTIMIZATION=ON \
  -DEUC_BOARD=example-wheel -DEUC_PROFILE=compat
cmake --build firmware/build/example-wheel-compat
```

Use a separate `example-wheel-clean` build folder with `-DEUC_PROFILE=clean` for the clean image.
These commands build target code without running host unit tests.

### Unimplemented extension points: firmware portability

There is no generic `firmware/hal/` directory in this checkout.
The current hardware interface is `firmware/ports/stm32f1/include/stm32f1/port.h`.
Those drivers fix several pins, timers, ADC ranks, serial ports, and device types.
Changing `board.h` alone cannot change all hardware wiring.
Keep reusable control code in `firmware/core/`.
Add or adapt port code and CMake selection for different hardware.

`firmware/ports/sim/` supplies the host SIL interface.
It does not prove target startup or peripheral compatibility.
`firmware/tools/gen_probes.py` and its image checks currently target the Begode application layout.
Extend that generation path for the new application.
Match the probe `layout` to the ELF `euc_probe_layout` symbol.
Do not reuse stock RAM addresses for our firmware.

## 8. Collect simulator and compatibility evidence

This section defines proposed verification for a wheel implementation.
It does not report completed runs.
Do not add or run tests without an explicit user request.
The integration owner must arrange the required scenario evidence before support enters release scope.

Use [the scenario guide](scenarios/writing-scenarios.md) and [the comparison guide](compare.md).
Give each new scenario its own name and wheel ID.
Keep existing Begode scenarios intact.
Use public synthetic inputs and generic rider settings.

### Required evidence checklist

- [ ] Record revision, profile origin, wheel ID, board revision, and firmware hashes.
- [ ] Record CPU backend, timing model (`fixed` or `flash`), CPI, exception timing, quantum size, sample rate, seed, and plant grid.
- [ ] Record settings, generic rider inputs, assist settings, terrain, and model uncertainties.
- [ ] Show stock boot, telemetry, safe command replies, resets, and calibration state.
- [ ] Show balance, acceleration, braking, and stopping with the live plant.
- [ ] Cover battery limits, heat, hall faults, sensor faults, and device communication faults.
- [ ] Cover numeric faults, watchdog behavior, service commands, and storage failures where applicable.
- [ ] Compare compat against the exact pinned stock build on identical scenario inputs.
- [ ] Explain each intentional clean change with a separate scenario and expected result.
- [ ] Record blockers, skipped checks, unpaired variables, and missing observations.
- [ ] Record unsupported cases as gaps rather than successful coverage.
- [ ] Check existing wheel behavior after shared code changes when verification receives authorization.

Start from the cases in `scenarios/`, without copying wheel-specific assumptions.
Use `stockmap.toml` only for the pinned build whose label matches `build`.
An empty map can load, but it cannot establish cutoff or internal-variable evidence.
Use matching semantic names in stock maps and our probes.
Confirm that each safety check observes the required state.

These proposed commands use paths that do not exist until the new implementation supplies them.
Run them only when verification receives authorization.

```sh
euc --profiles profiles --json sim run \
  --scenario scenarios/example-wheel-balance.toml \
  --firmware /absolute/private/stock.bin

euc --profiles profiles --json sim compare \
  --scenario scenarios/example-wheel-balance.toml \
  --a /absolute/private/stock.bin \
  --b /absolute/build/euc-fw-example-wheel.bin \
  --tolerance scenarios/compare/example-wheel-compat.toml \
  --out /absolute/private/compat-report.json
```

Keep the matching `.elf` beside our `.bin` for probes.
Pass an explicit file path when build output differs from the CLI's `target-compat` or `target-clean` conventions.
The `compat` and `clean` aliases search `firmware/build/target-*/euc-fw-<wheel-id>.bin` in the current folder and ancestors.

### Compatibility acceptance checklist

- [ ] Define channel and variable tolerances before accepting results.
- [ ] Review maximum error, RMS error, first divergence, and sample timing.
- [ ] Review both scenario verdicts separately.
- [ ] Review `checks_skipped` and `vars_unpaired` explicitly.
- [ ] Compare BLE frames and text behavior where the protocol supports comparison.
- [ ] Explain each shortened comparison window and excluded check.
- [ ] Preserve unresolved differences in the implementation gap report.

`same_checks` only compares checks that both runs judge.
The comparison's `ok` value does not require both scenario verdicts to pass.
Unlisted channels and variables receive no tolerance judgment unless a default applies.
`ble_identical` checks frames, not all text behavior.
Do not treat `ok` alone as sufficient evidence.

`scenarios/compare/compat-vs-stock.toml` records Begode limits and known differences.
Do not reuse those limits as evidence for another wheel.
`.github/workflows/compare.yml` currently fetches Begode stock and builds the existing target presets.
The integration owner must extend image selection, builds, and scenario routing for another wheel.

Scenario A/B comparison samples behavior over time.
It is not instruction-by-instruction equality between different firmware programs.
CPU lock-step checks in `crates/euc-armv7m/tests/lockstep.rs` serve a separate purpose.
Keep both forms of evidence distinct.

## 9. Stop at the bench boundary

M7 documentation and simulator evidence do not complete M8.
No step above authorizes hardware access, flashing, or riding.
The Flash workspace in `app/src/workspaces/Placeholders.tsx` is a placeholder.
An image gate result does not prove OTA recovery.

### Future M8 handoff checklist

- [ ] Obtain separate hardware authorization and a bench plan.
- [ ] Resolve unknown MCU, sensor, gate-driver, drive-mode, and dead-time facts.
- [ ] Establish a complete private SWD backup, including the bootloader, before the first flash.
- [ ] Establish the official stock image as the first flash under the flasher gates ([flasher](spec/flasher.md)).
- [ ] Demonstrate update fault recovery in simulation before any hardware update.
- [ ] Use a current-limited supply and keep the wheel off the ground.
- [ ] Compare bench traces with simulator traces before any ride consideration.

Keep backups and bench captures private.
Do not issue real service or drive-mode commands from this guide.
Do not infer hardware safety from estimated plant values.
Report remaining implementation gaps in an issue before release claims.
