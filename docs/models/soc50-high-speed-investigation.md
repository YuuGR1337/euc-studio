# SOC50 high-speed investigation

Date: 2026-10-21. Production source base: `70a827e`.
This continues the [first SOC50 investigation](soc50-investigation.md).
The launch trigger is now identified, and the actuator-clamp candidate is ruled out in two stock runs.
High-speed current demand rises before tracking fails.
The closed-loop outcome remains sensitive to CPU quantum size, including the finest tested sizes.
The accepted firmware images, profiles, physical model, scenario, and comparison limits stay unchanged.
Private observation hooks and numerical sensitivity runs retain separate identities.

## Launch cause confirmed

The 7.08 s transient follows entry into a new Hall sector after angle interpolation timed out.
Both images load angle -16385, then -21888, a change of -30.2289°.
This is an estimated electrical angle, not an instantaneous physical wheel rotation.

| Field at the FOC angle load | Stock | Compat |
|---|---|---|
| Sector | 3 → 1 | 3 → 1 |
| Calls since edge | 235 → 1 | 234 → 1 |
| Interpolation progress | -7960 → -41 | -7960 → -41 |
| Entry offset | 0 → 5461 | 0 → 5461 |
| Interpolation active | 0 → 1 | 0 → 1 |
| Id gain | 10 → 10 | 10 → 10 |

The new angle rotates the measured current coordinates.
The following FOC update produces the current and voltage values in the earlier PWM trace.
The gain does not change.
The timeout itself produces a separate positive angle step of approximately 13.5–13.7°.
It does not produce this negative 30.23° step.

The observer records the exact angle-load instructions and resulting CPU registers.
Stock reaches this new sector before the 7.08 s sample; compat reaches it afterward.
Their later complete PWM transfers remain 79.9167 µs apart.
All stock and compat q3000 traced samples and reports match their plain runs.
The earlier reason for the small trajectory phase difference remains open.
This event does not expose a different Hall-transition algorithm in compat.

## Current demand rises before tracking fails

A private engine copy observes successful motor-command stores and existing CCR1 writes.
It reads mapped SRAM and existing plant state without advancing the plant.
Command observations cover 11–20 s.
FOC observations cover 11.35–11.8 s and 16.3–16.9 s.
All three complete runs match their original samples, poses, events, and reports, except wall timing.

The trace clock is the existing quantum-start clock.
Its labels do not establish exact instruction timestamps.
Trace order establishes which command publication precedes each FOC observation.
Iq reference is inferred from the published command using the verified current-mode conversion.
The equal-preemption-priority TIM8 and ADC handlers cannot preempt each other in this configuration.
The RTC task cannot change the command during the ADC handler.

For diagnosis, a voltage-limited tracking episode requires these conditions:

- Positive Iq reference.
- `vq_active > 31000`.
- `Iq reference − measured Iq > 2000` Q15 counts.

These conditions label observations; they do not replace acceptance thresholds.
An episode is persistent when those conditions span at least 1 ms of trace-clock labels.
Pre-circle active Vq differs from the post-circle voltage vector.

All three runs have an early episode after the 50 km/h phase transition at approximately 11.44 s.
The command rises before the persistent combined deficit.

| Run | Early persistent-deficit clock label | Command near onset |
|---|---:|---:|
| Stock q3000 | 11.475521 s | Approximately 560 |
| Stock q4000 | 11.483840 s | Approximately 560 |
| Compat q3000 | 11.476184 s | Approximately 560 |

These commands remain below the actuator cap.
The runs recover from this episode and track current closely through approximately 15.5 s in the 100 Hz evidence.

Compat later develops a larger command oscillation.
Its command rises from 40.35 at label 16.279500 s to 343.19 at 16.307726 s.
Current error first exceeds 2000 counts at 16.303684 s.
Active Vq first exceeds 31000 at 16.307767 s.
The first persistent combined deficit starts at 16.308684 s.
Its inferred Iq reference is 6863, measured Iq is 4172, and active Vq is 31047.
The latest command publication precedes that FOC event in trace order.

The preceding 14 command gaps span 1.746–2.229 ms.
No long command gap immediately initiates this episode.
Across 11–20 s, compat's mean command gap is 2.045475 ms and its maximum is 6.166917 ms.
Stock's mean gap is approximately 2.036 ms, with a similar maximum.
Each run has 44 gaps longer than 4 ms in that interval.
These cadence results do not exclude effects from earlier delayed updates.

Neither stock run has the same combined deficit in the traced 16.3–16.9 s interval.
The 100 Hz evidence first identifies later episodes at 17.09 s for stock q4000 and 17.20 s for stock q3000.
The observed command rise therefore precedes the later current-tracking loss.
The reason for the different command oscillations remains open.

## The actuator boundary does not explain these two stock outcomes

Stock's soft-start limit alternates between 1050 and 1051.
Its setter forwards the resulting command.
Compat deliberately clamps its motor input to ±1050.
The first exact stock publications above 1050 occur at labels 11.519000 s for q3000 and 17.375466 s for q4000.

A separate private experiment replaces only stock command values in `(1050, 1051]` with 1050.
The replacement occurs after the verified successful command store, before the next instruction.
The runner verifies the pinned stock image.
The callback selects store PC `0x080096CA` and address `0x20000394`.
Its CPU caller restricts callbacks to four-byte writes.
Other out-of-range values stop the experiment.
The replacement adds no emulated instruction, scheduler event, or peripheral read.
All other inputs stay fixed.
This is a causal intervention, not an acceptance run or a production firmware change.

