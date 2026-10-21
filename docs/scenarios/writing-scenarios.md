# Writing a scenario

A scenario is a TOML file in `scenarios/` that rides one firmware in the simulator: the wheel boots on
its stand, the stand lets go, a simulated rider follows a target speed, and the electrical plant, the
body physics and the ground do the rest. Checks at the end of the file say what must hold. The
library and its results are in [models/scenarios.md](../models/scenarios.md); the format is defined
in `crates/euc-sim/src/scenario.rs` (with `crates/euc-physics/src/terrain.rs` for the ground).

The quickest start is to copy a scenario that is close to what you want and change it.

## Run it

```sh
# On the stock firmware (fetched and cached on first use), on our own Cortex-M3 core:
cargo run --release -p euc-cli -- sim run --scenario scenarios/<name>.toml

# On one of our builds:
cargo run --release -p euc-cli -- sim run --scenario scenarios/<name>.toml \
    --firmware firmware/build/target-clean/euc-fw-begode-extreme.bin

# Two firmwares in lock step, with the first divergence and per-signal statistics:
cargo run --release -p euc-cli -- sim compare --scenario scenarios/<name>.toml --a stock --b clean
```

Useful options of `sim run`: `--settings <preset>` (or `blank`) overrides the scenario's settings,
`--max-quantum <n>` sets the emulator's quantum (see "Robustness" below), `--record <file.eucrec>`
writes a recording (see [recordings.md](../recordings.md): `euc rec info`, CSV export, replay in the
studio, Python), and `--json` prints the result as JSON. `sim compare` takes
`--tolerance <file>` (see [compare.md](../compare.md)). Each check prints one line, `PASS` or `FAIL`,
then `RESULT`.

The studio lists scenarios in its run bar: a development build reads every `scenarios/*.toml` when the
web view connects (a new file shows without a rebuild); a release build offers the two built-in ones,
`ride-20kmh-stop` and `balance-60s`.

The CLI and studio use CPI 2.0 with exception overhead.
For historical reference timing, `sim run` accepts `--cpi 1.25 --no-exception-timing`.
Changing `--cpi` alone retains exception overhead.
Record the timing settings with each result. See [CPU timing](../models/cpu-timing.md).

## The file

### Top level

| Field | Required | Meaning |
|---|---|---|
| `schema` | yes | Format version: `1`. |
| `name` | yes | Short name, the same as the file name. |
| `description` | yes | One sentence for people. |
| `wheel` | yes | Wheel profile id: `begode-extreme`. |
| `seconds` | yes | Simulated seconds from power-on. |
| `release_s` | yes | When the stand lets the wheel go, inside the run. The stock firmware boots in about 1.7 s; `2.5` is usual. |
| `speed_kmh` | yes | The rider's target speed as `[seconds, km/h]` points, in time order, linear in between and held after the last. Negative is backwards. |
| `phase_start_s` | no | Start time for measured-speed lean phases, at or after release. Defaults to `release_s`. |
| `[[phase]]` | no | Ordered lean commands with measured-speed gates and finite timeouts. See below. |
| `settings` | no | Settings preset the wheel boots with, from `profiles/<wheel>/settings.toml` (`neutral`, `tarik-2026-10-15`). Without it the settings journal is blank, which tilts the stock pedals 4.8° nose down. |
| `step_off_s` | no | When the rider steps off the pedals (between `release_s` and the end); the wheel goes on alone. |
| `push` | no | Pushes on the rider, `[[start s, newtons forward, duration s], ...]`; overlapping pushes add up. |
| `[rider]`, `[rider.control]` | no | The rider, below. |
| `[assist]` | no | The roll aid, below. |
| `[terrain]` | no | The ground, below; flat without it. |
| `[[timeline]]` | no | Faults, below. |
| `[[inject]]` | no | BLE commands, below. |
| `[[expect]]` | no | Checks, below. |

Unknown fields are errors, so a typo does not pass silently.

