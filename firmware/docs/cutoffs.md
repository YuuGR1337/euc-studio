# Cut-offs and protections

Every place where the Begode Extreme firmware stops, limits or warns, as the stock firmware does it and as our
two profiles do it. Times are in RTC ticks of 2.016 ms (496 Hz) unless a unit is given. "Soft disable" means
the balance stops its law and ramps the motor command to zero by 2 per tick; "hard cut" means the bridge
outputs go off (TIM8 main output) until the next power-on. Sources: `core/balance/cutoff.c`,
`core/balance/tiltback.c`, `core/motor/`, `core/alarms/alarms.c`, `core/power/power.c`; the stock references
are in [quirks.md](quirks.md).

Variable names are the ones of `profiles/begode-extreme/stockmap.toml` (stock) and `probes.toml` (our
images), so a scenario check or `euc sim compare` reads the same quantity on every firmware:
`balance_disabled` (soft disable), `drive_fault` (hard cut), `hall_fault`, `alerts` (frame 04 byte 14),
`attitude_pitch_deg`, `drive_command`, `vq_active`, `pwm_alarm`, `tiltback_speed_deg`, `tiltback_battery_deg`,
`tiltback_pwm_deg`.

## Fall and attitude cut-offs (balance, every running tick)

| Detector | Input | Threshold | Time | Action | Restart | Depends on speed |
|---|---|---|---|---|---|---|
| Pitch | `attitude_pitch_deg`, or `tipped` | \|pitch\| > 40 deg | 52 ticks (105 ms) | `balance_disabled`, `fallen` | automatic: 101 ticks level (accelerometer tilt) and \|v\| <= 0.2 m/s, one 20-tick beep | no |
| Error | error angle (`attitude_pitch_deg` + tiltbacks + pedal trim - restart and handle offsets), or `tipped` | \|error\| > 40 deg | 11 ticks (22 ms). Clean: 52 ticks (105 ms) for the angle, 11 ticks with `tipped` (`ERROR_CUTOFF_22MS`) | `balance_disabled`, one 60-tick beep | none (only with the pitch detector) | no |
| Overload | `vq_active` | \|Vq\| > 31000 (command above 1033 of 1050) | 752 ticks (1.52 s) | `balance_disabled` | none | no; in practice only at high speed or a stalled wheel |
| Slow roll | roll | \|roll\| > 40 + 5 x `W X` deg | 21 ticks (42 ms) | `tipped` (feeds pitch and error) | with the pitch restart | no |
| Fast roll | roll | \|roll\| > 50 / 58 / 66 deg (`<` `=` `>`) and Vq > 30000 | 1 tick | `tipped` | with the pitch restart | through Vq only |
| Handle | handle button | two presses at a standstill | at once | soft disable; the third press restores | third press | yes: standstill only |

None of the angle thresholds or times depends on speed in either profile. Speed only enters the balance
through the gain schedule (Kp + 1 per m/s, Kd + 0.32 per cm/s), the tiltbacks and the restart condition.

In the stock firmware the error detector is the fastest one: 22 ms past 40 deg ends the ride, and it has no
automatic restart. That is shorter than a deep pothole dip that the balance is already recovering: on the 5 cm
pothole at 20 km/h (`pothole-5cm-20kmh`) the shell passes -40 deg for 0.06-0.09 s and comes back. The stock
firmware rides through only because its clipped gyro (+-500 deg/s) makes it believe the dip is shallower; the
clean firmware reads the true dip, so with the stock 22 ms it cut and the wheel fell. Clean therefore gives an
error angle the pitch detector's 105 ms (`ERROR_CUTOFF_22MS`): a fall still cuts, 83 ms later, and a roll
cut-off (`tipped`) keeps the stock 22 ms. See "Decisions and open questions" for the evidence.

## Speed and power limits (balance, no cut)

