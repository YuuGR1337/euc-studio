# Articulated rider physical acceptance

Status: focused mechanical acceptance, 2026-10-16, with merge checks on 2026-10-21.
Since 2026-10-22 the articulated rider is the default rider; independent foot contact is opt-in.
The rigid rider remains selectable as the controlled baseline. See the last section.

## Scope

These tests extend `crates/euc-physics/tests/articulated.rs`.
They use `articulated_sagittal_v1`, real MuJoCo body motion, and the public physics API.
They use no firmware image, electrical plant, hardware, private body data, or recording.
The initial acceptance work changed no production source, model parameter, safety threshold, or control default.
The merge corrections appear below.
Generated-map acceptance and recording UI remain with the parent.

The fixture uses the existing generic rigid-axle wheel and its 48-part tire.
It retains the selected rider mass, joint gains, torque caps, release rules, and external balance aids.
The fixture has no suspension. The public scenario `Terrain` supplies grades and the bump.
These cases do not exercise the separate generated-map implementation.

## Test controller

The tests use the same balance law as the existing rigid physics tests:

```text
motor torque = clamp(-2500 × shell pitch + 250 × IMU pitch-down rate, -300, +300)
```

The motor receives zero torque while the stand holds the shell.
The timestep remains 0.5 ms. Each case starts with another 0.5 seconds on the stand.
Inputs change rider intent only. No test sets a pose, velocity, or joint coordinate.
No new case changes the rider gains or uses an extra stabilizing force.

Every attached step checks finite body poses and joint states.
It checks the existing fall state, attachment state, and configured joint torque caps.
These checks use the production limits. They do not replace those limits with easier values.

## Added acceptance cases

### Neutral request

Two identical runs first hold +0.04 rad lean for three seconds after stand release.
One run then requests neutral for two seconds. The other keeps its lean request.
The request must not change the current pose, speed, wheel state, or clock.
The neutral run must return toward vertical and accelerate less than the matched control.
It must retain forward motion. Neutral lean does not mean zero speed.

Observed speed changes from 0.372339 to 0.379667 m/s under neutral.
Continued lean reaches 0.640663 m/s over the same interval.
Actual body lean falls from 0.036969 to 0.000020 rad under neutral.

### Controlled braking

The rider requests 2 m/s for eight seconds, then zero target speed for ten seconds.
The run must first exceed 1 m/s. This prevents a stationary fixture from passing a braking test.
Braking must command backward lean, produce negative motor torque, and remove net mechanical work through the axle.
The stopping distance must remain positive. The model must remain attached and upright.

The final speed limit is 0.3 m/s.
This limit and ten-second stop interval come from the existing rigid speed/brake regression.
They are numerical regression criteria, not measured stopping performance.

Observed speed falls from 1.758066 to 0.128039 m/s.
Stopping travel is 7.455584 m. The run retains a small final forward speed.
Estimated axle work is -68.416660 J.
Minimum commanded lean is -0.103257 rad. Minimum motor torque is -18.520593 N·m.
Axle work uses `sum(torque × relative wheel rate × timestep)`.
It does not represent full system energy, electrical recovery, or aid work.

### Mild grades

Separate runs use +2% and -2% grades.
Each grade starts at x = 2 m and reaches its full grade over a two-metre ramp.
The rider requests 1.5 m/s for eight seconds after stand release.
The wheel must cross the complete ramp and remain on the finite ground strip.
Its height change must follow the actual ground height within 10 mm.
This contact tolerance permits tire facets and soft contact. It is not a terrain calibration claim.

| Case | Final x | Ground change | Wheel height change | Final speed |
|---|---:|---:|---:|---:|
| +2% | 6.276309 m | +65.526 mm | +65.064 mm | 1.224705 m/s |
| -2% | 6.522473 m | -70.449 mm | -70.475 mm | 1.377625 m/s |

These checks prove grade traversal and contact response. They do not require exact target-speed convergence.

### Small bump

The wheel crosses a 10 mm cosine bump, one metre long, starting at x = 2 m.
The rider requests 1 m/s for eight seconds after stand release.
The wheel must rise more than half the bump height and then return within 5 mm of its original height.
It must pass the entire bump, remain attached, and keep positive forward speed.

Observed crest rise is 9.785 mm. Final height change is -0.402 mm.
Final x is 4.747817 m. Final speed is 0.951347 m/s.
This test does not compare shock absorption against the rigid rider or measure comfort.