Start the file with a comment that says what the run shows and why, and a line `# Rider: ...` that
says which rider it uses and how it differs from the default.

### The rider: `[rider]` and `[rider.control]`

Every field has a default; give only what you change. The model is described in
[models/body.md](../models/body.md).

| Field | Default | Meaning |
|---|---|---|
| `mass_kg` | 80 | Rider mass; every segment's mass scales with it. |
| `height_m` | 1.78 | Standing height; every segment's length scales with it. |
| `ankle_above_pedals_m` | 0.08 | Ankle joints above the pedals, shoe included. |
| `cda_m2` | 0.6 | Drag area of rider and wheel. |
| `control.lean_kp`, `control.lean_kd` | 6000, 900 | Ankle stiffness (N·m/rad) and damping (N·m·s/rad) about the lean. |
| `control.ankle_max_nm` | 400 | Strongest ankle torque, both legs. |
| `control.knee_bend_deg`, `control.torso_lean_deg` | 25, 5 | The standing pose: knee bend, and the torso's pitch forward of the lean. |
| `control.knee_kp`, `control.knee_kd`, `control.knee_max_nm` | 300, 30, 400 | How far the knees give under a bump (both legs). |
| `control.hip_kp`, `control.hip_kd`, `control.hip_max_nm` | 2500, 190, 400 | How the hips hold the torso's pitch (both legs). |
| `control.hip_strategy`, `control.hip_bend_max_deg` | 1, 30 | How far the torso bends when the ankles are at their limit. |
| `control.speed_kp`, `control.speed_ki` | 0.6, 0.03 | Acceleration asked per m/s of speed error, and its integral. |
| `control.accel_max`, `control.brake_max` | 1.5, 2.5 | Strongest acceleration and braking asked, m/s². |
| `control.lean_max_deg`, `control.lean_rate_deg_s` | 15, 12 | Largest lean, and how fast it changes. |
| `control.tiltback_feel_deg` | 2 | Tiltback the rider tolerates before easing off; `90` means the rider ignores tiltback. |
| `control.tiltback_ease_kmh_per_deg` | 1.5 | Speed given up per degree beyond that (`0`: ignores tiltback). |
| `control.tiltback_ease_brake` | 1.0 | Strongest braking while easing off, m/s². |
| `control.steer_kp`, `control.steer_kd`, `control.steer_max_nm` | 2000, 200, 60 | How the rider keeps the line (world +x). |

For example, a rider who ignores tiltback and leans harder:

```toml
[rider.control]
tiltback_feel_deg = 90.0
lean_max_deg = 18.0
```

### Measured-speed lean phases: `[[phase]]`

Use phases when the next rider command depends on measured ground speed.
The `speed_kmh` schedule still controls the rider before `phase_start_s`.
Each phase then sends `Intent::Lean` through the existing rider controller.
The scenario never sets body speed, body pose, or motor torque.

```toml
# Top-level fields, before any tables:
seconds = 68.0
release_s = 4.0
phase_start_s = 6.0
speed_kmh = [[0, 0]]

[rider.control]
lean_max_deg = 30.0
lean_rate_deg_s = 60.0
ankle_max_nm = 400.0

[[phase]]
name = "accelerate"
lean_deg = 30.0
until_speed_kmh = 70.0
crossing = "above"
timeout_s = 40.0

[[phase]]
name = "brake"
lean_deg = -30.0
until_speed_kmh = 1.0
crossing = "below"
timeout_s = 20.0
```

| Phase field | Meaning |
|---|---|
| `name` | Unique, nonempty name, at most 64 bytes. |
| `lean_deg` | Desired lean from vertical. Forward is positive. Its magnitude must stay within `rider.control.lean_max_deg`. |
| `until_speed_kmh` | Finite signed forward ground speed. This is a gate, not a speed command. |
| `crossing` | `above`: previous speed is below the threshold, then reaches or exceeds it. `below`: the reverse. |
| `timeout_s` | Positive finite time from this phase's first command. |

