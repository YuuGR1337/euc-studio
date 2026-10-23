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
An older engine's restart uses the parent's `engine.restart` path.

Mount `FreeRideOverlay` in the 3D view (the Viewport does, next to the Ride manual HUD).
It shows the prompt, the restart countdown, and the lean and turn bars while riding.
The controller writes it through `hud.ts`; nothing else does.

## Riding

The first push of a riding key, the stick or a trigger arms the controls.
The Ride button arms too, with the controls still.
Escape pauses from anywhere while the panel is active, and cancels a restart countdown.
Riding keys act wherever focus is, except in a text field, an open menu, list or dialog, the
command palette, the shortcut sheet, Recordings, and the wheel's own buttons.
Tabs, switches, sliders and selects keep their arrow keys and Space.
The controller listens in the capture phase; the global command listener uses the bubble phase.

| Key (default)       | Gamepad (standard mapping) | Action                                 |
| ------------------- | -------------------------- | -------------------------------------- |
| W / ↑, S / ↓        | Left stick, RT / LT, D-pad | Lean forward, lean back (brake)        |
| A / ←, D / →        | Left stick, D-pad          | Turn                                   |
| Space (hold)        | LB (hold)                  | Crouch                                 |
| X                   | B                          | Step off (the run ends; R rides again) |
| R                   | X                          | Restart here after a 3 s countdown     |
| Esc                 | Start                      | Pause                                  |
| C (studio shortcut) | Y                          | Camera                                 |

Keys are bound by `KeyboardEvent.code` in Input settings and kept in localStorage
(`euc-studio.freeRide.bindings`). Escape, Tab, Enter, modifier keys and the studio's own
shortcuts (C, `.`, `[`, `]`, `?`) cannot be bound. Gamepad buttons are fixed.

`lib/rideControls.ts` shapes the input; `docs/design/free-ride.md` explains where each value
comes from (EUC Thrills by VibezZzCoder, MIT).

- A key moves its lean or turn with a first-order lag (lean 0.15 s, turn 0.10 s) toward its
  level: keys lean at most 10°, a stick or trigger reaches the run's whole lean limit.
- Held keys build nothing while the controls are not riding, so the ramp starts after the
  neutral first armed packet.
- The stick has a radial, rescaled dead zone (15 % by default) and an optional response
  exponent; per axis the strongest of stick, trigger and D-pad wins.
- A full turn asks for the engine's `turn_cap_rad_s` (its speed-scheduled cap). An older engine
  gets the schedule here: 3.6 rad/s at rest to 0.85 rad/s by 9 m/s, held under a 20° turn lean
  and 30°/s.
- Near a stop (under 0.6 m/s) a back lean stays at zero until back has been held for 0.35 s:
  the rider stands up and the wheel stops before it rolls backwards. This shapes intent only.
- Crouch eases with 0.07 s and lands on an exact zero when released.

The device that pushes while the controls are paused becomes the source.
Disarmed packets are neutral, so the native host takes the new source from them; its 250 ms
neutral dwell then starts again, and the push arms once the dwell is met.
After a pause, held keys must all be let go, and the stick must come back to centre, before
they ride again.

R starts a 3 s countdown that R or Escape cancels.
At zero an engine with `free_ride_restart` starts a new run where the wheel is (the test pad:
its origin); an older engine restarts the run on its ground at its start point.
Either way the firmware boots again on the stand, and the prompt counts the boot down.

First-ride hints (turn, brake, then the keys once) are cleared by doing them and remembered in
localStorage (`euc-studio.freeRide.hints`). Gamepad settings are kept in
`euc-studio.freeRide.pad`.

## UI

The panel follows `app/DESIGN.md` and the gallery Controls and Panel examples.
Controls and World use separate tabs in one dock.
Sliders, selects, buttons, switches, pills, and tabs use the shared UI components.
Input settings, precise input, ride details, and exact map values stay closed until selected.
The map editor shows one feature at a time.
Only exact numeric fields use a local input, with the existing Tune field style.

Shared Radix controls keep their keyboard events.
Precise input sliders send their exact value while held, without the keys' shaping; they return
to zero on release and keep their pause guards.
Changing tabs pauses controls.
Returning to Controls never arms or starts the ride.
Wheel buttons use `data-wheel-control`; the existing `data-wheel-controls` container also works.
Their focus keeps the run active and sends neutral manual input.
Manual motion requires neutral controls for 250 ms after focus returns.
Space and Enter remain available for measured wheel button holds.
Escape pauses free ride and then passes through to Radix dialogs and menus.