### Moving release

The rider first builds forward motion with the same three-second lean input.
`step_off()` must preserve the clock, all exported poses, COM, wheel state, rider position, joint angles, and joint rates.
All six motor commands must become zero. External roll and yaw aid torques must remain zero after release.
The selected total mass must remain unchanged.

The test then follows the complete rider subtree for 50 ms without ground contact.
Conservative geometry bounds confirm that every human shape remains above the flat ground.
Gravity has no horizontal component. Passive joint damping remains internal to that subtree.
Its horizontal COM velocity must therefore remain constant within 0.00001 m/s.

Kinematic exports describe the start of the last step.
The test discards the first post-release export before estimating released velocity.
The release-boundary comparison permits 0.01 m/s for the finite-difference sampling offset.
Exact joint-rate and wheel-rate equality also check the zero-time transition.

Observed COM velocity before release is 0.372555637 m/s.
The first released estimate and subsequent airborne mean both equal 0.372598917 m/s at the shown precision.
This tests horizontal rider momentum during free flight. It does not prove whole-system angular momentum or landing behavior.
The existing separate release test still checks ground landing and disabled motors.

## Verification

Run the focused suite with the requested Python interpreter:

```sh
PYO3_PYTHON=/opt/homebrew/bin/python3 cargo test --locked --offline -p euc-physics --test articulated -- --nocapture
```

Actual results:

- Focused articulated suite: 14 passed, 0 failed.
- Full `euc-physics` suite: 41 passed, 0 failed, 0 ignored.
- Strict Clippy for the articulated test target: passed.
- Rust formatting and whitespace checks: passed.

The full crate run also includes the parent's generated-map tests. This work does not change those tests.
No production defect appeared in these cases. No production source change was needed.

Additional check commands:

```sh
PYO3_PYTHON=/opt/homebrew/bin/python3 cargo test --locked --offline -p euc-physics
PYO3_PYTHON=/opt/homebrew/bin/python3 cargo clippy --locked --offline -p euc-physics --test articulated -- -D warnings
rustfmt --edition 2024 --check crates/euc-physics/tests/articulated.rs
```

## Limits

These are mechanical tests with a test balance law. They do not run stock firmware or prove real FreeRide timing.
The cases above ran with welded feet. The model has no gait, independent elbow motion, or lateral human balance.
Mass fractions, inertias, posture control, and joint limits remain estimates.
External roll and yaw aids remain active while attached.
No result establishes human validity, high-speed safety, low-SOC behavior, injury risk, or real-wheel readiness.

## Merge checks, 2026-10-21

The merge replaced upper-body lean feedback with the COM angle but retained the upper-body joint torque mapping.
It also estimated lean rate from prior steps and forced zero rate at time zero.
The unchanged internal-power regression failed at its first step.
The correction restores the Human model's upper-body lean coordinate and measured angular rate.
It retains the original gains, joint torque mapping, torque caps, and power assertions.
The complete task removes 0.360 W in the fixed-foot test motion.
Applying that task only at the ankles adds 0.360 W.

The merge also added the rigid model's artificial step-off kick to the articulated model.
A new regression first found an unwanted 30 N·m shell moment.
The correction restores articulated release without that kick.
Rigid release retains its original kick.

Manual turn requests now fade to zero below the configured walking speed.
A stationary turn request changes neither the heading target nor the requested yaw moment.
The turn feed-forward uses the same command delay as the heading and lean requests.
Clearing input also clears delayed turn requests.
Both controller regressions failed before these corrections and pass afterward.
Both complete models pass a three-second stationary-turn check with less than 0.01 rad heading change.

The moving turn test requests 15 km/h and waits at most 20 seconds for measured speed above 4 m/s.
This replaces a fixed eight-second preparation interval. The original articulated model needs more time to reach that speed.
The test retains its original turn rate, radius, lean, path, aid-torque, and heading-recovery checks.

| Model | Turn speed | Turn rate | Radius | Mean roll | Maximum yaw aid |
|---|---:|---:|---:|---:|---:|
| Rigid | 4.33 m/s | 0.400 rad/s | 10.8 m | -9.7° | 2.6 N·m |
| Articulated | 4.29 m/s | 0.400 rad/s | 10.7 m | -9.5° | 3.3 N·m |

