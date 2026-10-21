# Physical articulated rider: implementation and next slice

Status: implemented first vertical slice, 2026-10-16.
This is a software model, not human validation.

## Delivered contract

`Rider.model` selects `rigid_ankle_v1` or `articulated_sagittal_v1`.
Omitting the field keeps the rigid baseline.
Existing mass, control, delay, release, and aid defaults remain unchanged.
Deserialize the complete `Rider`; unknown articulation fields fail validation.
Do not validate through a reduced struct which discards supplied settings.

```toml
model = "articulated_sagittal_v1"
[articulation]
joint_kp = 1000.0
joint_kd = 60.0
joint_max_nm = 180.0
```

These are estimated posture gains and per-joint hip/knee torque caps.
Ankle motors share the existing combined `ankle_max_nm` cap equally.
`Physics::set_crouch(knee_rad)` accepts 0 through 1.2 radians while attached.
It changes torque targets, not poses, velocities, or wheel speed.
`Physics::rider_joints()` returns six actual angles, rates, and saturated motor commands.
Its order is left hip/knee/ankle, then right hip/knee/ankle.
`RiderState.model` reports the actual selected topology.
`RiderJointState.posture_target_rad` separates the posture target from the whole-upper-body lean task moment.
`Physics::rider_com_m()` reports the actual moving subtree COM at the latest kinematic update.
For rigid only, that subtree excludes the legacy ankle proxy; articulated includes every limb.
These Rust diagnostic additions do not add required snapshot/JSON/frontend fields.

## Physical topology

The upper body has a free joint. It combines pelvis, trunk, head, and arms.
Both arms are fixed capsule shapes on `rider`, matching the rigid baseline's neutral silhouette.
They add no elbow joints or mass; the explicit upper-body inertial already includes arm mass.
Each side adds a thigh, shank, and foot with internal pitch motors.
Hip limits are -1.2 through 0.8 radians.
Knee limits are -0.03 through 2.2 radians.
Ankle limits are -0.9 through 0.7 radians.
These are numerical prototype limits, not measured joint ranges.
The solver applies soft joint limits and bounded motor torques.

Each thigh and shank is 0.245 times the selected generic standing height.
Each side allocates 10% of rider mass to thigh, 5% to shank, and 2% to foot.
The upper body receives the remaining 66%.
Upper-body inertial position solves the selected aggregate neutral COM exactly.
Limb inertias use positive geometric estimates; the upper body uses the configured gyration estimate.
The selected rider mass includes every articulated foot and limb.
No separate rigid ankle proxy remains in this model.

Two named foot-to-shell welds provide pedal support.
These are explicit constraints, not friction contacts or measured shoe grip.
Release disables both welds and every human motor without changing qpos or qvel.
All seven human bodies can then contact the ground.
The new model adds no artificial step-off kick.
The old rigid model retains its existing release behavior.
Human self collision and human/wheel collision remain disabled in this first slice.

The existing lean controller drives the ankle pair with the correct joint-reaction sign.
Hip and knee posture motors use symmetric crouch targets.
All six transmissions are internal joint motors.
Existing external roll and yaw aids remain enabled and identified separately.
Signed aid work is available; full joint-work and positive/negative energy accounting remain pending.
Crouch requests currently bypass the optional intent/feedback delay queues.

## Export and parent integration

Rigid export remains `wheel`, `shell`, `rider`.
Articulated export is:

1. `wheel`, role `Wheel`.
2. `shell`, role `Shell`.
3. `rider`, role `Rider`, the upper body.
4. `left_thigh`, `left_shank`, `left_foot`, each role `Rider`.
5. `right_thigh`, `right_shank`, `right_foot`, each role `Rider`.

`bodies()` and `poses()` use the same ordered mapping.
Every exported limb pose comes from MuJoCo.
The seven-float pose format remains unchanged.
Render each body's own geometry once; do not draw a complete human at each `Rider` role.

Parent owns FreeRideSpec selection, native factory, frontend selector, and recording context.
Construct the selected `Rider` without discarding other supplied rider parameters.
Keep rigid selected by default while broader firmware/free-ride checks remain pending.
Record the actual model and all selected articulation/control/aid settings.
Do not silently label articulated runs `RigidAnkleV1` in EUCR context.
Legacy readers need a clear unsupported-model result rather than incorrect topology claims.

The unchanged tire metadata contains 48 capsule shapes.
Parent replaced the recording reader's former 32-shape per-body cap with its existing total shape budget.
Rigid/articulated capture regressions cover the real profile metadata.
Keep total body/shape/header limits bounded. Do not drop tire shapes to meet the old cap.

## Regression evidence

Focused software tests use generic parameters and no firmware image or hardware.