| Check | Stock q3000 | Stock q4000 |
|---|---:|---:|
| Replaced publications | 366 | 167 |
| Fractional publications in that count | 1 | 0 |
| Identical samples before first replacement | 1151 | 1737 |
| Complete recorded samples and events after intervention | Identical | Identical |

Default-off controls match the original complete sample streams, events, and reports.
The full command-write prefixes also match before the first replacement.
The q3000 FOC trace sees 539 observations with a changed stored command.
Its observed voltage outputs and physical states remain identical.
Three q-integral observations differ, without changing those voltage outputs.
The q4000 replacements start after the available FOC trace intervals.
Its complete 100 Hz sample stream and outcome still remain identical.

An earlier exact-1051 pilot stopped on the legitimate fractional command 1050.9132.
That stopped pilot remains archived separately.
The final bounded experiment includes that value and completes normally.

This deliberate actuator boundary does not explain the two tested stock outcomes.
It remains a finite-command difference between stock and compat.
The experiment does not justify removing the boundary or claiming general firmware equivalence.

## Numerical timing sensitivity remains

Sixteen additional runs change only the maximum CPU quantum.
They use the frozen CLI, images, scenario, profiles, CPI 2.0, exception costs, and 100 Hz sampling.
The table also includes the six accepted stock/compat baseline runs.
None reports an emulator halt.

| Quantum | Stock reaches 70 km/h | Stock first failure | Compat reaches 70 km/h | Compat first failure |
|---:|---|---|---|---|
| 4000 | No | Cutoff at 18.56 s | Yes | Cutoff at 19.71 s |
| 3000 | Yes | Cutoff at 19.87 s | Yes | Cutoff at 19.71 s |
| 2000 | No | Cutoff at 18.56 s | Yes | Cutoff at 19.71 s |
| 1000 | No | Cutoff at 18.90 s | No | Cutoff at 19.49 s |
| 500 | Yes | Fall at 19.86 s | Yes | Cutoff at 19.25 s |
| 250 | No | Cutoff at 18.84 s | Yes | Cutoff at 20.01 s |
| 100 | No | Cutoff at 19.02 s | No | Cutoff at 18.68 s |
| 25 | No | Cutoff at 19.53 s | Yes | Cutoff at 19.79 s |
| 10 | No | Cutoff at 18.78 s | Yes | None; complete pass |
| 2 | No | Cutoff at 19.18 s | No | Cutoff at 18.38 s |
| 1 | No | Cutoff at 19.39 s | No | Cutoff at 18.66 s |

Stock q500 falls before its cutoff at 19.87 s.
That cutoff does not initiate its first recorded fall.
Compat q10 completes the full scenario, including the final observation period.
The neighboring finer runs fail, so that selected pass does not establish a correction.

At one instruction per quantum, both images cut before 70 km/h.
Stock cuts approximately 0.73 s after compat under that setting.
The last two quantum sizes still change cutoff time by approximately 0.21 s for stock and 0.28 s for compat.
The study therefore does not establish cutoff-time convergence over this trajectory.
It also does not establish a real controller's operating limit.
Fixed CPI and exception costs remain timing estimates at every quantum size.

The accepted comparison still fails.
No quantum, rider command, model parameter, safety deadline, or tolerance is selected to turn it into a pass.

## What this isolates, and what remains

The launch failure samples different phases of a shared Hall-angle transition.
The later failure follows increasing balance demand with insufficient measured current and constrained output voltage.
The documented actuator cap does not explain the tested stock trajectories.
An immediate long command gap does not initiate the traced compat episode near 16.3 s.

The origin of the different balance-command oscillations remains unresolved.
The present closed-loop comparison combines firmware logic, execution timing, sensor phase, and physical feedback.
Its timing sensitivity prevents attributing the remaining difference to one firmware rule from these results alone.
The next discriminator requires identical recorded inputs at the relevant balance and FOC calculations.
That comparison must retain separate evidence from the closed-loop physical outcomes.
Snapshot freshness also needs a separate regression and correction; it does not explain the matched-time q3000 launch sample.

## Evidence

The private follow-up archive is `research/timing-proto/soc50-20261021-followup/soc50-followup.tar.gz` beside the main checkout.
Its manifest records each retained file's SHA-256.
Build caches and plotting environments are excluded.

New evidence remains under `.cache/soc50-investigation/`:

- `hall-observer/`: executed Hall-angle loads, register values, validation, and hashes.
- `highspeed-mapping/`: derived addresses, instruction order, table parity, and remaining sampling hypotheses.
- `highspeed-trace/`: the three observation traces and exact original-result comparisons.
- `highspeed-analysis/`: existing-recording analysis and command/FOC event order.
- `clamp-experiment/`: bounded interventions, controls, and prefix/full-stream comparisons.
- `clamp-experiment-endpoint-pilot/`: the stopped pilot, retained for audit.
- `highspeed-quantum/`: separate numerical timing sensitivity runs.

The reviewed sine and circle tables match between the frozen images.
Different ADC read spacing and RTC input-capture positions remain possible sources of input differences.
The current evidence does not establish a mixed ADC pair or a mixed q-integral limit.
Those claims require observations at the original reads.
