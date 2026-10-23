# Free ride frontend

Free Ride is a play mode: nothing to click before riding, a crash never pauses, and R respawns.

## Parent connection

Import `FreeRidePanel` from `./index.ts`.
Pass the current `EngineClient` as `engine`.
Pass `active={true}` while the Ride workspace shows Free Ride (the dock may be folded away: the
keys still ride). Use one panel for each native run.

`onGround(ground, epoch)` supplies the run's world frame (or an old run's test pad grid) and its
run epoch. The callback sends null when controls become inactive.
Retain the last valid ground in the viewport cache, and display it only while the live `runMode`
is `free_ride` and its `runEpoch` matches.

The parent owns ground rendering and the viewport route.
The optional `onExit` callback must call `engine.setScenario` with the chosen scenario.

Mount `FreeRideOverlay` in the 3D view (the Viewport does, next to the Ride manual HUD).
It shows the prompt, the firmware's boot countdown, the lean and turn bars while riding, and the
respawn buttons. The controller writes it through `hud.ts`; nothing else does.

## Playing

Opening Free Ride rides. Without a free ride run the controller starts one in the city (seed 42,
or the world started last in this session); the engine boots the firmware on the stand (about
1.5 s of simulated time, stepped unthrottled) and plays. Keys and the stick drive as soon as the
status is ready. There is no arm step: every input packet rides, and input older than the
engine's 250 ms watchdog is neutral, so the idle wheel balances.

- A crash never pauses. Off the pedals, the run goes on and the prompt says how to respawn.
- R respawns at the last clear spot the wheel passed (`free_ride_restart`, a new run: the firmware
  boots again, about 0.7 s of wall time). Shift+R respawns at the world's start. The HUD has a
  button for each.
