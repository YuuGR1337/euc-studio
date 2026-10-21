# Native FreeRide acceptance

## Scope and identity

These checks use the completion checkout on 2026-10-16.
They use the actual native `build_free_host` factory, stock firmware, electrical plant, and MuJoCo bodies.
The CPU backend is `Own`. No hardware, UI, audio device, or recording capture runs.

The tests remain explicitly ignored because they need verified firmware in the local cache.
They fail before host construction if that cache entry is absent.
They do not fetch firmware for acceptance.

The test checks this full SHA before it accepts the stock probe map:

```text
Build: GW1825007
SHA256: cbfb328cab1e0b8219790dd3ef710bb266569c70b6ac396ccd06c247da9fd27d
Stock map build: GW1825007
```

The checks use the current registered `stm32f1.rtc` implementation without an override.
The profile routes RTC IRQ 3 and uses its RCC clock.
The RTC source SHA256 at execution is:

```text
4f9ab90172f678563c74405583e8696124776534161d877a92bcff1c8325be75
```

This update changes only test modules in `app/src-tauri/src/engine/free_ride.rs` and this document.
The production prefix in `free_ride.rs` stays byte-for-byte unchanged.

## Test inputs and observations

Both models use neutral settings, map seed 17, and run epoch 700.
The generated map has revision 700 and bounds from -16 m to +16 m on each horizontal axis.
Each test first runs the actual four-second stand boot.
It then requires fresh neutral input and explicit arming.
The neutral gate uses six actual 50 ms wall intervals with fresh input packets.
The test does not change the gate timestamps.

The sustained tests send one ordinary `ManualInput` packet before each 120 Hz host sample.
A test policy requests lean from speed error: `clamp(0.05 * (target_m_s - measured_m_s), -0.04, 0.04)`.
This policy supplies manual requests through the same native mailbox.
It does not write firmware RAM, body poses, wheel velocity, or physics state.
It does not establish speed tracking accuracy.

The tests inspect actual snapshot positions and quaternions.
Heading comes from the shell quaternion.
They calculate edge clearance independently from those positions and the host's specified footprint sizes.
The shell footprint uses half extents `[0.5, 0.3]` m.
The rider footprint uses half extents `[0.8, 0.8]` m.
No test moves a body to manufacture an edge crossing.

Every active sample requires finite body poses, no native halt, and no firmware fault or cutoff event.
It also requires shell pitch and roll below 10 degrees.
The sustained steering runs additionally require armed and ready status at every sample.

## Observed results

### Short stand, arm, lean, and neutral

Both existing six-second tests passed again with the current RTC.
Their driven portion lasts two simulated seconds after boot.
They check actual wheel travel, phase current, torque, firmware drive command, and articulated joint response.
They do not prove sustained riding.

### Steering on generated terrain

Each model runs 14 driven seconds after boot on a generated +1% grade along world x.
The protected spawn patch remains level.
The policy requests 0.6 m/s.
The test drives straight for four seconds, requests +0.08 rad/s for six seconds, then holds heading for four seconds.
The expected heading change is 0.48 rad.
Both physical paths leave the level spawn patch and cross nonzero terrain heights.

| Observation | Rigid | Articulated |
|---|---:|---:|
| Driven time, s | 14.000000 | 14.000000 |
| Physical path length, m | 5.978776 | 7.258831 |
| Actual heading change, degrees | 27.500565 | 27.499901 |
| World-y displacement, m | 1.623561 | 2.242375 |
| Maximum ground height under shell, m | 0.056092 | 0.067808 |
| Minimum footprint clearance, m | 9.607651 | 8.457504 |
| Peak shell roll, degrees | 0.077736 | 0.102008 |
| Peak motor iq, A | 28.721294 | 24.053394 |
| External yaw aid work, J | 0.205105 | 0.229890 |

The yaw aid contributes external energy. These checks do not establish unsupported human steering behavior.

### Pause and neutral rearm

Each model drives for three seconds before pause and three seconds after rearm on a flat generated map.
Pause clears manual input and freezes the actual body poses and simulation time.

One hundred twenty additional paused host calls produce no physical advance.
Immediate arming and an armed motion packet fail.
The test changes the input source from KeyboardSlider to Gamepad.
Fresh neutral input starts a new interval before explicit rearming succeeds.
The armed packet remains neutral before physical motion resumes.