| Protection | Trigger | Response | Depends on speed |
|---|---|---|---|
| Speed tiltback | speed above `W Y` (km/h, 200 = off) | nose-up offset, 10 deg per m/s over, time constant 4 s; releases with 2 s | yes |
| PWM tiltback | PWM alarm active and `e` set; clean also whenever the voltage vector \|Vd, Vq\| passes 75 %, or the driving Vd (positive Vd, the voltage the winding's inductance takes) low-passed over 0.3 s passes 35 % (`PWM_TILTBACK_VQ_ONLY`) | nose-up offset at 2.7 deg/s, at most 12 deg | through PWM |
| Battery / temperature tiltback | voltage outside 99.20..137.60 V or board above about 80 degC | nose-up offset at 2.38 deg/s (1.98 backward); holds in the gap to the inner window, releases in 20 s. Stock: no limit (about 48 deg in the end). Clean: at most 8 deg, and the 10 km/h speed limit of limp mode takes over until the inner window (`BATTERY_TILTBACK_UNBOUNDED`) | no (clean: yes) |
| Clean: limp mode | one stuck hall line found (`HALL_SINGLE_LINE_UNDETECTED`) or a stalled main loop (`NO_WATCHDOG`) | speed tiltback at 10 km/h at most, growing at most 2.5 deg/s, at most 12 deg, no automatic restart after the next stop | yes |
| Clean: riderless wheel | a sudden load drop, then faster than 1 m/s, \|Iq\| (0.3 s low-pass) under 3.5 A for 0.6 s, speed dropping less than 0.15 m/s, no other limit, 3 s after start (`RIDERLESS_UNDETECTED`) | beeps and the limp speed limit; released by \|Iq\| above 7 A, below 1 m/s or a stop ([riderless-detection.md](riderless-detection.md)) | yes |
| Output slew | every tick | command changes at most 120 per tick, soft start +1 per tick up to 1050 | no |
| Voltage-mode current back-off | \|Iq\| > 280 A (drive mode 0) | command reduced each step, slew 60 per step | no |
| q-integral limit | every tick | the current loop's integral supplies at most 160 x \|v\| (0.1 m/s, up to 20 m/s) of voltage | yes |
| Stop brake | soft-disabled and \|v\| <= 0.4 m/s | Vq 0, all low sides on | yes |

## Motor and sensor faults (hard cut unless stated)

| Fault | Trigger | Time | Action | Clean change |
|---|---|---|---|---|
| Hall fault | hall state 0 or 7 | more than fPWM / 8 calls (62.5 ms) | `hall_fault`, hard cut, `alerts` bit 0x40, 2 beeps every 250 ticks | the timer reads the raw lines also while a lost line is rebuilt |
| Lost hall line | one line stuck | never detected (stock) | none: wrong angle in 2 sectors of 6, speed reads 1/3 | found within about 3 electrical turns; state rebuilt from 2 lines, alert 0x40, 10 km/h tiltback |
| Current sensor start check | any phase reading above 80 A in readings 1..999 after power-on (3 of them before the tick starts) | 1 reading | hard cut, stop the drive, alert 0x02 | same |
| Over-current | phase current above the board level | never (stock counts only) | none | off in both profiles until a bench level is set (`NO_OVERCURRENT_TRIP`; level is a board placeholder) |
| IMU at power-on | WHO_AM_I wrong | once | hard cut, alert 0x04 | same |
| Gyro clip | pitch rate above the gyro range | while it lasts | none: the attitude loses the excess (+-500 deg/s) | +-2000 deg/s and a faster re-anchor after a rail (`GYRO_500DPS_CLIP`) |
| Numeric | NaN or infinity in attitude, law, ramp-down or output | at once | stock passes it on | command 0, motor off until power-off (`NAN_FAIL_OPEN`) |
| Exception | any fault handler | at once | both profiles turn the bridge off first | - |
| Main loop stall | no main-loop pass while the tick runs | 0.5 s | stock: nothing | limp mode and stall beeps, no reset (`NO_WATCHDOG`) |
| Dead tick or FOC | no tick, no FOC step, or a broken command | 250 ms (167..333 ms) | stock: nothing | independent watchdog resets; the next boot keeps the bridge off (`NO_WATCHDOG`) |
| Stale command | no balance command for 3 ticks (about 6 ms) | 6 ms | stock: the FOC keeps the last command | command ramps to 0 at 4200 per second and latches (`STALE_COMMAND_HELD`) |
| Free fall | all axes below 0.3 g for 10 ticks | while it lasts | gravity reference held at 1 g; never cuts | same |

## Battery, temperature and BMS (alarms only, no cut)

| Alarm | Trigger | Response |
|---|---|---|
| Low voltage | below 105.60 V | alert 0x08; 1 / 2 / 3 beeps every 1000 ticks below 2 / 4 / above 4 m/s |
| High voltage | 137.61 V or more | alert 0x10, 3 beeps every 1000 ticks |
| Board temperature | NTC above about 80 degC | alert 0x20, 2 beeps every 1000 ticks (plus the temperature tiltback above) |
| PWM | \|Vq / 30\| > 10 x `W P` | alert 0x01, 35-tick beep every 100 ticks at full volume |
| Speed | mode 0 above 8.3 m/s, modes 0 and 1 above 12.5 m/s | 2 or 3 beeps every 500 ticks |
| BMS | 3 failed health checks in a row on a block | 500-tick beep every 5000 ticks; nothing in the drive reads the BMS |

## System cuts (hard cut)

| Cause | Condition | Clean change |
|---|---|---|
| Power-off button | held 401 ticks (0.81 s) at \|v\| <= 0.50 m/s | same |
| Auto power-off | no hall step for 7200 s | same |
| Transport lock | gesture at a standstill with the handle held, or the lock state at power-on | the light also goes off (`LOCK_LIGHT_STAYS_ON`) |
| Service sequences `!@` `cy` `+-` | BLE bytes, no authentication | only with no rider and a power-button press within 10 s (`SERVICE_WITHOUT_CONFIRM`) |
| Ride settings over BLE | any speed (stock) | below 0.5 m/s only (`SETTINGS_WHILE_MOVING`) |

## Decisions and open questions

The decision studies below use the earlier mechanical baseline named in each section.
Their measured times and outcomes do not describe every later model revision.
The [current validation](../../docs/models/clean-cutoff-next.md#current-validation-2026-10-21) uses the final 2026-10-21 rigid-rider baseline.
It preserves the cutoff and warning settings in this document.

### The error detector against a recovery swing (decided 2026-10-20)

The stock error detector cuts after 22 ms past 40 deg. On the rigid body (2026-10-16, before the suspension) a
hard pothole edge tripped it in clean on a +44 deg nose-up swing; with the rear suspension the 2 cm pothole dips
the shell only to about -10 deg and nothing trips. The 5 cm pothole (`pothole-5cm-20kmh`) is the edge case: the
shell passes -40 deg for 0.06-0.09 s and the balance brings it back. Stock and compat ride through because their
clipped gyro makes them believe the dip is shallower (stock's attitude reaches 34.5 deg while the shell is at
-42.7 deg); clean reads the true dip, and with the stock 22 ms its error detector cut at 19.88 s at quantum 3000
and 2000 (error count 11 of 11, pitch count 2 of 52) and the wheel fell backwards.

Two clean candidates from the list of 2026-10-16 were built as quirks and judged at quantum 4000, 3000 and 2000
(model of 2026-10-16 with the CPI 2.0 timing: linkage ratio 1.2, rigid rider):

| Clean build | 2 cm pothole | 3 cm pothole | 5 cm pothole |
|---|---|---|---|
| stock cut-offs (22 ms) | holds | holds | **borderline** (holds, falls, falls): the error detector cuts at 19.88 s |
| (c) the error angle gets the pitch detector's 105 ms | holds | holds | holds; error count at most 20-26 of 52, shell -41.8..-42.3 deg, 0.06-0.07 s past 40 deg |
| (a) the law's rate terms clamped to +-500 | falls | falls | falls |
| (c) and (a) at +-500 | falls | falls | falls |
| (c) and (a) at +-1000 | holds | holds | falls |

(a) is rejected. The rate terms (0.1 x Kd(v) x pitch rate and 160 x the change of the error) are the balance's
damping: clamped to +-500, the 2 cm dip starts a growing oscillation (shell -9.8, +12.7, -36.6, +42.6 deg within
0.4 s), the error detector cuts at 20.21 s and the wheel falls at 20.33 s. With the suspension in the model the
full reverse command after a dip is what stops the swing, not what causes it.

(c) is in the clean profile (`ERROR_CUTOFF_22MS`). An error angle alone now counts 52 ticks, as the pitch
detector does; a roll cut-off (`tipped`) keeps the stock 11 ticks, so a wheel on its side still stops within
64 ms (21 + 11 ticks). On the acceptance cases the cut times are unchanged: rider-steps-off-15kmh 17.76-17.79 s,
rider-steps-off-5kmh 12.64 s, hall-loss-while-riding 15.07 s (hall fault); hard-braking-30kmh and
overlean-top-speed-30pct hold as before. A fall past 40 deg in pitch still cuts, 83 ms later than in stock; the
pitch detector already took that long, and the error detector still has no automatic restart. The two last
options of the list (a lower command slew near a zero crossing, a speed-aware cut-off) are not needed.

### Telemetry PWM % and the true headroom

Clean's headroom tiltback (`PWM_TILTBACK_VQ_ONLY`) reads the voltage vector \|Vd, Vq\|, but the telemetry PWM %
(frame 07) still reports Vq before the circle limit, as the stock firmware does. At speed Vq reads about 66 %
while the vector is already at 78..86 %, so an app's PWM alarm underestimates the true headroom. Options, to be
decided after periph's sensitivity check on the motor values:

1. Keep the stock meaning (apps are calibrated to stock wheels).
2. Report \|V\| in the same field (honest headroom, but a different meaning under the same name).
3. Keep Vq and add \|V\| in a spare telemetry field (no app changes its meaning; new apps can show both).

### Headroom tiltback with a high-inductance motor (decided 2026-10-20)

The clean headroom tiltback must give about 2 s of tiltback before the drive saturates for any plausible motor;
the inductance is not measured (200 uH is an estimate). With the vector trigger alone (\|Vd, Vq\| above 75 %) it
started only 0.3-0.5 s before saturation at twice the inductance.

Why: at 2x the drive does not run out of voltage first. Near 41 km/h the balance command and the current loop
start a 10 Hz oscillation that grows within about 0.8 s (command 165..250, then 42..409; Id +-7 A, then +-30 A)
while the vector averages only about 60 %; its peaks then reach the circle limit and the current loop loses
control. Vd is about w x L x Iq, so it grows with the inductance (at 2x it is larger than Vq). Clean therefore
also starts the PWM tiltback when the driving part of Vd (positive Vd, motoring in either direction; braking
makes it negative), low-passed over 0.3 s, passes 35 % (`PWM_TILTBACK_VQ_ONLY`). 38 % gave 1.7-1.9 s at 2x for
the default rider, 35 % gives at least 2.45 s.

Overlean riders on the maintainer's settings (accelerating at 5 km/h per second, leaning up to 18 deg), at 0.5x,
1x and 2x the plant's inductance (scratch profile copies), model of 2026-10-16 with the CPI 2.0 timing: linkage ratio 1.2, rigid rider. Before: clean with the vector trigger
only; after: with the Vd trigger. Times at quantum 4000; "lead" is from the tiltback start to the first sample
with \|V\| at 98 % or more (saturation); the verdicts are over quantum 4000, 3000 and 2000.

Full battery (`overlean-top-speed`; its rider ignores tiltback):

| Inductance | Rider | Before: tiltback, first \|V\| >= 98 %, verdict | After: tiltback, first \|V\| >= 98 %, lead, verdict |
|---|---|---|---|
| 0.5x | ignores tiltback | 22.44 s, - (lead -), holds | 22.43 s, -, -, holds |
| 0.5x | default | 22.44 s, - (lead -), holds | 22.43 s, -, -, holds |
| 0.5x | strong easing | 22.44 s, - (lead -), holds | 22.43 s, -, -, holds |
| 1x | ignores tiltback | 20.97 s, 26.69 s (lead 5.72 s), holds | 19.65 s, 26.68 s, 7.03 s, **borderline** (falls, holds, falls) |
| 1x | default | 20.97 s, 22.18 s (lead 1.21 s), holds | 19.65 s, -, -, holds |
| 1x | strong easing | 20.97 s, 37.88 s (lead 16.91 s), holds | 19.65 s, -, -, holds |
| 2x | ignores tiltback | 17.66 s, 17.96 s (lead 0.30 s), falls | 15.46 s, 17.91 s, 2.45 s, falls |
| 2x | default | 17.66 s, 17.96 s (lead 0.30 s), falls | 15.46 s, 18.44 s, 2.98 s, holds |
| 2x | strong easing | 17.66 s, 17.96 s (lead 0.30 s), **borderline** (holds, falls, falls) | 15.46 s, 19.57 s, 4.11 s, holds |

30 % battery (`overlean-top-speed-30pct`):

| Inductance | Rider | Before: tiltback, first \|V\| >= 98 %, verdict | After: tiltback, first \|V\| >= 98 %, lead, verdict |
|---|---|---|---|
| 0.5x | ignores tiltback | 20.97 s, - (lead -), holds | 20.93 s, -, -, holds |
| 0.5x | default | 20.97 s, - (lead -), holds | 20.93 s, -, -, holds |
| 0.5x | strong easing | 20.97 s, - (lead -), holds | 20.93 s, -, -, holds |
| 1x | ignores tiltback | 19.54 s, 29.66 s (lead 10.12 s), holds | 18.72 s, 28.07 s, 9.35 s, holds |
| 1x | default | 19.54 s, 23.24 s (lead 3.70 s), holds | 18.72 s, -, -, holds |
| 1x | strong easing | 19.54 s, - (lead -), holds | 18.72 s, -, -, holds |
| 2x | ignores tiltback | 16.42 s, 16.90 s (lead 0.48 s), falls | 14.78 s, 17.25 s, 2.47 s, falls |
| 2x | default | 16.42 s, 16.90 s (lead 0.48 s), falls | 14.78 s, 18.67 s, 3.89 s, holds |
| 2x | strong easing | 16.42 s, 16.90 s (lead 0.48 s), holds | 14.78 s, 18.85 s, 4.07 s, holds |

With the PWM tiltback switched off (`overlean-no-pwm-tiltback`, default rider) clean holds at all three
inductances before and after; at 2x the tiltback now starts at 14.8 s instead of 16.8 s.

No new triggers elsewhere: on every other shipped scenario at 0.5x and 1x the low-passed driving Vd stays below
25 % (hill climb 22 %, 5 cm pothole 25 %, hard braking 14 %, the rest below 12 %), and the PWM tiltback starts
and peaks as before. At 2x one scenario gains a tiltback: `hill-climb-low-battery` (3 % battery, 73 A at
25 km/h; Vd 41 % at 18.3 s, the PWM tiltback reaches 12 deg); it still holds.

The cost at 1x: the tiltback starts about 1.3 s earlier (about 50 instead of 54 km/h on a full battery), and the
easing riders top out 3-4 km/h lower.

Still open:

1. The full-battery rider who ignores tiltback at 1x stays **borderline**. From 25.5 s it rides at 67-72 km/h
   with the vector at 85-97 % for more than 3 s and the PWM tiltback at its 12 deg limit; only the speed
   tiltback (69 km/h setting) still grows. Whether a disturbance saturates the vector there depends on a few
   cycles of timing: the same rider held on the build before (all three sizes) and fell at 4000 on HEAD. The
   warning itself starts 7-9 s earlier. A larger cap (18 deg) held at all three sizes but tipped the pedals
   25 deg nose up; not adopted.
2. At 2x the riders who ignore tiltback still fall, now with about 2.5 s of tiltback first. Fixing the loop
   oscillation itself (d/q decoupling in the clean current loop) is the next step if a measured motor needs it.
3. The measured inductance (bench, M8) decides how much of this matters.

### Acceptance: what must still cut

Any change to a cut-off or to the law's limits must keep every case below cutting (`balance_disabled` or
`drive_fault` set), in both profiles, checked by a scenario:

- the shell stays past the limit and does not come back (rider off, wheel down): rider-steps-off-15kmh;
- the angle keeps growing while the command is saturated (overlean past the motor's limit);
- any over-angle at walking speed;
- roll past the slow or fast roll limit (wheel on its side), still 22 ms after `tipped` in both profiles;
- the hall fault, the start-up current-sensor check and the system cuts above, unchanged.