- Escape (the gamepad's Start) pauses; Escape again, or any riding key or a moved stick, goes on.
- The window losing focus, a text field, an open menu or dialog, or the wheel's own buttons take
  the keys: the rider's controls go neutral, and the next key press rides again. No click.
- New world starts a run from a random seed. The firmware is chosen in the run bar; a change
  rebuilds and plays the ride.

Riding keys act wherever focus is, except in a text field, an open menu, list or dialog, the
command palette, the shortcut sheet, Recordings, and the wheel's own buttons.
Tabs, switches, sliders and selects keep their arrow keys and Space.
The controller listens in the capture phase; the global command listener uses the bubble phase.

| Key (default)       | Gamepad (standard mapping) | Action                                         |
| ------------------- | -------------------------- | ---------------------------------------------- |
| W / ↑, S / ↓        | Left stick, RT / LT, D-pad | Lean forward, lean back (brake)                |
| A / ←, D / →        | Left stick, D-pad          | Turn                                           |
| Space (hold)        | LB (hold)                  | Crouch                                         |
| X                   | B                          | Step off                                       |
| R, Shift+R          | X, Back                    | Respawn here, at the start                     |
| P (hold)            |                            | Power button: tap, or hold to switch off or on |
| H (hold)            |                            | Handle button                                  |
| L                   |                            | Light: off, on, strobe                         |
| B                   |                            | Horn                                           |
| M                   |                            | Pedal mode: soft, medium, hard                 |
| Esc                 | Start                      | Pause, go on                                   |
| C (studio shortcut) | Y                          | Camera                                         |

Keys are bound by `KeyboardEvent.code` under Keys and kept in localStorage
(`euc-studio.freeRide.bindings`). Escape, Tab, Enter, modifier keys and the studio's own
shortcuts (C, `.`, `[`, `]`, `?`) cannot be bound. Gamepad buttons are fixed.

## The wheel's pad

- P and H hold the board's power (PB15) and handle (PA12) buttons through `engine.setButton`, with
  the same leases as the Display card's buttons (`ride/wheelButtonInput.ts`): the firmware times a
  tap or a long press. A long press switches the wheel off: the motor goes free, the rider falls
  if riding, and the run goes on; the prompt reads `outputs.controller_powered`. A press switches
  it on again.
- L, B and M (and the panel's Light, Pedal mode and Horn) send the phone's commands (`light`,
  `horn`, `pedal_mode`) through `engine.sendCommand`, as the Phone panel does. The firmware does
  not report the light or pedal mode back, so the panel shows what it sent last. A refused command
  shows in the prompt.

## Input shaping

`lib/rideControls.ts` shapes the input; `docs/design/free-ride.md` explains where each value
comes from (EUC Thrills by VibezZzCoder, MIT).

- A key moves its lean or turn with a first-order lag (lean 0.15 s, turn 0.10 s) toward its
  level: keys lean at most 10°, a stick or trigger reaches the run's whole lean limit.
- Held keys build nothing until the run is ready, so the ramp starts from zero.
- The stick has a radial, rescaled dead zone (15 %); per axis the strongest of stick, trigger and
  D-pad wins. The device that moved last rides.
- A full turn asks for the engine's `turn_cap_rad_s` (its speed-scheduled cap).
- Near a stop (under 0.6 m/s) a back lean stays at zero until back has been held for 0.35 s:
  the rider stands up and the wheel stops before it rolls backwards. This shapes intent only.
- Crouch eases with 0.07 s and lands on an exact zero when released.

First-ride hints (turn, brake, then the keys once) are cleared by doing them and remembered in
localStorage (`euc-studio.freeRide.hints`).

## UI

The panel follows `app/DESIGN.md`. It holds only what play needs: the ride's state, Respawn and
At the start, the wheel's pad (light, pedal mode, horn; power and handle are keys here and buttons
in the Display card), the world (its seed and New world) and the keys. The test pad, its map
editor, the world's density sliders, the rider model choice, precise input sliders, ride details
and gamepad tuning are gone. Old recordings that used the test pad still open: playback draws
their map.

## Native API

The frontend uses these exact command arguments:

| Command              | Arguments                                                                                                 |
| -------------------- | --------------------------------------------------------------------------------------------------------- |
| `free_ride_start`    | `{ spec: { wheel: 'begode-extreme', settings: 'blank', rider_model: 'articulated_sagittal_v1', world } }` |
| `free_ride_restart`  | `{ epoch, toStart }` (`to_start` natively: false at the last clear spot, true at the world's start)       |
| `free_ride_step_off` | `{ epoch }`                                                                                               |
| `free_ride_status`   | none                                                                                                      |
| `free_ride_world`    | `{ epoch }`                                                                                               |
| `free_ride_map`      | `{ epoch }` (only for a run on the old test pad)                                                          |
| `free_ride_input`    | `{ input: { run_epoch, seq, source, desired_lean_rad, heading_rate_rad_s, armed, crouch? } }`             |

`free_ride_arm` and `free_ride_pause` remain in the engine as no-ops for older frontends; this
frontend does not call them. `armed` in packets is ignored by the play-mode host and always sent
true.

- `free_ride_input` takes an optional `crouch`, 0 (standing) to 1 (a full crouch). The articulated
  rider bends toward it at 3 rad/s, and the rigid rider ignores it. The frontend sends it only
  when the status has a `crouch` field (the native host refuses unknown fields), and 0 when that
  field is null.
- `heading_rate_rad_s` may reach 2.5 rad/s, the rider's turn cap at a stop. The physics caps it
  further with speed. A packet above 2.5 rad/s is rejected.
- Status: `ready` (the firmware has booted; `armed` equals it), `turn_cap_rad_s`, `crouch` (0 to
  1, or `null` when the rider cannot crouch), `on_pedals` and `boot_remaining_s`. A step-off or a
  fall puts "Stepped off" or "Rider detached" on the timeline and never pauses the run.

The source values are `keyboard_slider` and `gamepad`.
Input sequences use uint32 values; the sequence increases across source changes and drawer
closure, and a new native epoch resets it. Native epochs must increase during the desktop session.

A start or respawn reply confirms a queued command. The panel waits for a new free ride epoch,
then fetches that run's world frame. A late status or world reply cannot replace data after
closure. A start or respawn without a new run shows an error after 20 seconds.

Controls send a packet every 50 ms of wall time while the run is ready. One input call and one
pending value bound the frontend queue. A refused input shows its error until inputs get through
again; nothing pauses.

## Limits

Only standard gamepad mapping is supported, and the first connected standard gamepad rides.
Browser support and device permission can limit gamepad access.
Closing Free Ride (the Telemetry button, another workspace) stops the input; the run goes on with
its rider still.

The controlled-transport browser tests (`e2e/free-ride-controls.spec.ts`, Chromium and WebKit)
cover the panel, the HUD, the controller and the API validation against a fixture that answers
as the play-mode engine does. No live ride with the native engine is part of these tests.
