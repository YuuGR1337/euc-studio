# Native FreeRide acceptance

## Play mode update (2026-10-22)

Free ride is now play mode: a 1.5 s stand boot, input live at once (no arming, no neutral gate),
no pause on a crash, respawn, wheel buttons, power off and on, and no run limit. The tests below
were adapted: `Rig::boot` replaces the arming step, the pause test resumes with the other source
at once, and a step-off keeps the run going. The finite map (the old test pad) keeps its edge
guard. New stock tests, all on the streaming world, seed 42:

| Test | What it checks | Observed (articulated; rigid where run) |
|---|---|---|
| `stock_*_idle_after_boot_balances_without_a_pause` | 10 s with no input after boot, then a 1 s lean and silence past the 250 ms watchdog | Peak 0.16 km/h (0.13), pitch 0.48 deg, drift 0.015 m; neutral controls after the watchdog, still on the pedals |
| `stock_*_crash_never_pauses` | Step-off at speed, then 5 s with a held lean | Off at 10.1 km/h (9.6); the run goes on 5 s, the firmware cuts and the wheel falls on its own, 840 BLE bytes in the last 2 s |
| `stock_articulated_respawns_upright_at_the_last_clear_spot_and_at_the_start` | R after a crash, then Shift+R | Upright (pitch and roll 0.00 deg) within 0.05 m of the spot; call to riding 0.81 s and 0.69 s wall |
| `stock_articulated_power_off_then_on` | A 1.5 s power hold at 2 m/s, then a hold at a stop, then a press | No power-off at speed; latch drops 0.817 s into the hold; rider off about 1.5 s after the rail goes; latched again 0.55 s into the press, display on 3.2 s later |
| `stock_articulated_buttons_while_riding` | Handle hold, two handle presses and a power press at speed; the horn command over BLE; two handle presses at a stop | Ignored at 6.8 km/h; the horn sounds; at a stop the second press cuts the drive command to 1 (19 after the first) |
| `stock_free_ride_keeps_no_ble_backlog` | 20 s with the runner's drains | 7,386 BLE bytes read, none left on the board |

Drive mode (2026-10-23), same world and rider, the owner's preset unless named, with the world's
total-mass fix (the rider senses the true air drag in the city):

| Test | What it checks | Observed |
|---|---|---|
| `{stock,compat,clean}_drive_speed_step_and_tiltback` | Half throttle 0 to 20 km/h, 6 s with no ask, half brake to a stop; then full throttle 20 s on the blank preset (tiltback at 20 km/h); quanta 4000, 3000, 2000 | All 9 runs: 20 km/h in 3.53 s, 20.0-24.8 km/h with no ask, stop in 3.3 s, then a rock within ±0.55 km/h; in the tiltback the rider eases off, top 35.4-35.7 km/h, nobody falls |
| `stock_drive_half_throttle_normal_ride_at_three_charges` | Half throttle to 40 km/h, 10 s with no ask, half brake, at 100, 50 and 20 % | 40 km/h in 6.7 s, 40.1-44.2 km/h with no ask, stop in 5.8 s, then a rock within ±0.6 km/h; no fall |
| `stock_drive_release_at_20_kmh_settles_at_the_speed_let_go` | Half, then full throttle from rest to 20 km/h, then 4 s with no ask (a let-go); bounds: peak under 6 km/h (half) and 10 km/h (full) over the let-go, settled within 1 km/h of it | Half: let go at 20.0 km/h, peak 4.7 km/h over it, 20.0 km/h after 4 s; full: let go at 20.1 km/h, peak 8.1 km/h over it, 20.9 km/h after 4 s; no fall |

The compat and clean images come from `firmware/build` (`EUC_FIRMWARE_DIR`). They must be built
from the same tree as the probe map. Top speed is not tested: the motor plant saturates near
68 km/h on a full pack, below the owner's report, and is being calibrated.

The rigid crash and idle tests passed with the numbers in brackets. The respawn wall times come from
a release build with four tests in parallel on the development Mac.

The release row's run-on is the wheel catching the rider: as the lean comes back, the firmware
drives the wheel forward under the body. The owner's ride log (2026-10-17) has the speed rising
0.6-7.9 km/h (median 3.0) from the let-go to the peak after pushes of about 2.2 m/s².

## Scope and identity

The play mode and drive mode updates above record their own runs. The sections from here to the
end record the checks of 2026-10-16, on that day's completion checkout.
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

