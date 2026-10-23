# Python API

The `euc-studio` package runs local firmware with the Rust simulator.
Import it as `euc_studio`.
It requires Python 3.11 or later, the pinned Rust toolchain, and MuJoCo 3.9.0.

## Build and test

Run these commands from the repository root.
Use an installed Python 3.11 or later.

```sh
python3.11 -m venv python/.venv
python/.venv/bin/python -m pip install maturin==1.15.0 pytest
python/.venv/bin/python tools/fetch-mujoco/fetch.py
cd firmware
cmake --workflow --preset target-clean
cd ..
cd python
.venv/bin/maturin build --release --locked --auditwheel skip --out dist
.venv/bin/python -m pip install --force-reinstall dist/*.whl
cd ..
EUC_STUDIO_REQUIRE_FIRMWARE=1 python/.venv/bin/python -m pytest python/tests -v
```

The firmware build needs the Arm GNU Toolchain from the [firmware guide](../firmware/README.md).
The test suite never downloads firmware.
It uses `firmware/build/target-clean/euc-fw-begode-extreme.elf` by default.
Set `EUC_STUDIO_FIRMWARE` to use an explicit local clean ELF instead.
Set `EUC_STUDIO_REQUIRE_FIRMWARE=1` to fail when that file is missing.
Without that setting, the suite skips tests that need missing firmware.

The wheel links the checkout's MuJoCo library through its runtime search path.
It does not bundle that library.
Keep the library at its build location when you use the wheel.
Set `MUJOCO_DYNAMIC_LINK_DIR` before the build to use another installed library location.

The bindings use a separate Cargo workspace and lockfile.
Run their Rust checks with a supported Python interpreter:

```sh
cargo fmt --manifest-path crates/euc-python/Cargo.toml --check
PYO3_PYTHON="$PWD/python/.venv/bin/python" \
  cargo clippy --manifest-path crates/euc-python/Cargo.toml --all-targets --locked -- -D warnings
```

## Run a scenario

```python
from pathlib import Path
import euc_studio as euc

firmware = Path("firmware/build/target-clean/euc-fw-begode-extreme.elf")
scenario = euc.load_scenario("scenarios/ride-20kmh-stop.toml")
result = euc.run_scenario(scenario, firmware)
print(result.completed, result.passed, result.halted)
print(result.outcome.top_speed_kmh)
print(result.samples[-1].signals.battery_v)
Path("result.json").write_text(result.to_json(indent=2))
```

`run_scenario` also accepts a TOML path directly.
It returns frozen data classes for samples, body descriptions, events, checks, and phase results.
Missing or nonfinite telemetry becomes `None` in Python and `null` in JSON.
`channels()` returns the Rust channel identifiers, labels, and units.
`to_dict()` and `to_json()` export result data.

Pass an ELF file, or pass a raw image with an optional matching ELF through `symbols_path`.
Cutoff checks require matching symbols and the profile's cutoff probes.
`profiles_dir` selects an explicit profile folder.
The default profiles come from the package's built-in profile set.

The API rejects known stock firmware pins.
It exposes no physical flashing, radio access, firmware memory, or firmware download operation.
Read [SAFETY.md](../SAFETY.md) before firmware work.

## Step and save a session

```python
session = euc.Session(scenario, firmware)
sample = session.step(100)
session.set_manual(lean=0.2, steer=-0.1)
sample = session.step(100)
session.release_manual()
while not session.finished:
    session.step(100)
session.save("ride.eucrec")
result = session.result()
session.reset()
```

`step(n)` advances at most `n` samples and returns the last sample.
It stops when the scenario ends or the firmware halts.
A call after the end raises `RuntimeError`.
`sample_count`, `time_s`, `sample_hz`, `finished`, and `halted` describe current progress.
`samples` and `events` return the data captured so far.
`result()` uses the same data classes as `run_scenario`.
A firmware halt makes `completed` and `passed` false.

Manual lean and steering values must be finite.
The engine clamps them to -1 through 1.
Positive values mean forward lean and left steering.
`set_push(newtons)` permits a finite push of up to 10,000 N in either direction.
Later scenario pushes can replace this push.
`set_held(bool)` changes the stand state, but the scenario releases it after `release_s`.
`step_off()` releases the rider from the pedals.

`save(path)` creates a new `.eucrec` file after at least one sample.
An existing file raises `FileExistsError`.
The capture includes samples, events, BLE frames, board output changes, and validated run context.
It stores the original scenario TOML and firmware hash, but no firmware image.
`reset()` clears captured data and reloads the same local input paths.

Create, use, and release each session on the same Python thread.
Interactive construction, stepping, and reset retain the GIL.
`run_scenario` releases the GIL during a complete run.

## Read a recording

```python
recording = euc.load_recording("ride.eucrec")
print(recording.header["wheel"], recording.start_s, recording.end_s)
print(recording.context)
print(recording.truncated, recording.skipped)
for change in recording.outputs:
    print(change.t_s, change.outputs.buzzer_hz)
```

The reader accepts the same `.eucrec` format as the desktop and CLI.
It needs no firmware image or active simulator.
It returns typed samples, events, BLE frames, and output changes.
Header and context values use dictionaries so callers can inspect saved metadata.
Unknown header fields remain available.
Free-ride context contains the saved height map, including its original grid values.
A run on the city world carries its world specification (`world`), and the lateral rider names its joints
(`rider_joints`).
The shared Rust context validator rejects invalid or unsupported context.

`truncated` explains recovery from an incomplete file, when applicable.
`skipped` counts unknown chunks or items.
Legacy files can have no context or output track.
The reader loads the complete capture into memory and accepts files up to 256 MiB.
It permits up to 100,000 samples and 8,000,000 stored signal or pose values.
Each sparse track permits up to 100,000 items, including unknown items.
Events, BLE frames, and output changes together permit 32 MiB of decoded data.
The reader checks these limits before it decodes chunk payloads.
Invalid index references or inconsistent item counts raise `ValueError`.
It does not replace missing output history with current board values.

## Limits and errors

Both run APIs accept these keyword limits:

| Argument | Default | Accepted values |
| --- | --- | --- |
| `sample_hz` | 100 | 1 through 1000 |
| `max_samples` | 100,000 | 1 through 100,000 |
| `max_seconds` | 600 | Greater than zero, up to 600 |
| `max_wall_seconds` | 300 | Greater than zero, up to 3600 |

The engine rounds sample periods to CPU cycles.
`RunResult` reports both the requested rate and the actual rate.
The limits also check whether the scenario fits before it starts.
Elapsed-time limits include user delays between interactive session calls.
The engine checks Python interrupts between samples.

File errors raise `OSError` subclasses.
Invalid values or incompatible files raise `ValueError`.
Elapsed-time limits raise `TimeoutError`.
A halted firmware run returns a failed result with its halt reason.
Local override profiles remain trusted inputs.