| Test | Acceptance |
|---|---|
| Default and legacy export | Rigid remains default, three body names/order, unchanged total mass |
| Articulated export | Nine body poses, seven physical human bodies, six finite hinge states |
| Fixed arms | Both arms match the rigid baseline's neutral geometry and belong to the upper body |
| Mass/COM/inertia | Selected rider mass and neutral aggregate COM agree; inertias are positive and satisfy triangle inequalities |
| Internal reactions | Joint transmissions; moving joints conserve rider-subtree angular momentum within 0.01 N·m·s in free flight |
| Crouch | Physical knees bend and pelvis lowers; motor commands remain capped |
| Release | No teleport at release, both feet land, every human motor becomes zero |
| Lean drive | A balanced physical wheel moves under a lean request without detachment |
| Settings | Full articulation and control-delay values survive TOML deserialization |
| Metadata | Rigid tire retains its actual 48 shapes; exported numeric values are finite |
| Existing rigid suite | Stand, speed/brake, suspension, release, and ground contacts retain prior checks |

Commands for this implementation:

```sh
cargo test --locked --offline -p euc-physics
cargo clippy --locked --offline -p euc-physics --all-targets -- -D warnings
```

Actual results in this worktree:

- `cargo test --locked --offline -p euc-physics`: 30 passed, 0 failed.
- Strict `euc-physics` Clippy with `--all-targets -- -D warnings`: passed.
- Parent recording tests `engine::recordings::tests::extreme_`: 2 passed, 0 failed.
  Both use real profile geometry and headless automatic capture/import.
- Rust formatting and whitespace checks passed for the changed source.
- A zero-ankle-cap regression first failed on an invalid MuJoCo control range.
  The generator now disables that range constraint when the cap is zero; the motor command remains zero.

A focused pass does not validate new firmware rides, contact friction, human comfort, injury, or real-world stability.
Ampere owns the separate high-speed/SOC stress work.

### Native stock acceptance: initial failure and physical correction

The appended `stock_rider_acceptance` module in native `free_ride.rs` uses the actual `build_free_host` factory.
It reads verified cached stock GW1825007, digest `cbfb328cab1e0b8219790dd3ef710bb266569c70b6ac396ccd06c247da9fd27d`.
It uses the Own CPU, the `neutral` settings preset, and a flat generated map.
No hardware, audio device, recording file, firmware variable write, or firmware download is used.
Both models retain their original mass, controller, articulation, and aid settings.

The test boots on the stand for four simulated seconds and checks the unarmed pause.
It supplies neutral packets across 300 ms of wall time, then calls explicit `arm`.
It runs 0.5 simulated seconds neutral, one second at +0.04 rad lean, then 0.5 seconds neutral.
It refreshes inputs every sample without editing mailbox timestamps or bypassing the watchdog.
The tests inspect real exported poses; relative body quaternions give the six physical hinge angles.
Motor torque, Iq, and phase currents come from the electrical plant driven by stock firmware.

Command:

```sh
cargo test --locked --offline --manifest-path app/src-tauri/Cargo.toml --lib stock_rider_acceptance -- --ignored --nocapture --test-threads=1
```

Three completed runs each produced one rigid pass and one articulated failure.
The following numbers are from the second run, not calibrated hardware measurements.

| Quantity | Rigid | Articulated |
|---|---:|---:|
| Speed before lean, km/h | -0.0002 | +0.9228 |
| Speed after one-second forward lean, km/h | +0.7645 | -2.0183 |
| Shell travel during lean, m | +0.0438 | -0.0582 |
| Peak absolute wheel pitch during lean, degrees | 0.8888 | 4.6483 |
| Peak absolute motor torque during lean, N·m | 28.5379 | 199.0253 |
| Peak absolute Iq during lean, A | 33.1451 | 231.1560 |
| Commanded lean at lean end, degrees | +2.2918 | +2.2918 |
| Actual upper-body lean at lean end, degrees | +2.3795 | +0.0744 |

Articulated left/right hip, knee, and ankle changes reach approximately 0.0459, 0.0669, and 0.1878 rad during lean.
All nine articulated poses stay finite. Both runs reach six simulated seconds without cutoff, fall, detach, or halt.
This only bounds the observed interval; it does not establish stable long-duration control.
The articulated model already produces a 183.4 N·m peak during its initial neutral interval.
It fails `lean must produce forward acceleration`; the test remains ignored by default and red when explicitly run.
The failure proves a stock-controller/model response defect, not its precise root cause.
No controller gains, model defaults, or acceptance limits were changed to conceal it.
This was the initial failed acceptance result. The correction and repeated acceptance below supersede that status.

The default native library suite passes 79 tests with 10 ignored, including these two opt-in checks.
Strict native library/test Clippy passed. These counts describe the initial acceptance revision.

### Root cause and corrected acceptance

The held rider oscillated even with zero firmware wheel torque. The stock firmware was not the source.
Upper-body pitch relative to either supported foot is `-(hip + knee + ankle)`.
The controller sensed upper-body pitch/rate but applied its lean task moment only at the ankles.
This omitted the hip/knee columns of that task's Jacobian transpose.
The resulting non-collocated feedback could add energy to internal leg motion instead of damping it.