These turns still use external roll and yaw aids. Neither model has physical hip roll or twist joints.
The first firmware matrix exposed high-speed lateral instability in the merged turn controller.
That controller reduced both its yaw spring and its yaw damping as speed increased.
A new test reproduced the fault without firmware. A straight rigid run exceeded 0.1 rad roll near 14 m/s.
The correction keeps yaw-rate damping active while the heading spring decreases with speed.
The test now stays upright for 30 seconds and exceeds 18 m/s without a manual turn request.
A separate controller test checks the damping moment at 20 m/s.
Both new tests failed before the correction.
The existing torque cap and all numerical gains remain unchanged.
The yaw aid remains an external moment at every speed.

The complete physics suite passes 49 tests with no failures or ignored tests.
Strict physics Clippy passes for all targets.

## Default rider and foot contact, 2026-10-22

The articulated rider became the default rider for scenarios, the Ride workspace and the CLI.
Its feet stay welded to the pedals by default.
`model = "rigid_ankle_v1"` in a scenario, or `--rider-model rigid_ankle_v1`, selects the rigid baseline.
Recordings from `euc sim run --record` now carry the recording context with the model that actually ran.
Free Ride keeps its own model selector.

Three model changes came with the switch. Each one has a regression test.

### Lean from the ankles

The scenario matrix first showed the articulated rider accelerating about half as hard as the rigid rider.
At the same lean command it reached 15 km/h at 15 s in `overlean-top-speed`, where the rigid rider reached 30 km/h.
The cause was the posture spring at the ankles.
The lean task moment acts equally at hip, knee and ankle, and all three joints had the same posture stiffness.
The joints shared the lean, so the legs leaned about a third as far as the upper body.
The centre of mass leaned about half as far as commanded.
A standing ankle now has no posture spring. The hips and knees hold the body straight.
The whole body leans from the ankles, as the rigid rider does.
The task moment mapping, its gains and its torque caps are unchanged.
A first attempt set the ankle posture target to the commanded lean plus the pedal pitch.
That added a second absolute lean loop, and the welded rider fell at the stand release of `hard-braking-30kmh`.
It was rejected.

With the change, the articulated rider accelerates slightly harder than the rigid rider at the same lean.
In `overlean-top-speed` on stock firmware it reaches 33.6 km/h at 15 s against 30.6 km/h.
The mechanical cases above now give these results (default welded feet):

| Case | Before (ankle spring) | Now |
|---|---|---|
| Neutral request: speed change | 0.372 to 0.380 m/s | 0.767 to 0.819 m/s (continued lean: 1.313 m/s) |
| Neutral request: body lean | 0.0370 to 0.00002 rad | 0.0422 to 0.0004 rad |
| Braking: speed | 1.758 to 0.128 m/s | 2.124 to 0.000 m/s |
| Braking: stopping travel, axle work | 7.46 m, -68.4 J | 5.05 m, -194.8 J |
| Braking: minimum lean command, motor torque | -0.103 rad, -18.5 N·m | -0.130 rad, -34.5 N·m |
| +2% grade: ground, wheel height change | +65.5, +65.1 mm | +122.4, +122.3 mm |
| -2% grade: ground, wheel height change | -70.4, -70.5 mm | -128.3, -128.5 mm |
| 10 mm bump: crest rise, final height change | 9.785, -0.402 mm | 9.932, -0.524 mm |
| Moving release: COM velocity before, released, airborne | 0.37256, 0.37260, 0.37260 m/s | 0.76751, 0.76775, 0.76775 m/s |
| 0.4 rad/s turn: speed, radius, mean roll, yaw aid | 4.29 m/s, 10.7 m, -9.5°, 3.3 N·m | 4.34 m/s, 10.8 m, -9.7°, 3.3 N·m |

The grade and bump runs travel further because the rider now reaches the requested speed sooner.
The turn now matches the rigid rider's (4.33 m/s, 10.8 m, -9.7°).
All the existing limits in these tests hold without change.

### Step-off push

The articulated rider had no step-off push. A riderless wheel in this symmetric model then rolls on upright.
Both shipped step-off scenarios lost their cut within 3 s.
The stepping foot now pushes off: the right foot presses its pedal straight down for 0.2 s.
The pedal pushes the foot up equally hard.
The force gives the rigid rider's 30 N·m roll moment about the heading.
It is an internal force pair at one point, so wheel and rider together get no external force or moment.
The rider's horizontal momentum is unchanged; the moving-release test still holds to 0.00001 m/s.

