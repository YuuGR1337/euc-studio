# Tune native acceptance

Status: focused acceptance, 2026-10-16.

## Scope

The tests are inside `app/src-tauri/src/engine/tune.rs`.
This work changes test modules only. It does not change production behavior.

Three test groups provide different evidence:

| Group          | Source                                                                  | Evidence                                                         |
| -------------- | ----------------------------------------------------------------------- | ---------------------------------------------------------------- |
| Request policy | Explicit synthetic `Host`                                               | Limits, expiry, errors, null values, and serialization           |
| Cached stock   | Pinned vendor image on the project Cortex-M3 interpreter, with MuJoCo   | Real Session RAM probes and simulated ISR counters               |
| Real runner    | Normal desktop host factory, cached stock image, and production threads | Pause, Step, restart, actual epochs, queue timeout, and recovery |

The stock tests use the production `RideHost` probe hooks.
They pass requests through production `enqueue`, `respond_with_handlers`, and `Pending::wait`.
The direct Session cases call the responder between steps.
That direct harness supplies run metadata. It does not prove runner status publication.
The separate real-runner cases use `Runner::start_without_audio_for_test`.
They start the same simulation and stream-pump threads as production.
They use the normal `firmware::build_host` factory and normal Tune and lifecycle queues.
Only the audio handle is unavailable. No test invokes the Tauri command wrappers.

The browser capture fixture is separate evidence.
It uses controlled transport replies and the production Tune UI.
It does not run firmware. Its results must not be described as native firmware acceptance.

## Image and session gate

The stock tests require an existing verified firmware cache.
They use `EUC_CACHE_DIR`, or the standard per-user cache path.
They never fetch an image. A missing image fails the explicit stock test.
The tests remain ignored in the default suite because the image is a local input.

The fixture loads the built-in `begode-extreme` profile.
It checks the current pin, the cached SHA-256, and the stock-map build label.
It also runs `euc_image::validate`, as the desktop image loader does.
Only then does it apply `profiles/begode-extreme/stockmap.toml`.

The direct fixture uses the neutral settings preset and a 120 Hz Session.
It uses the project Cortex-M3 interpreter, default CPI, default exception timing, and a 4000-instruction quantum.
The direct fixture keeps the wheel on its simulated stand.
It advances four simulated seconds before it reads the booted firmware.
The real runner uses the shipped `balance-60s` scenario, including its normal stand release.
That case stops after boot and one paused Step. It does not judge the complete scenario.

No image bytes, raw RAM bytes, recordings, or private research enter public files.
No physical wheel, BLE radio, flasher, or OS audio device opens.

## Complete checks

### Request policy

- Reject more than 64 requested names before queue admission.
- Reject empty names, control characters, and names longer than 128 UTF-8 bytes.
- Accept exactly 64 names of exactly 128 bytes each.
- Sort and remove duplicate request names.
- Keep the request queue at one slot.
- Report a full queue and a stopped receiver.
- Drop an expired request before any host read.
- Complete a response without blocking when its receiver disappears.
- End an unanswered wait after the two-second reply timeout.
- Reject an old session ID before any host or ISR read.
- Report unavailable hosts without substitute values.
- Filter, sort, and bound the catalog to 512 names.
- Return null with a reason for unknown, unavailable, or nonfinite values.
- Read ISR counters only when requested.
- Bound returned ISR rows to 128 and preserve the truncation flag.
- Preserve full `u64` counters as decimal JSON strings.
- Report absent ISR hooks as unavailable.

The timeout test checks the configured lower bound.
It does not assert a wall-time upper bound under OS scheduling delays.

### Cached-stock probes

The catalog must equal the profile's numeric RAM names.
The ASCII `version_string` field must stay outside the numeric catalog.
Units must match the documented semantic names. Other names retain unknown units.

The fixture independently decodes each declared numeric RAM field before it creates `RideHost`.
The response must match every decoded value exactly.
All 13 numeric fields must be finite after boot.
An unknown requested name must return null with a reason.

Repeated reads without a simulation step must preserve the full response.
This includes simulation time, RAM values, and ISR counters.
A later step must advance time and cumulative ISR counters.

A fresh Session resets the clock and cycle counters.
A request with the previous session ID must fail.
A request with the new session ID must return the fresh state.
The test supplies these session IDs. It does not prove that `Sim::build` increments them.

### Real runner lifecycle

The real-runner case prechecks the cached pin and image gates before startup.
It calls `Runner::play(false)` and waits for an actual paused catalog.
The catalog must contain all 13 numeric names and the actual backend, build, scenario, and source.

`Runner::play(true)` advances the firmware through four simulated seconds.
The test then pauses through the normal command queue.
All numeric probe values must be finite after boot.
Two paused snapshots must have identical time, values, and ISR counters.