A regression gives both feet zero translational/angular velocity, checked through their MuJoCo Jacobians.
Upper pitch rate is -0.02 rad/s, each hip +0.04 rad/s, each ankle -0.02 rad/s, and each knee zero.
An ankle-only projection adds +0.360 W after separating posture terms.
The corrected projection removes -0.360 W in that same motion.
The first regression run failed before the correction; a four-second neutral stand regression also failed.
Both now pass. The neutral stand no longer self-excites without firmware.

The correction shares the upper-body task moment between the legs and applies its negative at all three hinges.
Posture moments remain separate. Hip/knee and ankle torque caps remain unchanged and still apply after summation.
Every transmission remains an internal +y joint motor. No external pitch force or pose/velocity write was added.
Free-flight angular momentum drift is 0.002350 N·m·s, within the unchanged 0.01 N·m·s tolerance.
That conservation check alone could not detect the original feedback-coordinate defect: internal motors can add energy.
No gains, mass, controller delays, model selection defaults, or native acceptance thresholds changed.

Corrected real-factory acceptance results, same stock build/settings/map and sequence:

| Quantity | Rigid | Articulated |
|---|---:|---:|
| Speed before lean, km/h | -0.0002 | +0.0015 |
| Speed after one-second forward lean, km/h | +0.7645 | +0.5261 |
| Shell travel during lean, m | +0.0438 | +0.0156 |
| Peak absolute wheel pitch during lean, degrees | 0.8888 | 0.7481 |
| Peak absolute motor torque during lean, N·m | 28.5379 | 21.1792 |
| Peak absolute Iq during lean, A | 33.1451 | 24.5983 |
| Actual upper-body lean at lean end, degrees | +2.3795 | +2.1400 |
| Peak neutral motor torque, N·m | 0.1054 | 0.0272 |

Both models pass the unchanged forward speed and travel checks.
Both remain attached with finite poses and no cutoff/fall/halt through six simulated seconds.
Both show a short initial reverse transient while establishing forward lean; the one-second response is forward.
The articulated sampled speed is -0.5445 km/h at 0.25 seconds of lean, +0.2886 at 0.5 seconds, and +0.5261 at one second.
This pass does not establish long-duration or high-speed stability. The model remains experimental; rigid remains default.

The additional ignored `stock_exact_com_joint_and_motor_trace` uses a concrete `FreeRideHost` for exact read-only physics access.
Its machine settings match the native factory; it does not replace the factory acceptance gate.
It traces COM relative to ankle midpoint, desired/commanded/actual upper lean, all six posture/angle/rate/torque tuples,
shell pitch, electrical motor torque, and speed at stand/neutral/lean/recovery boundaries.
At articulated lean end, COM is +0.021691 m forward and 0.919750 m above the ankle midpoint.
Left hip/knee/ankle angles are -0.010950/-0.012047/-0.011694 rad; right values agree within 0.000001 rad.
Posture targets remain zero; the separate internal upper-body task produces the lean response.
The COM forward offset returns to +0.000258 m after the final 0.5-second neutral interval.

The corrected physics suite passes 32 tests. Both real-factory stock checks and the exact native trace pass.
Strict physics and native library/test Clippy pass.
The final default native library suite passes 80 tests with 12 ignored; the three stock checks pass when explicitly selected.

## Next vertical slice

1. Retain the now-passing rigid/articulated stock acceptance and the internal-power regression.
   Extend bounded terrain/duration checks before making broader model-validity claims.
2. Add bounded joint torque/rate/work diagnostics to capture, without changing existing channel meanings.
3. Compare rigid and articulated responses on the same existing low-speed input and terrain.
   Report joint motion, upper-body acceleration, saturation, and aid work together.
4. Replace the two pedal welds in a new model version with sole/pedal contact and separate support states.
   Keep sole/ground contact. Define friction as an estimate until measured.
5. Test zero-load release, one-foot support, slip, loss of both supports, and no automatic remount.
6. Add hip roll/twist only after contact and reaction accounting pass.
   Reduce aids through explicit presets, without changing the rigid baseline.

No unfinished model-selector skeleton or visual-only knee motion forms part of this slice.

## Merge correction, 2026-10-21

The merge retains the Human upper-body lean coordinate and its internal joint torque mapping.
COM angle cannot replace that coordinate without a matching torque mapping.
The unchanged fixed-foot power regression passes at -0.360 W with measured angular rate, including time zero.
Articulated release retains zero artificial step-off kick. Rigid release keeps its original kick.

Manual turns use a roll target and external aids that remain separately identified.
Turn requests fade to zero below walking speed and obey the configured command delay.
Stationary requests cannot accumulate a heading target or start a spin.
The rigid model remains the default. Articulated remains available by explicit selection.

The heading twist spring decreases with speed. External yaw-rate damping remains active to prevent high-speed lateral instability.
A new straight-line regression first failed near 14 m/s and passes after that correction.
The complete physics suite passes 49 tests. Strict physics Clippy passes for all targets.
`docs/models/articulated-acceptance.md` records the test setup and measured turn results.
