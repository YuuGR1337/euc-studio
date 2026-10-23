# Project status

Updated 2026-10-23 at commit `578a0e8` (W2).
This page says what works now, what is open, and where the evidence is.
Open work is tracked on the [EUC Studio Roadmap](https://github.com/users/tarikbc/projects/3) board; the issue
numbers below link to it. The milestone plan is in the [roadmap](roadmap.md).

## Current behavior

| Area | Current behavior | Open work |
| --- | --- | --- |
| CPU core | Own ARMv7-M (Cortex-M3) core, the only core in release builds; Unicorn (GPL-2.0) as a development oracle with lock-step and fuzz checks in CI; fixed timing (CPI 2.0 with exception costs) by default and a per-instruction flash timing model with `--timing flash` | Exact peripheral timestamps inside a quantum; the next MCU family, Cortex-M4F ([#54](https://github.com/tarikbc/euc-studio/issues/54)) |
| Engine | STM32F1 peripherals, IMU, BMS, display, LED strip and buzzer models; RTC correction, battery bounds, controller supply states, output telemetry, image seal, flash simulation | The IMU's digital low-pass filter ([#39](https://github.com/tarikbc/euc-studio/issues/39)) |
| Plant | Inverter with dead time, PMSM in dq form, halls, phase current sensing, battery packs, thermal models; Free Ride's HUD shows the firmware's PWM beside the simulated modulation | The modulation reaches 100 % near 68 km/h on a full pack while the firmware's PWM reads about 70 %, below the owner's report of the real wheel; calibration from ride logs ([#14](https://github.com/tarikbc/euc-studio/issues/14), [#36](https://github.com/tarikbc/euc-studio/issues/36), [#37](https://github.com/tarikbc/euc-studio/issues/37)) |
| Rider | Articulated rider `articulated_sagittal_v1` (the default; welded feet, lean from the ankles); the rigid baseline `rigid_ankle_v1`; opt-in contact feet `articulated_contact_v1`; opt-in lateral rider `articulated_lateral_v1` (L1 to L3: soft knees, legs on the side pads, turn posture); Free Ride drive mode (an acceleration ask); drag that follows the crouch by hand | External yaw and roll aids remain; balance by steering ([L4 #19](https://github.com/tarikbc/euc-studio/issues/19)), hip twist ([L5 #20](https://github.com/tarikbc/euc-studio/issues/20)), acceptance and the default decision ([L6 #21](https://github.com/tarikbc/euc-studio/issues/21)); roll stability in hard braking ([#17](https://github.com/tarikbc/euc-studio/issues/17)); kerb absorb ([#16](https://github.com/tarikbc/euc-studio/issues/16)) |
| Tyre and ground | A ring of 48 capsules from the profile's tyre torus; flat floor, a terrain strip (slope, bump, pothole, curb) or the city's height field; the ground ray sees the ground only | Soft tyre at real pressure ([#12](https://github.com/tarikbc/euc-studio/issues/12)) and its speed ([#13](https://github.com/tarikbc/euc-studio/issues/13)); rolling resistance from a ride log ([#15](https://github.com/tarikbc/euc-studio/issues/15)); a slide in very hard braking ([#38](https://github.com/tarikbc/euc-studio/issues/38)) |
| World | `euc-world`: a seeded, chunked, endless city (streets, curbs, sidewalks, plazas, potholes, materials) streamed into the physics, recordings, Ride, Compare and Free Ride, with daylight; a second ground window keeps a fallen rider on the city | A second streaming ring, a minimap, the wet look ([#30](https://github.com/tarikbc/euc-studio/issues/30)); ground detail, facades, trees ([#27](https://github.com/tarikbc/euc-studio/issues/27), [#28](https://github.com/tarikbc/euc-studio/issues/28), [#29](https://github.com/tarikbc/euc-studio/issues/29)) |
| Scenarios | 20 shipped scenarios ride the city (one on the flat floor on purpose); 6 world acceptance scenarios in `scenarios/world/`; compare tolerances re-baselined for the city | One rerun after the P1 bundle ([#18](https://github.com/tarikbc/euc-studio/issues/18)); smooth route steering ([#31](https://github.com/tarikbc/euc-studio/issues/31)); the riderless wheel's crawl ([#43](https://github.com/tarikbc/euc-studio/issues/43)) |
| Desktop | Ride, Tune, Compare, Recordings and Free Ride; scenario import; manual control; display, lights and buzzer sound; three cameras; quirk switches for the Ours-custom firmware in Tune and Compare. The Flash workspace is a preview | Runtime gain edits and raw memory writes; the flasher wizard ([#47](https://github.com/tarikbc/euc-studio/issues/47)) |
| Free Ride | A play mode in the city: no arming, a crash never pauses, R respawns; drive mode by default (lean mode selectable); Chase camera; the street-kit rider figure; sound from the simulation (motor, tyre, wind); a HUD with PWM and motor headroom; the wheel's power, handle, light, horn and pedal mode | Impact sounds ([#26](https://github.com/tarikbc/euc-studio/issues/26)); the lateral rider after L4 ([#22](https://github.com/tarikbc/euc-studio/issues/22)); feedback on drive mode ([#33](https://github.com/tarikbc/euc-studio/issues/33)) |
| SIL | Our firmware as a host library (`ours-sil`, Ours-custom in the studio) on the same machine, peripherals, plant and rider; every quirk switches at run time | Instruction timing (fixed estimated costs, no ISR load), a first CI run on Linux (`--defsym` link), Windows (refused with a clear message), and the library in the app bundle |
| Recordings | One `.eucrec` format across desktop, CLI and Python; capture, save, open, seek and playback; the run's rider model, aids, scenario text and world | Precise audio playback, complete parameter preset snapshots, broader storage-failure injection; raw BLE ride captures as recordings ([#34](https://github.com/tarikbc/euc-studio/issues/34)) |
| Python | `euc_studio` API, typed Session, phase results, reset, recording save and import | Linux and Windows package verification |
| Firmware | Compat and clean profiles boot and ride on the emulated MCU and as the SIL library; the compat speed-up cut its RTC tick from about 77,900 to 45,500 cycles and its interrupt load from about 95 % to 67 % (clean: 92 % to 68 %) | Compat's tick is still 1.2 times stock's and the 50 % charge regime still differs ([#42](https://github.com/tarikbc/euc-studio/issues/42)); standstill hunting from dead time ([#41](https://github.com/tarikbc/euc-studio/issues/41)); bench validation ([#55](https://github.com/tarikbc/euc-studio/issues/55)) |
| Flasher | Gated OTA flasher against the simulated wheel: gates 1-7, an outbound byte guard, YMODEM-1K, an emulated bootloader on the flash model, stock recovery, a JSONL byte log; `euc flash --sim`, `--dry-run` and `--campaign` | Image signing (gate 2, [#44](https://github.com/tarikbc/euc-studio/issues/44)), the 30 s emulator ride (gate 3, [#45](https://github.com/tarikbc/euc-studio/issues/45)), bootloader facts from the SWD dump, a listen-only test of the BLE central ([#46](https://github.com/tarikbc/euc-studio/issues/46)), and the app wizard; no real flash |
| Distribution | Local macOS package tooling and the development app | Signed bundles ([#48](https://github.com/tarikbc/euc-studio/issues/48)), docs site ([#50](https://github.com/tarikbc/euc-studio/issues/50)), browser demo ([#49](https://github.com/tarikbc/euc-studio/issues/49)) |

## Latest checks (W2, 2026-10-23)

These ran on `8305de4` plus the W2 change (`578a0e8`):

- fmt, workspace clippy with `-D warnings`, workspace tests, euc-world release tests, cargo deny, and the vendor guard
  with its deep scan.
- euc-python fmt and clippy.
- The app's cargo check, clippy and tests (122); typecheck, lint, prettier and Vitest (419).
- Playwright: the default suite (71), world-view with the real payload binary (24), free-ride-controls (28) and
  live-poses (10).

## City world (W2, 2026-10-23)

Every shipped scenario has a `[world]` table (seed 42, revision 2) at a spot that fits its test.
A file without `[world]`, and the scenario text inside older recordings, still rides the plane.
`full-effort-70kmh-stop-soc100` rides the flat floor on purpose (`[world] flat`): in the city its hard brake from
70 km/h grows the rigid rider's roll until the wheel falls, a rider-model limit that the P1 roll fix addresses.

- On stock, 18 of 20 verdicts are unchanged at quanta 4000, 3000 and 2000.
- `rider-steps-off-15kmh` is BORDERLINE: every profile still cuts at 17.26 s, but the end of the fall is chaotic
  ([#43](https://github.com/tarikbc/euc-studio/issues/43)).
- Compare (stock against compat) passes all 20 scenarios at all three quanta with re-baselined tolerances: roll
  0.25 deg (the city's height field gives a small roll), earlier pothole windows, and 1.6 V for the 50 % charge run
  (the known regime difference, already over 0.5 V on the flat floor).
- City runs take about 40 % longer than flat runs.

See [the world model](models/world.md) for the scenario spots and the numbers.

## Lateral rider (L1 to L3, 2026-10-22 to 2026-10-23)

`articulated_lateral_v1` is opt-in (`[rider] model` in a scenario, or `--rider-model`).

- L1: seven bodies with hip flexion, ab/adduction and rotation, knee, and ankle pitch and roll per leg; segment
  masses from published anthropometry; hips at their anatomical width; soft knees.
- L2: a tibia rotation per leg (14 joints), and legs that rest on the measured side pads with their knees, shins and
  calves (`crates/euc-sim/tests/lateral_pads.rs`).
- L3: the turn's posture (knees bend deeper, the inside knee swings out, the outside leg presses its pad), and a
  roll aid that leans wheel and rider as one machine and loads no joint.

The external aids still steer: L4 replaces the roll aid with balance by steering. Free Ride stays on the sagittal
rider until then. See [body physics](models/body.md).

## Free Ride (2026-10-22 to 2026-10-23)

- Play mode: opening Free Ride starts a city ride; the firmware boots on the stand in about 1.5 s of simulated time;
  input rides at once; a crash never pauses; R respawns at the last clear spot (a new run, the firmware boots again).
- Drive mode: the keys and the stick ask for an acceleration, and the rider leans for it
  (`tan(lean) = (a + drag) / g`); a brake ends in a clean stop.
- The rider figure is the street kit, EUC Studio's own look, built with code from EUC Thrills (MIT; see
  [NOTICE](../NOTICE)).
- Sound is synthesized from the simulator's signals on simulated time: motor, tyre per ground material, wind.
- The free-ride-controls browser suite covers the panel, the HUD and the controller against a fixture; no live ride
  with the native engine is part of it.

See the [Free Ride README](../app/src/workspaces/freeRide/README.md), [Free Ride design](design/free-ride.md) and
[Free Ride acceptance](models/free-ride-acceptance.md).

## Rider default, 2026-10-22

The articulated rider (`articulated_sagittal_v1`) is now the default for scenarios, the Ride workspace and the CLI.
Its feet stay welded by default. The rigid rider remains selectable per scenario or with `--rider-model`.
CLI recordings now carry the recording context with the model that actually ran.

- A rider-model defect was fixed: the ankle posture spring shared the lean with the hips and knees, so the centre of mass leaned about half as far as commanded. The body now leans from the ankles.
- Independent foot contact is the opt-in model `articulated_contact_v1`, with foot lift, per-foot loads and slip, lost-footing detection, and an external roll aid on the upper body that recordings name.
- Every scenario without a step-off now checks `rider_on_pedals`; only contact feet can fail it.
- Free Ride starts on `articulated_sagittal_v1` too. Since play mode (later on 2026-10-22) Free Ride has no rider choice.
- Contact feet stay opt-in. The lean controller does not keep the soles' centre of pressure under the feet, so the rider comes off the pedals in hard braking, the steep low-battery climb and the full-effort launches.
- The articulated step-off now has the stepping foot's push, as an internal force pair, so the riderless wheel tips as before.

The matrix covers 20 scenarios, three firmwares, three quanta and three rider models (540 runs, no halts).
All thirteen scenarios on the default rider keep every rigid-rider verdict.
Seven scenarios pin the rigid rider because their checks or documented numbers describe it.
The compat-vs-stock comparison passes on all twenty scenarios after three wider speed limits, each with a rider-model note.
See [the comparison](models/scenarios.md#rigid-and-articulated-riders-2026-10-22) and [articulated acceptance](models/articulated-acceptance.md#default-rider-and-foot-contact-2026-10-22).

## Flasher evidence (M6)

The fault campaign ran 320 seeded simulated flashes with up to three injected faults each.
None ended unrecoverable: 108 flashed, 35 restored the stock image, and 177 stopped safely.
Of the safe stops, 138 were gate refusals, 26 left the old application running, and 13 stopped and then took a clean stock restore.
Two further campaigns of 2,000 runs each also had no unrecoverable run.
The verdicts rest on the emulated bootloader's assumptions; see [the flasher spec](spec/flasher.md).
No build can flash a real wheel. Real writes need a feature no build enables and a per-wheel override.

## SIL evidence (M5)

The SIL library runs our compat and clean firmware within the compat-vs-stock tolerance of the emulated images.
This holds on balance-60s, ride-20kmh-stop and hall-loss-while-riding (to the fault), with the default rider.
The largest speed difference is 0.63 km/h (clean, ride-20kmh-stop; limit 0.8 km/h).
Every scenario check gives the same verdict on both sides.
The SIL has no instruction timing, so the balance acts up to about 1 ms sooner than on the image.
See [SIL](sil.md#validation) for the numbers and the remaining differences.

## Firmware evidence (2026-10-21 to 2026-10-22)

These runs predate the city world (W2): the scenarios rode the flat floor.

The final matrix contains 189 runs across stock, compat, and clean at three CPU quanta.
It includes all 20 shipped scenarios and a local 3 cm pothole.
Raw scenario expectations passed in 133 runs.
No run reported an emulator halt.

All ordinary ride guards, 2/3/5 cm potholes, step-off cases, and total hall-loss checks passed.
The 60 additional inductance cases produced 43 raw passes and no emulator halts.
A raw failure can expose an intended physical limit or a firmware difference.
It does not automatically indicate an emulator failure.

All 20 shipped-scenario comparisons passed their existing quantum-4000 windows.
Expanded windowed checks passed 35 of 36 cases.
SOC50 at quantum 3000 retained a 0.53842 V difference against the 0.5 V limit.
All three full-run SOC50 diagnostic comparisons failed.
No tolerance, window, same_checks rule, or C firmware behavior changed to hide these results.

See [full-effort acceptance](models/full-effort-acceptance.md) and [clean cutoff validation](models/clean-cutoff-next.md).
The current comparison windows do not prove complete stock/compat equivalence.

The later [SOC50 investigation](models/soc50-investigation.md) runs on `soc50-investigation`, from integration commit `70a827e`.
All six first cutoffs come from the balance-error detector.
The launch failure samples similar PWM transients approximately 80 µs apart.
Observation traces preserve the accepted results.
The earlier timing and high-speed trajectory causes remain open.
No production code, firmware image, physical input, or acceptance limit changes in this investigation.
The [follow-up](models/soc50-high-speed-investigation.md) confirms the launch Hall transition and rules out the actuator cap in two stock counterfactuals.
Sixteen additional quantum studies expose timing sensitivity down to one instruction per quantum.
Both one-instruction cases cut before 70 km/h; full-run equivalence remains open.

The [third SOC50 study](models/soc50-full-run-equivalence.md) (2026-10-22, base `63801a2`, rigid rider) finds the systematic cause.
Compat performs stock's arithmetic, but its RTC tick takes about 75,000 cycles against stock's 37,000.
Its libgcc double divide costs 528 instructions per call against stock's 75.
Compat therefore publishes the motor command about 0.6 ms later in each tick.
It stays voltage-limited longer at high speed and builds more PWM tiltback and pitch oscillation (p ≤ 0.001 over 12-16 distinct runs).
A private causal test that swaps only the tick cost swaps the two regimes, and also the SOC20 cutoff times.
The SOC50 cutoff time itself is chaotic: numerical perturbations alone spread it by more than 2 s.
No firmware image, tolerance or window changed in that study; the firmware speed-up followed.

The [speed-up](models/soc50-full-run-equivalence.md#the-speed-up-2026-10-22-not-yet-sufficient) follows (base `0575f14`).
An own double divide and float comparisons replace libgcc's through the linker's `--wrap`, and `euc_sqrt` becomes an exact integer root.
The LED strip and the FOC step lose work that changes no output, and the strip renders in the main loop from a queue of tick inputs, as stock's does.
Each change is checked bit for bit against the code it replaces: 1 M operand pairs per routine on the Cortex-M3 core, exhaustive checks where the input space allows, and 17.7 M strip calls.
The strip queue dropped no tick in 72 runs, and the SIL library gives the same ride before and after.
The full firmware gate, the CI compare loop and sil's six SIL compares pass.
Compat's tick drops to about 45,500 cycles (stock 37,700) and the FOC handler to 632 cycles (stock 671).
Over 29 runs per group the SOC50 regime still differs from stock (p = 0.001-0.005); at stock's tick cost it does not.
SOC20's cutoffs now overlap stock's range, and SOC100 agrees at quanta 4000, 3000 and 2000 on macOS.
Both comparison exceptions stay: SOC50's verdicts still differ at quantum 3000, and CI checks SOC100 on Linux when this lands.
Before the speed-up both compat and clean ran at 90-98 % interrupt load, a real-hardware risk of main-loop starvation, not only a simulator effect.

The [stage B timing model](models/cpu-timing.md#flash-timing-stage-b) is selectable with `--timing flash`.
Against the owner's wheel it gives a 2.515 s breathing period (video 2.1 ± 0.2 s; fixed timing 2.234 s).
Both timings give 307.45 ms BLE groups (capture 309.6 ms).
The fixed timing therefore stays the default.
On stock, both timings give the same 15 passes and 5 expected failures at quanta 4000, 3000 and 2000.

## Checks of the 2026-10-21 integration

- Full release Rust workspace: 620 passed, zero failed, one live-network test ignored.
- Native app library: 108 passed, zero failed, 26 optional checks ignored.
- Four cached-stock Tune checks passed.
- The cached-stock scenario recording-context regression passed.
- Nine cached-stock Free Ride checks passed for rigid and articulated models.
- Frontend unit suite: 283 passed.
- Python suite: 23 passed against the rebuilt native wheel.
- Strict Clippy, TypeScript, ESLint, dependency policy, and vendor checks passed.
- Vendor guard: 17 tests passed; 687 files passed the deep scan against three private local images.
- Free Ride controls: all 16 Chromium/WebKit checks passed after the firmware workload finished.
- Keyboard/manual controls and scenario selection: seven browser checks passed.
- Compare and screen checks passed after correcting the stale Compare workspace selector.

Earlier UI checks triggered the unchanged 250 ms input watchdog under heavy CPU load.
The final control run passed without relaxing the watchdog or test assertions.
The unchanged 1.5x engine speed requirement also passed in the final serial suite.

## Native application checks (2026-10-21)

The actual desktop app saved a captured stock run into Downloads.
The native file picker opened that file, and the main view replayed it.
The display, lights, phone frames, telemetry, and event timeline used recorded data.
Returning to simulation restored the paused live run.

Python read that same file.
It retained 1,575 samples, 20 events, 406 BLE frames, and 516 output changes.
Its scenario context identified the rigid baseline.
No truncation or skipped items occurred.

The live app also created an articulated Free Ride and reached its ready state.
The selected model appears in the controls.
Only one current development instance remains open.

## Open limits

Suspension geometry, tyre behavior, motor values and rider parameters still include estimates.
The rocker curve has two readings ([suspension linkage](hardware/suspension-linkage.md)).
External yaw and roll aids still hold the rider; independent pedal contact and the lateral rider are opt-in, with
their limits in [body.md](models/body.md).
Simulation results apply to their stated model, timing, firmware and duration. They do not authorize physical
flashing, and they do not establish a real wheel's operating limits.
Bench work still requires its separate procedure and recovery evidence ([#55](https://github.com/tarikbc/euc-studio/issues/55)).
