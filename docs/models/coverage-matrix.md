# Model and Workflow Coverage

Feature completion and physical validity are separate claims.
A scenario verdict applies to its inputs, duration, firmware, timing, and model assumptions.
Unknown behavior must remain visible. It must not become a successful hardware claim.

Updated 2026-10-23 for the rows marked below; the other rows keep their 2026-10-21 state.

## Evidence states

- **Implemented:** Source code supplies the behavior.
- **Checked:** A named, recorded check exercised the stated behavior on a specified revision.
- **Measured:** Real hardware supplied the parameter or observation, with units and conditions.
- **Estimated:** A stated assumption supplies the value.
- **Unknown:** Neither implementation nor sufficient evidence establishes the behavior.

A source test is not evidence that the current revision passed that test.
Build and lint success do not establish physical accuracy or runtime correctness.

## Coverage matrix

| Area | Current implementation | Missing evidence or behavior | Required closure |
|---|---|---|---|
| Battery depletion | Equivalent circuit and charge integration; a model-validity boundary halts the run ([battery validity](battery-validity.md)) | Real near-empty discharge curve and pack protection behavior | Verify charge bounds and compare measured loaded voltage/current over charge and temperature |
| Battery limits | Per-pack imbalance, weak-cell and resistance faults | Cell resistance versus charge, temperature and age; actual pack switching | Establish cell/pack data before introducing calibrated limits |
| Charge indication | Model charge telemetry | Agreement with the wheel app's charge estimate | Keep model charge distinct and compare both against measured discharge |
| Energy balance | Electrical motor/inverter energy equations and source checks | Battery usable-energy limits and coupled long-run energy audit | Account for electrical input, mechanical work, heat and stored energy under stated tolerances |
| Motor torque | dq motor equations and current control | Measured flux, resistance, inductance and current-sensor gain | Calibrate against independent torque/current/back-EMF observations |
| High-current behavior | Firmware and inverter current models | Magnetic saturation, hardware over-current trip level and driver faults | Keep unknown protection levels explicit; validate before bench operation |
| Temperature | Lumped thermal nodes | Measured cooling, temperature-dependent winding resistance and magnet flux | Compare measured temperature response and model parameter sensitivity |
| Inverter and supply | Averaged PWM, dead time, diode and conduction losses | DC-link capacitor, switching transients, gate-driver failure and shorted MOSFET behavior | Implement required fault families or label them outside supported claims |
| BMS communication | Pack telemetry and silent/garbled reply faults | Independent failures of individual BMS boards and power isolation | Separate communication correctness from actual pack power availability |
| Tire and terrain | A 48-capsule tyre ring; contact, friction, rolling resistance; terrain strip (slope, bump, pothole, curb) and the city's height field and solids (2026-10-23) | Measured traction, wet/loose surfaces and impact/contact calibration | Check geometric agreement and load-dependent traction against measurements |
| Suspension | Spring/damper, travel and optional ratio curve | Actual rocker curve, stroke, preload, damping and bump-stop behavior | Obtain geometry and sag/travel measurements; preserve estimate labels |
| Rider | Rigid and articulated bodies (articulated by default), ankle control, lean-to-turn, fall release, bounded command/feedback delays; opt-in contact feet and lateral rider with legs on the side pads (2026-10-23) | Balance without external aids, contact feet by default, calibrated reaction delays, and behavior diversity | Run a documented rider parameter range; avoid one ideal controller as the only evidence |
| Hill behavior | Gravity and grade with loaded electrical plant | Independent torque/power balance across slope, speed, charge and mass | Compare required wheel force with actual torque and acceleration; report model validity throughout |
| Cut-offs | Compat behavior and intentional clean changes | Borderline pothole/overlean cases and unmeasured motor sensitivity | Identify the first detector; preserve protections until controlled evidence supports a change |
| CPU timing | Fixed CPI and exception overhead with own/reference core support; a selectable per-instruction flash timing model (2026-10-23) | Bus contention and hardware interrupt-load measurement | Preserve timing configuration and distinguish simulated cycle estimates from hardware measurements |
| Peripheral completeness | Modeled MCU/device interfaces plus fallback behavior | Full behavior of unmapped registers and unsupported devices | Review all nonfatal blockers and read-back fallbacks before making support claims |
| Numerical robustness | Deterministic clock, finite steps and parameter validation | Systematic extreme/nonfinite inputs and integration-step convergence | Use explicit validity failures and record time-step sensitivity |
| Scenario coverage | Shipped scenario library and expected-result checks | Boundary combinations, longer durations and unsupported failure families | Define coverage by parameter/fault dimensions, not scenario count alone |
| Scenario duration | The run stops and marks the declared end | Runtime confirmation after integration | Check final time, event/result delivery and replay behavior without judging later untested time |
| Workflow completeness | Ride, Tune, Compare, Recordings, manual controls, and Free Ride in the city (2026-10-23) | Integrated desktop runtime checks, error/cancel/restart paths | Trace each visible control to real backend behavior and record unsupported features |
| Recording integrity | .eucrec format, bounded capture/import, and recorded telemetry, body, event, BLE, and output tracks | Full lifecycle and malformed-file runtime evidence | Identify full versus partial capture; preserve data, limits and source identity |
| Long runs and load | Bounded queues, histories and recording limits | Sustained connected-radio use, cancellation under load and memory trends | Record drops, latency, memory and recovery under declared conditions |
| Distribution | Local Mac package work bundles the physics runtime | Integrated launch, clean-machine installation, public signing/notarization | Verify the selected artifact and keep local signing separate from public-release status |
| Other wheels | Profile structure and onboarding guide | Working second wheel and unsupported MCU/protocol implementations | Require independent stock, compat, plant and image evidence for each target |
| Hardware | Simulator-first safety rules | Controlled bench validation and recovery proof | Follow the separate M8 procedure; simulation never authorizes riding |

## Release claim rules

1. Record the source revision, image identity, profile, scenario, timing, and result window.
2. Record skipped checks, nonfatal blockers, unpaired values, model limits, and dropped samples.
3. Keep estimated parameters visible in the result's scope.
4. Reject a physical verdict when the model leaves its validity domain.
5. Preserve a result as borderline when timing or model perturbations change its verdict.
6. Define expected behavior independently of the desired result.
7. Keep verification coverage separate from simulator agreement and hardware measurement.

## Verification work still required

This document defines coverage requirements. It reports no new test or scenario runs.
The current work uses build, type, lint, and source review checks.
Executable verification must cover the matrix before a broader completion or hardware-accuracy claim.

## Human model and free ride

The [detailed plan](human-and-free-ride.md) separates manual controls, generated terrain, and human model upgrades.
Update, 2026-10-23: Free Ride is a play mode in the generated city, with keyboard and standard gamepad controls
([Free Ride README](../../app/src/workspaces/freeRide/README.md)). The native engine controls the world and the
rider. An input watchdog makes input older than 250 ms neutral; there is no arming step.
Recordings preserve the world and the external aid parameters.
Articulated knees exist; contact feet and the lateral rider are opt-in. Balance without external aids and measured
human response remain open.
