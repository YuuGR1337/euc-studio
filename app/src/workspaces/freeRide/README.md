# Free ride frontend

## Parent connection

Import `FreeRidePanel` from `./index.ts`.
Pass the current `EngineClient` as `engine`.
Pass `active={true}` only while the Ride drawer is open and its route is active.
Use one panel for each native run.

`onMap(map, epoch)` supplies the native grid and its run epoch.
The callback sends null when controls become inactive or the active run changes.
Retain the last valid grid in the viewport cache.
Display it only when live `runMode` is `free_ride` and `runEpoch` matches its epoch.
Do not replace a physical map with a flat plane when controls close.

The parent owns map rendering and the viewport route.
The optional `onExit` callback must call `engine.setScenario` with the chosen scenario.
The panel pauses before it calls `onExit`.
A restart uses the parent's `engine.restart` path.

The panel installs a capture listener for Space and Escape.
Space pauses while the panel owns focus, including buttons and ride sliders.
Escape pauses from anywhere while the panel is active.
The existing global command listener uses the bubble phase.

## UI

The panel follows `app/DESIGN.md` and the gallery Controls and Panel examples.
Controls and Map use separate tabs in one dock.
Sliders, selects, buttons, switches, pills, and tabs use the shared UI components.
Input settings, ride details, and exact map values stay closed until selected.
The map editor shows one feature at a time.
Only exact numeric fields use a local input, with the existing Tune field style.

Shared Radix controls keep their keyboard events.
Ride sliders retain spring return and pause guards.
Changing tabs pauses controls.
Returning to Controls never arms or starts the ride.
Wheel buttons use `data-wheel-control`; the existing `data-wheel-controls` container also works.
Their focus keeps the run active and sends neutral manual input.
Manual motion requires neutral controls for 250 ms after focus returns.
Space and Enter remain available for measured wheel button holds.
Escape pauses free ride and then passes through to Radix dialogs and menus.

## Native API review

The frontend uses these exact command arguments:

| Command            | Arguments                                                                                               |
| ------------------ | ------------------------------------------------------------------------------------------------------- |
| `free_ride_start`  | `{ spec: { wheel: 'begode-extreme', settings: 'blank', rider_model: 'articulated_sagittal_v1', map } }` |
| `free_ride_status` | none                                                                                                    |
| `free_ride_map`    | `{ epoch }`                                                                                             |
| `free_ride_arm`    | `{ epoch, source }`                                                                                     |
| `free_ride_input`  | `{ input: { run_epoch, seq, source, desired_lean_rad, heading_rate_rad_s, armed } }`                    |
| `free_ride_pause`  | `{ epoch }`                                                                                             |

The source values are `keyboard_slider` and `gamepad`.
Input sequences use uint32 values.
The sequence increases across source changes and drawer closure.
A new native epoch resets it.
Native epochs must increase during the desktop session.

A start reply confirms a queued command.
The panel waits for a new free ride epoch before it accepts the map specification.
It fetches the authoritative grid for that epoch.
A late map or status reply cannot replace data after closure or source replacement.

Ready controls send neutral, disarmed packets at 20 Hz.
The frontend requires a 250 ms neutral period after the first native input reply.
Native status must confirm the selected source before Arm.
Arm waits for the previous input call to finish.
The frontend stops disarmed packets while Arm is pending.
Native Arm starts play; the frontend never starts play after a status update.
The Arm IPC reply only confirms a queued command.
The panel checks native status every 25 ms for up to 200 ms after that reply.
It shows Armed only when epoch, source, ready, and armed match the request.
The existing 250 ms Arm timeout still pauses controls.
No disarmed heartbeat runs during this confirmation wait.
The first confirmed armed packet is neutral.
Rejection and timeout keep the panel disarmed and show an error.

Armed controls send packets every 50 ms of wall time.
One input call and one pending value bound the frontend queue.
The native 250 ms watchdog remains the final input guard.
Blur, hidden state, text focus, closure, and source changes clear input and request pause.
A late Arm reply triggers another pause.
Gamepad reconnection requires device selection and a new Arm action.

The map specification matches `crates/euc-physics/src/map.rs`.
The editor checks dimensions, base grade, feature limits, and spawn clearance.
The native generator checks final heights and triangle grades.
The editor preview shows supports only.
It does not calculate a second height surface.

## Limits

Only standard gamepad mapping is supported.
Gamepad buttons do not arm controls.
Browser support and device permission can limit gamepad access.

The native API does not return the current map specification.
This module retains its last applied specification during the desktop session.
An external restart can leave the editor without the specification for the new epoch.
The editor shows this state and starts a new flat draft.
Cancel never changes the native map.

The start API has no cancellation token or returned run epoch.
Closure queues a general pause after an accepted start.
A bounded status check also pauses a late run.
A start without a new run reply shows an error after 20 seconds.
The panel keeps that request pending to prevent duplicate starts.
Any Pause also marks the pending start cancelled.
The panel pauses the new epoch when a cancelled start arrives.

No live ride, scenario, automated test, or hardware check forms part of this implementation.
