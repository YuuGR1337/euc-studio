# Body physics

The body model is everything mechanical: the tire on the ground, the wheel spinning in the shell,
the rear suspension between them, the shell with its pedals and IMU, and the rider standing on the
pedals. It runs in MuJoCo 3.9
(through `mujoco-rs` 5.0) in `crates/euc-physics`, and couples to the electrical plant
([electrical.md](electrical.md)) and the emulated controller in `crates/euc-sim`. The wheel's values
live under `[body]` in `profiles/<wheel>/plant.toml`; the rider and the aids come from the scenario,
or their defaults.

**Defaults are not measurements.** Most values below are estimates (marked DEFAULT in `plant.toml`).
They make a plausible Begode Extreme with an 80 kg rider, and they let the stock firmware balance and
ride, but the real wheel differs. Replace each one when it is measured, and record the measurement here.

## How it runs

```
             motor torque (mean over the step)                  wheel angle, speed
 electrical plant ─────────────────────────▶ axle hinge ──────────────────────────▶ electrical plant
   (5 µs grid)                                 │ reaction on the shell                (halls, back-EMF)
                                               ▼
                      MuJoCo, 0.5 ms: tire, wheel, shell, rider ──▶ IMU site ──▶ MPU-6500 model
                              ▲ ankle torque          ▲ roll spring, air drag
                         rider controller        aids (M2)
```

- **One clock.** Everything counts the controller's HCLK cycles (72 MHz). A physics step is 36 000
  cycles (0.5 ms); the electrical plant steps every 360 cycles (5 µs), lazily. The `plant` board
  component schedules each physics step as an event on the cycle grid, so a run is deterministic and
  repeats exactly.
- **Each step:** advance the electrical plant to the step's cycle, take its mean wheel torque since
  the last step, apply it to the axle hinge (MuJoCo applies the reaction to the shell), step MuJoCo,
  hand the wheel's new angle and speed back to the plant (which extrapolates the angle at that speed
  until the next step), and sample the IMU.
- **Hall edges** get their own events at the cycle the plant predicts (see electrical.md), so the
  firmware sees each edge on time, not on the 0.5 ms grid.
- The step uses MuJoCo's two-phase step: positions and velocities first, then the rider controller
  and the aids act on that state, then accelerations and integration (`implicitfast`). The IMU and
  the body poses describe the state at the start of the last step: 0.5 ms old, against a firmware
  that reads the IMU every 2 ms.

## Frames and signs

