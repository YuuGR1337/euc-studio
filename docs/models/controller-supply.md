# Controller supply model

## Evidence and limits

The stock trace uses the local pinned GW1825007 image, SHA-256
`cbfb328cab1e0b8219790dd3ef710bb266569c70b6ac396ccd06c247da9fd27d`.
Before this change, PB14 drops at 4.815932 s after a Power hold starts at 4 s.
After release, the old model keeps the CPU running and the display lit.
`BoardLogic` previously records the latch without switching a supply domain.

The board profile maps PB14 to the latch and PB15 to the active-low Power input.
The boot contract in `firmware/apps/begode-extreme/main.c::power_on_hold` waits for button release when it refuses to latch.
This supports a momentary contact that sustains the rail independently of the latch.

The model uses **INFERRED** wiring: `Power contact pressed OR latch high` enables the controller rail.
The initial synthetic button hold also supplies this rail.
No measured circuit, rail voltage, capacitor decay, or transistor switching delay validates this assumption.
The model does not establish real hardware power-cycle timing.
Released latch GPIO does not sustain the rail.

## Supply states

`Machine::powered()` reports controller supply, not the firmware latch or battery availability.
A low latch while Power remains held does not remove supply.
Releasing the last supply source removes it at the simulation boundary.
No arbitrary display timeout or UI-only blanking occurs.

Without controller supply:

- The CPU retires no instructions and takes no interrupts.
- Powered-device timers, queued transfers, and output signals stop.
- The display driver resets off. The buzzer emits silence through its existing monotonic change history.
- GPIO outputs release. The headlight pin floats; effective light output is zero.
- All inverter legs enter their existing `Off` state.
- Motor current can decay through the existing electrical model. No torque or speed is forced to zero.
- The electrical plant, battery, body, rider, and physical board contacts keep their simulation time and state.
- Firmware RAM values become unavailable. Session cutoff state uses `controller_supply_off`, distinct from a firmware flag.

The simplified powered domain contains modeled MCU and off-chip protocol devices, except physical board contacts and the plant.
The model does not establish separate supply rails for the display, lights, sensors, or BMS protocol electronics.
Pack charge, temperature, motor state, and world state remain in the plant.
Existing backup-domain reset behavior follows the profile's backup-power configuration.

## Wake and input readiness

`Machine::power_button_ready()` admits live Power input while off and renews an existing Power hold after latch drop.
`BoardLogic::buttons_ready()` remains the Handle and normal-control check.
Handle cannot wake the controller.
The desktop runner still owns epoch checks and the 250 ms wall lease.
Lease expiry releases the same physical contact; it does not issue a runner restart.

A Power-down edge supplies the rail and resets the controller core and powered devices.
It preserves world pose, velocity, simulation time, plant state, and the held physical Power contact.
The firmware must execute its normal boot and assert its own latch.
If Power releases before that latch assertion, supply drops again.
Only the initial synthetic boot recipe schedules an automatic button release.
An interactive wake hold never gets that synthetic release.

Hall edges cannot reach unpowered GPIO.
Before the first wake instruction, the plant publishes all three current Hall levels again.
This prevents a moving unpowered rotor from leaving stale GPIO inputs at boot.
The plant does not reset rotor angle to obtain a matching code.

## Verification

The default engine tests cover the supply-source rule, Power-only wake, hold renewal, and early wake release.
A tiny own test program drops PB14 and places an undefined instruction immediately afterward.
The controller powers off before it reaches that instruction.

The native integration tests require an explicit local image and never fetch firmware:

```sh
EUC_TEST_STOCK_IMAGE=/absolute/path/to/the/pinned-stock.bin \
  cargo test --manifest-path app/src-tauri/Cargo.toml --lib stock_power_ -- --ignored --nocapture
```

The tests use the own CPU, CPI 2.0, default exception costs, and a live plant.
The off/wake test runs at quanta 4000, 3000, and 2000.
It checks actual Host outputs, stopped CPU and handlers, continued physics time, and unchanged world state at wake.
It also checks real firmware display initialization after wake.
A separate test moves the unpowered wheel through several Hall sectors with rider effort and gravity.
It checks current GPIO Hall truth before the first wake instruction.
These are simulator results under the inferred supply wiring, not hardware validation.

## Final acceptance checks

The host checks use the current pinned image and verify its SHA-256 before selecting the stock map.
Shutdown and wake pass at quantum 4000, 3000, and 2000. Controller power stops the CPU and
interrupts, blanks the real display driver, silences the buzzer, and leaves physics running.

The moving restart check uses half the firmware shutdown-speed limit as its rider target.
Measured speed is about 0.274 m/s before shutdown. All three timing settings cross four Hall
sectors while off. Current Hall levels reach GPIO before the first restart instruction.
Complete physical and electrical snapshots compare equal across each zero-time supply transition.

Stock firmware disables PWM before its settings save and latch release. Its current is already
zero when this normal shutdown removes the rail. This scenario therefore does not prove energized
current decay. A separate plant coupling test establishes more than 1 A through modeled PWM,
removes the controller supply, checks that current and physical state are not reset, and then
checks physical decay below one percent after 20 ms. That test is a component stimulus, not a
stock-firmware scenario result.