The parser accepts at most 16 phases.
The run duration must include the start time, all timeouts, and two seconds of final observation.
A phase completes only at a healthy measured crossing before its timeout.
A crossing at the exact timeout can complete the phase.
Cutoff, fall, halt, and nonfinite speed take priority over a crossing.
A cutoff or fall after release also aborts a pending launch.
Normal boot cutoff flags before release do not abort it.

The next command starts from the crossing sample.
At the default 100 Hz, detection can take up to one sample period.
No interpolation moves the command back in time.
The existing command delay, lean slew limit, ankle torque cap, and rigid rider aids still apply.
Manual lean does not use the speed controller's acceleration limits or tiltback easing.
Firmware protections remain active.
The target speed telemetry channel is unavailable during manual lean.

`Outcome.phases` adds structured measurements to JSON results.
Each phase also adds a verdict check, so an incomplete gate cannot produce a passing run.
The phase status is `reached`, `target_unreachable`, or `not_started`.
Failure reasons include `timeout`, `cutoff`, `fall`, `halted`, `nonfinite_speed`, and `duration_ended`.
Later phases remain `not_started` after failure.
Their time, distance, speed, current, and voltage measurements stay null.

Phase results include command and end times, elapsed time, start and end speeds, and maximum forward speed.
`distance_m` is the signed forward distance change from `Signals.distance_m`, not total path length.
For a straight forward brake, it measures braking distance from the measured 70 km/h crossing.
The stopping gate uses 1 km/h, not exact zero speed.
Current results include the largest absolute motor phase sample and signed q-current and battery-current ranges.
Voltage results give the minimum and maximum bus samples.
These are sample-rate measurements, not electrical-step peak bounds.

After the last healthy crossing, the rider requests zero speed for two seconds of physical observation.
After failure, the rider requests neutral lean for two seconds, unless the machine already halted.
No pose or velocity reset occurs.
Upright, cutoff, and final-speed checks still cover this observation period.
A reached stopping gate does not prove stable standing afterward.

#### Full effort to 70 km/h, then stop

The public cases are `full-effort-70kmh-stop-soc100`, `full-effort-70kmh-stop-soc50`, and `full-effort-70kmh-stop-soc20`.
They use the documented generic 80 kg rider and initial SOC values of 100%, 50%, and 20%.
A `battery_soc` event at time zero sets the initial charge once.
Normal discharge and regeneration then change the charge.
These cases use the same settings, ground, rider limits, and rigid rider aids.

The current cases use five phases, a 30° lean cap, a 24°/s slew limit, and a 400 N·m ankle cap.
The stand releases at 4 s; the first effort command starts at 6 s.

| Phase | Desired lean | Measured forward-speed crossing | Timeout |
|---|---:|---|---:|
| `launch` | +30° | Above 35 km/h | 15 s |
| `build` | +20° | Above 50 km/h | 10 s |
| `accelerate` | +15° | Above 70 km/h | 40 s |
| `brake` | -30° | Below 15 km/h | 15 s |
| `settle` | -5° | Below 1 km/h | 15 s |

Every transition requires an actual measured crossing in the specified direction.
The thresholds do not command speed or motor torque.
A cutoff, fall, halt, or timeout prevents later phases from starting.
The generated phase checks prevent an incomplete target from passing.

Full braking includes **both `brake` and `settle`**.
Its time starts at the measured 70 km/h crossing and ends at the measured crossing below 1 km/h.
Calculate that time as `settle.end_s - brake.start_s`.
Calculate braking distance as `brake.distance_m + settle.distance_m`.
These distances are signed forward changes, not total path length.
Both stopping phases must reach their gates before full stopping time or distance becomes available.
If the 70 km/h target fails, stopping metrics remain **unavailable**, and later phase measurements remain null.
A failed stopping phase can retain partial measurements; those values do not describe a completed stop.

