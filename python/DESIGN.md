# Native Python binding design

The package runs local firmware inside the existing Rust simulator.
PyO3 provides the native module. Maturin builds the Python package.
The package requires Python 3.11 or later.

`load_scenario(path)` parses TOML with `euc_sim::scenario::Scenario`.
`run_scenario(scenario, firmware_path, ...)` returns frozen Python data classes.
The result contains samples, body descriptions, events, and scenario outcomes.
Each result supports dictionary and JSON export.
Unknown telemetry values become `None` in Python and `null` in JSON.

`run_scenario` builds `WheelFiles`, `Machine`, `Session`, and `Ride` inside `Python::detach`.
The machine stays on the calling thread throughout its life.
Only owned input values and serializable results cross the closure boundary.
The binding uses `Backend::Own`, default CPU timing, and existing scenario checks.
It preserves all protection thresholds.

Firmware and optional symbol files require explicit local paths.
Local profile folders require an explicit `profiles_dir` argument.
The default profiles come from `ProfileSource::Builtin`.
The binding rejects known stock firmware pins.
Custom firmware without probe symbols supports physical telemetry only.
Checks that need cutoff probes require a matching probe map.
The binding exposes no firmware bytes, memory access, radio, or flashing API.

Limits bound input size, simulated duration, sample rate, sample count, and elapsed time.
The loop stops when the CPU halts or fails to advance.
The binding checks Python signals between sample steps.
Elapsed time checks also occur between sample steps.
Local profiles remain trusted inputs, rather than a security boundary.

`Session` adds interactive stepping, manual rider commands, reset, and capture.
It keeps the non-Send machine on its creating Python thread.
Construction, stepping, and reset retain the GIL under stable PyO3.
The elapsed-time limit includes time between calls.

`Session.save` and `load_recording` use the shared `.eucrec` writer and reader.
Captures include samples, events, decoded BLE frames, and board output changes.
Headers include the original scenario TOML and validated `RecordingContext`.
The reader preserves the saved map in free-ride context.
The binding does not implement another recording parser.
Result objects include typed phase results from measured-speed scenarios.

The root workspace excludes `crates/euc-python`.
The binding has its own workspace and Cargo.lock for PyO3 0.29.3.
Maturin 1.15.0 is pinned in python/pyproject.toml.
The native module also needs the existing MuJoCo 3.9.0 library.

Source archives include the reviewed built-in profile TOML files.
The `profiles` and `crates` links expose repository inputs to maturin's include rules.
The include rules select only the required profile files and optional backend wrapper source.
Cargo metadata needs that wrapper's manifest even when its feature stays disabled.
The wheel contains neither those source folders nor the optional backend.
Source archive creation requires checkout support for directory links.
