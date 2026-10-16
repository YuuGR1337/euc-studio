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
peripheral models for its MCU, a protocol plugin, and the onboarding guide in `docs/adding-a-wheel.md` (coming with M7).