The 2026-10-16 update changed only test modules in `app/src-tauri/src/engine/free_ride.rs` and
this document, and the production prefix in `free_ride.rs` stayed byte-for-byte unchanged. Later
updates changed production code: FR2b and play mode (2026-10-22), drive mode and the FR6 sound
(2026-10-23).

## Test inputs and observations

Both models use neutral settings, map seed 17, and run epoch 700.
The generated map has revision 700 and bounds from -16 m to +16 m on each horizontal axis.
Each test first runs the actual stand boot (four seconds when these numbers were taken; 1.5 s
since play mode). Before play mode it then required fresh neutral input and explicit arming.

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
The sustained steering runs additionally require ready status at every sample.

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

Play mode replaced this test with `stock_*_pause_and_resume`: pause clears the input, and a packet
from the other source drives at once with no neutral interval. The old turn request does not
resume. The numbers below are from the old test.

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

All the tests use the prefix `engine::free_ride::stock_rider_acceptance::`. Each one is ignored
by default.

- 2026-10-16 (results above):
  - `stock_rigid_sustained_steering_on_generated_terrain`
  - `stock_articulated_sustained_steering_on_generated_terrain`
  - `stock_rigid_pause_and_resume` (was `stock_rigid_pause_and_neutral_rearm`)
  - `stock_articulated_pause_and_resume` (was `stock_articulated_pause_and_neutral_rearm`)
  - `stock_rigid_finite_map_edge_admission`
  - `stock_articulated_finite_map_edge_admission`
  - the short `stock_rigid_stand_arm_lean_and_neutral` and
    `stock_articulated_stand_arm_lean_and_neutral`, and `stock_exact_com_joint_and_motor_trace`
- The streaming world (results in `world.md`): `stock_rigid_rides_the_streaming_world` and
  `stock_articulated_rides_the_streaming_world`
- FR2b, riding by hand (results in `body.md`, "Riding by hand"):
  - `stock_articulated_pivots_carves_and_crouches_by_hand`
  - `stock_rigid_pivots_and_carves_by_hand`
  - `stock_articulated_quick_reset_restarts_where_the_wheel_is`
- Play mode (results in the play mode table):
  - `stock_articulated_idle_after_boot_balances_without_a_pause`
  - `stock_rigid_idle_after_boot_balances_without_a_pause`
  - `stock_articulated_crash_never_pauses`
  - `stock_rigid_crash_never_pauses`
  - `stock_articulated_respawns_upright_at_the_last_clear_spot_and_at_the_start`
  - `stock_articulated_power_off_then_on`
  - `stock_articulated_buttons_while_riding`
  - `stock_free_ride_keeps_no_ble_backlog`
- Drive mode (results in the drive table):
  - `stock_drive_speed_step_and_tiltback`
  - `compat_drive_speed_step_and_tiltback` and `clean_drive_speed_step_and_tiltback` (they need
    the built compat and clean images)
  - `stock_drive_half_throttle_normal_ride_at_three_charges`
  - `stock_drive_release_at_20_kmh_settles_at_the_speed_let_go`

Run these commands from the repository root with the pinned firmware already cached.
Use the local Python path for the current native dependency build.
The MuJoCo path below uses the repository's existing downloaded library.

```sh
export PYO3_PYTHON=/opt/homebrew/bin/python3
export MUJOCO_DYNAMIC_LINK_DIR="$PWD/.cache/mujoco/mujoco-3.9.0/lib"
export DYLD_LIBRARY_PATH="$MUJOCO_DYNAMIC_LINK_DIR"

cargo test --manifest-path app/src-tauri/Cargo.toml --lib \
  engine::free_ride::stock_rider_acceptance -- --ignored --nocapture --test-threads=1

# Without the built compat and clean images, the stock tests only:
cargo test --manifest-path app/src-tauri/Cargo.toml --lib \
  engine::free_ride::stock_rider_acceptance::stock_ -- --ignored --nocapture --test-threads=1

cargo test --manifest-path app/src-tauri/Cargo.toml --lib \
  engine::free_ride::tests -- --test-threads=1

cargo clippy --manifest-path app/src-tauri/Cargo.toml --lib --tests -- -D warnings
rustfmt --check --edition 2024 app/src-tauri/src/engine/free_ride.rs
```

On 2026-10-16 the combined acceptance command ran six new cases, two existing short cases, and one existing exact COM/joint/motor trace.
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
