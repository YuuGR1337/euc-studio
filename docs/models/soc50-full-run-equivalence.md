# SOC50 full-run equivalence: tick latency and chaos

Date: 2026-10-22. Source base: `63801a2` plus the stage B timing work ([CPU timing](cpu-timing.md)).
This continues the [first](soc50-investigation.md) and [second](soc50-high-speed-investigation.md) SOC50 studies.

**Result.** Compat performs stock's arithmetic, but its RTC tick handler takes about twice stock's cycles.
That later command publication systematically changes the high-speed regime of both full-effort cases that fail.
A causal test that swaps only the tick cost swaps the regimes.
On top of this, the final cutoff time is chaotic at the wheel's stability limit: numerical perturbations alone move it by 2-3 s.
No tolerance, comparison window, scenario or physical input changes in this study.

**Speed-up.** Bit-identical firmware changes, and moving the LED strip rendering to the main loop as stock does, bring compat's tick from about 77,900 to 45,500 cycles (stock 37,700) and its interrupt load from about 95 % to 67 % (stock 64 %).
That is not yet enough: over 29 runs per group the regime metrics still differ from stock (p = 0.001-0.005).
The work before the command store is the part that is still slower than stock's; with the whole tick at stock's cost the metrics no longer differ.
See [the speed-up](#the-speed-up-2026-10-22-not-yet-sufficient).

Evidence tags as elsewhere: **OBSERVED** in the simulator, **INFERRED** from the code or the measurements.

## Inputs and controls

- Firmware: the frozen stock image `cbfb328c` and compat image `7fc8a128` with its ELF ([identity table](full-effort-acceptance.md#identity-and-reproduction)).
- Profiles and scenarios: those of `63801a2`, whose profile hashes match the accepted baseline.
- Engine: `63801a2` plus the stage B timing changes only.
  Other agents' unfinished work in the shared tree is excluded.
- With the fixed timing, the stock reports at quanta 4000, 3000 and 2000 match the accepted matrix byte for byte, except wall time.
- Private instrumentation (not in the tree) writes the 100 Hz samples that the run already takes: signals, firmware variables, and per-handler cycles.
  It counts instructions per PC, and the RTC-context instructions before each store to the motor command.
  It adds no quantum boundary, plant synchronisation or peripheral access.
  The instrumented stock and compat runs produce reports identical to the plain runs.
- Ensembles change only the instruction quantum (4000 to 100, 17 sizes) and the timing (fixed or flash).
  Runs whose 100 Hz traces are identical count once ("distinct trajectories").
  For example, quanta 4000 and 2000 always coincide.

## Outcomes of the ensembles

SOC50, all 17 quanta per firmware and timing (OBSERVED):

| Firmware, timing | Distinct runs | Reach 70 km/h | Complete pass | Cutoff, median (range) |
|---|---:|---:|---:|---|
| Stock, fixed | 16 | 4 | 0 | 19.28 s (18.54-21.27) |
| Stock, flash | 17 | 6 | 0 | 19.52 s (18.84-21.29) |
| Compat, fixed | 12 | 5 | 1 | 19.02 s (18.18-20.01) |
| Compat, flash | 14 | 2 | 0 | 18.80 s (18.12-21.63) |

- The cutoff time and the 70 km/h gate vary widely within every group.
  The only change is a simulation parameter that has no physical meaning.
- Stock and compat cutoff times do not differ under the fixed timing (Mann-Whitney p = 0.35).
  Under the flash timing, compat cuts earlier (p = 0.001).
  The reach-70 rate does not differ in either timing (Fisher p = 0.43 and 0.24).
- One compat run (quantum 900, fixed) completes the whole scenario.
  It does not establish a correction, because its neighbours fail.
- SOC100 passes in all 12 runs (stock and compat, both timings, quanta 4000, 3000, 1000).
- SOC20 fails in every run, as in the accepted baseline; see [SOC20](#soc20) for its cutoff times.

## Trajectories part at the same rate

The table gives the median RMS difference of speed (km/h) in 1 s windows for pairs of runs.
Same-firmware pairs differ only in quantum, or only in timing.

| Pair class | 4 s | 7 s | 10 s | 12 s | 14 s | 16 s | 18 s |
|---|---:|---:|---:|---:|---:|---:|---:|
| Stock vs stock, quanta (fixed) | 0.0003 | 0.023 | 0.033 | 0.038 | 0.041 | 0.13 | 0.83 |
| Stock fixed vs stock flash | 0.0006 | 0.023 | 0.033 | 0.041 | 0.038 | 0.12 | 0.81 |
| Compat vs compat, quanta (fixed) | 0.0002 | 0.023 | 0.035 | 0.16 | 0.074 | 0.56 | 0.91 |
| Compat fixed vs compat flash | 0.0035 | 0.020 | 0.036 | 0.081 | 0.070 | 0.51 | 0.90 |
| Stock vs compat, same quantum (fixed) | 0.0022 | 0.024 | 0.034 | 0.076 | 0.051 | 0.34 | 0.86 |

- Before the stand release at 4 s, the runs agree to about 0.01 % in the drive command.
- The release transient is sampled at slightly different phases, so the samples differ by up to 0.02 km/h for about 0.1 s.
  The runs converge again by 4.13 s.
- From 7 s, stock-compat pairs differ no more than pairs of one firmware under a numerical change.
- Within each firmware, differences grow by about three orders of magnitude between 4 s and 18 s.
  This is sensitive dependence on tiny timing differences: timing chaos.
- From 12 s, compat's own pairs part faster than stock's.
  The next section shows the regime that causes this.

## The systematic difference: leaving voltage saturation

Both firmwares reach full voltage (`vq_active` 32000) and the PWM alarm at the 50 km/h phase change near 11.5 s.
Both PWM tiltbacks rise to about 0.35° by 11.6 s.
In the quantum-4000 fixed pair, stock's alarm clears and its tiltback decays from 11.7 s.
Compat's alarm returns intermittently until about 12.2 s, and its tiltback grows to 0.90°.

Over all distinct runs (fixed timing; the flash timing gives the same directions):

| Metric | Stock | Compat | p |
|---|---:|---:|---:|
| PWM tiltback peak, 11.4-13 s | 0.60° | 0.98° | < 0.001 |
| Last voltage-saturated sample, 11.4-13 s | 11.72 s | 12.00 s | < 0.001 |
| Samples with the PWM alarm, 11.4-13 s | 23 | 41.5 | 0.001 |
| Share of samples at \|Vq\| > 31000, 11-15 s | 4.5 % | 7.3 % | 0.001 |
| Pitch RMS about its 0.5 s mean, 15-17 s | 0.13° | 2.22° | < 0.001 |
| Speed at 16 s | 63.47 km/h | 63.48 km/h | 0.64 |

Compat therefore enters the high-speed phase with more tiltback and a larger balance oscillation, at the same speed.
The [second study's](soc50-high-speed-investigation.md#current-demand-rises-before-tracking-fails) compat command oscillation near 16.3 s is this regime.

## Cause: compat's tick takes twice as long

Per-handler cycles, 9-11 s of the quantum-4000 runs (OBSERVED):

| Handler | Stock, fixed | Compat, fixed | Stock, flash | Compat, flash |
|---|---:|---:|---:|---:|
| RTC tick, cycles per call | 37,300 | 76,846 | 38,188 | 73,661 |
| RTC tick, CPU share | 25.4 % | 52.4 % | 26.0 % | 49.8 % |
| ADC1_2 (FOC), cycles per call | 671 | 797 | 706 | 766 |
| All interrupts, CPU share | 64.4 % | 95.3 % | 68.2 % | 93.8 % |

The per-PC profile of a whole quantum-4000 fixed run counts RTC-context instructions per routine.
Both firmwares make about as many calls (0-20 % apart), but compat's calls cost up to seven times more:

| Routine (stock name / compat name) | Calls, stock / compat | Instructions per call, stock / compat |
|---|---:|---:|
| double divide (`__aeabi_ddiv`) | 159.6 k / 159.7 k | 75 / 528 |
| double add and subtract | 673 k / 808 k | 49 / 67 averaged |
| double multiply | 950 k / 1,033 k | 46.5 / 45 |
| `asin` / `euc_asin` (with stock's polynomial kernel) | 66.6 k / 70.4 k | about 105 / 365 |
| whole RTC context | 179 M / 387 M instructions | 17.4 k / 36.4 k per tick |

Compat's library double divide alone is 22 % of its tick instructions.
The divide comes from libgcc; stock's comes from its own toolchain library.
Both are IEEE double divides, so a faster correctly rounded divide changes no result (INFERRED).

The motor command reaches the FOC later:

| Firmware | RTC instructions before the command store, median | Share of the tick's instructions |
|---|---:|---:|
| Stock | 17.0 k (p10-p90 16.6-18.7 k) | 98 % |
| Compat | 30.2 k (28.3-33.4 k) | 83 % |

The extra 13 k instructions are about 26 k cycles of tick work.
With the FOC handlers taking about 40 % of the CPU, the command arrives about 0.6 ms later in each 2.016 ms tick (INFERRED).

## Causal test: swap only the tick cost

A private engine build multiplies the fixed cost of instructions in the RTC handler (exception 19) by a factor.
Nothing else changes: not the firmware, the inputs, the other handlers or the scheduler.
Compat at ×0.49 takes stock's tick time; stock at ×2.05 takes compat's.
Twelve quanta each, fixed timing, SOC50:

| Run set | Distinct | Tiltback peak | Last saturated | Alarm samples | Pitch RMS 15-17 s | Cutoff median | Reach 70 / pass |
|---|---:|---:|---:|---:|---:|---:|---|
| Stock | 16 | 0.60° | 11.72 s | 23 | 0.13° | 19.28 s | 4 / 0 |
| Compat | 12 | 0.98° | 12.00 s | 41.5 | 2.22° | 19.02 s | 5 / 1 |
| Compat, tick ×0.49 | 10 | 0.61° | 11.69 s | 23.5 | 0.09° | 20.03 s | 6 / 1 |
| Stock, tick ×2.05 | 12 | 1.07° | 11.99 s | 44.5 | 3.48° | 18.77 s | 3 / 1 |

- The faster-tick compat matches stock on the first four metrics (p 0.34-0.98) and differs from compat (p ≤ 0.003).
- The slower-tick stock matches compat on the first four metrics (p 0.53-1.0) and differs from stock (p ≤ 0.023).
- Each modified firmware also differs from its new peer in pitch oscillation (p = 0.02), in the direction of its own tick time: compat at ×0.49 publishes earlier than stock does.

### SOC20

The same swap decides the SOC20 cutoff (fixed timing, distinct runs over six quanta):

| Run set | Cutoff times | Median | p against stock | p against compat |
|---|---|---:|---:|---:|
| Stock | 16.81-18.01 s | 17.38 s | - | 0.011 |
| Compat | 16.13-17.32 s | 16.57 s | 0.011 | - |
| Compat, tick ×0.49 | 17.68-17.99 s | 17.81 s | 0.068 | 0.009 |
| Stock, tick ×2.05 | 15.98-16.72 s | 16.47 s | 0.004 | 0.93 |

## What this establishes

- Compat differs from stock in behaviour through execution time, not through its arithmetic.
  Its RTC tick publishes the motor command about 0.6 ms later, which changes how the wheel leaves voltage saturation.
  That changes the PWM tiltback and the balance oscillation, and moves SOC20's cutoff 0.8 s earlier.
  On hardware the same image would have the same tick time, so this is a firmware property, not a simulator artefact (INFERRED).
- The SOC50 cutoff time and the 70 km/h gate are chaotic at the stability limit.
  Numerical perturbations alone spread them by more than 2 s in every group, including both modified groups.
  A single stock/compat comparison of the full ride therefore cannot pass or fail on its own.
- The finding went to the lead as a firmware item.
  The fix direction keeps compat's results bit-identical and makes the tick as fast as stock's.
  [The speed-up](#the-speed-up-2026-10-22-not-yet-sufficient) does that part of the way.
  Stock also writes its display before its command store, so compat's work before the store matches stock's order.
- The first studies' open questions 2 and 3 now have an answer: the systematic part is tick latency, and the rest is chaos.
  The startup difference between quanta (question 1) is a quantum effect of the shared engine, not of compat.
  It is a timestamp effect: MMIO accesses see the quantum's start time ([CPU timing, next steps](cpu-timing.md#next-steps)).

## The speed-up (2026-10-22, not yet sufficient)

The firmware changes below shorten compat's tick (and clean's) without changing any result.
Each is checked against the code it replaces, bit for bit, or moves work in time only.

| Change | Where | Check |
|---|---|---|
| Own double divide, reached through the linker's `--wrap` (about 105 instructions per call in the ride, libgcc 528) | `firmware/core/math/soft_float.c` | 1 M random and constructed operand pairs against libgcc's `__aeabi_ddiv` on our Cortex-M3 core, 0 differ; Unicorn spot runs and a table of 38 special pairs agree; host unit test against hardware division |
| Own float comparisons `lt`, `le`, `ge`, `gt` (about 21 instructions, libgcc 36-41) | same | 1 M operand pairs each against libgcc, 0 differ |
| Exact integer `euc_sqrt` (about 68 instructions, the old digit loop about 597) | `firmware/core/math/libm.c` | 1 M operands against HEAD's `libm.c`, 0 differ; `euc_asin`, `euc_atan2` and `euc_log` 1 M each, 0 differ, half of the asin operands uniform in [-1, 1]; 29 boundary inputs identical on both cores |
| LED strip pattern 1 computes the 12 colour-wheel steps that reach an LED instead of stepping 72 times | `firmware/core/lights/strip.c` | HEAD's module against the new one on 17.7 M random tick and render calls, both profiles: state bytes and changed masks identical after every call; a unit test against an independent 72-step reference at all 74 ring positions |
| `motor_wrap_i16` in a form GCC inlines | `firmware/core/motor/motor_internal.h` | all 4,294,934,528 inputs where the old form is defined, 0 differ |
| No `memset` of the FOC output in the 24 kHz step | `firmware/core/motor/motor.c` | every field is set; the struct has no padding |
| The LED strip renders in the main loop, as stock's does: the tick queues each tick's strip inputs, the main loop renders the queued ticks in order | `firmware/apps/begode-extreme/` (`app.h`, `control.c`); [firmware note](../../firmware/docs/watchdog-and-overcurrent.md#led-strip-in-the-main-loop-both-profiles) | no tick dropped in any run (below); HEAD's SIL library against the new one on three scenarios and both profiles: every channel, firmware value and BLE frame identical, and each of the 98 samples where the strip state differs is the same sequence seen before its next step |

All routines were written for this project from the IEEE 754 rules; no library or vendor code was used.
libgcc's double add, subtract and multiply stay: they are fast, and libgcc's subtraction rounds about 1 in 25,000 random effective subtractions with an exponent gap of 33 or 34 differently from IEEE (the same on our core and on Unicorn), so an IEEE replacement would change compat's results.
An always-inline variant of the small float helpers in `float_ops.h` saved about 500 cycles per tick for 2,248 bytes and was dropped.

The strip queue holds 32 ticks (64 ms). Two simulator probes watch it: `strip_jobs_dropped` (ticks that found it full) and `strip_jobs_most_waiting`.
Over 72 runs (the 20 shipped scenarios on compat, four clean rides, six SIL runs, nine SOC compare runs, the 30-run SOC50 ensemble, SOC50 traces of compat and clean, and a balance-60s trace) no tick was dropped, and at most 7 ticks (14 ms) waited at once (clean); compat's largest backlog is 5 ticks, in the SOC50 trace once, at 4.24 s, just after the stand is released.
The only longer main-loop waits are the 200 ms delays of the power-off and bootloader paths, after which the wheel turns off or resets.
The tail-light breathing keeps its period: 2.130 s on balance-60s with HEAD's compat and with this one (video 2.1 ± 0.2 s).
The full firmware gate passes; compat grows from 34,336 to 35,012 bytes and clean from 36,760 to 37,448 bytes (warning at 45,056); `probes.toml` and its layout id are regenerated for the two probes.

SOC50, quantum 4000, fixed timing, 4.5-9 s (OBSERVED):

| | Stock | Compat before | Compat now | Clean before | Clean now |
|---|---:|---:|---:|---:|---:|
| RTC tick, cycles per call | 37,651 | 77,888 | 45,491 | 72,872 | 40,677 |
| ADC1_2 (FOC), cycles per call | 671 | 796 | 632 | 756 | 709 |
| All interrupts, CPU share | 63.7 % | 94.6 % | 66.6 % | 92.3 % | 68.1 % |
| RTC instructions, IMU read to command store, median | 16,470 | 29,587 | 20,569 | - | - |
| RTC instructions per tick, median | 17,403 | 36,441 | 21,919 | - | - |

The regime metrics over 25-30 distinct runs per group (17 quanta from 4000 to 100 and 13 more below 1250, where the trajectories part). "Before the strip move" is this build with the strip still rendered in the tick:

| Run set | Distinct | Tiltback peak | Last saturated | Alarm samples | Saturated 11-15 s | Pitch RMS 15-17 s |
|---|---:|---:|---:|---:|---:|---:|
| Stock | 29 | 0.65° | 11.73 s | 25 | 5.0 % | 0.15° |
| Compat before | 12 | 0.98° | 12.00 s | 41.5 | 7.3 % | 2.22° |
| Compat now | 29 | 0.80° | 11.85 s | 33 | 6.0 % | 0.60° |
| Compat, before the strip move | 25 | 0.82° | 11.85 s | 29 | 6.0 % | 0.67° |
| the same, tick ×0.88 | 25 | 0.70° | 11.74 s | 27 | 5.0 % | 0.36° |
| the same, tick ×0.75 | 26 | 0.69° | 11.75 s | 26.5 | 5.2 % | 0.11° |

- Compat now still differs from stock: p = 0.001-0.005 on all five metrics. It also differs from compat before (p 0.025-0.07 on the first four metrics).
- Moving the strip work out of the tick did not move the regime (0.80° against 0.82°): it shortened the tick after the command store, not before it.
- At stock's tick cost (×0.75, private counterfactual engine, which scales the whole tick) no metric differs from stock (p 0.07-0.67); at ×0.88 it comes close (p 0.07-0.62), except the pitch oscillation (p = 0.047).
- Publish latency alone at stock's level is not enough either.
  On the first build of the speed-up, ×0.80 puts the command store at stock's 17.0 k instructions, while the tick stays at 22.4 k (stock 17.4 k).
  Its tiltback peak still differs from stock's (0.78°, p = 0.018 over 10 runs).
  So both the work before the command store and the whole tick have to come down to stock's.
- With 12 runs per group the p-values swing: two builds with the same tick cost gave p = 0.10 and p = 0.003 for the tiltback peak. The larger groups above replace the 12-run comparison of the causal test for this purpose.

What remains of compat's extra tick is almost all before the command store, about 4,100 instructions per tick:

- About 1,900 instructions in libgcc's double add, slower than stock's library; replacing it would change results (above).
- About 750 instructions in the float comparison calls; GCC calls a helper for each float comparison, where stock's image makes about a quarter as many comparison calls (INFERRED: its compiler compares many inline).
- About 600 instructions in the out-of-line `euc_f32_flush` calls (the KEIL_FTZ flush), and about 350 in the divide, which is still slower than stock's.

The speed-up is accepted for this phase as it is: bit-identical, with compat at 66.6 % interrupt load against stock's 63.7 %.
Both comparison exceptions stay.

Next step (open): in the hot balance code, compare floats on their bit patterns and inline the KEIL_FTZ flush at the call sites, instead of calling the helpers.
Every result stays bit-identical; the comparison and flush calls above are about 1,350 instructions of the 4,100 before the command store.
Rejected: an own double add that reproduces libgcc's rare misrounding.
Only black-box tests could pin that misrounding down, so bit-identity could not be shown.

Both compat and clean ran at 90-98 % interrupt load before the speed-up.
That is a real-hardware risk for those builds, not only a simulator effect: on a real STM32 the main loop gets 2-10 % of the CPU and may be starved, with late display, storage and BLE work (INFERRED; the flash timing model gives the same picture, 93.8 %).
The speed-up brings compat to 66.6 % and clean to 68.1 %; stock runs at about 64 %.

Stock against compat now, `euc sim compare`, fixed timing, macOS arm64, base `0575f14` (OBSERVED; gates: launch, build, accelerate, brake, settle):

| Scenario | Quantum | Stock | Compat now | Verdicts | Shipped tolerances |
|---|---:|---|---|---|---|
| SOC100 | 4000 | all gates, 72.25 km/h | all gates, 72.31 km/h | agree | pass |
| SOC100 | 3000 | all gates, 72.33 km/h | all gates, 72.28 km/h | agree | pass |
| SOC100 | 2000 | all gates, 72.25 km/h | all gates, 72.31 km/h | agree | pass |
| SOC50 | 4000 | cut 18.56 s | cut 18.77 s | agree | pass |
| SOC50 | 3000 | accelerate gate, cut 19.87 s | cut 18.81 s | differ | fail: battery 0.542 V against 0.5 V before 9 s (HEAD's compat: 0.538 V) |
| SOC50 | 2000 | cut 18.56 s | cut 19.61 s | agree | pass |
| SOC20 | 4000 | cut 16.81 s | cut 16.66 s | agree | pass |
| SOC20 | 3000 | cut 17.45 s | cut 16.54 s | agree | pass |
| SOC20 | 2000 | cut 16.81 s | cut 17.13 s | agree | pass |

- SOC20's cutoffs (16.54-17.13 s) now overlap stock's range (16.81-17.45 s); before the speed-up compat cut about 0.8 s earlier.
- Over the whole ride both SOC100 and SOC50 exceed the default limits at every quantum (at SOC100 the battery voltage from 9.5 s, then a few km/h and up to about 30° of pitch while braking from 20.9 s), so the 9.0 s window stays.
- Both exceptions in `scenarios/compare/compat-vs-stock.toml` stay. SOC50's verdicts still differ at quantum 3000. SOC100's agree at all three quanta here, but they already did on macOS before the speed-up; the platform-sensitive case is the Linux x86-64 runner, which CI runs when this lands.
- The CI compare loop (20 scenarios, default quantum) passes, and so do sil's six SIL-against-image compares ([SIL](../sil.md#validation)).

## Evidence

The private evidence archive is `research/timing-proto/m5-20261022/m5-evidence.tar.gz` beside the main checkout.
Its `SHA256SUMS` and `README.md` list the content.
It holds the scripts, the reports and 100 Hz traces of every run, the profiles, and the private engine hooks.
The working copies are under `target/m5-scratch/` in the shared worktree, which git ignores.
The speed-up's evidence is a second archive, `research/timing-proto/m5-speedup-20261022/m5-speedup-evidence.tar.gz`, with its own `README.md` and `SHA256SUMS`.
It holds the target harness, the host checks, every measured build, the gate logs, the ensembles with traces and the compares.