## Native API review

The frontend uses these exact command arguments:

| Command              | Arguments                                                                                               |
| -------------------- | ------------------------------------------------------------------------------------------------------- |
| `free_ride_start`    | `{ spec: { wheel: 'begode-extreme', settings: 'blank', rider_model: 'articulated_sagittal_v1', map } }` |
| `free_ride_restart`  | `{ epoch }` (engines whose status has `on_pedals`)                                                      |
| `free_ride_step_off` | `{ epoch }` (engines whose status has `on_pedals`)                                                      |
| `free_ride_status`   | none                                                                                                    |
| `free_ride_map`      | `{ epoch }`                                                                                             |
| `free_ride_arm`      | `{ epoch, source }`                                                                                     |
| `free_ride_input`    | `{ input: { run_epoch, seq, source, desired_lean_rad, heading_rate_rad_s, armed, crouch? } }`           |
| `free_ride_pause`    | `{ epoch }`                                                                                             |

Native additions for FR2b (2026-10-22), used by the frontend since FR2:

- `free_ride_input` takes an optional `crouch`, 0 (standing) to 1 (a full crouch). The articulated
  rider bends toward it at 3 rad/s, and the rigid rider ignores it. A crouch is motion, so arming
  needs it at 0. The frontend sends it only when the status has a `crouch` field (the native host
  refuses unknown fields), and 0 when that field is null.
- `heading_rate_rad_s` may reach 2.5 rad/s, the rider's turn cap at a stop. The physics caps it
  further with speed. A packet above 2.5 rad/s is rejected.
- `free_ride_restart` with `{ epoch }` is the quick reset. It starts a new run (a new epoch) of the
  same free ride at the wheel's spot and heading on a world; the test pad restarts at its origin.
  The firmware boots again on the stand. The spot must be clear: no solid within 1.5 m and level
  ground. Otherwise the run restarts at the last clear spot the wheel passed, or at its own start.
- `free_ride_step_off` with `{ epoch }` makes the armed rider step off the pedals. The run then
  ends with the reason "Stepped off. Reset to start another free ride".
- Status adds `turn_cap_rad_s` (the turn cap at the current speed), `crouch` (0 to 1, or `null` when
  the rider cannot crouch), `on_pedals` and `boot_remaining_s` (the boot countdown).

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
Arm requires 250 ms of acknowledged neutral packets on the current source.
Native status must confirm the selected source before Arm.
Arm waits for the previous input call to finish.
The frontend stops disarmed packets while Arm is pending.
Native Arm starts play; the frontend never starts play after a status update.
The Arm IPC reply only confirms a queued command.
The panel checks native status every 25 ms for up to 200 ms after that reply.
It shows Riding only when epoch, source, ready, and armed match the request.
The existing 250 ms Arm timeout still pauses controls.
No disarmed heartbeat runs during this confirmation wait.
The first confirmed armed packet is neutral.
Rejection and timeout keep the panel disarmed and show an error.

Armed controls send packets every 50 ms of wall time.
One input call and one pending value bound the frontend queue.
The native 250 ms watchdog remains the final input guard.
Blur, hidden state, text focus, an open menu or dialog, closure, and source changes clear input
and request pause.
A late Arm reply triggers another pause.
A disconnected gamepad pauses a gamepad ride; the next push of a connected stick or a key rides
again.

The map specification matches `crates/euc-physics/src/map.rs`.
The editor checks dimensions, base grade, feature limits, and spawn clearance.
The native generator checks final heights and triangle grades.
The editor preview shows supports only.
It does not calculate a second height surface.

## Limits

Only standard gamepad mapping is supported.
Gamepad buttons other than the sticks, triggers and D-pad do not arm controls.
Browser support and device permission can limit gamepad access.
One device rides at a time: the native host takes one source per arm.
An engine without `on_pedals` in its status has no crouch, quick reset or step off; Space and X
say so, and R restarts the run at its start point.
A rider off the pedals does not arm again; R starts another ride.

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

The controlled-transport browser tests (`e2e/free-ride-controls.spec.ts`, Chromium and WebKit)
cover the panel, the controller and the API validation. No live ride with the native engine is
part of these tests.