Two seconds of zero-speed rider control follow the final gate.
The upright, cutoff, and final-speed checks still apply during this observation.
The 103 s duration covers all phase timeouts and that observation.
Run-wide top speed and distance include the observation after success or failure.
They do not establish a healthy 70 km/h crossing or a completed stopping distance.

Full effort describes commands inside this bounded envelope.
It does not define a measured human maximum or prove a global acceleration or braking optimum.
Keep failed gates as results when comparing firmware images.
Motor, tire, suspension, battery, and rigid rider results remain PRELIMINARY.
See [full-effort acceptance](../models/full-effort-acceptance.md) for the corrected-RTC evidence and rejected alternatives.

An initial 100% SOC does not guarantee a full battery at the braking gate.
Acceleration consumes charge first.
A battery capacity-limit halt means the model cannot accept further charge.
It does not establish real BMS cutoff voltage, pack isolation, or wheel recovery behavior.
Run all three firmware images at quanta 4000, 3000, and 2000 before a robustness claim.
Keep separate rider-mass studies outside public scenarios and identify their mass in private results.

### The roll aid: `[assist]`

The rider cannot balance sideways yet (the joints bend only forward and back), so a roll spring on
the shell holds it upright:
`roll_stiffness` (5000 N·m/rad) and `roll_damping` (500 N·m·s/rad). Set `roll_stiffness = 0.0` to
see the wheel fall over sideways.

### The ground: `[terrain]` and `[[terrain.feature]]`

| Field | Default | Meaning |
|---|---|---|
| `slope_pct` | 0 | Grade from `slope_start_m` on, percent (uphill positive, within ±50). |
| `slope_start_m` | 0 | Where the grade starts, metres along the path. |
| `slope_ramp_m` | 10 | Length over which the grade builds up (a parabola, no kink). |
| `length_m` | 300 | Length of the ground (at least 1 m); features must end before it. |

Each `[[terrain.feature]]` has a `kind`:

| `kind` | Fields | Shape |
|---|---|---|
| `pothole` | `at_m`, `depth_m`, `length_m` | A hole with steep walls. |
| `bump` | `at_m`, `height_m`, `length_m` | A smooth hump, `height · (1 − cos)/2` over its length. |

A feature must start at 1 m or later, end before `length_m`, be at least 8 cm long, and have a depth
or height between 0 and 1 m. Features add to the slope. The path runs along world +x; the ground is a
3 m wide strip sampled every 2 cm (flat ground is an infinite plane). The wheel starts at x = 0, so a
feature at `at_m` is reached after that distance: at 20 km/h about 5.6 m per second, after the
speed-up.

### Faults: `[[timeline]]`

Each entry has `at_s` (start), optional `for_s` (length; without it the fault lasts to the end),
`fault = "<kind>"`, and the kind's fields. Device faults can name the board instance with `device`
(`imu`, `bms`).

| `fault` | Fields | What breaks |
|---|---|---|
| `hall_stuck` | `line` (`a`, `b`, `c`), `level` (`high`, `low`) | A hall line held at a level. |
| `hall_lost` | `line` | A hall line disconnected (reads high). |
| `hall_noise` | `noise_deg` | Extra jitter on the sensed angle, electrical degrees. |
| `current_sensor` | `sensor` (`a`, `b`, `c`, `bus`), `offset_a`, `gain` (1), `noise_a` | A current sensor reads `gain · i + offset_a`, plus noise. |
| `open_phase` | `phase` (`a`, `b`, `c`) | A motor phase wire open. |
| `stage_overtemp` | `rate_k_per_s`, `max_c` | The power stage heats up as if its cooling failed. |
| `cell_low` | `pack`, `cell`, `drop_v` | One cell group sits below its curve. |
| `pack_imbalance` | `pack`, `soc_delta` | One pack's charge jumps at the start (stays). |
| `battery_soc` | `soc` (0..1) | Every pack's charge is set at the start (stays). |
| `high_resistance` | `pack`, `factor` | One pack's internal resistance times `factor`. |
| `imu_noise` | `accel_g`, `gyro_dps` | Gaussian noise on every IMU sample. |
| `imu_bias` | `accel_g` `[x, y, z]`, `gyro_dps` `[x, y, z]` | Constant offsets on the IMU. |
| `imu_stuck` | none | The IMU's sample registers stop updating. |
| `bms_silent` | none | The smart BMS never answers. |
| `bms_garbled` | `byte_error_rate`, `missing_bytes` | The smart BMS answers with damaged bytes. |

