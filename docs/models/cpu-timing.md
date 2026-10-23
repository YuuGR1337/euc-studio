# CPU timing

**Status: two instruction timings exist.**
The fixed timing (stage A: CPI 2.0 plus exception costs) remains the shipped default.
The flash timing (stage B: per-instruction Cortex-M3 costs behind the STM32F1 flash interface) is selectable.
The owner's wheel favours the fixed timing; see [Validation against the owner's wheel](#validation-against-the-owners-wheel).
The sections after the stage B results record the earlier timing studies and their evidence.

## Selecting the timing

| Timing | Name | What sets a quantum's duration | Backends |
|---|---|---|---|
| Fixed (stage A, default) | `fixed` | retired instructions × `--cpi` (2.0) | own core, Unicorn |
| Flash (stage B) | `flash`, or `flash-wN` for N wait states | the core's own model clock | own core only |

- The CLI takes `euc sim run --timing ...` and `euc sim compare --timing ...`.
  Without the flag, the own core uses `euc_sim::cpu::DEFAULT_TIMING` (fixed) and Unicorn uses fixed.
- Rust callers use `Backend::factory_with(Timing)` and `Timing::exception_timing()`.
  `Backend::factory()` keeps the default.
- The JSON output names the timing in `timing`.
  A recording header stores `timing`, and stores `cpi` only for the fixed timing.
- Unicorn has no timing model, so `--cpu unicorn --timing flash` is refused.
- Every timing keeps the exception costs below: 12 + W cycles for entry and return, 6 + W for a tail-chain.
  W is 2 for the fixed timing and the flash timing's wait states otherwise.

## Fixed timing (stage A)

`euc sim run`, `sim compare`, `sim serve`, and the app share `DEFAULT_CPI = 2.0`.
Both CPU backends use the same exception costs:

| Switch | HCLK cycles |
|---|---|
| Exception entry | 14 |
| Exception return | 14 |
| Tail-chain into a pending handler | 8 |

Arm specifies 12-cycle entry and return, and 6-cycle tail-chaining, with zero-wait memory
([Cortex-M3 TRM](https://documentation-service.arm.com/static/5e8e107f88295d1e18d34714)).
The extra two cycles represent flash latency and remain an **ESTIMATE**.
This model does not simulate individual flash fetches or bus contention.
The scheduler checks timers after each context switch. A return inside an IT block keeps its
switching time and does not stack an already-taken exception again.

Reference tests explicitly use CPI 1.25 and `ExceptionTiming::REFERENCE`, with no exception cost.
For a historical CLI run, add `--cpi 1.25 --no-exception-timing`.
Changing `--cpi` alone retains the normal exception costs.

### RTC startup correction

PRL and CNT writes now reload the RTC divider, as [RM0008 section 18.4.4](https://www.st.com/resource/en/reference_manual/rm0008-stm32f103xx-advanced-armbased-32bit-mcus-stmicroelectronics.pdf) requires.
The previous model retained the reset divider until its first rollover.
At 62.5 kHz, that reset period was 524.304 ms instead of the configured 2.016 ms.
The nominal control period remains 2.016 ms.

Release boot tests on both CPU backends measure the first RTC handler at 1177.753 ms and first telemetry at 1480.700 ms.
Both produce 45 telemetry frames over six simulated seconds, with the existing `V` and `N` command pauses.
These runs use stock `cbfb328c`, blank settings, CPI 1.25, no exception costs, and quantum size 4000.
The boot tests retain their 10 ms telemetry tolerance, boot marker checks, and performance floors.

The cadence and scenario measurements below precede this RTC correction.
They describe the previous RTC model and require new runs before they establish current behavior.

### Previous cadence check

With the owner's mode-1 preset, a 30-second stock idle run gave these results:

| Measurement | Previous RTC model | Real recording |
|---|---|---|
| Light breathing period | mean 2.235 s; 2.18–2.28 s over nine periods | about 2.1 ±0.2 s, one video cycle |
| BLE frame-group period | 307.44 ms | 309.6 ms |

The light period is about 6% above the video estimate. The BLE period is about 0.7% below the capture.
These checks supported the fixed timing estimate under the previous RTC model.
They do not establish exact instruction timing or current light cadence.

### Clean firmware scheduling

The slower model exposed a clean-firmware failure during ordinary balance.
The display transfer blocked the RTC task long enough to trip the stale-command latch.
The latch first appeared at 1.905 s, before stand release. Clean then fell at 5.19 s at all three quantum sizes.
CPI 2.0 without exception overhead also failed. The exception-cost implementation alone did not cause the failure.

Clean now sends display frames from the main loop (`DISPLAY_IN_BALANCE_TICK` off).
The RTC task requests each refresh and keeps its normal strip-speed sample.
The three-tick stale-command deadline remains in force. Compat retains the stock scheduling.
Target presets also enable link-time optimization. This reduces interrupt cost and prevents early main-loop starvation in the 12 kHz preset.
The freestanding memory routines stay outside LTO, so generated `memcpy` and `memset` calls can resolve.
CI checks clean balance, ride, and pothole runs with the shipped timing.
The pothole case requires at least 19 km/h, which rejects the early-limp regression.
See [watchdog-and-overcurrent.md](../../firmware/docs/watchdog-and-overcurrent.md#display-transfers-and-the-stale-command-deadline).

Evidence tags as elsewhere: **OBSERVED** = measured in the simulator or in a real recording;
**INFERRED** = follows from the code or the measurements; **ESTIMATE** = a model or a figure from
outside the project.

## Flash timing (stage B)

`euc_armv7m::Flash` and `FlashPipe` (`crates/euc-armv7m/src/timing.rs`) time every retired instruction.
The core still counts its zero-wait Cortex-M3 TRM costs as before.
The flash model then delays them as the STM32F1 flash interface does
([RM0008](https://www.st.com/resource/en/reference_manual/rm0008-stm32f103xx-advanced-armbased-32bit-mcus-stmicroelectronics.pdf) section 3.3.3, "Read interface").
The firmware sets `LATENCY` 2 and `PRFTBE` before it raises HCLK to 72 MHz (OBSERVED).
The model applies two wait states from reset, because the engine also runs HCLK at 72 MHz from reset.

| Rule | Source |
|---|---|
| The array reads one 64-bit line in W + 1 HCLK cycles and finishes a read once it has started. | RM0008 3.3.3 (latency); not aborting a started read is an **ESTIMATE** |
| The prefetch buffer holds two lines: the current line and the next one. The next line is read back to back after a miss, otherwise when the core leaves the older line. | RM0008 3.3.3 ("2 blocks", "read request when at least one block is free") |
| A taken branch into the current or the prefetched line is served from the buffer. Any other target is a demand read, which replaces a prefetch that has not started. | INFERRED from RM0008 3.3.3 |
| Data reads from flash (literal pools, `TBB`/`TBH` tables, constants, data flash) go before a prefetch that has not started and wait for a read in progress. | RM0008 3.3.3 ("DCode accesses have priority over prefetch accesses") |
| A line serves an instruction one decode cycle after its read completes, so W = 0 reproduces the TRM costs, including the refill P = 2. | Cortex-M3 TRM table 18-1; a unit test checks the equality |
| Fetches and loads outside the flash array see no wait states. | RM0008 |
| Exception entry and return cost 12 + W, a tail-chain 6 + W, charged by the engine as before. | Cortex-M3 TRM; the W is an **ESTIMATE** |

The model is deterministic and uses only integer cycles.
Its clock does not depend on how the engine splits a program into runs; a test checks this.
With the model on, the machine gives each quantum a cycle limit at the next event instead of an instruction budget from the CPI.
The quantum lasts the cycles that the model counted.
The model changes no architectural result.
The decoder, the executor and the lock-step results are those of the fixed timing.

Not modelled: peripheral bus wait states (AHB to APB bridges), bus contention with DMA, and the Cortex-M3 core's own fetch queue.
The real core's branch speculation changes the zero-wait refill P only, not the wait states a missed target adds.

On the idle stock firmware with the owner's settings, the model gives these cycles per instruction.
They were OBSERVED over 10 s with a private counter build, which is not in the tree.

| Context | TRM, zero wait | Flash model, W = 2 | Stall per instruction: sequential, branch, data |
|---|---:|---:|---|
| Thread (`led_strip_effects`) | 1.64 | 2.00 | 0.10, 0.26, 0.01 |
| RTC tick | 1.52 | 2.10 | 0.20, 0.29, 0.09 |
| ADC1_2 and TIM8_UP | 1.71 | 2.22 | 0.08, 0.26, 0.17 |

Taken branches miss the buffer in 64-84 % of cases.
The thread code averages the fixed timing's CPI 2.0, but the interrupt code is 5-11 % slower than in the fixed timing.

## Validation against the owner's wheel

`crates/euc-armv7m/tests/stock_cadence.rs` boots the stock image idle with the owner's preset (`tarik-2026-10-15`, LED mode 1).
It measures the tail-light breathing period from the LED strip output and the BLE frame-group period from the telemetry.
The real references are one video cycle (2.1 ± 0.2 s, ESTIMATE) and the 25 s idle capture (76 groups, 309.6 ms, OBSERVED).
Each simulated run lasts 30 s, so the means cover 8-15 breathing periods and about 90 groups.

| Timing | Breathing period | BLE group period | Lost RTC ticks | Interrupt share |
|---|---:|---:|---:|---:|
| Fixed, CPI 1.8 | 1.619 s | 304.44 ms | 0 % | 50.0 % |
| **Fixed, CPI 2.0 (default)** | **2.234 s** (2.18-2.32) | **307.45 ms** | 0.99 % | 55.3 % |
| Fixed, CPI 2.2 | 2.787 s | 307.47 ms | 0.99 % | 60.7 % |
| Flash, W = 0 | 1.474 s | 304.41 ms | 0 % | 45.9 % |
| Flash, W = 1 | 1.776 s | 304.45 ms | 0.01 % | 51.1 % |
| **Flash, W = 2** | **2.515 s** (2.48-2.59) | **307.45 ms** | 0.99 % | 60.0 % |
| Flash, W = 3 | 4.454 s | 310.46 ms | 1.96 % | 72.1 % |
| Owner's wheel | 2.1 ± 0.2 s | 309.6 ms | about 1.7 % | unknown |

- **Breathing.** The fixed timing lies inside the video's band (+0.13 s).
  The flash timing lies 0.42 s above it, about 20 % slow.
  Its slower interrupt code takes main-loop time, and the breathing period scales with the main-loop passes per tick.
- **BLE.** Both timings lose one RTC tick per display refresh and give the same 307.45 ms, 0.7 % short of the capture.
  The capture implies about 1.7 lost ticks per 100.
  The real display refresh must therefore last close to the two-tick threshold, longer than either model predicts.
  W = 3 crosses that threshold, but it is not the hardware setting and makes the breathing twice as slow as the video.
- **Decision.** The fixed timing is better validated, so it stays the default.
  The flash timing stays selectable for timing studies and as a second timing estimate.
  Neither model reproduces both measurements, and the flash model has no free parameter to fit: W = 2 is the firmware's setting.
- **Data that would settle it.** A 20 s tripod video with the LED mode noted would give the breathing period to about 2 %.
  A logic-analyser trace of one GPIO toggled by a test build would give the FOC and tick handler durations directly.
  A second BLE capture would confirm the lost-tick rate.

The cadence test asserts that the default timing stays inside the video band and within 1 % of the capture period.
It prints both timings' numbers.

## Scenario robustness on stock

These runs used the rigid rider on the flat floor: they predate the articulated default and the scenarios on the
city (W2). The verdicts on the city are in [project status](../project-status.md#city-world-w2-2026-10-23).

All 20 shipped scenarios ran on stock at quanta 4000, 3000 and 2000 with both timings, from `63801a2` plus these changes.
The fixed-timing reports match the accepted baseline (`target/fw-scratch/resume-accepted/matrix/`) byte for byte, except wall time.

| Result per quantum, both timings | Scenarios |
|---|---|
| Pass | `balance-60s`, `coasting-stop`, `downhill-cruise-2pct`, `full-effort-70kmh-stop-soc100`, `hall-loss-while-riding`, `hall-single-line-loss`, `hard-braking-30kmh`, `hill-climb-low-battery`, `low-battery-tiltback`, `overlean-no-pwm-tiltback`, `pothole-20kmh`, `pothole-5cm-20kmh`, `ride-20kmh-stop`, `rider-steps-off-15kmh`, `rider-steps-off-5kmh` |
| Fail, as in the accepted baseline | `ble-service-cut-standing`, `full-effort-70kmh-stop-soc20`, `full-effort-70kmh-stop-soc50`, `overlean-top-speed`, `overlean-top-speed-30pct` |

The flash timing therefore changes no stock verdict at any of the three quanta.
Quanta 4000 and 2000 produce bit-identical reports in both timings (OBSERVED for every scenario).
The three-quantum check therefore tests two distinct trajectories, not three.
Quantum 3000 and the finer quanta in [the SOC50 study](soc50-full-run-equivalence.md) give distinct trajectories.

## Unicorn and the flash timing

Unicorn stays the instruction oracle.
`crates/euc-armv7m/tests/lockstep.rs` adds `stock_image_keeps_lockstep_under_the_flash_timing_model`.
In it, Unicorn executes, the shadow core runs the flash model, and the machine takes its time from the shadow's clock.
Over 6 simulated seconds it compares 455,103 quanta and 173 M instructions with **0 divergences**.
The fixed-timing lock-step tests also pass unchanged (30 s: 704,013 quanta, 1.66 G instructions, 0 divergences).
A run with `--cpu unicorn` always uses the fixed timing, so its trajectory differs from a flash-timed own-core run.

The flash model costs speed: the idle stock firmware runs at 3.04 times real time against 3.79 with the fixed timing (release build, loaded machine).

## Previous scenario validation

These checks used the previous RTC model, CPI 2.0, exception costs, and the LTO target builds.
The frozen image hashes start with `3fe25c10` (compat) and `fe83cb1d` (clean).
The stock image stays `cbfb328c` (GW1825007). The plant, rider, and firmware safety deadlines retain their prior settings.

Each firmware ran all 16 shipped scenarios at quantum sizes 4000, 3000, and 2000.
The table reports physical fall or hold states. Some scenario checks intentionally fail when stock cuts or clean avoids a stock fault.
The borderline clean result lists states in quantum order: 4000, 3000, 2000.

| Scenario | Stock | Compat | Clean |
|---|---|---|---|
| `balance-60s` | holds (3/3) | holds (3/3) | holds (3/3) |
| `ble-service-cut-standing` | falls (3/3) | falls (3/3) | holds (3/3) |
| `coasting-stop` | holds (3/3) | holds (3/3) | holds (3/3) |
| `downhill-cruise-2pct` | holds (3/3) | holds (3/3) | holds (3/3) |
| `hall-loss-while-riding` | falls (3/3) | falls (3/3) | falls (3/3) |
| `hall-single-line-loss` | holds (3/3) | holds (3/3) | holds (3/3) |
| `hard-braking-30kmh` | holds (3/3) | holds (3/3) | holds (3/3) |
| `hill-climb-low-battery` | holds (3/3) | holds (3/3) | holds (3/3) |
| `low-battery-tiltback` | holds (3/3) | holds (3/3) | holds (3/3) |
| `overlean-no-pwm-tiltback` | falls (3/3) | falls (3/3) | holds (3/3) |
| `overlean-top-speed-30pct` | falls (3/3) | falls (3/3) | holds (3/3) |
| `overlean-top-speed` | falls (3/3) | falls (3/3) | **borderline**: falls, holds, holds |
| `pothole-20kmh` | holds (3/3) | holds (3/3) | holds (3/3) |
| `ride-20kmh-stop` | holds (3/3) | holds (3/3) | holds (3/3) |
| `rider-steps-off-15kmh` | falls (3/3) | falls (3/3) | falls (3/3) |
| `rider-steps-off-5kmh` | falls (3/3) | falls (3/3) | falls (3/3) |

The rider-step-off cases intentionally end with the empty wheel on its side.
All 16 stock-versus-compat comparisons pass their recorded limits.
The hill's distance limit changes from 0.2 m to 0.5 m for a measured 0.315 m difference across about 150 m.
The pitch limit and three-second step-off cutoff limit retain their prior values.

Additional pothole runs change only the depth:

| Pothole | Stock | Compat | Clean, quantum 4000 / 3000 / 2000 |
|---|---|---|---|
| 3 cm | holds (3/3) | holds (3/3) | holds / holds / holds |
| 5 cm | holds (3/3) | holds (3/3) | **borderline**: holds / falls / falls |

Every additional run reaches the hole. A run that never reaches the obstacle does not establish impact performance.
Both riderless guard scenarios record zero detector triggers at all three quantum sizes.
They also record no main-loop stall. The step-off diagnostic records one riderless trigger, which confirms that LTO retains the counter.

The full-battery clean overlean result and the clean five-centimetre impact result remain borderline.
These cases require further work. The earlier lean-sweep and sensitivity tables use the historical timing stated on their pages.
They do not establish the same margins under this timing model.

## The question

The stock firmware's standing tail-light pattern breathes with a period of about **2.1 s** on the
owner's wheel (video, one cycle, ±0.2 s) and **0.49 s** in the simulator (display's run: blank
settings, Unicorn block quanta, fixed CPI 1.25). Is the simulated main loop too fast, and why?

How the firmware makes the period (OBSERVED, see [lights.md](../hardware/lights.md)):
`led_strip_effects` (0x08005A5C) runs once per main-loop pass. While the wheel stands, every pass
that runs during a tick whose LED tick counter is a multiple of 10 steps the brightness by ±1
(0 to 131 and back). So the period is inversely proportional to the main-loop passes per RTC tick
(496 Hz, 2.016 ms). In our runs, period × passes per tick × 49.6 Hz ≈ 289 steps per cycle.

## Historical measurements at CPI 1.25

Own core, stock GW1825007, idle (`euc sim run begode-extreme`), 20 simulated seconds. A study-only
wrapper single-stepped the core and counted instructions per exception context, main-loop passes
and the brightness byte. The profiled runs were bit-identical to plain runs (same instruction
count, same BLE frame times). OBSERVED.

| | blank settings | owner preset `tarik-2026-10-15` |
|---|---|---|
| LED mode, drive mode | 0, voltage 10 kHz | 1, current 12 kHz |
| TIM8_UP / ADC1_2 interrupts per s | 19.6 k / 19.2 k | 23.6 k / 23.1 k |
| CPU time: thread / RTC / ADC1_2 / TIM8_UP | 63 / 12 / 12 / 7 % | 61 / 12 / 13 / 9 % |
| Thread time inside `led_strip_effects` | 97 % | 97 % |
| Instructions per standing main-loop pass | 6.7 k | 11.9 k |
| Main-loop passes per RTC tick | 11.75 | 6.32 |
| Breathing period | **0.495 s** | **0.922 s** |
| BLE frame-group period | 304.42 ms | 304.42 ms |

- The 0.495 s matches display's 0.49 s, so block versus exact quanta does not matter here.
- `ws2812_set_pixel` alone is 62 % of all instructions in LED mode 0.
- The BLE scheduler queues a frame group every 151 counted ticks. The simulator gives exactly
  151 ticks (304.4 ms). The real capture (`tests/data/begode-extreme/idle-2026-10-15.hex`, 76
  groups in 25 s) gives **309.6 ms**: the real wheel loses about 1.7 % of its ticks. This is a
  second, independent measure of the CPU speed, and more precise than one video cycle.

## Causes found

1. **LED mode (INFERRED from the capture).** The owner's capture reports `W M` 1, so his wheel is
   in LED mode 1 (the preset boots in mode 1, OBSERVED). The 0.49 s was measured in mode 0. A
   mode-1 pass costs 1.8 times a mode-0 pass, so mode 1 alone gives 0.92 s. This explains a
   factor of 1.9 of the 4.3.
2. **CPI (ESTIMATE).** The original machine charged a fixed 1.25 cycles per instruction. Our core's TRM
   model (`euc-armv7m` `timing`) gives 1.52 to 1.88 per context. The STM32F1 runs from flash with
   2 wait states at 72 MHz (the firmware sets latency 2 and the prefetch buffer on, OBSERVED). A
   flash model on top of the TRM counts:
   - fetch bandwidth: 8 bytes per 3 HCLK cycles in each straight-line run;
   - W extra cycles per taken branch and per data read from flash (literal pools, constant tables,
     TBB/TBH);
   - 12 + W cycles for each exception entry and each return (the original model charged nothing).

   With W = 2 the effective CPI is 2.07 (thread 2.02, RTC 1.88, ADC1_2 2.01, TIM8_UP 2.40).
   Cross-check: ST's family figure of 177 CoreMark at 72 MHz (2.46 per MHz) against about 3.3 per
   MHz for a Cortex-M3 at zero wait states is a 1.35x slow-down over the TRM ideal. The model gives
   1.3x.
3. **Interrupt share (INFERRED).** The interrupt work per tick is fixed, so a slower CPU takes it
   from the main loop. At CPI 1.25 the handlers use about 36 % of the time. At CPI 2.07 they use
   about 60 %, and the main loop keeps 40 % instead of 64 %. Passes per tick fall by the CPI ratio
   (1.65) times 1.6, about 2.6.
4. **Lost RTC ticks (INFERRED).** With the slower CPU, about 1 % of the RTC ticks are lost.
   Every 100th tick the RTC handler runs `display_refresh`. Its TM1640 bit-banging uses software
   delay loops of about 77 k instructions. With FOC pre-emption, that handler then lasts more than
   2 ticks, and a second-flag event is lost (the flag does not count). The BLE scheduler counts
   ticks, so its group period grows. This is the mechanism behind the real 309.6 ms.

## Prototype results (study snapshot only, not in the tree)

Owner preset, idle, 30 simulated seconds. Fixed CPI = the machine's existing `cpi`. Model = the
per-instruction flash model above, with the quantum's time taken from the model. OBSERVED in the
snapshot. Fixed-CPI prototype runs excluded exception overhead; the flash-model runs included it.

| Timing | Breathing | Passes per tick | RTC ticks lost | BLE group period |
|---|---|---|---|---|
| CPI 1.25 (original) | 0.92 s | 6.32 | 0 | 304.4 ms |
| CPI 2.0 | **2.17 s** | 2.70 | 0.98 % | 307.4 ms |
| Model, W = 1 | 1.84 s | 3.16 | 0.98 % | 307.4 ms |
| Model, W = 2 | **2.31 s** | 2.53 | 0.98 % | 307.5 ms |
| Model, W = 3 | 2.94 s | 2.03 | 1.95 % | 310.5 ms |
| Real wheel | 2.1 ± 0.2 s | about 2.5 | about 1.7 % | 309.6 ms |

- CPI 2.0 and the W = 2 model are both within 10 % of the video. The BLE period prefers a CPU
  slightly slower than both. The lost-tick count is a threshold effect (1 or 2 ticks per display
  refresh), so it moves in steps.
- LED mode 0 with the W = 2 model gives 1.10 s and 5.35 passes per tick.
- Scenarios (stock, own core, snapshot) pass in all three timings:

| Scenario | CPI 1.25 | CPI 2.0 | Model W = 2 |
|---|---|---|---|
| balance-60s | 4/4, max pitch 1.028° | 4/4, 1.026° | 4/4, 1.025° |
| ride-20kmh-stop | 5/5, top 20.3975 km/h, 81.764 m | 5/5, 20.4003 km/h, 81.766 m | 5/5, 20.4010 km/h, 81.767 m |

## Ruled out

- **Poll fast-forward.** It acts in thread mode only and skips to the next event, so it keeps
  time. In steady state it is small: `usart3_rx_parse` reading `DMA1.CNDTR3` (0.52 s in 30 s);
  the rest is boot `delay_ms`.
- **Blocking waits in the main loop.** 97 % of the thread time is LED code. The UART frame pacing
  is modeled (2.24 ms between frames, one frame per pass).
- **SPI3 transfer time (not modeled).** After init the firmware sets the MPU6500 SPI to PCLK1/2 =
  18 MHz; one IMU read is about 0.5 k cycles per tick (0.3 %).
- **ADC conversion time (not modeled).** The FOC is interrupt-driven (`TIM8_UP` starts the
  injected group, `ADC1_2` on JEOC runs the loop); nothing busy-waits, so the CPU load is the same.
- **The app's "ISR 31 %".** It came from the synthetic source (`app/src/sim/synthetic.ts`). Since then the live
  engine fills `isr_load_pct` from the CPU's exception cycles when exception timing is on (`Session::step`).

Not modeled, small (ESTIMATE, not checked): the RTC `RTOFF` wait after clearing `SECF` (up to
3 RTC clocks, 48 µs per tick, at most 2.4 %), and bus contention from the WS2812 DMA at 800 kHz
(about 2 %).

## Next steps

1. Done (stage B): the per-instruction model is in `euc-armv7m` with `Cpu::model_cycles` and
   `Cpu::run_for`, and the machine takes quantum time from it. It is selectable, not the default; see
   [Validation against the owner's wheel](#validation-against-the-owners-wheel). The stage B model
   refines the prototype above: line-granular prefetch, buffer hits, and arbitration with reads in progress.
2. Better real data: a 20 s video on a tripod with the LED mode noted (period to ±2 %), a second BLE
   capture, and a logic-analyser trace of handler durations from a test build.
3. MMIO accesses still see the quantum's start time (`System::now()`), with either timing. The flash
   model's clock could time each access exactly. That change would alter every accepted baseline, so
   it needs its own validation.
4. If the project wants a profiler, add an `euc sim profile` command, built from the prototype.

Prototype tools: private research repo, `research/timing-proto/` (the single-step profiler, the
flash-model prototype and the analysis scripts, with a README on how to run them).
