# Integration status

Updated 2026-10-21 for the local m7-human-rider branch, and on 2026-10-22 for the rider default.
The integration combines the accepted Claude and Human work from base aa129c4.
Since 2026-10-22 the articulated rider is the default scenario rider; independent foot contact is opt-in.
The rigid model remains selectable as the controlled baseline; seven scenarios pin it on purpose.
See [the rider change](#rider-default-2026-10-22) below.

## Integrated behavior

| Area | Current behavior | Remaining work |
| --- | --- | --- |
| Rider | Six-joint articulated rider (the default), lean from the ankles, opt-in independent foot contact, lean-to-turn, command delay, and high-speed yaw damping | Centre-of-pressure-aware lean control (then contact feet by default), lateral balance without aids, and measured human calibration |
| Desktop | Ride, Tune, Claude Compare, scenario import, manual controls, generated maps, native sound, lights, and buttons | Runtime gain edits and runtime quirk toggles |
| Recordings | One .eucrec format across desktop, CLI, and Python; capture, save, open, seek, and playback | Precise audio playback and complete historical profile snapshots |
| Recording integrity | Limits apply before decoding; duplicate indexes, inconsistent counts, and invalid sample bounds fail | Broader storage-failure injection |
| Recording context | Actual rider model, external aids, scenario text, and generated Free Ride map; CLI scenario recordings too | Complete parameter preset snapshots |
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

## Rider default, 2026-10-22

The articulated rider (`articulated_sagittal_v1`) is now the default for scenarios, the Ride workspace and the CLI.
Its feet stay welded by default. The rigid rider remains selectable per scenario or with `--rider-model`.
CLI recordings now carry the recording context with the model that actually ran.

- A rider-model defect was fixed: the ankle posture spring shared the lean with the hips and knees, so the centre of mass leaned about half as far as commanded. The body now leans from the ankles.
- Independent foot contact is the opt-in model `articulated_contact_v1`, with foot lift, per-foot loads and slip, lost-footing detection, and an external roll aid on the upper body that recordings name.
- Every scenario without a step-off now checks `rider_on_pedals`; only contact feet can fail it.
- Free Ride starts on `articulated_sagittal_v1` too; the rigid baseline and contact feet stay selectable.
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

## Open limits

Suspension geometry, tire behavior, motor values, and rider parameters still include estimates.
The rocker curve remains open. Independent pedal contact is in place as an option; its limits are in [body.md](models/body.md#the-articulated-rider-default).
Simulation results do not authorize physical flashing.
Bench work still requires its separate procedure and recovery evidence.
