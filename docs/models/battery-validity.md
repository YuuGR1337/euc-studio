# Battery model validity

## Charge boundary

The battery model uses parallel packs with an OCV curve, constant resistance, and one polarisation RC pair.
Its SOC describes remaining modeled charge. The OCV curve alone does not limit that charge.
The previous SOC clamp allowed continued discharge at zero SOC while the OCV table retained a positive voltage.

`Battery::step` now checks each pack before it accepts a step.
For pack current `I`, capacity `Q = capacity_ah * 3600`, and step duration `h`, it calculates `delta_soc = I * h / Q`.
Positive current removes charge. Negative current adds charge.
An accepted step keeps every pack inside `0 <= soc <= 1`.
The model rejects a step that needs more charge than a pack contains.
It also rejects a charging step that exceeds modeled capacity.
It does not discard charge through an SOC clamp.

The check applies to individual packs, including current between unequal packs.
An empty pack supplies zero positive discharge current over any positive step duration.
It can accept regenerative current and return to positive SOC.
The standalone battery API permits this recovery.
One empty pack invalidates plant discharge because the model has no pack isolation or switching behavior.

The model rejects a whole step instead of splitting it at the charge boundary.
Thus, the last accepted SOC can remain slightly above zero when the next step needs more charge.
The rejected step supplies no motor energy and changes no battery state.
The electrical plant also rejects its candidate motor, sensor, thermal, torque, and time updates.
The rejected step latches an error for that plant instance.
A later fault cannot clear this error and make the same run valid again.

This design keeps the existing integration grid and avoids a sudden bus-voltage collapse.
The RC and thermal updates use exponential relaxation with `exp_m1` for small steps.
The parallel solve uses a reference voltage to prevent false circulation between identical rested packs.
The motor retains its existing backward Euler solver.
The model does not claim exact terminal-energy accounting across the staggered voltage and current updates.
Its boundary prevents an unlimited empty-pack charge supply.

## API and stop contract

`Battery::step(load_a, ambient_c, h)` returns `Result<(), BatteryLimit>`.
Success commits all packs together. Failure changes no pack or terminal voltage.
Zero duration changes nothing.
Nonfinite inputs, negative duration, nonfinite results, and nonpositive loaded voltage produce `InvalidStep`.
The voltage condition marks the circuit model's mathematical domain. It defines no real undervoltage threshold.

`Battery::discharge_validity()` reports an empty pack, including an initially empty battery.
`ElectricalPlant::battery_validity()` reports an empty pack or the first rejected battery step.
`ElectricalPlant::advance()` stops at the last accepted grid state when this check fails.
It returns normally, so engine integration must inspect `battery_validity()`.
Wheel and inverter input setters ignore further changes while the model is invalid.
`next_hall_edge()` returns no event while the model is invalid.
`bus_v()` retains the last accepted voltage. That voltage does not establish model validity.

`BatteryLimit` supports structured serialization:

| `kind` | Additional field | Meaning |
|---|---|---|
| `exhausted` | `pack`, zero-based integer | A pack is empty or the next step exceeds its remaining charge |
| `charge-saturated` | `pack`, zero-based integer | The next charging step exceeds a pack's capacity |
| `invalid-step` | None | The step leaves the finite, positive-voltage circuit domain |

The parent must check validity before the first firmware run and after every electrical advance.
The check must precede `mean_torque_nm()`, body physics, and another firmware step.
The engine must stop event processing and return a distinct invalid-model result on failure.
A stopped electrical clock must not cause repeated scheduling of the same event.
The scenario runner must reject success after any model validity error.
The app must report model exhaustion separately from a firmware cutoff or rider fall.

`PlantState` keeps its current fields. This patch adds no JSON snapshot field.
`bus_v`, `bus_a`, `soc`, and torque still describe the last accepted electrical state.
They must not describe later engine time after a validity error.

The parent needs these connections:

| Component | Required change |
|---|---|
| `euc-sim/src/plant.rs`, `Live::step_to` | Check validity after electrical advance, before physics or torque transfer |
| Engine plant/event advance path | Stop before firmware observes or acts on stale electrical state |
| `euc-sim/src/session.rs`, `Session::step` | Return or record an invalid-model result before normal snapshot success |
| `euc-sim/src/scenario.rs` | Preserve the error and prevent a passing verdict |
| Bridge, recording, CLI, and app | Carry a structured error, the last accepted electrical cycle, and the pack index when present |

The parent must define any new result fields across these consumers together.
The existing ride snapshot does not carry the new error.
Until the parent completes this connection, the library stop alone does not prevent a false successful ride report.

Existing standalone callers must handle the new `Battery::step` result.
All eight existing battery test call sites now use `unwrap()` for accepted steps.
The heating test runs for 300 seconds instead of 600 seconds at 150 A total.
Each identical pack supplies 75 A and reaches its charge boundary after 432 seconds from the configured initial SOC.
The shorter test retains its heating and reading checks within the accepted charge range.

