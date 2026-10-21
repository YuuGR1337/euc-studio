# Full-effort acceleration and braking

## Current baseline

Date: 2026-10-21. Working-tree base: `aa129c4`.
The tested tree includes the corrected RTC and the rigid rider's retained yaw-rate damping at speed.
The three cases use the generic 80 kg `rigid_ankle_v1` rider.
They start at 100%, 50%, and 20% SOC.
The target remains measured forward ground speed of 70 km/h.
All cases retain the same rider commands, firmware settings, and checks.

These cases test a bounded effort sequence.
They do not establish the fastest possible trajectory or a real-wheel operating limit.
The rider receives lean commands, never a scripted motor torque, speed, pose, or battery-current command.
The current profile uses the 1.7 suspension linkage ratio.
The motor, tire, suspension, drag, and battery parameters still lack full ride validation.

All results below use the own CPU, CPI 2.0, exception costs of 14/14/8 cycles, and 100 Hz sampling.
The instruction quanta are 4000, 3000, and 2000.
The final matrix contains 27 full-effort runs.
Six runs pass, and 21 fail a phase or scenario check.
None halts the emulator.

## Commands and unchanged checks

The rider retains the 400 N·m ankle cap, 30° lean cap, and 24°/s lean slew limit.
The stand releases at 4 s.
The first phase starts at 6 s.

| Phase | Desired lean | Measured transition | Timeout |
|---|---:|---|---:|
| `launch` | +30° | Above 35 km/h | 15 s |
| `build` | +20° | Above 50 km/h | 10 s |
| `accelerate` | +15° | Above 70 km/h | 40 s |
| `brake` | -30° | Below 15 km/h | 15 s |
| `settle` | -5° | Below 1 km/h | 15 s |

All transitions use actual speed crossings.
A cutoff, fall, halt, or timeout leaves later phases `not_started`.
Incomplete gates fail the generated phase checks.
The upright, no-cutoff, and final-speed checks retain their original thresholds.
No assertion or command changed to obtain these results.

The last crossing starts two seconds of ordinary zero-speed rider control.
This observation does not reset speed or pose.
The maximum duration is 103 s, including all timeouts and final observation.
Manual lean bypasses the speed controller's tiltback easing.
Firmware tiltback and cutoff rules remain active.
The rigid rider's external roll and yaw aids also remain active.

## Results

Maximum acceleration speed below excludes braking and motion after phase failure.
A failed 70 km/h gate does not constitute a completed braking test.

| Firmware | Initial SOC | Complete passing runs | Maximum acceleration speed | Result |
|---|---:|---:|---:|---|
| Stock | 100% | 3/3 | 70.01–70.04 km/h | Completes all five gates |
| Stock | 50% | 0/3 | 68.24–70.03 km/h | Two acceleration cutoffs; one braking cutoff |
| Stock | 20% | 0/3 | 65.40–66.83 km/h | Cutoff before 70 km/h |
| Compat | 100% | 3/3 | 70.04 km/h | Completes all five gates |
| Compat | 50% | 0/3 | 70.10 km/h | Reaches 70 km/h; cuts during braking |
| Compat | 20% | 0/3 | 65.15 km/h | Cutoff before 70 km/h |
| Clean | 100% | 0/3 | 69.89–69.95 km/h | Upright without cutoff; acceleration times out |
| Clean | 50% | 0/3 | 69.81–70.02 km/h | One acceleration cutoff; two failures during braking |
| Clean | 20% | 0/3 | 65.00–65.39 km/h | Cutoff before 70 km/h |

The passing full-battery runs have these measurements.
Acceleration time starts at the first effort command.
Braking time and signed forward distance include `brake` and `settle`.
They end at the below-1-km/h crossing, before final observation.

| Firmware | Acceleration time | Braking time | Braking distance |
|---|---:|---:|---:|
| Stock | 14.84–14.87 s | 6.69–6.73 s | 64.54–64.62 m |
| Compat | 14.94 s | 6.67 s | 64.42 m |

At quantum 4000, stock briefly reaches 72.25 km/h during the braking transition.
Its final sampled speed is -0.36 km/h.
The case reaches the specified stopping band, rather than a permanently motionless equilibrium.
Clean never starts a full-battery braking phase in this matrix.
Its timeout proves neither a cutoff nor successful braking.

### The unresolved 50% SOC difference

