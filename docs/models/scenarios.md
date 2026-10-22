# Scenario library: what the stock firmware does

**Timing version.** The detailed measurements below use the former CPI 1.25 model, without exception overhead.
The current default is CPI 2.0 with exception costs. Clean's full-battery overlean result is now borderline.
See [current timing results](cpu-timing.md#scenario-validation) for the new scenario checks.
To reproduce the older timing, add `--cpi 1.25 --no-exception-timing` with the firmware builds listed below.

The situations EUC riders fear most, ridden in the simulator on the stock Begode Extreme firmware
(GW1825007) with the electrical plant ([electrical.md](electrical.md)) and the body physics
([body.md](body.md)). Each scenario in `scenarios/` records what happened with `record` checks, which
always pass, and gives its safety verdict with ordinary checks (`upright`, `no_cutoff`, ...), which
may fail. A failing verdict is a finding about the stock firmware, not a broken scenario. To write a
new one (more potholes, a different slope, a fault), see
[scenarios/writing-scenarios.md](../scenarios/writing-scenarios.md).

**The numbers depend on defaults.** Motor inductance and flux, battery resistance, the suspension's
stroke and linkage, the rider model and the aids are estimates until measured; the top-speed numbers
move with the motor's inductance, and anything with an impact moves with the body model (see below).
Read them as how the stock firmware behaves on this model, and compare runs with each other rather
than with a real wheel.

Run one with `cargo run --release -p euc-cli -- sim run --scenario scenarios/<name>.toml` (our own
core, the default; development builds can add `--cpu unicorn` as the reference). Results of 2026-10-16
with the measured 3.00-12 tire (rolling radius 0.2251 m, 28 pole pairs) and the rear suspension
(the measured 750 lb/in spring and 200 mm shock; stroke and linkage ratio estimates); "maintainer's"
settings are the `tarik-2026-10-15` preset (current-mode drive, hard pedals, PWM alarm 80 % with PWM
tiltback, speed tiltback 69 km/h). The rider in the table below is the rigid rider, welded to the
pedals; it can step off (`step_off_s`), steers to keep the line, feels the firmware's own tiltback
(the sum of its speed, battery and PWM offsets), and lets go of the pedals 0.1 s after a cut that
tips the wheel past 20°, or after any fall past 45°. Our builds in these runs: clean `4054b4d0`,
compat `00bb2466`.

**Rider model (2026-10-22).** The default rider is now articulated, its feet welded to the pedals
([body.md](body.md#the-rider)). Seven scenarios pin the rigid rider on purpose, because their
checks or the numbers below come from it: the three `full-effort-*` runs, the three `overlean-*`
runs and `low-battery-tiltback`. The other thirteen ride the articulated rider.
[Rigid and articulated riders](#rigid-and-articulated-riders-2026-10-22) compares the rigid rider,
the default and the opt-in contact-feet model on every scenario.

**Robustness.** Every scenario ran on stock, compat and clean at three emulator quantum sizes
(`--max-quantum` 4000, 3000 and 2000). Near a limit, a fall can depend on the emulator's timing; a
verdict that changes between the three sizes is marked **borderline**. Every shipped scenario keeps its
verdict at all three sizes on all three firmwares; the one borderline result is clean on a 5 cm
pothole (below).

| Scenario | Settings | Stock outcome | Key numbers |
|---|---|---|---|
| `overlean-top-speed` | maintainer's; the rider ignores tiltback and leans up to 18° | **Cutout: falls** at all three sizes. | Accelerating at 5 km/h per second: first PWM alarm (80 %) at 26.0 s; the PWM saturates 0.1 s later; the drive is disabled at 27.7 s at 73.3 km/h and the wheel falls at once (27.7-28.2 s over the three sizes); PWM tiltback 3.9° by then. Compat falls the same way; clean holds at all three sizes. |
| `overlean-top-speed-30pct` | same, battery at 30 % (115 V at rest), leaning up to 15° | **Cutout: falls** at all three sizes. | PWM alarm at 23.2 s; saturated at 25.8 s; drive disabled at 27.5 s at 67.5 km/h, and the wheel falls at once (27.5-28.2 s); bus 108 V; PWM tiltback 5.2°. Compat falls the same way; clean holds at all three sizes. |
| `overlean-no-pwm-tiltback` | same 30 % battery and climb, the default (easing) rider, the PWM tiltback turned off by `x` at 4 s | **Cutout: falls** at all three sizes. | No PWM tiltback at all: the alarm beeps at 23.4 s, the PWM saturates at 29.3 s, and the drive is disabled at 30.8 s at 70.4 km/h (29.5-30.8 s), after a nose dive past −30°. Compat the same. Clean tilts back from 20.0 s (at most 3.1°), the easing rider holds about 57 km/h, and the wheel stays up at all three sizes. |
| `hard-braking-30kmh` | maintainer's | **Holds.** No cutoff, no fall, no alert. | Braking from about 29 km/h: below 1 km/h after 4.8 s; braking torque peaks at −154 N·m with −179 A of Iq (the firmware's limit is 280 A); regeneration only −14 A at the battery (131.2 V on the bus); pedals tip 6.9° nose up. The rider's speed gain is 1.0: 2.0 made a 4 Hz rider-wheel limit cycle before the brake. |
| `hill-climb-low-battery` | maintainer's, battery at 3 % (101.8 V at rest); the rider ignores tiltback | **Holds, tilts back** on the climb; no cutoff. | 20 % grade from 40 m (reached at 17.0 s, 25 km/h): low-voltage alert already on from boot (1.7 s, below 105.6 V); the climb sags the bus below 99.2 V and the battery tiltback passes 2° at 20.9 s and keeps growing, to 26° at 36 s (pedals 31° nose up); bus down to 97.3 V, Iq up to 73 A. Clean caps the tiltback at 8° (pedals at most 22°). |
| `low-battery-tiltback` | neutral | **Warns and tilts back,** no cutoff: stop and go. | 98.5 V: alert 0x08 at 1.7 s. The tiltback grows as soon as the wheel rolls; the rider eases off from 2° of it and gets no faster than 6.6 km/h; the pedals pass 8° at 10.1 s and reach 13.5°. The rider stops, rolls back a little, and sets off again when the tiltback has gone. Clean: 8° at 12.3 s, at most 10.3°. |
| `hall-single-line-loss` | neutral | **No cutoff,** degraded drive. | Firmware speed reads a third of the truth (6.7 against 20.2 km/h): speed tiltback and alarms act at three times their setting. Torque ripple 19.0 N·m (3.8 healthy). |
| `hall-loss-while-riding` | neutral | **Cut, then falls.** | Hall fault 70 ms after the connector comes off; alert 0x40; nose dive past −30° within 0.12 s; the rider lets go at 15.19 s. |
| `pothole-20kmh` | maintainer's | **Holds** (PRELIMINARY: the rider has no knees; the suspension's stroke and linkage are estimates). | A 2 cm deep, 50 cm long pothole with steep edges: the far edge takes the wheel from 20 to 17.7 km/h; forward current peaks at 246 A; the suspension compresses 35.5 of its 66 mm; the shell dips to −8.4° and turns at most 250 °/s, inside the gyro's range; the firmware's attitude stays within 0.4° (RMS) of the truth. Deeper holes: below. |
| `ble-service-cut-standing` | maintainer's | **Cut, then falls.** | A phone sends "c", then "y" while the rider stands still: the motor is cut 10 ms after the "y" (drive fault and balance disabled); the rider lets go 0.9 s later and the wheel falls 1.0 s after the cut. No pairing, no confirmation. The clean profile answers `SERVICE:DENIED` and keeps balancing. |
| `rider-steps-off-15kmh`, `rider-steps-off-5kmh` | maintainer's | **Cut once the wheel goes down,** no runaway. | The riderless wheel is easy to balance, so the drive stays on until the wheel tips onto its side; the roll cut-off disables it 70 ms after the roll passes 40°: 2.75 s after the step-off at 15 km/h (at 7.8 km/h), 0.64 s at 5 km/h (at 4.0 km/h). Stock, compat and clean cut the same way; clean also suspects a riderless wheel 1.0 s after the 15 km/h step-off (beeps and its speed limit). |
| `downhill-cruise-2pct`, `coasting-stop` | maintainer's | **Hold** (guards for riderless detection). | 20 km/h down a 2 % slope: the motor gives 2.6-8.6 N·m (mean 4.1) with a rider on. A coast from 20 km/h to a stop at 0.4 m/s²: −12.5 to +10.8 N·m (mean −4.6). No cutoff, no fall on any firmware, and clean's riderless detection never triggers (fw's runs, all three sizes). |
| `balance-60s`, `ride-20kmh-stop` | neutral | Hold (CI regression runs). | |

## Rigid and articulated riders (2026-10-22)

Every shipped scenario ran with three rider models on stock, compat and clean at `--max-quantum`
4000, 3000 and 2000: the rigid rider (`rigid_ankle_v1`), the default articulated rider
(`articulated_sagittal_v1`, welded feet) and the opt-in articulated rider on contact feet
(`articulated_contact_v1`). The build is the integration checkpoint plus the rider change only (no
other in-flight work), with the shipped fixed timing (CPI 2.0 with exception costs); compat and clean
come from the `target-compat` and `target-clean` presets of the same checkpoint. `--rider-model` set
the model for every run, so pinned scenarios also show the articulated results. Each cell gives the
verdict at quanta 4000, 3000 and 2000 (P pass, F fail). None of the 540 runs halted.

| Scenario | Firmware | Rigid | Articulated (default) | Articulated, contact feet |
|---|---|---|---|---|
| balance-60s | stock | P P P | P P P | P P P |
| balance-60s | compat | P P P | P P P | P P P |
| balance-60s | clean | P P P | P P P | P P P |
| ble-service-cut-standing | stock | F F F | F F F | F F F |
| ble-service-cut-standing | compat | F F F | F F F | F F F |
| ble-service-cut-standing | clean | F F F | F F F | F F F |
| coasting-stop | stock | P P P | P P P | P P P |
| coasting-stop | compat | P P P | P P P | P P P |
| coasting-stop | clean | P P P | P P P | P P P |
| downhill-cruise-2pct | stock | P P P | P P P | P P P |
| downhill-cruise-2pct | compat | P P P | P P P | P P P |
| downhill-cruise-2pct | clean | P P P | P P P | P P P |
| full-effort-70kmh-stop-soc100 (pinned rigid) | stock | P P P | P P P | F F F |
| full-effort-70kmh-stop-soc100 (pinned rigid) | compat | P P P | P P P | F F F |
| full-effort-70kmh-stop-soc100 (pinned rigid) | clean | F F F | F F F | F F F |
| full-effort-70kmh-stop-soc20 (pinned rigid) | stock | F F F | F F F | F F F |
| full-effort-70kmh-stop-soc20 (pinned rigid) | compat | F F F | F F F | F F F |
| full-effort-70kmh-stop-soc20 (pinned rigid) | clean | F F F | F F F | F F F |
| full-effort-70kmh-stop-soc50 (pinned rigid) | stock | F F F | F F F | F F F |
| full-effort-70kmh-stop-soc50 (pinned rigid) | compat | F F F | F F F | F F F |
| full-effort-70kmh-stop-soc50 (pinned rigid) | clean | F F F | F F F | F F F |
| hall-loss-while-riding | stock | P P P | P P P | P P P |
| hall-loss-while-riding | compat | P P P | P P P | P P P |
| hall-loss-while-riding | clean | P P P | P P P | P P P |
| hall-single-line-loss | stock | P P P | P P P | P P P |
| hall-single-line-loss | compat | P P P | P P P | P P P |
| hall-single-line-loss | clean | F F F | F F F | F F F |
| hard-braking-30kmh | stock | P P P | P P P | F F F |
| hard-braking-30kmh | compat | P P P | P P P | F F F |
| hard-braking-30kmh | clean | P P P | P P P | F F F |
| hill-climb-low-battery | stock | P P P | P P P | F F F |
| hill-climb-low-battery | compat | P P P | P P P | F F F |
| hill-climb-low-battery | clean | P P P | P P P | F F F |
| low-battery-tiltback (pinned rigid) | stock | P P P | F F F | P P P |
| low-battery-tiltback (pinned rigid) | compat | P P P | F F F | P P P |
| low-battery-tiltback (pinned rigid) | clean | F F F | F F F | F F F |
| overlean-no-pwm-tiltback (pinned rigid) | stock | P P P | F F F | F F F |
| overlean-no-pwm-tiltback (pinned rigid) | compat | P P P | F F F | F F F |
| overlean-no-pwm-tiltback (pinned rigid) | clean | F F F | F F F | F F F |
| overlean-top-speed (pinned rigid) | stock | F F F | P P P | F F F |
| overlean-top-speed (pinned rigid) | compat | F F F | P P P | F F F |
| overlean-top-speed (pinned rigid) | clean | F F P | P P P | F F F |
| overlean-top-speed-30pct (pinned rigid) | stock | F F F | P P P | P P P |
| overlean-top-speed-30pct (pinned rigid) | compat | F F F | P P P | P P P |
| overlean-top-speed-30pct (pinned rigid) | clean | F F F | P P P | P P P |
| pothole-20kmh | stock | P P P | P P P | P P P |
| pothole-20kmh | compat | P P P | P P P | P P P |
| pothole-20kmh | clean | P P P | P P P | P P P |
| pothole-5cm-20kmh | stock | P P P | P P P | P P P |
| pothole-5cm-20kmh | compat | P P P | P P P | P P P |
| pothole-5cm-20kmh | clean | P P P | P P P | P P P |
| ride-20kmh-stop | stock | P P P | P P P | P P P |
| ride-20kmh-stop | compat | P P P | P P P | P P P |
| ride-20kmh-stop | clean | P P P | P P P | P P P |
| rider-steps-off-15kmh | stock | P P P | P P P | P P P |
| rider-steps-off-15kmh | compat | P P P | P P P | P P P |
| rider-steps-off-15kmh | clean | P P P | P P P | P P P |
| rider-steps-off-5kmh | stock | P P P | P P P | P P P |
| rider-steps-off-5kmh | compat | P P P | P P P | P P P |
| rider-steps-off-5kmh | clean | P P P | P P P | P P P |

**Articulation (rigid against the default).** All thirteen scenarios on the default rider keep every
verdict of the rigid rider at all three quanta on all three firmwares. Seven scenarios pin the rigid
rider, because their checks or documented numbers describe it. Their articulated outcomes are known
limitations of the model, not firmware findings:

- **`low-battery-tiltback`.** The rigid rider rolls back a little after the stand release, which
  unwinds the stock battery tiltback (it ramps with the sign of the speed), and then sets off. The
  articulated rider stands still, the tiltback stays at about 6-7° nose up, and the rider, waiting
  for it to fall within 2°, never sets off (top 0.6 km/h). The pedals reach 7.37° on stock, 7.11°
  on compat and 6.9-7.2° on clean by 15 s, short of the 8° check.
- **`overlean-top-speed`, `overlean-top-speed-30pct`.** The articulated rider, leaning the same 18°,
  drives the wheel with a smoother torque at the edge of the voltage headroom. On stock its PWM alarm
  comes at 26.4 s (rigid 24.3 s) at full charge and 23.7 s (rigid 23.6 s) at 30 %, its drive torque
  keeps the PWM under 100 %, and the tiltbacks hold the wheel near 75 km/h: no cutout on any
  firmware. The rigid rider's drive torque swings by about ±10 N·m near the limit; one swing takes
  the PWM over 80 % and then to 100 % within 0.4 s. The outcome at this edge already depended on the
  emulator's timing.
- **`overlean-no-pwm-tiltback`.** The articulated rider reaches the 69 km/h speed tiltback before the
  voltage runs out and eases off at most 71 km/h; stock and compat do not cut.
- **`full-effort-70kmh-stop-soc*`.** These measure firmware headroom with the rider of
  [full-effort acceptance](full-effort-acceptance.md). The articulated rider completes the full-battery
  run on stock and compat as the rigid rider does; at 20 % it falls in the 20° build phase at about
  9.8 s, and at 50 % it falls there at about 10.3 s or never reaches 70 km/h.

**Foot contact (the default against contact feet).** On contact feet the soles carry an ankle moment
only while the centre of pressure stays under them, and the lean controller does not keep within
that yet. The rider comes off the pedals while the wheel stays up, and `rider_on_pedals` fails, in
four scenarios at every firmware and quantum: `hard-braking-30kmh` (17.7-17.8 s, in the hard brake),
`hill-climb-low-battery` (33.4-34.0 s, stalled on the 20 % grade), `overlean-top-speed` (34.8-39.0 s,
held near 75 km/h) and every full-effort launch (8.7-15.2 s, at 30° lean). In the other sixteen
scenarios the contact rider keeps the welded rider's verdicts, except `low-battery-tiltback`, where
it rolls enough to unwind the tiltback and passes on stock and compat like the rigid rider. That is
why contact feet stay opt-in.

The `rider_on_pedals` check (every scenario without a step-off) passes by construction for welded
feet and the rigid rider, so it changes no verdict of the default or the baseline, and no
compat-vs-stock verdict.

The compat-vs-stock comparison (`.github/workflows/compare.yml`, run as there) passes on all twenty
scenarios. Three needed wider speed limits, each written in the tolerance file's notes: the
articulated rider sways for a few seconds after a push or a stop (about 0.4 Hz, pitch about ±0.4°),
where the rigid rider settles, so a few cycles of timing part the two firmwares' runs more:

| Scenario | Largest difference, articulated | Rigid | New limit |
|---|---|---|---|
| `balance-60s` | speed 0.585 km/h at 42.96 s | 0.056 km/h | 0.9 km/h |
| `hard-braking-30kmh` | speed 0.545 km/h at 21.41 s | 0.260 km/h | 0.85 km/h |
| `hill-climb-low-battery` | speed 0.521 km/h, pitch 6.65° at 34.5 s | 0.283 km/h, 2.12° | 0.8 km/h, 10° |

## What the overlean runs show

The overlean runs use a rider who never eases off for tiltback (`tiltback_feel_deg = 90`): the default
rider eases off and stays up, which is what the PWM tiltback is for; the danger is the rider who leans
through it. How hard that rider leans decides whether the motor runs out of voltage, so the library
sweeps it (`lean_max_deg`, quantum 3000; at 18° all three sizes):

| Lean at most | Full battery: stock / compat / clean | 30 % battery: stock / compat / clean |
|---|---|---|
| 15° | holds / holds / holds | **falls / falls / holds** |
| 16° | **falls / falls / holds** | falls / falls / falls |
| 17° | **falls / falls / holds** | falls / falls / falls |
| 18° (`overlean-top-speed`) | **falls / falls / holds** (all three sizes) | falls / falls / falls (all three sizes) |
| 19° | falls / falls / falls | falls / falls / falls |
| 20° | falls / falls / falls | falls / falls / falls |

Top speeds lie between 65 and 82 km/h. So clean holds where stock falls in a narrow band: on a full
battery for a lean of 16-18°, on a 30 % battery only at 15°. Beyond that both fall.

On stock the sequence is the same each time: the PWM alarm, then the q-current loop saturates, the
current stops following its reference, the balance command runs to its ±1050 limit, and the drive is
disabled. On a 30 % battery the alarm comes 4.3 s before the cut (23.2 and 27.5 s); on a full battery,
with the harder-leaning rider, only 1.7 s before it. The PWM tiltback has reached only 4-5° by the
fall: it warns, but at this acceleration it cannot stop a rider who keeps leaning.

Why clean holds in that band (one-quirk-off builds, 2026-10-16: only `PWM_TILTBACK_VQ_ONLY` makes
clean fall; the battery cap and riderless detection do not matter here): clean starts its tiltback when
the voltage vector |Vd, Vq| passes 75 %, at about 20 s and 51 km/h on a 30 % battery, while the
displayed Vq is only 64 %. It reaches its 12° limit about 5 s later. The nose-up target makes the
balance law ask for less forward torque even though the rider keeps the full lean, so the acceleration
falls from about 3.9 to 0.8 km/h per second and the vector never saturates. With the stock rule (Vq
only) the tiltback starts about 5 s later and has 1-2° when the vector saturates. Two caveats: the
rider here ignores the tiltback, so the save comes from the balance law, not from the rider; and the
motor values are ESTIMATES: at twice the estimated inductance clean's tiltback starts only after the
vector saturates, so the save is not guaranteed for every motor.

Pedal height (an ESTIMATE from the scan, ±15 mm) moved the cutout along this ramp but hardly its speed
(measured before the suspension and the rider weld). Before the measured tire and pedals, the
full-battery run fell only 0.4 s after the first PWM alarm, at 63.6 km/h. So the warning time depends
on the motor values; it needs measured motor constants before it means anything for a real wheel.

The motor's inductance (an ESTIMATE, 200 µH) matters most, because the PWM value the firmware shows
and alarms on is Vq alone, and the d voltage it leaves out grows with the inductance
([findings](../findings/stock-firmware.md), finding 10). On our compat build, which follows stock here:
with 100 µH the vector never reached the limit; with 400 µH the first PWM alarm came at 45 km/h, 0.1 s
before saturation, and the drive was disabled at 49 km/h. The real Extreme is ridden well above
49 km/h, so that end of the range is unlikely; ride logs from a real wheel would bound it.

## The gyro clips, and the attitude estimate stays wrong

The stock firmware sets the MPU-6500's gyro to ±500 °/s (65.5 LSB per °/s). The IMU model, like the
real part, saturates its 16-bit output at the range limit, so in an impact that turns the shell faster
the firmware integrates 500 °/s and its Mahony filter loses the excess; the accelerometer correction
(hard pedals: Mahony gain 0.2) removes the error by about 5° per second.

With the rear suspension, the 2 cm pothole turns the shell at most about 250 °/s (from the true pitch
at 100 Hz, so short peaks are higher), inside the range, and the attitude stays within 0.4° (RMS). A
3 cm pothole reaches about 550 °/s and leaves the stock firmware 1.6° off (RMS over 20.8-22 s; clean
0.7°); a 5 cm pothole reaches about 700 °/s, and the stock firmware believes +1.0° while the shell is
at −4.0° (RMS error 5.0°; clean 0.5°). Stock and compat ride through all three.

The clean profile runs the gyro at ±2000 °/s and re-anchors faster after a saturated sample
(`GYRO_500DPS_CLIP`). In the 5 cm pothole it sees the real dip, which reaches about −40°; its 40° pitch
cut-off trips at 19.89 s at quantum 3000 and 2000 and the wheel falls, while at 4000 the dip stops at
−40.4° and it rides through: **borderline**. Stock rides through on its wrong attitude. How the
cut-offs should treat a deep, recovering dip is an open item in
[cutoffs.md](../../firmware/docs/cutoffs.md).

## Preliminary: impacts

The body has the rear suspension, but the rider has no knees, the suspension's stroke and linkage are
estimates, and the tire is a soft contact rather than a carcass. Impact results stay PRELIMINARY.
Depths at 20 km/h with steep edges, on stock: 2 cm dips the shell to −8°, 3 cm to −27°, 5 cm to −42°,
with the suspension compressed 35.5, 42.5 and 43.1 mm of its 66 mm; all three ride through, without a
cut. On the rigid body, before the suspension, 2 cm dipped past 45° and 3 and 5 cm fell.

## Open questions

- The hard brake on factory settings (voltage mode, medium pedals, speed tiltback at 20 km/h) is not
  in the library: with the same rider the wheel overshoots to 40 km/h before the brake, then hits the
  current limit and falls backwards. How the 20 km/h tiltback interacts with the rider model is not
  understood yet.
- The hill climb stops at 36 s on purpose. Run on, the battery tiltback reaches 40° by 39 s, the
  rider who never gives up on 25 km/h stalls the wheel on the slope, the bus dips to 89 V and the
  firmware disables the drive at about 41 s; a person would have stepped off long before.
- Before the rider could steer, the default rider, who eases off for tiltback, fell on the hill climb
  at 23.7 s (drive disabled at 24.3 s), after the firmware swung Iq from +25 to −227 A in 0.3 s. That
  was a model effect, not a stock firmware finding: the rigid rider could not steer, and on the 20 %
  grade a wheel that slows turns across the slope. The shell's heading passes 1.4° at 23.0 s, 6.1° at 23.2 s and 27° at 23.4 s; the
  roll follows (2.9° at 23.5 s); Iq turns negative only at 23.37 s. The same happens without any
  tiltback (full battery, a rider who slows from 25 to 12 km/h on the grade: falls at 23.8 s), but not on
  flat ground (the same slowdown, heading and roll stay at 0.0°, −22 A at most) or in a steady 10 km/h
  climb. More tire grip (friction 1.2) does not help; the roll spring's gains decide only how the turned
  wheel ends. The rider now steers (`steer_kp` 2000 N·m/rad, `steer_kd` 200 N·m·s/rad, at most
  `steer_max_nm` 60 N·m, released when the rider steps off), and the default rider stays up on the
  stock firmware: it eases off from 20 s, stops at 26.2 s with the pedals 12.7° nose up and rolls back
  at about 3 km/h (the rider has no feel for the grade yet), Iq positive throughout (25 to 57 A). The
  shipped scenario keeps the rider who ignores tiltback, because it shows how far the stock tiltback
  grows.

## Terrain

Scenarios can ride on terrain (`crates/euc-physics/src/terrain.rs`): a `[terrain]` table with a
constant grade (`slope_pct`, from `slope_start_m`, built up over `slope_ramp_m`, 10 m by default, so
the wheel meets no kink) and `[[terrain.feature]]` bumps (a smooth hump) and potholes (steep walls),
each with `at_m`, `length_m` and `height_m` or `depth_m`. Flat ground is the infinite floor plane;
anything else is a heightfield strip sampled every 2 cm along the path, from 5 m behind the start to
`length_m`.

## Not modeled yet

Rider reactions: the rider model follows a target speed by leaning, eases off for the firmware's
tiltback and steers to keep the line, but it does not react to beeps, has no feel for a grade (on a
slope it stops and then rolls back slowly), and with its aids it can hold on longer than a person
during a nose-down struggle. The default rider bends at the ankles, knees and hips, but only forward
and back: it cannot shift or balance sideways without the roll aid. Its feet are welded to the pedals
by default. With the opt-in contact feet they can lift, rock and slip, but the controller does not yet
keep the soles' centre of pressure under the feet, and the rider cannot recover with the hips or by
stepping.
