# Roadmap

| Milestone | Goal | Done when |
|---|---|---|
| **M0 Foundation** | Repo, license, vendor guard, CI; engine, firmware and app skeletons; stock firmware fetch | The guard blocks vendor material; `euc fetch begode-extreme` verifies the pinned build; our empty firmware image passes `euc image validate`; the app shell and design system run |
| **M1 Stock boots** | Engine port of the proven prototype emulator | The stock Begode Extreme firmware boots, sends telemetry and answers app commands, at real time or faster |
| **M2 Stock rides in 3D** | Motor, inverter, battery and body physics; 3D viewport | The stock firmware balances, rides to 20 km/h and stops, in 3D |
| **M3 Own CPU core** | ARMv7-M interpreter, conformance tests | Matches the reference emulator instruction by instruction on the stock firmware; releases ship without it |
| **M4 Human rider** | Full rider model and controller, recordings, Python bindings, scenarios | A scripted ride works; a cutout throws the rider; our firmware's compat build matches stock |
| **M5 Our firmware on target** | Complete C firmware in compat and clean profiles | Our image boots in the simulated MCU and matches stock on the replay suite; every protection reproduces |
| **M6 Flasher** | BLE OTA flasher with safety gates and a simulated bootloader | Hundreds of simulated flashes with injected faults all end in success or a recoverable state |
| **M7 Release** | Signed desktop builds, docs site, browser demo, a second wheel | Public release |
| **M8 Bench** | Our firmware on a real controller on the bench | Matches the simulator traces; nobody rides before this |

Other brands, boards and processors (Cortex-M4F, RISC-V, other EUC protocols) follow the same path: a wheel profile,
peripheral models for its MCU, a protocol plugin, and the onboarding guide in `docs/adding-a-wheel.md` (available now).

## Current checkpoint

The 2026-10-21 integration combines the rider, desktop, recording, Python, and engine work.
The [request ledger](project-status.md) records the checks and remaining limits.
The articulated rider works in Free Ride. Scripted comparisons retain the rigid baseline by default.
The local integration does not complete every M3, M4, or M5 acceptance criterion.
M6, M7, and M8 remain separate milestones.

## Next sequence

1. Review the integrated checkpoint, run hosted CI on all three operating systems, and merge the tested baseline.
2. Close M3–M5 evidence gaps with fixed model inputs. Resolve full-run stock/compat differences, including the SOC50 case.
3. Complete rider and physics validation. Add independent foot contact, reduce external aids, and calibrate suspension and motor inputs.
4. Complete the developer controls: bounded parameter edits, presets, and scenario sweeps. The SIL host and runtime quirks are in place ([SIL](sil.md)).
5. Complete M6 transport and recovery. Run at least 100 simulated flashes with injected faults.
6. Complete M7 distribution: signed bundles, documentation site, browser demo, and a second working wheel.
7. Start M8 under its separate bench procedure. Obtain the full SWD backup before hardware firmware tests.

M4 and M5 work can proceed in separate branches against the fixed baseline.
Changes to the physical model require renewed scenario evidence.
A limited comparison window does not establish full-run firmware equivalence.
The current SOC50 comparison differences remain documented in [full-effort acceptance](models/full-effort-acceptance.md).
The [SOC50 investigation](models/soc50-investigation.md) identifies all six first cutoff detectors and the launch sample's PWM timing difference.
Earlier divergence causes and full-run equivalence remain open.
The [high-speed follow-up](models/soc50-high-speed-investigation.md) separates the confirmed Hall trigger, actuator-cap experiment, and numerical timing sensitivity.

## Model limits

The [human and Free Ride plan](models/human-and-free-ride.md) defines the remaining rider and map increments.
Current bodies and joints are estimates. Feet remain welded to the pedals.
External yaw and roll aids remain visible in the model configuration.
Suspension geometry, motor values, and tire behavior still need measurements.
Simulation results apply to their stated model, timing, firmware, and duration.
They do not establish a real wheel's operating limits.

See [recording acceptance](models/recording-acceptance.md) for capture, replay, and import limits.
See [coverage](models/coverage-matrix.md) for the broader physical validation requirements.