```toml
[[timeline]]
at_s = 15.0
fault = "hall_lost"
line = "a"
```

### BLE commands: `[[inject]]`

What a phone sends: `at_s` and `bytes`, the command as text (the wheel's protocol is in
[protocol/](../protocol/)).

```toml
[[inject]]
at_s = 10.0
bytes = "c"
```

### Checks: `[[expect]]`

Two kinds. Verdict checks can fail: a failing verdict is a finding about the firmware, not a broken
scenario. `record` checks never fail: they print a measurement. Times default to the release
(verdicts) or to power-on (`record`, `firmware_bits`), and to the end of the run.

| `check` | Fields | Passes when |
|---|---|---|
| `upright` | `from_s` | The wheel never falls over (pitch or roll beyond 45°). |
| `pitch` | `max_abs_deg`, `from_s` | \|pitch\| stays within the bound. |
| `no_cutoff` | `from_s` | The firmware never sets a cutoff flag. |
| `cutoff` | `from_s`, `within_s`, `flag` | The firmware cuts within `within_s` of `from_s`, with that flag set if given (`hall_fault`, `balance_disabled`, `drive_fault`). |
| `speed_reached` | `kmh` | The ground speed reaches at least this. |
| `stopped_at_end` | `max_abs_kmh` | \|speed\| at the end is at most this. |
| `pitch_reaches` | `deg`, `from_s`, `by_s` | The pitch reaches `deg` (nose up positive; a negative value means a nose dive at least that deep). |
| `ripple` | `signal`, `max`, `from_s`, `by_s` | The signal's standard deviation stays at most `max`. |
| `firmware_tracks` | `var`, `scale`, `signal`, `max_rms`, `from_s`, `by_s` | A firmware variable times `scale` follows a signal within an RMS error. |
| `firmware_bits` | `var`, `bits`, `from_s`, `by_s` | A firmware variable has every bit of `bits` set. |
| `record` | `signal` or `var`, `scale`, `stat`, `threshold`, `from_s`, `by_s` | Always: prints the statistic. |

`stat` is one of `max`, `min`, `mean`, `first_above` and `first_below` (both need `threshold`),
`at_fall` (the value when the wheel first falls) and `at_cutoff` (the value when the firmware first
cuts).

`signal` is a telemetry key: `speed_kmh`, `wheel_rad_s`, `pwm_pct`, `modulation_pct`, `phase_a_a`,
`phase_b_a`, `phase_c_a`, `iq_a`, `torque_nm`, `battery_v`, `battery_a`, `soc_pct`, `pitch_deg`
(nose up positive), `roll_deg`, `board_temp_c`, `motor_temp_c`, `distance_m`, `isr_load_pct`,
`rider_lean_deg`, `target_speed_kmh`, `suspension_mm`, `shock_mm`. An unknown key is an error that
lists them.

`var` is a firmware variable: by stock-map name for the stock image (`profiles/<wheel>/stockmap.toml`:
`alerts`, `speed_cms`, `vq_active`, `pwm_alarm`, `tiltback_speed_deg`, `tiltback_battery_deg`,
`tiltback_pwm_deg`, `attitude_pitch_deg` (nose down positive), `drive_command`, `balance_disabled`,
`drive_fault`, `hall_fault`), and by the same names for our builds (`profiles/<wheel>/probes.toml`, which
adds a few of its own, such as `riderless_triggers`). A name the firmware's map does not have stops
the run with an error before it starts.