`Runner::step()` advances the paused run by one 60 Hz display sample.
The response must remain paused.
Its ISR interval must remain positive and no larger than the elapsed simulated cycle count.

`Runner::restart(None)` rebuilds the actual host.
The catalog must change from actual epoch 1 to actual epoch 2.
The previous epoch must fail with the normal session-change error.
The new epoch must return a fresh clock and smaller cumulative cycle count.
Normal Play and Pause commands must work after reset.

The timeout case gates only entry to the real host factory.
It does not replace the host, transport, simulation loop, or request responder.
The gate has an eight-second wait limit, including failure cleanup.
While construction waits, the real Tune queue must reject a second request.
It must also reject an oversized request before admission.
The first pending request must time out after two seconds.
After construction continues, the runner must discard the expired request and answer a fresh catalog and snapshot.
The test uses bounded waits. It never extends the production probe timeout.

### ISR metrics

The real machine counters must satisfy these bounds:

- Each returned handler has a positive exception number and occupied cycle count.
- Returned handler names are not empty.
- Cumulative ISR cycles do not exceed total simulated cycles.
- The complete handler sum equals cumulative ISR cycles.
- A truncated handler sum does not exceed cumulative ISR cycles.
- Counters do not decrease across the measured interval.
- Interval ISR cycles stay between zero and elapsed simulated cycles.

The reference-timing case disables return and tail-chain costs.
That case must report ISR metrics as unavailable.
Its Session load signal must remain nonfinite instead of reporting a false zero.

These counters describe occupied simulated cycles.
They do not measure a physical controller or establish cycle timing accuracy.

## Verification

Run these commands from the repository root:

```sh
PYO3_PYTHON=/opt/homebrew/bin/python3 cargo test --manifest-path app/src-tauri/Cargo.toml --lib engine::tune::acceptance_tests -- --nocapture
PYO3_PYTHON=/opt/homebrew/bin/python3 cargo test --manifest-path app/src-tauri/Cargo.toml --lib engine::tune::cached_stock_acceptance_tests -- --ignored --nocapture --test-threads=1
rustfmt --edition 2024 --check app/src-tauri/src/engine/tune.rs
```

The focused run passes all 16 Tune tests: 12 request-policy tests and four cached-stock tests.
The explicit run uses `--include-ignored --test-threads=1`.
Strict Clippy passes for the app library and test targets.
Rust formatting and document formatting checks also pass.
No production defect appears in these cases.

The cached-stock probe, paused-read, reset-generation, and ISR interval check passes.
It finds 13 numeric names.
The measured interval has 9,260,544 ISR cycles across 18,000,002 simulated cycles.
This ratio is about 51.45% simulated ISR occupancy.
The real runner reports 701,180 occupied ISR cycles across its 1,200,000-cycle paused Step in the initial focused run.
That interval depends on the actual pause boundary. It is not a fixed expected value.

Additional check commands:

```sh
PYO3_PYTHON=/opt/homebrew/bin/python3 cargo test --manifest-path app/src-tauri/Cargo.toml --lib engine::tune:: -- --include-ignored --nocapture --test-threads=1
PYO3_PYTHON=/opt/homebrew/bin/python3 cargo clippy --manifest-path app/src-tauri/Cargo.toml --lib --tests -- -D warnings
```

## Incomplete checks and required hooks

| Boundary                                  | Status                    | Required work                                           |
| ----------------------------------------- | ------------------------- | ------------------------------------------------------- |
| Production queue and responder            | Complete                  | No substitute transport                                 |
| Real mapped Session and RideHost          | Complete                  | Cached pinned stock image                               |
| Actual runner thread and command ordering | Complete                  | Uses a test-only audio-free constructor                 |
| Tauri command wrapper and webview IPC     | Incomplete                | Tauri mock runtime support in the test dependency setup |
| Renderer Hold/Rearm generation            | Separate browser evidence | No native acquisition API owns these generations        |
| Trigger capture storage                   | Frontend scope            | Native snapshots remain read-only requests              |
| Raw memory writes and runtime quirks      | Not implemented           | Outside this acceptance work (runtime quirks: SIL, M5)  |

The integration supplies `Runner::start_without_audio_for_test(factory, scenario, backend, recordings, manual)`.
That test-only constructor uses the common startup path with no native audio handle.
Production `Runner::start` still initializes native audio before simulation startup.
These tests make no claim about physical audio output or startup sound.

The Tauri dependency does not currently enable its mock-runtime test feature.
No test claims actual Tauri invocation, webview IPC, or engine-state lock release.
The runner checks do not close those wrapper-level gaps.