## Model limits

These boundaries are software validity limits. They do not describe this wheel's BMS cutoff, contactor, or load-switch behavior.
The profile defaults and firmware safety thresholds stay unchanged.
The model adds no per-cell disconnect threshold.

R0, R1, and the RC time constant stay constant for a healthy pack.
Faults can multiply resistance, but no measured SOC, temperature, current, or aging resistance curve exists in this model.
The OCV curve is a default estimate. Its zero-SOC voltage is a curve endpoint, not a BMS trip measurement.
Voltage recovery does not restore charge unless current flows into a pack.

Model SOC comes from initial SOC, configured capacity, and integrated pack current.
`Signals::soc_pct` currently equals 100 times the mean modeled pack SOC.
That value is not an independently measured wheel or phone-app battery percentage.
An app can use voltage, filtering, or a different calibration to calculate its percentage.
The arithmetic mean also hides pack imbalance and a single empty pack.

The model has no BMS power switches, pack isolation, reconnection delay, or current-limited pack switching.
It also omits a DC link capacitor, charge acceptance limits, cell balancing, OCV hysteresis, and capacity aging.
Cell-drop faults change OCV but do not model a separate capacity for each cell group.
The model cannot predict which real cell or pack disconnects first.

The motor uses fixed resistance, inductance, and magnetic flux.
It omits magnetic saturation, iron loss, and temperature changes in resistance and flux.
Its winding thermal node does not change those electrical parameters.
These limits affect current, heating, voltage margin, and low-charge torque predictions.

## Minimum probes and calibration evidence

Future comparisons need synchronized model probes for each pack's SOC, current, terminal voltage, and temperature.
They also need minimum cell voltage, bus voltage/current, requested and accepted grid time, and `BatteryLimit`.
Motor probes need phase currents, dq currents, applied voltage, wheel speed, torque, and winding temperature.
The existing aggregate SOC and bus voltage cannot establish individual pack validity.

Minimum calibration evidence includes rested OCV against independently counted charge and usable capacity across the intended operating range.
Load and recovery records need synchronized pack currents, bus voltage, cell voltages, and pack temperatures.
Current steps can separate immediate voltage drop from slower polarisation recovery.
Motor calibration needs resistance, inductance, flux, torque/current behavior, and their changes with current and temperature.
BMS switching needs separate evidence before the model can claim a cutoff or reconnection rule.
These are evidence requirements. This change performs no hardware measurements or access.

## Regression test evidence

The user authorized automated tests on 2026-10-16.
This test update changes only the test modules in `battery.rs` and `plant.rs`.
It adds nine battery tests and four plant tests. It changes no production code.

| Test group | Checks |
|---|---|
| Empty battery | Positive discharge fails without charge removal or state changes, despite positive OCV |
| Rested endpoints | Empty and full packs accept zero load without false circulation; zero duration changes nothing |
| Regeneration recovery | Charging restores positive SOC and discharge validity in the standalone battery |
| Full battery | Charging fails atomically; discharge remains valid |
| Exact endpoints | Accepted steps can reach SOC zero or one exactly; further outward steps fail |
| Unequal pack oversteps | Pack 1 crosses its boundary after pack 0 passes; neither pack commits any change |
| Single empty pack | Either empty pack invalidates discharge despite positive mean SOC |
| Pack circulation | Zero bus load still checks each pack's charge boundary |
| Invalid steps | Nonfinite inputs, negative duration, and nonpositive loaded voltage fail without state changes |
| Initially empty plant | Time, motor state, torque, wheel input, and inverter inputs remain unchanged |
| Plant discharge rejection | The rejected step preserves accepted time, motor state, pack state, readings, heat, and torque accumulators |
| Plant error latch | An SOC recovery fault cannot clear the rejected-step error or restart integration |
| Full plant regeneration | Diode rectification fails without a motor, torque, or time update |

The endpoint test uses a synthetic constant OCV curve for exact arithmetic.
The plant discharge tests use a small synthetic capacity to reach rejection within a fixed step count.
These values support software regression checks. They do not represent measured wheel parameters.

Commands from the repository root:

```sh
cargo test -p euc-plant --lib --target-dir target/battery-regression
cargo clippy -p euc-plant --all-targets --target-dir target/battery-regression -- -D warnings
rustfmt --check --edition 2024 crates/euc-plant/src/battery.rs crates/euc-plant/src/plant.rs
```

Results: 58 tests passed, zero failed. Clippy passed with warnings denied. The format check passed.
The separate build directory avoids contention with the parent's workspace test build.
No scenarios or hardware checks ran for this test update.
These tests establish library boundary behavior. They do not establish physical accuracy or complete engine stop behavior.