## Two worked examples

### A road with three potholes

Potholes 1, 2 and 3 cm deep, 20 m apart, ridden at 20 km/h:

```toml
# Three potholes of growing depth at 20 km/h, on the maintainer's settings.
#
# Rider: the default rider.
schema = 1
name = "three-potholes-20kmh"
description = "Ride over potholes 1, 2 and 3 cm deep at 20 km/h: does the wheel stay up?"
wheel = "begode-extreme"
seconds = 30.0
release_s = 2.5
speed_kmh = [[0, 0], [6, 0], [11, 20]]
settings = "tarik-2026-10-15"

[terrain]
length_m = 150.0

[[terrain.feature]]
kind = "pothole"
at_m = 50.0
depth_m = 0.01
length_m = 0.5

[[terrain.feature]]
kind = "pothole"
at_m = 70.0
depth_m = 0.02
length_m = 0.5

[[terrain.feature]]
kind = "pothole"
at_m = 90.0
depth_m = 0.03
length_m = 0.5

# The safety verdict.
[[expect]]
check = "upright"

[[expect]]
check = "no_cutoff"

# Measurements: they always pass.
[[expect]]
check = "record"
signal = "distance_m"
stat = "first_above"
threshold = 90.0

[[expect]]
check = "record"
signal = "pitch_deg"
stat = "min"
from_s = 15.0

[[expect]]
check = "record"
signal = "suspension_mm"
stat = "max"
from_s = 15.0

[[expect]]
check = "record"
signal = "iq_a"
stat = "max"
from_s = 15.0
```

The `distance_m` record tells when the last pothole comes (at about 27 s), so a later check can start
just before it.

### A bump, then a slope

A 6 cm speed bump at 15 km/h, then an 8 % climb that builds up from 60 m:

```toml
# A 6 cm speed bump at 15 km/h, then an 8 % climb, on the maintainer's settings.
#
# Rider: the default rider.
schema = 1
name = "bump-then-hill-15kmh"
description = "A 6 cm bump at 15 km/h, then an 8 % climb: the impact, then the climbing current."
wheel = "begode-extreme"
seconds = 40.0
release_s = 2.5
speed_kmh = [[0, 0], [6, 0], [10, 15]]
settings = "tarik-2026-10-15"

[terrain]
slope_pct = 8.0
slope_start_m = 60.0      # the grade builds up over slope_ramp_m (10 m by default)
length_m = 200.0

[[terrain.feature]]
kind = "bump"
at_m = 30.0
height_m = 0.06
length_m = 0.5

[[expect]]
check = "upright"

[[expect]]
check = "no_cutoff"

[[expect]]
check = "record"
signal = "pitch_deg"
stat = "max"
from_s = 10.0
by_s = 18.0

[[expect]]
check = "record"
signal = "iq_a"
stat = "mean"
from_s = 30.0

[[expect]]
check = "record"
signal = "speed_kmh"
stat = "min"
from_s = 30.0
```

## Robustness: check a fall or a survival at three quantum sizes

The emulator runs the firmware in quanta of up to 4000 instructions. Near a limit, whether the wheel
falls can depend on that timing. Before you write down a fall or a survival as a result, run the
scenario at `--max-quantum 4000`, `3000` and `2000` (on every firmware you report). If the verdict is
the same at all three, it is robust; if it changes, call it **borderline** in the scenario's header and
in [models/scenarios.md](../models/scenarios.md), and do not use it as evidence on its own.

## PRELIMINARY results

Mark a result PRELIMINARY, in the scenario's header and wherever it is reported, when it depends on a
part of the model that is known to be too simple for that case, and say which part (for example: the
rider has no knees; the suspension values are estimates; the tire is a soft contact). Remove the mark
when that part is modelled or measured. Never tune a scenario's values until a check passes: change
the scenario only when the situation it describes needs it, and say why in the header.
