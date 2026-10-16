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
 │  body (MuJoCo): wheel, shell, tire contact, human rider, terrain     │
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
- **Body.** MuJoCo models the wheel, the tire contact, the shell and a full human rider whose feet rest on the pedals.
  A rider controller leans to accelerate and steers to keep lateral balance; when the firmware cuts out, the rider falls.

## Wheels are data

A wheel profile (`profiles/<wheel>/`) describes the board (memory map, peripheral instances, pin wiring, sensors),
the plant (motor, battery, wheel size, masses), the body model and where its firmware comes from. A wheel can contain
several nodes on one clock (main controller, BMS packs, dash, BLE module), each running real firmware or a model.
New wheels, brands and processors add data and peripheral models, not engine changes.

## Firmware

`firmware/` is our own controller firmware, written in C11:

- `core/` holds control, protocol and safety logic with no hardware access and no global state;
- `hal/` declares the hardware interface; `ports/<mcu>/` implements it for a chip; `ports/sim/` for the simulator;
- `boards/<wheel>/` holds board constants and tuning; `apps/<wheel>/` composes and schedules the modules.

Two profiles build from the same code: **compat** reproduces the stock firmware's behavior, quirks included, so it can
be compared with the stock image in lock-step; **clean** fixes those quirks one by one, each fix backed by a scenario.

## Stock firmware

Stock images are never stored in this repository. `euc fetch <wheel>` downloads them from the vendor's own update
server on your machine and checks them against pinned sha256 hashes. See [the image spec](spec/euc-info.md) for how
our own images identify themselves.