| Quantity | Convention |
|---|---|
| World, shell | x forward, y left, z up, metres |
| Wheel angle and speed | rotation of the wheel about the shell's +y; positive rolls forward |
| Motor torque | on the wheel about +y, positive forward; the shell takes the opposite |
| Rider lean | angle of the rider from vertical, positive forward |
| Telemetry pitch | shell pitch, **nose up positive** (the firmware's own pitch is nose down positive) |
| Telemetry roll | right side down positive |
| IMU | the shell frame at the IMU site: x forward, y left, z up; specific force reads +1 g on z at rest |

The IMU mapping follows the balance findings: the firmware's pitch is the rotation about the IMU's
+y, positive nose down, with x forward. A nose-down shell therefore reads `ax = −g·sin(pitch)` and a
positive y rate. The roll sign of the real mounting is not known (UNKNOWN in the findings); it does not
matter while the roll spring keeps the wheel upright.

## The model

```
world ── floor (plane, or the scenario's terrain)
  └─ shell (free joint)     scanned solids (packs, seat, handle, roller, shock, pedals), IMU site;
       │                    origin at the axle at full extension
       ├─ carriage (slide "suspension")   stator, axle clamps, slider blocks; spring, damper, end stops
       │    └─ wheel (hinge "axle", y)    tire: a torus, as a ring of 48 capsules
       └─ feet (hinge "lean", y)    rigid rider only: hinged at the ankles, ankle motor "ankle"
  └─ rider (free joint)     articulated (default): upper body
       ├─ left_thigh (hinge "left_hip") ── left_shank ("left_knee") ── left_foot ("left_ankle")
       └─ right_thigh ...                  each foot welded to the shell ("on_<side>_pedal"), or for
                                           articulated_contact_v1 a sole-on-pedal pair "<side>_sole_on_pedal"
     rigid: one body, welded to the feet ("on_pedals") until it steps off
```

Without `[body.suspension]` the carriage is left out and the wheel hinges straight in the shell.

`Physics::mjcf()` returns the generated MJCF for inspection.

- **Tire.** A torus around the axle with the scanned crown radius (tube 0.058 m, from
  `geometry.toml` `[collision]`), built as a ring of 48 capsules (the radius ripples by 0.4 mm). The
  ring is sized so the tire stands on its loaded rolling radius `radius_m` (0.2251 m): MuJoCo's contact
  barely sinks in, so the 4.5 mm the real tire flattens under a rider is taken off the scanned free
  radius (0.2296 m) up front. At speed it rolls on 0.223 m (MuJoCo's soft friction creeps about 1 %),
  so the firmware reads speeds about 1 % high. Without measured shapes the tire is an ellipsoid with
  semi-axes (r, half width, r). Contact `condim = 6` gives sliding, torsional and rolling friction.
  Rolling resistance is MuJoCo's rolling friction, a length: `crr · r`, so the resisting torque is
  `crr · N · r`, the classic `crr · N` force at the contact. The contact time constant (`solref`) is
  the tire's softness.
- **Wheel.** The spinning part of the hub motor (rotor, magnets, rim, tire). The stator rides on the
  suspension carriage (in the shell without a suspension). The spin inertia is set directly; the other
  two axes are half of it (a ring).
- **Suspension.** The Extreme's motor axle slides up into the frame on two vertical rods, held down
  by a coil-over shock at the rear through a rocker linkage (per side: carriage, link, triangular
  rocker, shock). In the model the carriage (stator, axle clamps, slider blocks: `unsprung_mass_kg`)
  slides on a joint along `axis_forward_deg` from the shell's vertical, and the wheel spins on the
  carriage, so the motor's reaction torque still reaches the shell. The joint counts compression
  from full extension (the scanned state, where the shell's heights are measured). The linkage's
  leverage `r(x)` (axle travel per shock travel) may change with the axle travel `x`: `ratio_curve`
  gives it as points joined by straight lines, or `linkage_ratio` holds it constant. The shock
  compresses `s(x) = ∫ dx / r` (exact on each straight piece), its whole stroke sets the axle
  travel, and each step `Physics::step` pushes the carriage down with
  `(k·(s(x) + preload) + c·ẋ / r(x)) / r(x)` (the coil never pulls). With a constant ratio that is
  a wheel rate `k / r²` and damping `c / r²`. MuJoCo's joint limits are the end stops (top-out and
  bottom-out, time constant 4 ms). The model starts at the static sag under the shell and the
  rider (where that force carries their weight); on the stand the spring presses the tire into the
  ground, so the sag shows once the stand lets go. Pedals, IMU and rider are on the frame (sprung).
  The tire and the shell never touch each other (an explicit contact exclusion: the tire runs inside
  the frame). Telemetry channels: `suspension_mm` (axle travel) and `shock_mm` (shock travel, for
  drawing the shock).
- **Shell.** Mass, centre of mass and inertia from the parameters. The shapes are the scan's collision
  solids (`geometry.toml` `[collision]`: battery packs with their slider channels, seat, handle and
  headlight, rear roller, shock, pedal brackets, and estimated pedal decks), for drawing and for the
  contacts of a fall; without them, generic packs, handle and pedals sized from the tire.
- **Stand.** A weld from the shell to the world holds the wheel upright and still while the firmware
  boots; the scenario releases it. Before time starts, the model settles on the stand for 0.2 s, so
  the first IMU sample already reads a wheel at rest (both the weld and the tire contact are soft).

## The rider

Two rider models share one controller (below). A scenario names its model in `[rider] model`.

- **`articulated_sagittal_v1` (the default since 2026-10-22).** An upper body (trunk, head, fixed
  arms) on two legs, each a thigh, a shank and a foot on hip, knee and ankle hinges that turn
  forward and back only. Its feet are welded to the pedals. See
  [the articulated rider](#the-articulated-rider-default) below.
- **`articulated_contact_v1` (opt-in).** The same articulated rider with each sole standing on its
  own pedal through friction contact instead of a weld. It has its own id so that a recording or a
  result that names `articulated_sagittal_v1` keeps meaning welded feet.
- **`rigid_ankle_v1`.** One rigid body: mass, centre of mass height above the pedals and a radius
  of gyration, hinged at the ankles and welded to the feet. It is the controlled baseline of the
  results before 2026-10-22; a scenario selects it with `model = "rigid_ankle_v1"`, and
  `euc sim run --rider-model rigid_ankle_v1` overrides any scenario. Recordings name the model
  that ran (`context.rider_model`).

The rider controller sets the lean the ankles hold:

1. **Target speed to lean.** `a = kp·(v_target − v) + ki·∫(v_target − v)`, limited to the rider's
   strongest acceleration and braking; `lean = atan((a + drag(v_target)/m) / g)`, limited to ±15°
   and ramped at 12°/s. The integral only runs within 1 m/s of the target and while the request is
   not limited, so it trims the steady speed without winding up.
2. **Lean to ankle torque.** `τ = kp_lean·(lean_cmd − lean) − kd_lean·lean_rate`, limited to the
   strongest ankle torque. The pedals take the opposite torque; that is what the firmware's balance
   loop feels when the rider leans.
3. **Tiltback eases the rider off.** When the firmware tips the pedals back (nose up) by more than
   2° while the rider wants to go forward, and the rider is not already slowing down, the rider
   aims 1.5 km/h below the present speed for every degree beyond 2°, braking at most 1 m/s² (it
   stops pushing rather than brakes hard), down to a stop and never backwards, so a speed, battery
   or PWM tiltback (or a limp mode) slows the ride the way it slows a real rider. A stopped rider
   waits for the tiltback to fall back within 2° before setting off again. The tiltback the rider
   feels is the firmware's own (the sum of its speed, battery and PWM offsets) when the simulator can
   read it, as for the stock map and our builds; otherwise it is the pedals' pitch above the riding
   pitch, the angle the rider is used to (a 3 s average that does not rise while a tiltback is felt).
   Either way a tiltback counts in full even when the rider leans hard and the shell rides nose down.
   A tiltback read from the firmware is felt at once, as the firmware commands it, a little before
   the pedals have moved; a real rider feels the pedals.
   Set `[rider.control] tiltback_ease_kmh_per_deg = 0` or `tiltback_feel_deg = 90` to ride through
   tiltback.
4. **Steering.** The rider keeps the line: feet and legs twist the wheel back with
   `τ = −kp_steer·heading − kd_steer·yaw_rate` (2000 N·m/rad and 200 N·m·s/rad, at most 60 N·m; all
   DEFAULT). A rigid rider cannot twist, so the torque acts on the shell about the vertical, like the
   roll spring; it stops when the rider steps off. Without it, a wheel that slows on a steep grade
   turns across the slope and falls. The line is world +x, the direction of every scenario so far; a
   scenario that turns will need a path heading.
5. **Letting go.** A rider leaves a wheel that goes down: once the firmware has cut the drive (the
   session watches the cutoff flags) and the shell's pitch or roll passes 20°, or as soon as either
   passes 45° (a fall, cut or not), the rider lets go of the pedals 0.1 s later. The feet leave the
   pedals as in a step-off, but without the stepping foot's push, so the rider keeps their own
   momentum; ankle torque, steering and the roll aid stop. The timeline records "rider off" with
   the reason. `[rider.let_go]` sets `after_cut_deg`, `fall_deg` and `reaction_s` (DEFAULT 20°, 45°,
   0.1 s), and `enabled = false` keeps the rider on the pedals whatever happens (with contact feet
   the soles can still leave the pedals physically).

Leaning works on a real wheel the same way: the firmware keeps the pedals near its set angle, so a
rider who shifts the centre of mass forward makes it accelerate to stay under them. In a steady
acceleration `a = g·tan(lean)` the ankles need no torque at all, which is why the controller holds a
lean and does not push.

### The articulated rider (default)

Segment masses are fractions of the rider's mass (each foot 2 %, shank 5 %, thigh 10 %, upper body
66 %); segment lengths scale with `height_m`; the upper body's centre of mass sits where the whole
rider's lands at `com_above_pedals_m`. All are generic ESTIMATES, not anyone's body. The hips and
knees hold a posture (`[rider.articulation] joint_kp`, `joint_kd`, at most `joint_max_nm` per joint;
DEFAULT 1000 N·m/rad, 60 N·m·s/rad, 180 N·m), so they give under a bump and come back.

- **Lean from the ankles.** The lean controller turns the upper body's pitch error into one task
  moment, applied equally at the hip, knee and ankle of each standing leg (the Jacobian transpose
  of the upper body's pitch over a supported foot), so the joints keep internal reactions and the
  damping removes energy. A standing ankle has no posture spring: the hips and knees keep the body
  straight and the whole body leans from the ankles, as on a real wheel and as the rigid rider
  does. Until 2026-10-22 the ankles also held a posture spring; the three joints then shared the
  lean, the legs leaned about a third as far as the upper body, and the wheel accelerated about half
  as hard as the speed controller asked. Measured before and after the change:

  | Measure | Ankle spring (before) | No standing ankle spring (now) | Rigid rider |
  |---|---|---|---|
  | `overlean-top-speed`, stock: speed at 15 s | 17.1 km/h | 33.6 km/h | 30.6 km/h |
  | Steady 3.3 m/s, test balance law: upper-body lean, centre-of-mass lean | 1.92°, 1.28° | 0.74°, 0.93° | (one body) |
  | Largest gap between the two leans | 0.69° | 0.19° | 0 |
  | Mechanical braking test: stopping travel, axle work | 7.46 m, −68 J | 5.05 m, −195 J | |

  The centre-of-mass lean is the angle from vertical of the line from the ankles to the rider's
  centre of mass; the rigid rider's hinge angle is exactly that line. A regression test keeps the
  two leans within 0.5° in steady riding.
- **Feet on the pedals.** With `articulated_sagittal_v1` (the default) both feet are welded to
  the pedals until the rider leaves. With `articulated_contact_v1` each sole is a box (24 × 9 × 6 cm,
  from 9 cm behind the ankle to 15 cm ahead) that touches only its own pedal, through an explicit
  contact pair: friction `sole_friction` (DEFAULT 1.0, rubber on grip pins), softness
  `sole_contact_time_s` (DEFAULT 5 ms). A foot can lift, rock onto heel or toe and slip while the
  other keeps its contact; the pedal pushes and never pulls. The soles therefore carry an ankle
  moment only while the centre of pressure stays under them: for the default 80 kg rider about
  70 N·m backward (heel) and 120 N·m forward (toes), both feet together, where a welded foot passes
  the controller's whole 400 N·m. The lean controller was tuned on welded feet and does not keep
  the centre of pressure under the soles: on contact feet the rider comes off the pedals in the hard
  brake of `hard-braking-30kmh` (at 17.7 s), on the 20 % climb of `hill-climb-low-battery` (33.4 s)
  and in the 30° launch of the full-effort runs, while the riderless wheel stays up. Contact feet
  stay opt-in until the controller keeps within what the soles carry. The `rider_on_pedals`
  check, in every scenario without a step-off, fails when this happens.
- **The roll aid holds the upper body too** (`articulated_contact_v1` only). The legs have no roll joints and
  contact feet do not hold the body to the shell, so the same roll spring that holds the shell also
  holds the upper body at the wheel's roll target; it stands in for the hip and ankle roll a person
  balances with. Its torque and work add to the reported roll aid. Without it a one-foot stance, or
  the end of a turn, topples the rider sideways off the pedals.
- **Lifting a foot** (`articulated_contact_v1`). `Physics::set_foot_lift(foot, rad)` bends one leg (that much more hip flexion,
  twice that at the knee, the foot kept level) at 3 rad/s; 0.3 rad lifts the foot about 2 cm. One
  foot at a time; the other leg then carries the whole lean task. Its sole sticks to its pedal
  meanwhile (under 1 mm of slip in a quarter-second lift), but setting the foot down again unloads
  the other foot: it hops for up to about 1 s and lands 2-5 cm back on its deck.
- **Foot state.** `Physics::feet()` reports each foot: contact, the pedal's normal load and friction,
  and how far the sole has moved on its deck since the model was built (slip) and above it (lift).
- **Leaving.** A step-off, a let-go and (contact feet) a lost footing release the pedal welds or
  switch the sole-pedal pairs off, so the feet pass the decks, and stop every joint motor and aid.
  A step-off adds
  the stepping foot's push: the right foot presses its pedal down for 0.2 s with the force that
  gives the rigid rider's 30 N·m roll kick, and the pedal pushes the foot up as hard (an internal
  pair, so the rider keeps its horizontal momentum). Without the push the riderless wheel rolled on
  upright, and neither step-off scenario was cut within 3 s; with it stock cuts 2.24 s after the
  15 km/h step-off and 1.50 s after the 5 km/h one (rigid rider: 2.75 s and 0.63 s). When both soles
  have been off their pedals for
  0.5 s with contact feet (longer than the 0.05 s a 2 cm pothole unloads them at 12 km/h), the rider
  counts as off:
  "rider off: both feet off the pedals (the wheel stays up)".

What neither rider can do yet: balance sideways or twist without the external aids, step, use the
hips to recover when the soles' centre of pressure reaches heel or toe, or brace the legs against
the shell. The rigid rider also cannot bend its knees and hips.

## Aids (M2)

- **Roll spring.** Neither rider can balance sideways (a real rider steers and twists), so a
  torque about the heading holds the shell upright: `τ = −k·roll − c·roll_rate`, k = 5000 N·m/rad,
  c = 500 N·m·s/rad, far stiffer than the toppling moment of wheel and rider (about 1200 N·m/rad).
  It shows in recordings as a roll that stays at zero. Set `[assist] roll_stiffness = 0` in a
  scenario to see the wheel fall over sideways.
- **Upper-body roll aid** (`articulated_contact_v1` only). The same law, with the same gains, holds
  the articulated upper body at the wheel's roll target, because contact feet do not hold the body to
  the shell and the legs cannot roll. It is an external torque, like the shell's roll spring and the
  yaw aid of the steering (point 4 above); its torque and work add to the reported roll aid.
- **Recorded.** A recording's context (`model_aids`) carries every aid the run had: the shell's roll
  stiffness and damping, the yaw aid's gains and cap, and for `articulated_contact_v1`
  `upper_body_roll_stiffness` and `upper_body_roll_damping` (absent for the other models).
- **Air drag** acts on the rider's centre of mass: `F = ½·ρ·CdA·v·|v|`, ρ = 1.2 kg/m³,
  CdA = 0.6 m² (sim.py's value, an upright adult).

## Parameters

Wheel (`[body]` in `plant.toml`, Begode Extreme values):

| Parameter | Value | Source |
|---|---|---|
| `tire.radius_m` | 0.2251 m | INFERRED: loaded rolling radius, 28 pole pairs × 0.0505 m (the firmware's distance per electrical turn) / 2π; the scanned 3.00-12 tire's free radius is 0.2296 m |
| `tire.half_width_m` | 0.0407 m | MEASURED: 81.4 mm wide (used only without a torus) |
| `geometry.toml` `tire_torus` | tube 0.058 m | MEASURED: crown lateral radius; the ring radius follows `radius_m` |
| `tire.friction` | 0.9 | DEFAULT: rubber on dry asphalt |
| `tire.crr` | 0.012 | DEFAULT: research sim.py |
| `tire.contact_time_s` | 0.02 s | DEFAULT: about 200 kN/m |
| `wheel.mass_kg` | 16 kg | DEFAULT: rotor, rim and tire |
| `wheel.spin_inertia_kgm2` | 0.487 kg·m² | DEFAULT: 0.6·m·r² |
| `shell.mass_kg` | 20 kg | DEFAULT: 42 kg with the wheel and the carriage |
| `shell.com_above_axle_m` | 0.13 m | DEFAULT: 0.10 m for a 26 kg shell with the stator, the stator taken out |
| `shell.inertia_kgm2` | 0.76, 0.82, 0.45 kg·m² | DEFAULT: a 0.35 × 0.30 × 0.55 m box (0.85, 0.92, 0.46 with the stator), less the stator's own and offset terms |
| `shell.pedal_below_axle_m` | −0.012 m | MEASURED (approx.): deck tops 12 mm above the axle at full extension: the scanned pins at +7.5 mm plus half a 9 mm deck; the owner measured about 240 mm from the floor to the deck top (unloaded), 10.4 mm above the 229.6 mm free radius, within the ±5 mm of the deck estimate |
| `shell.imu_above_axle_m` | 0.15 m | DEFAULT: research sim.py; on the frame (sprung) |
| `suspension.unsprung_mass_kg` | 6 kg | ESTIMATE: stator, axle clamps, slider blocks (the wheel's 16 kg moves too) |
| `suspension.axis_forward_deg` | −0.13° | MEASURED: the two 30 mm slider rods are vertical within 0.15°, top to the rear (scan) |
| `suspension.spring_rate_n_per_mm` | 131.3 N/mm | MEASURED: "DNM - 750 LBS/IN" printed on the owner's spring |
| `suspension.preload_mm` | 2 mm | ESTIMATE |
| `suspension.stroke_mm` | 55 mm | ESTIMATE: 200 mm DNM coil-overs come with 50-57 mm of stroke |
| `suspension.eye_to_eye_mm` | 200 mm | MEASURED (owner; drawing only); the scan's older shock was 204 mm |
| `suspension.linkage_ratio` | 1.2 | ESTIMATE: 66 mm of axle travel (the rods leave 108 mm free above the carriage); used while `ratio_curve` is empty |
| `suspension.ratio_curve` | none yet | the rocker's leverage along the travel, from the scan and the owner's photos (model) |
| `suspension.damping_n_s_per_m` | 2200 N·s/m | ESTIMATE: damping ratio about 0.25 for the sprung 100 kg |

With these values an 80 kg rider sags the suspension 8 mm (13 % of the travel), and the sprung mass
bounces at about 4.8 Hz with a damping ratio of about 0.25.

Rider and aids (scenario `[rider]`, `[rider.control]`, `[assist]`; these are the defaults):

| Parameter | Value | Note |
|---|---|---|
| `model` | `articulated_sagittal_v1` | `rigid_ankle_v1`: the controlled baseline; `articulated_contact_v1`: contact feet (opt-in) |
| `mass_kg` | 80 kg | |
| `com_above_pedals_m` | 1.0 m | about 55 % of a 1.78 m adult's height |
| `ankle_above_pedals_m` | 0.08 m | the hinge |
| `gyration_m` | 0.35 m | pitch and roll; 9.8 kg·m² about the centre of mass (rigid); the articulated upper body uses it for its own inertia |
| `height_m` | 1.78 m | drawing; the articulated segment lengths scale with it |
| `articulation.joint_kp`, `joint_kd`, `joint_max_nm` | 1000 N·m/rad, 60 N·m·s/rad, 180 N·m | ESTIMATE: hip and knee posture (and a lifted foot's ankle), per joint |
| `articulation.sole_friction`, `sole_contact_time_s` | 1.0, 0.005 s | DEFAULT: rubber on grip pins; shoe and pedal give (`articulated_contact_v1`) |
| `cda_m2` | 0.6 m² | air drag |
| `control.lean_kp`, `lean_kd` | 6000 N·m/rad, 900 N·m·s/rad | about 1.3 Hz, damping 0.6 about the ankles |
| `control.ankle_max_nm` | 400 N·m | both legs |
| `control.speed_kp`, `speed_ki` | 0.6 /s, 0.03 /s² | |
| `control.accel_max`, `brake_max` | 1.5, 2.5 m/s² | |
| `control.lean_max_deg`, `lean_rate_deg_s` | 15°, 12°/s | |
| `control.tiltback_feel_deg`, `tiltback_ease_kmh_per_deg`, `tiltback_ease_brake` | 2°, 1.5 km/h per degree, 1 m/s² | DEFAULT: how a rider reacts to tiltback |
| `control.steer_kp`, `steer_kd`, `steer_max_nm` | 2000 N·m/rad, 200 N·m·s/rad, 60 N·m | DEFAULT: how a rider keeps the line |
| `let_go.enabled`, `after_cut_deg`, `fall_deg`, `reaction_s` | true, 20°, 45°, 0.1 s | DEFAULT: when a rider leaves a wheel that goes down |
| `assist.roll_stiffness`, `roll_damping` | 5000 N·m/rad, 500 N·m·s/rad | on the shell; for `articulated_contact_v1` also on the upper body |

## Scenarios

A scenario (`scenarios/*.toml`) is a scripted ride with expectations; `euc sim run --scenario FILE`
runs it on the wheel it names and prints each check. The target speed is a list of `[seconds, km/h]`
points, linear in between. `release_s` is when the stand lets go; BLE commands go in `[[inject]]`;
`push = [[start s, N, duration s]]` pushes the rider forward (or back, negative) at the centre of mass;
`settings = "<preset>"` boots the wheel with a settings preset (see [../bridge.md](../bridge.md#settings-presets)).
Both shipped scenarios use `neutral`: factory settings with level pedals.
The stand should let go soon after the firmware starts balancing: held against a rigid stand, the
stock firmware's balance output climbs to its limit if its set angle is not the stand's.
Checks (`[[expect]]`, judged after the run, from the release unless `from_s` says otherwise):

| `check` | Passes when |
|---|---|
| `upright` | pitch and roll stay within 45° |
| `no_cutoff` | none of the firmware's cutoff flags (stock map RAM variables with `role = "cutoff"`) is set |
| `pitch` | \|pitch\| stays within `max_abs_deg` |
| `speed_reached` | the ground speed reaches `kmh` |
| `stopped_at_end` | \|speed\| at the end is within `max_abs_kmh` |
| `rider_on_pedals` | the rider never comes off the pedals while the wheel stays up; every scenario without `step_off_s` has it by default; only `articulated_contact_v1` can fail it |

`--record FILE` writes a recording the studio replays: a JSON header (wheel, firmware, scenario,
bodies with their shapes, channel schema), the telemetry batches of `euc-telemetry`, and the events
(see `crates/euc-sim/src/recording.rs` for the layout).

## To measure on a real wheel

- Rolling radius: mark the tire, roll one turn with a rider on, measure.
- Masses: weigh the wheel; weigh the shell side with the motor out if it is ever apart.
- IMU position and mounting signs: open the shell; check the signs with the wheel tilted.
- Shell inertia: swing the wheel as a pendulum from the handle.
- Rolling resistance: coast-down from 10 km/h with the motor off (free-wheel), log the speed.
- Suspension: the shock's make, spring rate (stamped on the coil), stroke and preload; the axle
  travel from top-out to bottom-out (zip tie on a rail); the sag with the rider on (pedal height
  above the floor with and without the rider).