| Quantum | Stock | Compat | Clean |
|---:|---|---|---|
| 4000 | Cutoff before 70 km/h | Cutoff during braking | Cutoff before 70 km/h |
| 3000 | Cutoff during braking | Cutoff during braking | Cutoff during braking |
| 2000 | Cutoff before 70 km/h | Cutoff during braking | Fall during braking; cutoff follows |

Stock cuts at 18.56–19.87 s across these quanta.
Compat cuts at 19.71 s at every quantum.
Clean cuts at 24.03–27.95 s.
At clean quantum 2000, the fall occurs at 24.00 s and cutoff follows at 24.03 s.
The cutoff does not initiate that particular fall.

The standard reports expose `balance_disabled`, but do not identify the first disabling instruction or detector.
Instruction timing and current tracking near voltage saturation remain possible causes of the stock/compat difference.
These reports do not establish either cause.
A further investigation needs aligned current, voltage, detector-counter, and first-store traces.
No speculative compat behavior change follows from this matrix.

## Comparison limits

The existing stock/compat tolerance file compares full-effort samples only through 9.0 s.
This interval covers launch, before high-speed acceleration and braking.
At 50% SOC, `same_checks = false` also skips equality of scenario-check verdicts.
The complete scenario failures above remain relevant even when the windowed comparison passes.
A passing window does not establish complete-ride equivalence.
The tolerance thresholds and comparison windows did not change for this validation.

The 50% SOC launch comparison passes at quanta 4000 and 2000.
It fails at quantum 3000.
Battery voltage differs by 0.53842 V at 7.08 s against the unchanged 0.5 V limit.
The largest speed difference in that window is 0.08744 km/h.
This sampled voltage difference remains unresolved.

## Identity and reproduction

The stock image is the explicit local GW1825007 build.
The CLI verifies its digest before it applies `stockmap.toml`.
Own firmware uses a matching sibling ELF and the current `probes.toml`.
Both target builds pass their probe-layout checks.
The local manifest records source hashes and every frozen input hash.

Final evidence is under `target/fw-scratch/resume-accepted/`.
Its `matrix/` directory contains the JSON reports.
Its `manifest.json` identifies the source and frozen inputs.
The earlier `resume-final/` directory contains a rejected steering-regression baseline, not these acceptance results.

| Input | SHA-256 |
|---|---|
| Stock image | `cbfb328cab1e0b8219790dd3ef710bb266569c70b6ac396ccd06c247da9fd27d` |
| Compat image, 34,336 bytes | `7fc8a128adc07221fc4fd02f33d62e8015b33c575bcb4dbfe1f5d98c61f5e6bf` |
| Clean image, 36,760 bytes | `e69b05435c2e58078774fdb7cc67b511b12487c12c41ceba5c5d17ce59cfdf83` |
| Plant profile | `fd03b3d51ebeec2af968b001f4378ff42640d2aead0afa19e09349e7657f036a` |
| Probe profile | `2cbc2766af8586916fd0b0ae83517c766c3242b51fb61f4196211ca49041883f` |
| Release CLI | `a31f21823637a294570bfbca5aef9b8500f6cb405e158a561c2a644661d9cea4` |

Build and run with explicit local inputs:

```sh
cargo build --release -p euc-cli
target/release/euc --json --profiles /absolute/path/to/frozen/profiles \
  sim run --scenario scenarios/full-effort-70kmh-stop-soc100.toml \
  --firmware /absolute/path/to/pinned-stock.bin --cpu own \
  --max-quantum 4000 --record /absolute/path/to/output/ride.eucrec
```

Repeat for all three SOC files, all three firmware images, and all three quanta.
Use explicit local firmware paths to avoid a download.
Keep the local MuJoCo library available to the executable.

## Evidence limits

All current and voltage extrema come from 100 Hz samples.
They do not bound electrical-step peaks.
The rigid rider cannot bend its knees or shift foot contact.
Its external roll and yaw aids can add energy and hide lateral instability.
Initial 100% SOC also does not mean full SOC at the braking gate.
Acceleration consumes charge before regeneration starts.

The earlier acceptance report used different firmware and mechanical inputs.
Its compat 50% pass and clean full-battery pass do not describe this baseline.
The earlier search over lean rates and private rider-mass variants does not establish an optimum for the current model.
The public generic-rider results above remain separate from private owner measurements.
