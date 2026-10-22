# Tune triggers and handler counters

## Design and scope

Free mode follows live samples.
Rising and falling modes compare consecutive finite samples against a user threshold.
A crossing starts acquisition for all selected scope histories at the same sampled time.
The After trigger control selects 0, 1, 5, 10, or 15 simulation seconds; the default is 5.
Zero holds immediately. Other values hold at the first observed sample at or after the requested end.
No interpolation invents an exact end-time sample. The capture keeps its actual crossing and end times.
The crossing appears as a vertical marker in each scope.
Manual hold freezes scope data without stopping the simulator or current value reads.
Rearm discards the capture and waits for a new crossing.
Configuration changes, session changes, errors, and time reversal reset trigger baselines.

The shared time cursor reads the nearest stored sample without interpolation.
The four wall-time polls per second cannot detect edges between samples.
Each capture retains up to 120 pre-trigger observations and at most 240 total observations per variable.
Paused duplicate timestamps do not consume capacity. Unavailable values remain gaps.
A sample limit or terminated run holds the available data and explicitly marks the capture incomplete.
Manual Hold stops an active acquisition without discarding its pre-trigger data.
Only the last 30 simulation seconds appear in the plot. Older retained observations do not extend outside its axes.
This is sampled desktop acquisition, not an MCU or hardware trigger.

The ISR table uses cumulative machine counters from two snapshots.
It computes interval percentages from exclusive occupied cycles and elapsed machine cycles.
Counter strings preserve exact unsigned 64-bit values in JavaScript.
Paused intervals show no percentage measurement.
Counter decreases and session changes discard the baseline.
The table identifies simulator cycle estimates separately from physical timing measurements.

## Parent-owned wiring

The parent adds `Host::handler_counters() -> Option<super::tune::HandlerCounters>` with a default `None` result.
The RideHost implementation returns `Some(super::tune::handler_counters(self.session.machine()))`.
The StillHost implementation can return `Some(super::tune::handler_counters(&self.machine))`.
The parent calls `tune::respond_with_handlers` from the runner.
Its last argument is `|host| host.handler_counters()`.
The parent confirmed this wiring during implementation.
The obsolete `respond` wrapper is removed.
This worker does not modify host or runner files.

## Scope ledger

- Named probes, filtering, and bounded scopes remain implemented.
- This patch adds sampled rising and falling triggers, hold, rearm, and a shared time cursor.
- This patch adds optional bounded ISR counters and interval table logic.
- The ISR table uses the parent-owned Host and runner wiring.
- Memory writes, register fields, gain editing, and runtime quirks remain incomplete.
- Bounded post-trigger acquisition is implemented. Edges between polls remain undetectable.
- Nine trigger state-machine tests cover acquisition, pause, gaps, termination, capacity, resets, and manual hold.

## Checks

The trigger state-machine suite passes nine tests. The desktop TypeScript and scoped ESLint checks pass.
Eleven Chromium interaction checks pass against controlled probe replies. The fixture uses the production workspace, hook, trigger state, scopes, API client, and CSS. It covers post-trigger duration, paused time, incomplete captures, sample capacity, manual Hold/Rearm, delayed success and failure replies, session resets, and 1280×720 layout. The controls and both plots fit without horizontal overflow.
These browser checks do not run native firmware. Separate [native acceptance](../../../../docs/models/tune-acceptance.md) passes sixteen tests, including cached stock and actual runner lifecycle cases. Full Tauri IPC invocation remains unchecked.

## Snapshot API

JavaScript can request `{ sessionId, names, includeHandlers: true }` through `engine_tune_snapshot`.
Omitted `includeHandlers` defaults to false.
The reply adds nullable `handler_counters` and `handler_reason` fields.
The counter object contains `cycle`, `isr_cycles`, `rows`, `total_handlers`, and `truncated`.
Each row contains `exception`, `name`, and `occupied_cycles`.
All cycle counts use decimal strings.
The server sends at most 128 rows with nonzero session counts.
The full ISR counter supplies aggregate load even when rows truncate.

Missing hooks or unavailable metrics produce a null counter object and an explicit reason.
Demo and recording playback do not request live values.
Completed sessions remain inspectable without false interval measurements after simulation stops.

## Changed paths in this delivery

- `app/src-tauri/src/engine/tune.rs`
- `app/src/engine/tune.ts`
- `app/src/workspaces/tune/useTune.ts`
- `app/src/workspaces/tune/triggers.ts`
- `app/src/workspaces/tune/handlers.ts`
- `app/src/workspaces/tune/TuneWorkspace.tsx`
- `app/src/workspaces/tune/ProbeScope.tsx`
- `app/src/workspaces/tune/TriggerControls.tsx`
- `app/src/workspaces/tune/HandlerTable.tsx`
- `app/src/workspaces/tune/SCOPE.md`
- `app/src/workspaces/tune/EXTENSIONS.md`
