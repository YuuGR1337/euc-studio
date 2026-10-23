# Software in the loop (SIL)

**Status: milestone M5.** Our firmware (`firmware/`: core, the Begode Extreme app and the host port in
`firmware/ports/sim`) builds as a host library, `libeuc_sil`. The engine runs it as a CPU backend of the same
machine an image runs on: the same peripheral models, plant, rider, board logic and BLE link. Every quirk of
[`firmware/config/quirks.def`](../firmware/config/quirks.def) switches at run time, so a run can keep or fix any
single stock behaviour without a new image.

## Using it

Build the library (CMake 3.25+, Ninja, a host C compiler):

```sh
cd firmware && cmake --workflow --preset host-sil   # build/host-sil/libeuc_sil.{dylib,so}
```

The CLI finds it in `firmware/build/host-sil/` in the current folder or above:

```sh
euc sim run --scenario scenarios/balance-60s.toml --firmware ours-sil --quirks clean
euc sim run --scenario scenarios/ride-20kmh-stop.toml --firmware ours-sil --quirks clean+ASIN_ACCEL
euc sim compare --scenario scenarios/ride-20kmh-stop.toml --a compat --b ours-sil:compat
euc sim compare --scenario scenarios/ride-20kmh-stop.toml --a ours-sil:clean --b ours-sil:clean-NO_WATCHDOG
euc sim serve begode-extreme --firmware ours-sil --quirks compat-GYRO_500DPS_CLIP
```

`--firmware` also takes a library file (`.dylib`, `.so`). `ours-sil:QUIRKS` names the quirks inside the
firmware value, which `euc sim compare` needs for two SIL runs with different quirks; `--quirks` applies to every
SIL run that does not name its own. Without either, a SIL run has the compat quirks.

The library runs on macOS and Linux. Windows is not supported yet: the CLI and the studio's Ours-custom say
"ours-sil is not supported on Windows yet" (`euc_sim::sil::UNSUPPORTED`), and the rest of the engine and the app
work as before.

Quirk text: a base, then quirks to add or remove, by commas or written one after the other.

| Text | Mask |
|---|---|
| `compat`, `clean` | the profile's quirks |
| `none`, `all` | no quirk, every quirk |
| `0x1f`, `31` | the bits (bit *i* is the *i*-th entry of `quirks.def`) |
| `clean+ASIN_ACCEL`, `compat,-NO_WATCHDOG` | a base with quirks added (`+`) or removed (`-`) |
| `ASIN_ACCEL,KEIL_FTZ` | those quirks only (no base: none) |

Names are case-insensitive. Reports, recordings and labels write a mask the same way, from the profile it is
closest to (`ours-sil (clean+ASIN_ACCEL)`), so a label pasted back into `--quirks` gives the same run. JSON output
names it in `firmware.quirks`; `cpu` and `timing` say `sil`.

In the studio (`pnpm tauri dev`), the firmware choice **Ours-custom** ("Ours (custom quirks)") runs the library:

- **Tune → Quirks** lists every quirk with a switch (on keeps the stock behaviour, off fixes it), starts from the
  running firmware's quirks, offers compat and clean presets, and restarts the run with Ours-custom and the
  switches ("Restart with these quirks"). The run bar's Ours-custom runs the last quirks again.
- **Compare → Quirks** switches the quirks of each side set to Ours-custom; the next run takes them, and a change
  after a run shows "Run again to apply".

The app looks for the library where it looks for our images: `EUC_FIRMWARE_DIR` (`<dir>/host-sil/` or
`<dir>/sil/`), the bundle's `firmware/` resources, and in development builds this checkout's `firmware/build`.

## How it runs

```
 engine (Rust, euc-sim)                                   libeuc_sil (C, one copy per power-on)
 Machine::run ─ quantum ─▶ SilCpu::run_for ─ resume ─▶ coroutine: euc_sil_run()
   events, timers, plant                                   app_init(), then for (;;) app_main_loop_pass()
   exception unit takes IRQs                                  │ every register access / spin
   between quanta                    ◀── read/write/spin ─────┘ (struct euc_sil_host)
                                       answer: go on | run context i | stop
```

**HAL boundary.** `struct euc_sil_host` (`firmware/ports/sim/include/euc/sil.h`, ABI 2) is the board as the
firmware sees it: register `read` and `write` on the board's bus, `spin` (CPU time without a bus access),
`returned` (a context's exception return), `load` and `store` (memory the board shares: flash pages and DMA
buffers, without time), and `fault`. `firmware/ports/sim/port.c` implements `stm32f1/port.h` with the register
sequences of `ports/stm32f1`, through that table, so the TIM8, ADC, USART, SPI, RTC, flash, BKP and IWDG models,
the MPU-6500, the BMS, the display, the LED strip and the buzzer see what they see from the image. A change to a
driver in `ports/stm32f1` needs the same change in `ports/sim/port.c`.

