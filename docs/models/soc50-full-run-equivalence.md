# SOC50 full-run equivalence: tick latency and chaos

Date: 2026-10-22. Source base: `63801a2` plus the stage B timing work ([CPU timing](cpu-timing.md)).
This continues the [first](soc50-investigation.md) and [second](soc50-high-speed-investigation.md) SOC50 studies.

**Result.** Compat performs stock's arithmetic, but its RTC tick handler takes about twice stock's cycles.
That later command publication systematically changes the high-speed regime of both full-effort cases that fail.
A causal test that swaps only the tick cost swaps the regimes.
On top of this, the final cutoff time is chaotic at the wheel's stability limit: numerical perturbations alone move it by 2-3 s.
No tolerance, comparison window, scenario, physical input or firmware image changes in this study.

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
  The fix direction keeps compat's results bit-identical and makes the tick as fast as stock's:
  a faster correctly rounded double divide and add, a faster bit-identical `euc_asin`, and a check that no non-balance work precedes the command publication.
  Third-party soft-float code must be licence-compatible: GPL code such as qfplib cannot be used.
- The first studies' open questions 2 and 3 now have an answer: the systematic part is tick latency, and the rest is chaos.
  The startup difference between quanta (question 1) is a quantum effect of the shared engine, not of compat.
  It is a timestamp effect: MMIO accesses see the quantum's start time ([CPU timing, next steps](cpu-timing.md#next-steps)).

## Evidence

The private evidence archive is `research/timing-proto/m5-20261022/m5-evidence.tar.gz` beside the main checkout.
Its `SHA256SUMS` and `README.md` list the content.
It holds the scripts, the reports and 100 Hz traces of every run, the profiles, and the private engine hooks.
The working copies are under `target/m5-scratch/` in the shared worktree, which git ignores.
