# Tune scope and design

## Design

Tune reads named firmware variables from the active simulator session.
The simulation thread owns every probe read.
Tauri commands send bounded requests to that thread and wait for bounded replies.
No request reads raw memory or changes firmware state.

The page contains a source summary, a searchable variable table, and selected variable scopes.
The source summary reports the backend, scenario, firmware label, build, and map source.
The table shows real variable names and values.
Only documented names receive units.
The scopes keep a fixed number of samples and use simulation time.
New sessions and engine errors clear old values and history.

The browser and demo show an explicit unavailable state.
The still host reports that session probes are unavailable.
Missing maps, unknown names, unreadable values, and nonfinite values remain unavailable.
Numeric decoding follows `Session::firmware_value`; the API exposes no independent type metadata.
The page never replaces missing values with zero.

## Scope ledger

The workspace and commands are registered in the desktop application.
Implementation, controlled browser checks, and native firmware acceptance are separate evidence.

| Promised capability               | Scope                                                                     |
| --------------------------------- | ------------------------------------------------------------------------- |
| Named firmware variable inspector | Implemented. Catalog and bounded snapshot commands read Session probes.   |
| Current values and filtering      | Implemented. The table reads actual values and filters names.             |
| Variable scopes                   | Implemented. Four selected scopes keep bounded history.                   |
| Source and build identity         | Implemented. Replies report runner identity and Session map availability. |
| Raw memory inspector and writes   | Incomplete. No raw memory API enters this change.                         |
| Runtime quirk switches            | Implemented. The Quirks tab restarts the run on Ours-custom (SIL).        |
| Firmware gain editing             | Incomplete. Named reads do not establish safe parameter writes.           |
| Persistent tuning presets         | Incomplete. No write path or preset storage enters this change.           |
| Compare workspace                 | Separate workspace with its own A/B runner and acceptance checks.         |

Register bit fields, gain editing, and memory writes remain incomplete.
Quirk switches need the Ours-custom firmware, which runs the SIL library; built images keep their quirks.
A switch never changes the running firmware: "Restart with these quirks" starts a new run with them.
Sampled triggers, a shared cursor, and per-handler ISR intervals now appear in [EXTENSIONS.md](EXTENSIONS.md).
The UI lists those limits explicitly.
The probe API does not expose a firmware hash or exact map file.

## Bounds and lifecycle

The catalog permits at most 512 names.
A snapshot permits at most 64 unique names, each at most 128 bytes.
The UI requests at most 48 table values and four selected scope values per poll.
The UI polls at most four times per wall second with one request in flight.
Live history holds at most 120 samples per variable over a 30-second simulation window.
An acquired capture retains at most 240 samples per variable, including pre-trigger history.
The plot shows the last 30 simulation seconds of retained data.
Inactive workspaces stop polling.
Replies carry a session generation to prevent history across restarts.
The client also rejects replies from before Hold, Rearm, retry, or a source change.
Rearm establishes its baseline from the first fresh response.
Missing replies and engine errors clear stale measurements.

## Integration

`engine::tune` defines the catalog and snapshot Tauri commands.
`lib.rs` registers both commands, and `Shell.tsx` mounts `TuneWorkspace` with its visibility state.
The simulation runner serves the bounded request queue between simulation steps.
Host hooks read session variables and machine handler counters on that thread.

### Command contract

`engine_tune_catalog` takes no arguments beyond managed `EngineState`.
It returns `run`, `available`, `reason`, `variables`, `total`, and `truncated`.
Each variable contains `name`, optional `unit`, and optional `unit_reference`.
Units describe raw map values without inferred conversions.

`engine_tune_snapshot` takes `{ sessionId, names }` from JavaScript.
It returns `run`, `t_s`, and `values`.
Each reading contains `name`, nullable `value`, and nullable `reason`.
The server rejects a snapshot for a different session generation.

Both replies describe the backend, scenario, firmware label, build, state, and read source in `run`.
`run.firmware` contains the runner's actual label, not a requested firmware ID.
`run.source` identifies the Session firmware map API.
The current Session API does not expose the exact map file or image hash.
Still-host, missing-map, and engine errors do not produce fabricated values.

The server queues at most one pending request.
It rejects excess requests and waits at most two seconds for a reply.
It expires queued work before any probe read.
The frontend also has a three-second transport timeout.
Every read occurs between simulation steps, including reads while paused or halted.

### Implemented extension: triggers and handler statistics

[EXTENSIONS.md](EXTENSIONS.md) describes the integrated extension and its API.
Free, rising, and falling modes use sampled firmware values.
Hold freezes all selected scopes at one simulation time.
Rearm waits for a new sampled crossing.
A shared time cursor reads stored samples without interpolation.

The ISR table uses the parent's machine counters through the Host and runner hooks.
It calculates interval load from two snapshots and preserves exact 64-bit cycle counts.
Paused or completed runs do not produce false zero-load intervals.
Demo and recording playback show explicit unavailability.
Bounded sampled post-trigger acquisition is implemented; see EXTENSIONS.md for limits and evidence.
Memory writes remain unsupported. Quirk switches restart the run on Ours-custom ([docs/sil.md](../../../../docs/sil.md)).

## Verification

- Nine trigger state-machine tests cover capture, pause, missing values, termination, bounds, resets, and manual Hold.
- Eleven Chromium checks use the production workspace and controlled transport replies. They exercise capture controls, stale responses, quick resets, and 1280×720 layout.
- The complete frontend suite passes 259 tests. TypeScript and full frontend ESLint checks pass.
- Separate native acceptance passes sixteen tests, including cached stock and actual runner lifecycle cases. Browser fixtures do not establish native firmware behavior. Full Tauri IPC invocation remains unchecked.

See [EXTENSIONS.md](EXTENSIONS.md) for capture and handler-counter details.

Changed files:

- `app/src/engine/tune.ts`
- `app/src/workspaces/tune/TuneWorkspace.tsx`
- `app/src/workspaces/tune/ProbeScope.tsx`
- `app/src/workspaces/tune/useTune.ts`
- `app/src/workspaces/tune/SCOPE.md`
- `app/src-tauri/src/engine/tune.rs`
- `app/src-tauri/src/engine/host.rs`
- `app/src-tauri/src/engine/runner.rs`
