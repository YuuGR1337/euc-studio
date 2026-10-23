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
It shows the prompt, the firmware's boot countdown, the drive (or lean) and turn bars while
riding, the motor readout, and the respawn buttons. The motor readout shows two figures, each
labelled with its source: "PWM (firmware)", the firmware's own PWM (`latest.pwm`), and "Motor
headroom (sim)", 100 % less the simulated modulation (`latest.modulation`), the share of the
battery voltage the inverter really applies. The firmware's figure can read about 70 % while the
modulation is at its limit, so only the headroom warns: at 25 % or less it turns the caution
colour and reads "low". No speed or lean limit is drawn from it: the motor plant is still being
calibrated. The controller writes it through `hud.ts`; nothing else does.

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
| W / ↑, S / ↓        | Left stick, RT / LT, D-pad | Ride forward, brake (a throttle, or the lean)  |
| A / ←, D / →        | Left stick, D-pad          | Turn                                           |
| Space (hold)        | LB (hold)                  | Crouch; at speed also a tuck (less drag)       |
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
  tap or a long press. Held about 0.8 s at 0.5 m/s or less, power switches the wheel off (the
  stock firmware ignores it while riding): the motor goes free, the rider falls if riding, and the
  run goes on; the prompt reads `outputs.controller_powered`. Held again until the firmware
  latches (`outputs.power_latched`, about 0.55 s), it switches the wheel on. At a stop the second
  handle press cuts the drive and a third restores it; at speed the firmware ignores both.
- L, B and M (and the panel's Light, Pedal mode and Horn) send the phone's commands (`light`,
  `horn`, `pedal_mode`) through `engine.sendCommand`, as the Phone panel does. The firmware does
  not report the light or pedal mode back, so the panel shows what it sent last. A refused command
  shows in the prompt.

## Sound

Free Ride sounds like the ride (FR6): the native audio service that plays the firmware's buzzer
also plays the ride bed, on simulated time (`src-tauri/src/engine/ride_sound.rs`). Every sound
comes from a simulator signal, each sample: the motor from its electrical frequency (the plant's
pole pairs × wheel revolutions per second) and phase current, low as on a real wheel; the tyre
from the wheel speed and the world's material under the wheel (a band and grain per material,
joint ticks on pavers and setts, a hiss when wet, the hum of the tread's 15 knob repeats); the
wind from the speed. The ride ducks by 0.22 under each beep. Only a free ride sends the bed's
samples; other runs keep the buzzer alone.

The panel's Sound row sets the master volume (squared into the gain, kept in localStorage as
`euc-studio.soundVolume`) and mutes (`euc-studio.soundMuted`); `useSoundMix` sends them with
`engine_sound_mix {volume, bed}`. While the window is hidden the run goes on and the bed fades out
over 0.3 s; the buzzer still sounds. The Display card's bell switches the wheel's sound off
altogether. Nothing runs on the frame loop for sound.

## Drive and lean

Forward and back have two modes, chosen under Riding and kept in localStorage
(`euc-studio.freeRide.drive`). The choice shows only when the engine reports drive caps.

- Drive (the default): forward and back are a throttle, as in EUC Thrills. Each packet carries
  `desired_accel_mps2` and a zero lean; the engine's rider leans for the acceleration
  (`tan(lean) = (a + drag) / g`), tracks the speed the asks add up to, and ends a brake to a stop
  standing (a clean stop). Full throttle asks for `accel_cap_mps2`, full back for
  `-brake_cap_mps2`, and a released throttle for minus the drag, so the wheel coasts. The caps
  are `g·tan(22°) ∓ drag` at the speed now, so the ask is
  `throttle · (accel_cap + brake_cap) / 2 + (accel_cap - brake_cap) / 2`, held to ±12 m/s².
- Lean: forward and back set the rider's lean itself (up to 10° on the keys), as before drive
  mode. An engine without drive caps always rides this way.

## Input shaping

`lib/rideControls.ts` shapes the input; `docs/design/free-ride.md` explains where each value
comes from (EUC Thrills by VibezZzCoder, MIT).

- A key moves its lean or turn with a first-order lag (lean 0.15 s, turn 0.10 s) toward its
  level: keys lean at most 10°, a stick or trigger reaches the run's whole lean limit.
- Held keys build nothing until the run is ready, so the ramp starts from zero.
- The stick has a radial, rescaled dead zone (15 %); per axis the strongest of stick, trigger and
  D-pad wins. The device that moved last rides.
- A full turn asks for the engine's `turn_cap_rad_s` (its speed-scheduled cap).
- In drive mode the keys' throttle has the keys' lag; the stick's throttle follows with EUC
  Thrills' lean lag (0.14 s, at most 6 shares a second).
- Lean mode only: near a stop (under 0.6 m/s) a back lean stays at zero until back has been held
  for 0.35 s: the rider stands up and the wheel stops before it rolls backwards. This shapes
  intent only. In drive mode the engine's clean stop does this: a brake held after the stop
  keeps the wheel standing, and back pressed again from a stop rides backwards.
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

| Command              | Arguments                                                                                                          |
| -------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `free_ride_start`    | `{ spec: { wheel: 'begode-extreme', settings: 'blank', rider_model: 'articulated_sagittal_v1', world } }`          |
| `free_ride_restart`  | `{ epoch, toStart }` (`to_start` natively: false at the last clear spot, true at the world's start)                |
| `free_ride_step_off` | `{ epoch }`                                                                                                        |
| `free_ride_status`   | none                                                                                                               |
| `free_ride_world`    | `{ epoch }`                                                                                                        |
| `free_ride_map`      | `{ epoch }` (only for a run on the old test pad)                                                                   |
| `free_ride_input`    | `{ input: { run_epoch, seq, source, desired_lean_rad, heading_rate_rad_s, armed, crouch?, desired_accel_mps2? } }` |

`free_ride_arm` and `free_ride_pause` remain in the engine as no-ops for older frontends; this
frontend does not call them. `armed` in packets is ignored by the play-mode host and always sent
true.

- `free_ride_input` takes an optional `crouch`, 0 (standing) to 1 (a full crouch). The articulated
  rider bends toward it at 3 rad/s, and the rigid rider ignores it. The frontend sends it only
  when the status has a `crouch` field (the native host refuses unknown fields), and 0 when that
  field is null.
- `heading_rate_rad_s` may reach 2.5 rad/s, the rider's turn cap at a stop. The physics caps it
  further with speed. A packet above 2.5 rad/s is rejected.
- `desired_accel_mps2` (drive mode) is the asked acceleration, m/s², forward positive, net of
  drag, at most 12 either way; `desired_lean_rad` must then be 0. The frontend sends it only when
  the status has an `accel_cap_mps2` field.
- Status: `ready` (the firmware has booted; `armed` equals it), `turn_cap_rad_s`, `crouch` (0 to
  1, or `null` when the rider cannot crouch), `on_pedals`, `boot_remaining_s`, and in drive-mode
  engines `accel_cap_mps2`, `brake_cap_mps2` (m/s², positive, drag included) and `accel_mps2`
  (the achieved acceleration, smoothed). A step-off or a fall puts "Stepped off" or "Rider
  detached" on the timeline and never pauses the run.

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