The old turn request does not resume.

| Observation | Rigid | Articulated |
|---|---:|---:|
| Paused simulation time, s | 7.000001 | 7.000001 |
| Final simulation time, s | 10.000001 | 10.000001 |
| Heading at pause, degrees | 8.310705 | 8.313063 |
| Heading after resumed phase, degrees | 8.310994 | 8.314469 |
| Final speed, km/h | 1.891454 | 2.114439 |

This checks the host and mailbox contract. It does not run the desktop runner, focus handling, or UI controls.

### Finite map edge admission

Each model drives toward the boundary of a flat generated map through ordinary input packets.
The test policy requests 0.8 m/s and zero turn rate.
The test allows at most 45 driven simulated seconds to reach the guard.
It requires passage through the 2 m warning band while input remains armed.
It then requires the host to reject further admission at the 1 m pause band.

| Observation | Rigid | Articulated |
|---|---:|---:|
| Driven time to stop, s | 21.050000 | 17.633334 |
| Actual shell x at stop, m | 14.202878 | 14.198030 |
| Footprint clearance one sample before stop, m | 1.001901 | 1.003638 |
| Footprint clearance at stop, m | 0.995266 | 0.995115 |

Both runs report `Map edge. Reset before the finite ground ends`.
The map guard disarms input and rejects rearming.
Both footprints retain positive clearance, and both actual body centers retain valid ground samples.
An additional one-second host call leaves the time and all body poses unchanged.
The finite surface ends at +16 m. Neither run reaches that surface boundary.

## Reproduction

The new tests use the prefix `engine::free_ride::stock_rider_acceptance::`:

- `stock_rigid_sustained_steering_on_generated_terrain`
- `stock_articulated_sustained_steering_on_generated_terrain`
- `stock_rigid_pause_and_neutral_rearm`
- `stock_articulated_pause_and_neutral_rearm`
- `stock_rigid_finite_map_edge_admission`
- `stock_articulated_finite_map_edge_admission`

Run these commands from the repository root with the pinned firmware already cached.
Use the local Python path for the current native dependency build.
The MuJoCo path below uses the repository's existing downloaded library.

```sh
export PYO3_PYTHON=/opt/homebrew/bin/python3
export MUJOCO_DYNAMIC_LINK_DIR="$PWD/.cache/mujoco/mujoco-3.9.0/lib"
export DYLD_LIBRARY_PATH="$MUJOCO_DYNAMIC_LINK_DIR"

cargo test --manifest-path app/src-tauri/Cargo.toml --lib \
  engine::free_ride::stock_rider_acceptance -- --ignored --nocapture --test-threads=1

cargo test --manifest-path app/src-tauri/Cargo.toml --lib \
  engine::free_ride::tests -- --test-threads=1

cargo clippy --manifest-path app/src-tauri/Cargo.toml --lib --tests -- -D warnings
rustfmt --check --edition 2024 app/src-tauri/src/engine/free_ride.rs
```

The combined acceptance command includes six new cases, two existing short cases, and one existing exact COM/joint/motor trace.
The combined run passed all nine tests with zero failures in 68.48 wall seconds.
Focused executions also passed all six new cases and both short cases.
The stop comparison permits 0.1 mm of rounding error in streamed float32 positions.
The warning-band assertion excludes that rounding margin at the exact pause threshold.
Both edge tests passed again after this rounding-margin check, with the same stopping positions.

| Final check | Result |
|---|---|
| Cached-stock acceptance group | 9 passed, 0 failed |
| Final focused edge rerun | 2 passed, 0 failed |
| FreeRide mailbox and context unit tests | 24 passed, 0 failed |
| Native library and test Clippy | Passed with warnings denied |
| `free_ride.rs` format check | Passed |
| Production prefix hash comparison | Unchanged |

No production defect appeared in these bounded runs. This statement does not cover untested durations or inputs.

## Limits

The runs establish only the stated bounded model behavior on this checkout and CPU backend.
They do not establish 600-second stability, arbitrary terrain handling, high-speed behavior, or performance on another host.
They do not establish safety, calibration accuracy, BMS behavior, or physical rider validity.
The models retain pedal constraints and external roll and yaw aids.
No hardware measurement, device access, commit, or deployment forms part of this acceptance task.
