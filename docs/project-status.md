# Integration status

Updated 2026-10-21 for the local m7-human-rider branch.
The integration combines the accepted Claude and Human work from base aa129c4.
The rigid model remains the controlled scenario baseline.
The articulated model is available in Free Ride and explicit scenario configuration.

## Integrated behavior

| Area | Current behavior | Remaining work |
| --- | --- | --- |
| Rider | Six-joint articulated rider, physical damping, lean-to-turn, command delay, and high-speed yaw damping | Independent foot contact and measured human calibration |
| Desktop | Ride, Tune, Claude Compare, scenario import, manual controls, generated maps, native sound, lights, and buttons | Runtime gain edits and runtime quirk toggles |
| Recordings | One .eucrec format across desktop, CLI, and Python; capture, save, open, seek, and playback | Precise audio playback and complete historical profile snapshots |
| Recording integrity | Limits apply before decoding; duplicate indexes, inconsistent counts, and invalid sample bounds fail | Broader storage-failure injection |
| Recording context | Actual rider model, external aids, scenario text, and generated Free Ride map | Complete parameter preset snapshots |
| Python | Existing euc_studio API, typed Session, phase results, reset, recording save, and recording import | Linux and Windows package verification |
| Engine | RTC correction, battery bounds, supply-state handling, output telemetry, image seal, and flash simulation | Complete OTA transport and recovery proof |
| Firmware | Earlier approved clean cutoff and headroom changes remain | System-level stock/compat equivalence beyond limited comparison windows |
| Distribution | Local package tooling and development app | Final release packaging, public signing, and notarization |

## Checks completed

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

## Native application checks

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

## Firmware evidence

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

## Open limits

Suspension geometry, tire behavior, motor values, and rider parameters still include estimates.
The rocker curve and independent pedal contact remain open.
Simulation results do not authorize physical flashing.
Bench work still requires its separate procedure and recovery evidence.
