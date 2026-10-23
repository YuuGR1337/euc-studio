# Roadmap

Updated 2026-10-23. The open work of every milestone is on the public
[EUC Studio Roadmap](https://github.com/users/tarikbc/projects/3) board, as issues grouped by
[milestone](https://github.com/tarikbc/euc-studio/milestones). [Project status](project-status.md) says what works now
and where the evidence is.

## Milestones

| Milestone | Goal | Done when | State |
| --- | --- | --- | --- |
| **M0 Foundation** | Repo, license, vendor guard, CI; engine, firmware and app skeletons; stock firmware fetch | The guard blocks vendor material; `euc fetch begode-extreme` verifies the pinned build; our empty firmware image passes `euc image validate`; the app shell and design system run | Done |
| **M1 Stock boots** | Engine port of the proven prototype emulator | The stock Begode Extreme firmware boots, sends telemetry and answers app commands, at real time or faster | Done |
| **M2 Stock rides in 3D** | Motor, inverter, battery and body physics; 3D viewport | The stock firmware balances, rides to 20 km/h and stops, in 3D | Done; measured inputs open |
| **M3 Own CPU core** | ARMv7-M interpreter, conformance tests | Matches the reference emulator instruction by instruction on the stock firmware; releases ship without it | Done |
| **M4 Human rider** | Full rider model and controller, recordings, Python bindings, scenarios | A scripted ride works; a cutout throws the rider; our firmware's compat build matches stock | Most done; the rider still leans on external aids |
| **M5 Our firmware on target** | Complete C firmware in compat and clean profiles | Our image boots in the simulated MCU and matches stock on the replay suite; every protection reproduces | Most done; the 50 % charge regime differs |
| **M6 Flasher** | BLE OTA flasher with safety gates and a simulated bootloader | Hundreds of simulated flashes with injected faults all end in success or a recoverable state | Simulator done (320 + 2 x 2,000 runs); gates 2 and 3, the wizard and real-link checks open |
| **M7 Release** | Signed desktop builds, docs site, browser demo, a second wheel | Public release | Started |
| **M8 Bench** | Our firmware on a real controller on the bench | Matches the simulator traces; nobody rides before this | Planned |

Other brands, boards and processors (Cortex-M4F, RISC-V, other EUC protocols) follow the same path: a wheel profile,
peripheral models for its MCU, a protocol plugin, and the onboarding guide in [Adding a wheel](adding-a-wheel.md).
Milestone [Next wheels and brands](https://github.com/tarikbc/euc-studio/milestone/10) collects that work.

## Current tracks

| Track | Milestone | What it delivers | State |
| --- | --- | --- | --- |
| W2 Scenarios in the city | [W2](https://github.com/tarikbc/euc-studio/milestone/1) | The shipped scenarios ride the generated city instead of a flat plane | Done on 2026-10-23 ([#11](https://github.com/tarikbc/euc-studio/issues/11)) |
| P1 Physics realism bundle | [P1](https://github.com/tarikbc/euc-studio/milestone/2) | Soft tyre at real pressure, calibrated motor and rolling resistance, kerb absorb, roll stability, then one rerun of every scenario | In progress ([epic #57](https://github.com/tarikbc/euc-studio/issues/57)) |
| Lateral rider L4-L6 | [L4-L6](https://github.com/tarikbc/euc-studio/milestone/3) | The rider balances and turns with its own body; the external aids go | L1-L3 done ([epic #58](https://github.com/tarikbc/euc-studio/issues/58)) |
| Free Ride polish | [Free Ride](https://github.com/tarikbc/euc-studio/milestone/4) | Sound, city look and feel of the play mode | Play mode, drive mode, camera, daylight, sound and HUD done |
| Calibration and real-world data | [Calibration](https://github.com/tarikbc/euc-studio/milestone/5) | Tools and measurements that tie the simulator to real wheels | Open ([epic #59](https://github.com/tarikbc/euc-studio/issues/59)) |
| Firmware findings | [Findings](https://github.com/tarikbc/euc-studio/milestone/6) | Firmware behavior found in the simulator that needs a decision or a fix | Open |

## Next sequence

1. Finish the P1 physics bundle and rerun every scenario once on stock, compat and clean
   ([#18](https://github.com/tarikbc/euc-studio/issues/18)).
2. Lateral rider L4 to L6: balance by steering, hip twist at walking pace, acceptance, then decide the default rider
   and switch Free Ride to it.
3. Calibrate the plant and the tyre from real ride logs and bench measurements (the
   [real-wheel data](https://github.com/tarikbc/euc-studio/issues/new?template=real-wheel-data.yml) form collects them).
4. Resolve the remaining stock/compat differences, including the 50 % charge case
   ([#42](https://github.com/tarikbc/euc-studio/issues/42)).
5. Complete M6: signed images (gate 2), the 30 s emulator ride (gate 3), a listen-only test of the BLE central, and
   the studio's flasher wizard ([flasher spec](spec/flasher.md)).
6. Complete M7: signed bundles, documentation site, browser demo and a second working wheel.
7. Start M8 under its separate bench procedure. Obtain the full SWD backup before hardware firmware tests.

## Rules for the model

Changes to the physical model require renewed scenario evidence.
A limited comparison window does not establish full-run firmware equivalence; the SOC50 differences remain documented
in [full-effort acceptance](models/full-effort-acceptance.md) and the
[SOC50 studies](models/soc50-full-run-equivalence.md).
Current bodies, joints, tyre, suspension and motor values are estimates until measured.
External yaw and roll aids remain visible in the model configuration and in recordings.
Simulation results apply to their stated model, timing, firmware and duration.
They do not establish a real wheel's operating limits.

See [recording acceptance](models/recording-acceptance.md) for capture, replay and import limits, and
[coverage](models/coverage-matrix.md) for the broader physical validation requirements.