### Independent foot contact (opt-in)

The model id `articulated_contact_v1` stands each sole on its own pedal; `articulated_sagittal_v1` keeps welded feet.
A separate id keeps every recording and result that names `articulated_sagittal_v1` meaning the same physics.
Each sole touches only its own pedal through an explicit MuJoCo contact pair.
No collision mask changes: torso, legs and wheel still never touch the shell.
Friction is 1.0 and the contact time constant is 5 ms (both DEFAULT).
The pedal pushes and never pulls.
The soles therefore carry an ankle moment only while the centre of pressure stays under them.
For the default rider that is about 70 N·m backward and 120 N·m forward, both feet together.

Contact feet do not hold the body to the shell, and the sagittal legs cannot roll.
The roll aid that holds the shell therefore also holds the upper body at the wheel's roll target.
Its torque and work add to the reported roll aid.
Without it, the end of a turn and any one-foot stance longer than about 0.3 s topple the rider sideways off the pedals.
When both soles have been off their pedals for 0.5 s, the rider counts as off (`Leaving::LostFooting`).
Release switches the sole-pedal pairs off with a pair margin of -1000 m on a model copy.
The swap keeps every coordinate and velocity.
MuJoCo 3.9 does not apply a pair's `gap` to explicit pairs, so `gap` could not do this.

`crates/euc-physics/tests/feet.rs` checks the contact with the test balance law and the profile's suspension:

| Case | Result |
|---|---|
| Standing still, 5 s | Each foot carries 392.4 N, half the rider's weight. Slip and lift stay below 0.002 mm. |
| One-foot lift, 0.3 rad for 0.25 s | The right sole rises 24 mm and is off its pedal for 0.20 s. The left foot keeps its contact and carries a mean 787 N, at least 560 N. Both feet stand again; they end 22 and 8 mm from where they stood. |
| One-foot stance, 1 s | The rider stays on with the roll aid. The feet end 63 and 59 mm back along the decks, 2 mm sideways. |
| One-foot stance without any roll aid | The rider topples over the standing foot. 0.5 s after both soles leave their pedals the rider counts as off. All aids and joint motors stop. |
| 2 cm pothole at 12 km/h | Both soles slip at most 3.7 mm and lift at most 1.0 mm. Both are unloaded together for 0.053 s. The peak sole load is 2088 N. |
| 0.4 rad/s turn at 15 km/h | The wheel turns at 0.400 rad/s and leans -8.9°. The feet move at most 1.4 mm sideways; the least sole load is 255 N. The wheel stands up again after the turn. |
| Manual lean -0.05 rad after the turn | The wheel slows by more than 0.5 m/s, both feet stay on. |
| Let-go after a drive cut at 15 km/h | The sole-pedal pairs switch off; neither foot carries load; every joint motor is zero. |
| Step-off at rest | The feet land on the ground within 25 cm. |

Contact feet stay opt-in because the lean controller was tuned on welded feet.
It does not keep the soles' centre of pressure under the feet, and the rider cannot recover with the hips or step.
On stock firmware at all three quanta, with contact feet, the rider comes off the pedals while the wheel stays up:

| Scenario | Rider off (stock, quantum 4000) | What happens |
|---|---|---|
| `full-effort-70kmh-stop-soc100` | 8.69 s | At 30° lean the wheel accelerates about 3 m/s²; the rider pitches forward over the toes |
| `hard-braking-30kmh` | 17.75 s | The hard brake (up to 6 m/s² asked) throws the rider off the soles |
| `hill-climb-low-battery` | 33.35 s | The rider who never gives up stalls on the 20 % grade |
| `overlean-top-speed` | 38.69 s | Late in the run, held by the stock tiltbacks near 75 km/h |

These runs predate the `rider_on_pedals` check: the other checks look at the wheel, so `hard-braking-30kmh` and `hill-climb-low-battery` passed with a riderless wheel.
Every scenario without a step-off now has `rider_on_pedals`, which fails in exactly these cases (see the contact matrix in [scenarios.md](scenarios.md#rigid-and-articulated-riders-2026-10-22)).
In the other sixteen shipped scenarios the contact rider leaves the pedals only where the rigid rider does.

### Limits of the default rider

Lateral balance and twisting still come from the external aids.
Welded feet pass the controller's whole ankle torque to the shell, more than real soles carry.
The segment fractions, joint gains, sole size and friction remain estimates.
