# Architecture

EUC Studio runs real electric-unicycle controller firmware inside a simulated wheel and rider. The same firmware image
that would run on the wheel's microcontroller runs here, instruction by instruction, against models of every part it
talks to.

```
 firmware image (stock, or ours)
        │
 ┌──────▼──────────────────────────────────────────────────────────────┐
 │ Engine (Rust, deterministic, one clock in MCU cycles)                │
 │                                                                      │
 │  CPU core ──── bus ──── peripheral models (timers, ADC, UART, SPI,   │
 │  (ARMv7-M)              DMA, flash, RTC, ...) ─── devices (IMU, BMS, │
 │                                                    display, LEDs)    │
 │                   │ PWM duty, enable          ▲ currents, halls,     │
 │                   ▼                           │ voltages, IMU        │
 │  plant: inverter → motor (PMSM) → battery and thermal models         │
 │                   │ torque                    ▲ wheel speed          │
 │                   ▼                           │                      │
 │  body (MuJoCo): wheel, shell, tyre, human rider, ground or city      │
 └──────┬───────────────────────────────────────────────────────────────┘
        │ state stream, events, recordings
 ┌──────▼───────────────┐   ┌─────────────────────┐   ┌──────────────────┐
 │ EUC Studio (Tauri)   │   │ CLI `euc`            │   │ Python bindings   │
 │ 3D, scopes, inspector│   │ scenarios, CI, flash │   │ notebooks, sweeps │
 └──────────────────────┘   └─────────────────────┘   └──────────────────┘
```

## Engine

- **One clock.** Time is a 64-bit count of MCU clock cycles. Peripherals schedule events on it; nothing reads the wall
  clock, so every run is reproducible from its inputs and seed.
- **Rates.** The CPU runs in short quanta up to the next event. The electrical plant steps on a 5 µs grid; the body
  physics steps every 0.5 ms; the IMU is sampled when the firmware reads it; the app receives snapshots at 60-120 Hz.
- **CPU core.** The engine talks to the CPU through a `Cpu` trait and owns the exception model, so cores are
  interchangeable. Releases use our own ARMv7-M (Cortex-M3) interpreter. Unicorn is an optional development backend
  and test oracle only; it is never part of a release.
- **Peripherals and devices** are Rust models with register-level behavior. Registers that are not modeled yet are
  traced instead of failing, so a new firmware shows what it needs.
- **Plant.** Inverter with dead time, PMSM motor in dq form, hall sensors, phase current sensing, battery packs with
  internal resistance, thermal models.
- **Body.** MuJoCo models the wheel, the tyre contact, the shell with its rear suspension, and a human rider on the
  pedals. Four rider models exist: the articulated rider (the default), a rigid baseline, and the opt-in contact-feet
  and lateral riders. A rider controller leans to accelerate and steers; external roll and heading aids still help it
  today. When the firmware cuts out, the rider falls. See [body physics](models/body.md).
- **Ground and world.** A run rides a flat floor, a terrain strip (slope, bump, pothole, curb) or the city of
  `euc-world`: a seeded, chunked, endless city that streams into MuJoCo as a height-field window around the wheel plus
  a pool of solid boxes (curbs, steps), and into recordings and the studio's 3D view. See [the world](models/world.md).
- **SIL.** Our firmware can also run as a host library (`libeuc_sil`) in place of the emulated image, on the same
  machine, peripherals, plant and rider, with every quirk switched at run time. See [SIL](sil.md).

## Wheels are data

A wheel profile (`profiles/<wheel>/`) describes the board (memory map, peripheral instances, pin wiring, sensors),
the plant (motor, battery, wheel size, masses), the body model and where its firmware comes from. A wheel can contain
several nodes on one clock (main controller, BMS packs, dash, BLE module), each running real firmware or a model.
New wheels, brands and processors add data and peripheral models, not engine changes.

## Firmware

`firmware/` is our own controller firmware, written in C11:

- `core/` holds control, protocol and safety logic with no hardware access and no mutable global state;
- `ports/stm32f1/` drives the MCU at register level; `ports/sim/` is the SIL library's port on the engine's board bus;
- `boards/<wheel>/` holds board constants, the memory map and tuning; `apps/<wheel>/` composes and schedules the
  modules; `config/quirks.def` lists the stock behaviors the compat profile keeps.

Two profiles build from the same code: **compat** reproduces the stock firmware's behavior, quirks included, so it can
be compared with the stock image in lock-step; **clean** fixes those quirks one by one, each fix backed by a scenario.

See [firmware/README.md](../firmware/README.md) for the build presets and the layer rules.

## Flasher

`euc-flasher` is the gated OTA flasher (M6): seven gates, an outbound byte guard (`euc-ble`), YMODEM-1K
(`euc-ymodem`), stock recovery and a JSONL byte log. Today it runs end to end only against a simulated wheel with an
emulated bootloader; no build flashes a real wheel. See [the flasher spec](spec/flasher.md) and
[SAFETY.md](../SAFETY.md#flasher-rules).

## Stock firmware

Stock images are never stored in this repository. `euc fetch <wheel>` downloads them from the vendor's own update
server on your machine and checks them against pinned sha256 hashes. See [the image spec](spec/euc-info.md) for how
our own images identify themselves.

## Adding a wheel

Use [Adding a wheel](adding-a-wheel.md) for the second-wheel implementation sequence and evidence checklists.
The guide identifies current CPU, peripheral, firmware, protocol, and image limits.
The architecture above includes planned interfaces.
The guide distinguishes those plans from current implementation.

## Where the code lives

| Crate | Role |
| --- | --- |
| `euc-core` | Master clock, event scheduler, bus, CPU and peripheral traits |
| `euc-armv7m` | Our Cortex-M3 (ARMv7-M Thumb-2) interpreter, the CPU that ships |
| `euc-unicorn` | Unicorn (QEMU) as a CPU backend: development and CI oracle only, never shipped (GPL-2.0) |
| `euc-stm32f1` | Register-level models of the STM32F1 high-density peripherals |
| `euc-devices` | Off-chip devices: IMU, smart BMS, display, LED strip, buzzer |
| `euc-display` | Which LEDs of the front display and tail light the firmware lights, and where they are |
| `euc-plant` | Electrical plant: inverter, PMSM, halls, current and voltage sensing, battery, thermal |
| `euc-physics` | MuJoCo mechanics: tyre, shell, suspension, rider models, IMU |
| `euc-world` | The seeded, chunked city for Free Ride and `[world]` scenarios |
| `euc-sim` | The simulated controller: memory map, run loop, sessions, scenarios, compare, SIL, recordings |
| `euc-telemetry` | Channel schema, snapshots, events, the binary batch format and `.eucrec` recordings |
| `euc-profile` | Wheel profile schema and loader (`profiles/<wheel>/`) |
| `euc-image` | The image gate: size, vectors, `.euc_info` block, provenance, sealing |
| `euc-fwsource` | Fetch, verify (sha256 pins) and cache stock firmware; manual import |
| `euc-proto-begode` | The Begode (Gotway) BLE protocol: frames, commands, text replies |
| `euc-ble-radio` | A wheel's BLE link on a real radio (macOS): GATT profiles, capture replay |
| `euc-ble`, `euc-ymodem`, `euc-flash-sim`, `euc-flasher` | The OTA path: byte guard, YMODEM, flash model, gated flasher |
| `euc-cli` | The `euc` command |
| `euc-python` | The Python bindings (its own cargo workspace) |

The desktop app's native side (`app/src-tauri`, its own cargo workspace) hosts the same engine for the studio.

## Engineering rule

Fix causes. Do not hide missing behavior with simulated UI responses, delayed replay, or a misleading success state.
Use one authoritative source for each behavior. Keep presentation interpolation separate from raw simulation measurements.
Preserve bounded resource use, cancellation, and source identity through every state change.
If a feature lacks required evidence or implementation, state that limit and complete the missing work.
A build result does not establish runtime correctness or physical accuracy.

Controller rail switching is described in [Controller supply](models/controller-supply.md). The pack and physical world continue while the controller is off.
