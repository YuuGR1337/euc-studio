# CPU timing

**Status: fixed-cost timing is implemented.** The CLI and app use CPI 2.0 with exception overhead.
The instruction-level flash model remains future work. The study below records the earlier timing
and its evidence.

## Current timing

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

### Cadence check

With the owner's mode-1 preset, a 30-second stock idle run gave these results:

| Measurement | Current model | Real recording |
|---|---|---|
| Light breathing period | mean 2.235 s; 2.18–2.28 s over nine periods | about 2.1 ±0.2 s, one video cycle |
| BLE frame-group period | 307.44 ms | 309.6 ms |

The light period is about 6% above the video estimate. The BLE period is about 0.7% below the capture.
These checks support the fixed timing estimate. They do not establish exact instruction timing.

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

## Scenario validation

These checks use CPI 2.0, exception costs, and the LTO target builds.
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
- **The app's "ISR 31 %".** It comes from the synthetic source (`app/src/sim/synthetic.ts`); the
  live engine never fills `isr_load_pct`.

Not modeled, small (ESTIMATE, not checked): the RTC `RTOFF` wait after clearing `SECF` (up to
3 RTC clocks, 48 µs per tick, at most 2.4 %), and bus contention from the WS2812 DMA at 800 kHz
(about 2 %).

## Next steps

1. **Per-instruction model** in `euc-armv7m` (the flash model above), with the machine taking
     quantum time from the core. It is more faithful when the code mix changes (delay loops,
     soft-float, our own firmware). Unicorn has no model, so its timing would then differ from our
     core's. It needs a cycle budget in `Cpu::run`, or a CPI estimate for the budget.
2. Better real data: a 20 s video on a tripod with the LED mode noted (period to ±2 %), and a
   second BLE capture. Both pin W between 2 and 3.
3. If the project wants a profiler, add an `euc sim profile` command, built from the prototype.

Prototype tools: private research repo, `research/timing-proto/` (the single-step profiler, the
flash-model prototype and the analysis scripts, with a README on how to run them).