**Contexts.** Thread mode is one call, `euc_sil_run()`: the reset path (VTOR), the app's `app_init()`, then the
main loop with a fixed cost per pass. The four interrupt contexts are the app's own handlers (TIM8 update,
ADC1/2 end of injection, USART1, RTC second); `euc_sil_context_*()` give their IRQ numbers. The engine's exception
unit decides when an interrupt is taken, with its priorities, nesting and tail-chaining, exactly as for an
emulated core. The library's thread mode runs on a coroutine stack (`corosensei`), so the engine can pause it in
the middle of any host call (a busy wait during start-up, a flash wait, a delay) and resume it at the next
quantum. When the active exception changed in between, the paused call answers with that context's index, and
the library runs it nested before the interrupted code goes on; its return is the exception return.

**Resets and power.** A system reset, the watchdog, or a supply loss makes the engine call `Cpu::reset`: the paused
host call answers stop, the library `longjmp`s back to `euc_sil_run()`, and the copy is dropped. The next power-on
loads a fresh copy of the file (the library's static state is the firmware's RAM), sets the quirk mask, and starts
again. Two sessions at once (Compare) run two copies.

**Image and probes.** A SIL machine boots a synthetic 1 KiB image: a vector table whose reset vector names thread
mode and whose handled interrupts each name a context of their own (every other one shares the default handler,
which the machine reports as a blocker, as for an image). The probes are those of
`profiles/begode-extreme/probes.toml` (`firmware/tools/gen_probes.py --emit-sil` writes the same list for the library,
which measures its own offsets). After every quantum the backend copies them into emulated SRAM from
`0x20000000`, so `Session` reads them like an image's variables: cut-off flags, scenario checks, `euc sim
compare`, recordings and Tune work unchanged.

**Memory.** The storage pages (`ld_*_page`) live in host memory: the SIL link aliases the linker symbols to the
port's arrays (`-alias` on macOS, `--defsym` elsewhere), and the port reloads a page from the board's flash model
after every write. The DMA buffers of the BMS link and the LED strip sit in emulated SRAM from
`EUC_SIL_DMA_RAM_ADDRESS` (`0x20008000`); exception frames go below `0x20008000`.

## Timing

The firmware's own code runs natively and takes no simulated time. Time passes only through the host calls:

| Cost | HCLK cycles | Why |
|---|---|---|
| a register access | 12 (`ACCESS_CYCLES`) | about the six instructions around an access in the port's loops at CPI 2, so loops that count their polls (the SPI wait, the HSE start) time out as late as on the image |
| a main-loop pass | 300 (`EUC_SIL_MAIN_LOOP_PASS_CYCLES`) | an idle pass of the image's main loop |
| a display line pause | 240 | the image's 20-turn volatile loop |
| exception entry, return, tail-chain | 0 (`sil::EXCEPTION_TIMING`) | no instruction timing |

The engine's poll fast-forward works as for an image (16 identical thread-mode reads skip to the next event), so
the start-up delays cost nothing to simulate. A run is about 6 to 15 times faster than the emulated image. The ISR
load is not available (`NaN`). The SIL machine states that it has no exception timing, so `euc sim compare` lists
the ISR load under `channels_unavailable` instead of calling it a divergence. A `NaN` that no run explains stays a
difference.

These costs are **ESTIMATES**. What follows from having no instruction timing, measured on the emulated images:

- **The balance acts sooner.** On the image, the RTC tick spends about 16,600 cycles between the IMU read and the
  balance (the commands, sensors) and about 69,000 (clean) to 79,000 (compat) cycles from there to the lights and
  buttons, interrupts included, at the fixed timing: the motor command reaches the FOC up to about 1 ms after the
  IMU sample ([the SOC50 study](models/soc50-full-run-equivalence.md) measures 0.6 ms for compat). In SIL it is
  there at once. This is the largest dynamic difference: it shows when a push or a stop excites the balance.
- **No missed RTC ticks.** The compat image's tick with the display refresh (quirk `DISPLAY_IN_BALANCE_TICK`)
  outlasts 2.016 ms, so about 1 % of ticks are lost: 2,455 instead of 2,480 ticks in 5 s, a 100-tick boot beep of
  205 ms instead of 202 ms, and 1,000 instead of 1,015 telemetry frames in `balance-60s`. SIL loses none.
- **Start-up is about 1 ms shorter.** The latch goes high at 543.3 ms (image 544.3 ms), the RTC tick starts at
  1175.1 ms (1176.2 ms) and the first frame goes at 1479.6 ms (1484.0 ms).
- **Interrupt latency is zero.** The ADC is started and the PWM compares are written at the event itself; with
  compare preload, both take effect at the same timer update as on the image.
- **A C loop that never calls the host hangs the run.** Every wait in the app and the port calls the host; a
  future one must too.

## Validation

`euc sim compare` with `scenarios/compare/compat-vs-stock.toml` (the tolerance CI judges compat against stock
with), fixed timing for the image, the default articulated rider, 2026-10-22, base `50969b4` plus this work:

| A (image) | B (SIL) | Scenario | speed max | pitch max | Iq RMS | torque RMS | frames A / B | Result |
|---|---|---|---|---|---|---|---|---|
| compat | `ours-sil:compat` | balance-60s | 0.286 km/h | 0.91° | 3.68 A | 3.17 N·m | 1000 / 1015 | within |
| compat | `ours-sil:compat` | ride-20kmh-stop | 0.590 km/h | 0.67° | 3.01 A | 2.59 N·m | 630 / 635 | within |
| compat | `ours-sil:compat` | hall-loss-while-riding | 0.078 km/h | 0.14° | 0.91 A | 0.78 N·m | 220 / 225 | within (to 15 s) |
| clean | `ours-sil:clean` | balance-60s | 0.320 km/h | 0.32° | 1.00 A | 0.86 N·m | 1010 / 1015 | within |
| clean | `ours-sil:clean` | ride-20kmh-stop | 0.630 km/h | 1.03° | 4.15 A | 3.57 N·m | 635 / 635 | within |
| clean | `ours-sil:clean` | hall-loss-while-riding | 0.056 km/h | 0.11° | 0.66 A | 0.56 N·m | 225 / 225 | within (to 15 s) |

Limits: speed 0.9 km/h on balance-60s, 0.8 on ride-20kmh-stop, 0.5 on hall-loss-while-riding; pitch 1.5°, Iq
RMS 6 A, torque RMS 5 N·m. Every scenario check passes on both sides. The hall-loss window ends at the fault, as
the tolerance file sets it for compat against stock. The text replies are identical, except 6 bytes between frames
in compat balance-60s. The SIL sends up to 15 more frames in 63 s; on compat, these are the missed ticks above.

The firmware speed-up ([SOC50 study](models/soc50-full-run-equivalence.md#the-speed-up-2026-10-22-not-yet-sufficient):
faster soft-float routines, less strip and FOC work, the strip rendered in the main loop) changes both images'
timing. The same six compares on base `0575f14` with it:

| A (image) | B (SIL) | Scenario | speed max | pitch max | Iq RMS | torque RMS | frames A / B | Result |
|---|---|---|---|---|---|---|---|---|
| compat | `ours-sil:compat` | balance-60s | 0.240 km/h | 0.91° | 3.59 A | 3.09 N·m | 1005 / 1015 | within |
| compat | `ours-sil:compat` | ride-20kmh-stop | 0.571 km/h | 0.77° | 3.13 A | 2.69 N·m | 630 / 635 | within |
| compat | `ours-sil:compat` | hall-loss-while-riding | 0.075 km/h | 0.17° | 0.89 A | 0.77 N·m | 221 / 225 | within (to 15 s) |
| clean | `ours-sil:clean` | balance-60s | 0.132 km/h | 0.16° | 0.49 A | 0.43 N·m | 1014 / 1015 | within |
| clean | `ours-sil:clean` | ride-20kmh-stop | 0.191 km/h | 0.40° | 1.22 A | 1.05 N·m | 635 / 635 | within |
| clean | `ours-sil:clean` | hall-loss-while-riding | 0.052 km/h | 0.14° | 0.53 A | 0.46 N·m | 225 / 225 | within (to 15 s) |

Every check gives the same verdict on both sides. The SIL library itself rides the same before and after the
speed-up: every channel, firmware value and BLE frame is identical, and only the strip state is sampled a moment
earlier, before the main loop renders it.

The largest differences come at a push or at the stop, where the latency difference above shows, and they move
with a few cycles of timing: the clean image itself differs by 0.373 km/h at the balance-60s push between its fixed
and flash timings, and an earlier library build of the same day gave 0.594 km/h on clean balance-60s at 42.9 s. A
fixed 25,000 or 50,000-cycle cost after the IMU read in SIL brought that speed to 0.48 km/h but the pitch to 1.08°,
so the SIL keeps no such cost.

Unit tests: `cargo test -p euc-sim sil` (quirk text, the image, the map) and, with the library built,
`cargo test --release -p euc-sim sil -- --include-ignored` (boot, `V` answer, telemetry, two runs with different
quirks at once, a power cycle loading a fresh copy). Without the library those tests are ignored, with the
reason. CI (`ci-engine`) builds the library on Ubuntu and macOS, so it links with `--defsym` and `-alias`, and
runs them there; `ci-firmware` builds it and runs its C tests on Ubuntu.
