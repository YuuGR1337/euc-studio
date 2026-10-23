//! The rider's controller: target speed to lean, lean to ankle torque, and steering.
//!
//! An EUC rider rides by leaning: lean forward and the wheel speeds up to stay under you, lean
//! back and it slows. The controller turns a target speed into a lean from vertical, ramped at
//! a human rate, and the ankles hold that lean with a stiff spring and damper. The ankle torque
//! pushes on the pedals with the opposite sign; that is what the firmware's balance loop feels.
//! Steering uses a capped external yaw aid on the shell. The rigid rider has no twisting joints.

use std::{
    collections::VecDeque,
    f64::consts::{PI, TAU},
};

use crate::params::RiderControl;

/// Speed error inside which the rider trims with the integral term, m/s.
const INTEGRAL_BAND_MPS: f64 = 1.0;

/// Time constant of the riding pitch, seconds: the pedal angle the rider is used to, against
/// which a tiltback is felt when the firmware's own tiltback is not known.
const RIDING_PITCH_S: f64 = 3.0;

/// By hand, how far the line may run ahead of the wheel's heading, radians. A wheel held back (by a
/// wall, or the twist aid's cap) then neither winds up a turn it would finish after the input
/// stops, nor one past ±π that would turn it the other way.
const MANUAL_HEADING_SLACK_RAD: f64 = 0.5;

/// Wraps a finite angle to [-π, π], retaining small angles without rounding changes.
/// Nonfinite values pass through; public heading setters reject them before wrapping.
pub fn wrap_angle(rad: f64) -> f64 {
    if !rad.is_finite() {
        return rad;
    }
    let angle = rad % TAU;
    if angle > PI {
        angle - TAU
    } else if angle < -PI {
        angle + TAU
    } else {
        angle
    }
}

/// A tick delay retains at most 600 previous samples (601 during insertion).
#[derive(Debug, Clone, PartialEq)]
struct TickDelay<T> {
    samples: VecDeque<T>,
}

impl<T: Copy> TickDelay<T> {
    const fn new() -> Self {
        Self { samples: VecDeque::new() }
    }

    fn clear(&mut self) {
        self.samples.clear();
    }

    fn sample(&mut self, seconds: f64, value: T, initial: T) -> T {
        // Setup validation rejects out-of-range values. Clamp here as well because callers can
        // construct a standalone RiderController without building a Physics model.
        let ticks = (seconds / crate::TIMESTEP_S).ceil().clamp(0.0, 600.0) as usize;
        if ticks == 0 {
            return value;
        }
        if self.samples.is_empty() {
            self.samples.reserve_exact(ticks + 1);
            self.samples.resize(ticks, initial);
        }
        self.samples.push_back(value);
        self.samples.pop_front().expect("delay contains an inserted sample")
    }
}

/// What the rider wants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Intent {
    /// Ride at this ground speed, m/s, forward positive.
    Speed(f64),
    /// Hold this lean from vertical, radians, forward positive.
    Lean(f64),
}

/// Rolling resistance and air drag, as decelerations.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Drag {
    /// Rolling resistance, m/s².
    pub rolling: f64,
    /// Air drag per (m/s)², 1/m.
    pub aero: f64,
}

impl Drag {
    /// Speed below which rolling resistance fades out, m/s, so it never pushes a wheel at rest.
    pub const ROLLING_FADE_MPS: f64 = 0.3;

    /// Deceleration at `speed` (m/s), with the sign of the speed.
    pub fn decel(self, speed: f64) -> f64 {
        self.rolling * (speed / Self::ROLLING_FADE_MPS).tanh() + self.aero * speed * speed.abs()
    }
}

/// The speed the rider aims for: `wanted`, unless the pedals tip back (`felt`, radians of
/// tiltback) while the rider wants to go forward and is not already slowing down. Then the rider
/// eases off, the more the steeper the tiltback, as a rider does when the wheel pushes the toes
/// up: down to a stop, never backwards, and a stopped rider waits for the tiltback to go before
/// setting off again.
fn eased(wanted: f64, felt: f64, s: &Sense, c: &RiderControl) -> f64 {
    let tilt = (felt - c.tiltback_feel_deg.to_radians()).max(0.0).to_degrees();
    if tilt <= 0.0 || c.tiltback_ease_kmh_per_deg <= 0.0 || wanted <= 0.0 || wanted < s.speed {
        return wanted;
    }
    wanted.min(s.speed - c.tiltback_ease_kmh_per_deg * tilt / 3.6).max(0.0)
}

/// What the rider senses in one step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Sense {
    /// Step, seconds.
    pub dt: f64,
    /// Ground speed, m/s.
    pub speed: f64,
    /// Lean of the rider from vertical, radians, forward positive.
    pub lean: f64,
    /// Its rate, rad/s.
    pub lean_rate: f64,
    /// What resists motion.
    pub drag: Drag,
    /// Gravity, m/s².
    pub gravity: f64,
    /// The stand holds the wheel: the rider stands still and does not wind up.
    pub held: bool,
    /// The pedals' pitch, radians, nose up positive (tiltback lifts the nose).
    pub pitch_up: f64,
    /// Heading from world +x, radians, left positive.
    pub heading: f64,
    /// Its rate about the vertical, rad/s, left positive.
    pub yaw_rate: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Command {
    intent: Intent,
    heading: f64,
    turn_rate: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Observation {
    sense: Sense,
    tiltback: Option<f64>,
}

/// The rider's controller.
#[derive(Debug, Clone, PartialEq)]
pub struct RiderController {
    control: RiderControl,
    intent: Intent,
    applied_intent: Intent,
    desired_lean: f64,
    lean_command: f64,
    heading_target: f64,
    applied_heading_target: f64,
    integral: f64,
    /// The pedal pitch the rider is used to, radians nose up.
    riding_pitch: f64,
    /// The firmware's tiltback, radians nose up, when the simulator can read it.
    commanded_tiltback: Option<f64>,
    commands: TickDelay<Command>,
    observations: TickDelay<Observation>,
    feedback: Option<Sense>,
    mode_changed: bool,
    /// How fast the rider turns the line they keep, rad/s, left positive (manual riding).
    turn_rate: f64,
    /// Manual turn rate after the same delay as the heading and lean requests.
    applied_turn_rate: f64,
    /// The lean into the turn the rider holds now, radians of roll, right side down positive.
    turn_lean: f64,
    /// A person rides by hand (Free Ride, Ride's manual mode): the manual limits apply.
    manual: bool,
}

/// How the rider steers in one step.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Steer {
    /// The roll the rider holds the wheel at, radians, right side down positive: leaning the
    /// wheel over makes the cambered tire carve the turn.
    pub roll: f64,
    /// External yaw aid on the shell, N·m, left positive. Heading twist fades with speed;
    /// yaw-rate damping remains active to suppress wobble.
    pub yaw_nm: f64,
}

impl RiderController {
    /// A rider who wants to stand still.
    pub const fn new(control: RiderControl) -> Self {
        Self {
            control,
            intent: Intent::Speed(0.0),
            applied_intent: Intent::Speed(0.0),
            desired_lean: 0.0,
            lean_command: 0.0,
            heading_target: 0.0,
            applied_heading_target: 0.0,
            integral: 0.0,
            riding_pitch: 0.0,
            commanded_tiltback: None,
            commands: TickDelay::new(),
            observations: TickDelay::new(),
            feedback: None,
            mode_changed: false,
            turn_rate: 0.0,
            applied_turn_rate: 0.0,
            turn_lean: 0.0,
            manual: false,
        }
    }

    /// Turns the line the rider keeps (the heading target) at `rad_s` (left positive) from the
    /// next command on; back to 0, the rider keeps the heading reached. Nonfinite rates are ignored.
    /// Scripted riding: the request is capped at `turn_rate_max_rad_s` and fades to zero below
    /// walking pace, so it cannot spin a stationary wheel. Manual riding: capped by
    /// [`manual_turn_cap`](Self::manual_turn_cap), without the fade.
    pub const fn set_turn_rate(&mut self, rad_s: f64) {
        if rad_s.is_finite() {
            self.turn_rate = rad_s;
        }
    }

    /// A person rides by hand (`true`: Free Ride, Ride's manual mode) or a script rides (`false`,
    /// the default). By hand the manual limits of [`RiderControl`] apply: a turn cap that falls
    /// with speed instead of `turn_rate_max_rad_s`, no turn fade at walking pace (at a stop the
    /// twist aid turns the wheel and the rider with it), a deeper and quicker lean into turns and
    /// a quicker forward lean. Scripted riding never uses them.
    pub const fn set_manual(&mut self, manual: bool) {
        self.manual = manual;
    }

    /// Whether a person rides by hand ([`set_manual`](Self::set_manual)).
    pub const fn manual(&self) -> bool {
        self.manual
    }

    /// The largest manual turn rate at `speed` (m/s), rad/s: `manual_turn_rate_max_rad_s` at a
    /// stop, falling at speed to the rate whose lean into the turn is `manual_turn_lean_max_deg`
    /// (`v·rate = g·tan(lean)`).
    pub fn manual_turn_cap(&self, speed: f64, gravity: f64) -> f64 {
        let c = self.control;
        let lateral = gravity * c.manual_turn_lean_max_deg.to_radians().tan();
        if speed.abs() * c.manual_turn_rate_max_rad_s <= lateral {
            c.manual_turn_rate_max_rad_s
        } else {
            lateral / speed.abs()
        }
    }

    /// The rider's turn rate, rad/s, left positive.
    pub const fn turn_rate(&self) -> f64 {
        self.turn_rate
    }

    /// The lean into the turn the rider holds now, radians of roll, right side down positive.
    pub const fn turn_lean(&self) -> f64 {
        self.turn_lean
    }

    /// The line the rider keeps: the heading target, radians from world +x, left positive.
    pub const fn line(&self) -> f64 {
        self.heading_target
    }

    /// Drops tick history after model settling without changing the settled controller target.
    pub(crate) fn reset_delays(&mut self) {
        self.commands.clear();
        self.observations.clear();
        self.feedback = None;
    }

    /// Tells the rider the firmware's tiltback (radians, nose up), or `None` when it is not known.
    /// A known tiltback is what the rider feels; otherwise the rider feels the pedals' pitch above
    /// the riding pitch, the angle they are used to.
    pub const fn set_commanded_tiltback(&mut self, tiltback: Option<f64>) {
        self.commanded_tiltback = tiltback;
    }

    /// The tiltback the rider feels now, radians, nose up positive.
    fn felt_tiltback(&self, s: &Sense) -> f64 {
        self.commanded_tiltback.unwrap_or(s.pitch_up - self.riding_pitch)
    }

    /// Follows the riding pitch: slowly, and never upwards while a tiltback is felt, so a lasting
    /// tiltback stays felt instead of becoming the new normal.
    fn track_riding_pitch(&mut self, s: &Sense) {
        if s.held {
            self.riding_pitch = s.pitch_up;
            return;
        }
        let feeling = s.pitch_up - self.riding_pitch > self.control.tiltback_feel_deg.to_radians();
        if !feeling || s.pitch_up < self.riding_pitch {
            self.riding_pitch += (s.pitch_up - self.riding_pitch) * (s.dt / RIDING_PITCH_S).min(1.0);
        }
    }

    /// Changes the desired request. Nonfinite requests leave the previous request unchanged.
    /// The command delay and lean slew limit apply on physics steps, never on input arrival.
    /// A speed/lean mode switch clears the speed integral and reseeds delays on the next step.
    pub const fn set_intent(&mut self, intent: Intent) {
        let (Intent::Lean(value) | Intent::Speed(value)) = intent;
        if !value.is_finite() {
            return;
        }
        if !matches!((self.intent, intent), (Intent::Lean(_), Intent::Lean(_)) | (Intent::Speed(_), Intent::Speed(_))) {
            self.integral = 0.0;
            self.mode_changed = true;
        }
        self.intent = intent;
    }

    /// Sets desired world heading, radians, left positive; zero follows world +x.
    /// Rejects nonfinite values without changing controller state.
    pub fn set_heading_target(&mut self, rad: f64) -> Result<(), String> {
        if !rad.is_finite() {
            return Err("heading target must be finite".into());
        }
        self.heading_target = wrap_angle(rad);
        Ok(())
    }

    /// Desired world heading before command delay, radians.
    pub const fn heading_target(&self) -> f64 {
        self.heading_target
    }

    /// Heading target after command delay, radians.
    pub const fn applied_heading_target(&self) -> f64 {
        self.applied_heading_target
    }

    /// Clears pending requests for pause/disarm/release without changing physical state.
    /// Neutral lean and the supplied measured heading bypass command delay. The existing lean
    /// command still returns to neutral through its normal slew limit when physics resumes.
    pub fn clear_input(&mut self, heading_rad: f64) -> Result<(), String> {
        self.set_heading_target(heading_rad)?;
        self.intent = Intent::Lean(0.0);
        self.applied_intent = self.intent;
        self.desired_lean = 0.0;
        self.applied_heading_target = self.heading_target;
        self.integral = 0.0;
        self.commands.clear();
        self.observations.clear();
        self.feedback = None;
        self.mode_changed = false;
        self.turn_rate = 0.0;
        self.applied_turn_rate = 0.0;
        Ok(())
    }

    /// What the rider wants.
    pub const fn intent(&self) -> Intent {
        self.intent
    }

    /// Intent after command delay, before the lean slew limit.
    pub const fn applied_intent(&self) -> Intent {
        self.applied_intent
    }

    /// Desired manual lean before delay and limits, radians. In speed mode this reports the
    /// most recent lean target computed from the delayed speed request and rider observations.
    pub const fn desired_lean(&self) -> f64 {
        match self.intent {
            Intent::Lean(lean) => lean,
            Intent::Speed(_) => self.desired_lean,
        }
    }

    /// The lean the ankles are holding, radians from vertical, forward positive.
    pub const fn lean_command(&self) -> f64 {
        self.lean_command
    }

    /// The ankle torque for this step, N·m about the lean hinge (positive tips the rider
    /// forward).
    pub(crate) fn ankle_torque(&mut self, s: &Sense) -> f64 {
        let c = self.control;
        if self.mode_changed || s.held {
            self.commands.clear();
            self.observations.clear();
            if self.mode_changed {
                // Seed the new mode from measured state. Do not replace poses, velocities, or
                // the existing rate-limited lean command.
                self.applied_intent = match self.intent {
                    Intent::Lean(_) => Intent::Lean(s.lean),
                    Intent::Speed(_) => Intent::Speed(s.speed),
                };
                self.applied_heading_target = s.heading;
                self.applied_turn_rate = 0.0;
            }
            self.mode_changed = false;
        }
        let turn_rate = if s.held {
            0.0
        } else if self.manual {
            let cap = self.manual_turn_cap(s.speed, s.gravity);
            self.turn_rate.clamp(-cap, cap)
        } else {
            self.turn_rate.clamp(-c.turn_rate_max_rad_s, c.turn_rate_max_rad_s)
                * (s.speed.abs() / c.twist_fade_mps).min(1.0)
        };
        if turn_rate != 0.0 {
            self.heading_target = wrap_angle(self.heading_target + turn_rate * s.dt);
        }
        if self.manual && !s.held {
            let ahead = wrap_angle(self.heading_target - s.heading);
            if ahead.abs() > MANUAL_HEADING_SLACK_RAD {
                self.heading_target = wrap_angle(s.heading + ahead.signum() * MANUAL_HEADING_SLACK_RAD);
            }
        }
        let request = Command { intent: self.intent, heading: self.heading_target, turn_rate };
        let initial = Command {
            intent: self.applied_intent,
            heading: self.applied_heading_target,
            turn_rate: self.applied_turn_rate,
        };
        let command = self.commands.sample(if s.held { 0.0 } else { c.command_delay_s }, request, initial);
        self.applied_intent = command.intent;
        self.applied_heading_target = command.heading;
        self.applied_turn_rate = command.turn_rate;
        let observation = Observation { sense: *s, tiltback: self.commanded_tiltback };
        let mut observation =
            self.observations.sample(if s.held { 0.0 } else { c.feedback_delay_s }, observation, observation);
        // Stand/release state and the integration interval are never delayed observations.
        observation.sense.held = s.held;
        observation.sense.dt = s.dt;
        self.feedback = Some(observation.sense);
        let s = &observation.sense;
        let max_lean = c.lean_max_deg.to_radians();
        let felt = if c.feedback_delay_s == 0.0 || s.held {
            self.felt_tiltback(s)
        } else {
            observation.tiltback.unwrap_or(s.pitch_up - self.riding_pitch)
        };
        self.track_riding_pitch(s);
        let target = match self.applied_intent {
            Intent::Lean(lean) => lean,
            Intent::Speed(wanted) => {
                let speed = eased(wanted, felt, s, &c);
                let brake_max = if speed < wanted { c.brake_max.min(c.tiltback_ease_brake) } else { c.brake_max };
                let error = speed - s.speed;
                let accel = c.speed_kp * error + c.speed_ki * self.integral;
                let limited = accel.clamp(-brake_max, c.accel_max);
                let saturated = accel > c.accel_max || accel < -brake_max;
                // Integrate only near the target and while the request is not saturated, so the
                // integral trims the steady speed and never winds up on the way there.
                if !s.held && !saturated && error.abs() < INTEGRAL_BAND_MPS {
                    self.integral += error * s.dt;
                }
                // Lean also for the drag the target speed will meet, so the rider holds it without
                // a steady error.
                ((limited + s.drag.decel(speed)) / s.gravity).atan()
            }
        };
        self.desired_lean = target;
        let target = target.clamp(-max_lean, max_lean);
        let rate = if self.manual { c.manual_lean_rate_deg_s } else { c.lean_rate_deg_s };
        let step = rate.to_radians() * s.dt;
        self.lean_command += (target - self.lean_command).clamp(-step, step);
        let torque = c.lean_kp * (self.lean_command - s.lean) - c.lean_kd * s.lean_rate;
        torque.clamp(-c.ankle_max_nm, c.ankle_max_nm)
    }

    /// How the rider steers this step: back onto the line (the heading target), turning at the
    /// turn rate. The rider asks for a turn rate from the heading error, leans the wheel over so
    /// that the cambered tire carves it (`tan(lean) = v·rate / g` balances the turn), and below
    /// walking pace, where a lean turns nothing, twists the wheel with the feet. The external
    /// heading spring fades with speed; external yaw-rate damping remains active at every speed.
    /// The legs have no twist joint, so both moments remain identified as external aids.
    pub(crate) fn steer(&mut self, s: &Sense) -> Steer {
        let c = self.control;
        let dt = s.dt;
        let s = if c.feedback_delay_s > 0.0 { self.feedback.as_ref().unwrap_or(s) } else { s };
        let error = wrap_angle(s.heading - self.applied_heading_target);
        // By hand the turn cap falls with speed, so the lean into the turn below never asks for
        // more than `manual_turn_lean_max_deg`; scripted riding keeps its fixed cap.
        let (cap, lean_max, lean_rate) = if self.manual {
            (self.manual_turn_cap(s.speed, s.gravity), c.manual_turn_lean_max_deg, c.manual_turn_lean_rate_deg_s)
        } else {
            (c.turn_rate_max_rad_s, c.turn_lean_max_deg, c.turn_lean_rate_deg_s)
        };
        let wanted = (self.applied_turn_rate - c.heading_kp * error).clamp(-cap, cap);
        // Left (positive) turns need the left side down (negative roll); backwards, the other way.
        let max = lean_max.to_radians();
        let target = (-(s.speed * wanted / s.gravity).atan()).clamp(-max, max);
        let step = lean_rate.to_radians() * dt;
        self.turn_lean += (target - self.turn_lean).clamp(-step, step);
        let twist = 1.0 / (1.0 + (s.speed / c.twist_fade_mps).powi(2));
        // Fade the heading spring, not yaw-rate damping. Removing damping at road speed lets
        // tire camber and the roll aid excite a growing yaw/roll wobble even on a straight run.
        let yaw = -(c.steer_kp * error * twist + c.steer_kd * (s.yaw_rate - wanted));
        Steer { roll: self.turn_lean, yaw_nm: yaw.clamp(-c.steer_max_nm, c.steer_max_nm) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sense(speed: f64) -> Sense {
        Sense {
            dt: 0.001,
            speed,
            lean: 0.0,
            lean_rate: 0.0,
            drag: Drag::default(),
            gravity: 9.81,
            held: false,
            pitch_up: 0.0,
            heading: 0.0,
            yaw_rate: 0.0,
        }
    }

    #[test]
    fn leans_forward_to_speed_up_at_a_human_rate() {
        let mut rider = RiderController::new(RiderControl::default());
        rider.set_intent(Intent::Speed(5.0));
        let torque = rider.ankle_torque(&sense(0.0));
        assert!(torque > 0.0, "tips forward: {torque}");
        let rate = RiderControl::default().lean_rate_deg_s.to_radians() * 0.001;
        assert!((rider.lean_command() - rate).abs() < 1e-12, "ramped: {}", rider.lean_command());
        for _ in 0..5000 {
            rider.ankle_torque(&sense(0.0));
        }
        // a = 1.5 m/s² at most: atan(1.5 / 9.81) = 8.7°.
        assert!((rider.lean_command().to_degrees() - 8.69).abs() < 0.05, "{}", rider.lean_command().to_degrees());
    }

    #[test]
    fn leans_back_to_brake_and_holds_a_lean_on_request() {
        let mut rider = RiderController::new(RiderControl::default());
        for _ in 0..5000 {
            rider.ankle_torque(&sense(5.0));
        }
        assert!(rider.lean_command() < -0.2, "leans back: {}", rider.lean_command());
        rider.set_intent(Intent::Lean(0.05));
        for _ in 0..5000 {
            rider.ankle_torque(&sense(5.0));
        }
        assert!((rider.lean_command() - 0.05).abs() < 1e-9);
    }

    #[test]
    fn eases_off_when_the_pedals_tip_back() {
        let c = RiderControl::default();
        let riding = sense(10.0);
        let felt = f64::to_radians;
        assert_eq!(eased(12.0, felt(1.0), &riding, &c), 12.0, "a little tilt is fine");
        let eased_speed = eased(12.0, felt(6.0), &riding, &c);
        assert!((eased_speed - (10.0 - 1.5 * 4.0 / 3.6)).abs() < 1e-9, "4° beyond the feel: {eased_speed}");
        assert_eq!(eased(5.0, felt(6.0), &riding, &c), 5.0, "already braking: no change");
        assert_eq!(eased(12.0, felt(30.0), &riding, &c), 0.0, "a steep tiltback stops the rider, never reverses");
        let stopped = sense(-0.1);
        assert_eq!(eased(12.0, felt(6.0), &stopped, &c), 0.0, "stopped: waits for the tiltback to go");
        assert_eq!(eased(-2.0, felt(6.0), &stopped, &c), -2.0, "riding backwards: no change");
        let none = RiderControl { tiltback_ease_kmh_per_deg: 0.0, ..c };
        assert_eq!(eased(12.0, felt(6.0), &riding, &none), 12.0, "ignoring it rides through");
    }

    #[test]
    fn feels_tiltback_against_the_riding_pitch() {
        let mut rider = RiderController::new(RiderControl::default());
        rider.set_intent(Intent::Speed(10.0));
        // Riding nose down at full lean for a while: that becomes the riding pitch.
        let lean_down = Sense { pitch_up: f64::to_radians(-6.0), ..sense(10.0) };
        for _ in 0..20_000 {
            rider.ankle_torque(&lean_down);
        }
        assert!((rider.riding_pitch.to_degrees() + 6.0).abs() < 0.01, "{}", rider.riding_pitch.to_degrees());
        // A 9° tiltback lifts the pedals only to +3°, but the rider feels all 9° of it, and keeps
        // feeling it while it lasts.
        let tipped = Sense { pitch_up: f64::to_radians(3.0), ..lean_down };
        for _ in 0..30_000 {
            rider.ankle_torque(&tipped);
        }
        assert!(
            (rider.felt_tiltback(&tipped).to_degrees() - 9.0).abs() < 0.5,
            "{}",
            rider.felt_tiltback(&tipped).to_degrees()
        );
        // A tiltback the firmware reports is felt as it is.
        rider.set_commanded_tiltback(Some(f64::to_radians(4.0)));
        assert!((rider.felt_tiltback(&tipped).to_degrees() - 4.0).abs() < 1e-9);
    }

    #[test]
    fn leans_the_wheel_to_turn_and_twists_it_only_at_walking_pace() {
        let mut rider = RiderController::new(RiderControl::default());
        let c = RiderControl::default();
        // Heading left of the line at 5 m/s: the rider wants to turn right (0.01 rad/s), and leans
        // right (positive roll) towards tan(lean) = v·rate/g, ramping at the turn lean rate.
        let off_left = Sense { heading: 0.01, yaw_rate: -0.01, ..sense(5.0) };
        let steer = rider.steer(&off_left);
        assert!(steer.roll > 0.0 && (steer.roll - c.turn_lean_rate_deg_s.to_radians() * 0.001).abs() < 1e-12);
        for _ in 0..5000 {
            rider.steer(&off_left);
        }
        let steady = rider.steer(&off_left).roll;
        assert!((steady - (5.0_f64 * 0.01 / 9.81).atan()).abs() < 1e-9, "{steady}");
        // With the requested yaw rate already reached, only the faded heading spring acts.
        assert!(steer.yaw_nm < 0.0 && steer.yaw_nm.abs() < 0.1 * c.steer_kp * 0.01, "{}", steer.yaw_nm);
        // Standing still, the feet twist the wheel back with the full aid, within its cap.
        let mut standing = RiderController::new(RiderControl::default());
        let torque = standing.steer(&Sense { heading: 0.01, ..sense(0.0) }).yaw_nm;
        let expected = -(c.steer_kp * 0.01 + c.steer_kd * c.heading_kp * 0.01);
        assert!((torque - expected).abs() < 1e-9, "{torque} vs {expected}");
        assert_eq!(standing.steer(&Sense { heading: 1.0, yaw_rate: 1.0, ..sense(0.0) }).yaw_nm, -c.steer_max_nm);
        assert!(standing.steer(&sense(0.0)).roll.abs() < 1e-9, "no lean standing still");
        // The lean into a turn is capped.
        let mut fast = RiderController::new(RiderControl::default());
        fast.set_turn_rate(1.0);
        for _ in 0..20_000 {
            fast.ankle_torque(&sense(10.0));
            fast.steer(&sense(10.0));
        }
        assert!((fast.turn_lean() + c.turn_lean_max_deg.to_radians()).abs() < 1e-9, "{}", fast.turn_lean());
    }

    #[test]
    fn turns_the_line_at_the_turn_rate_and_keeps_the_heading_reached() {
        let mut rider = RiderController::new(RiderControl::default());
        rider.set_turn_rate(0.5);
        for _ in 0..1000 {
            rider.ankle_torque(&sense(5.0));
        }
        assert!((rider.line() - 0.5).abs() < 1e-9, "{}", rider.line());
        // Still heading along +x, the rider leans left (roll negative) towards the line.
        assert!(rider.steer(&sense(5.0)).roll < 0.0);
        rider.set_turn_rate(0.0);
        rider.ankle_torque(&sense(5.0));
        let on_line = Sense { heading: rider.line(), ..sense(5.0) };
        for _ in 0..5000 {
            rider.steer(&on_line);
        }
        assert!(rider.steer(&on_line).roll.abs() < 1e-9, "keeps the heading reached");
        // Across ±π the shorter way round.
        rider.set_heading_target(3.1).unwrap();
        rider.ankle_torque(&sense(5.0));
        let past = Sense { heading: -3.1, ..sense(5.0) };
        assert!(rider.steer(&past).yaw_nm < 0.0, "turns right, back across π");
    }

    #[test]
    fn by_hand_the_rider_turns_at_a_stop_and_the_turn_cap_follows_the_lean_it_needs() {
        let c = RiderControl::default();
        // Scripted: the request fades at walking pace, so a stopped rider keeps the line.
        let mut scripted = RiderController::new(c);
        scripted.set_turn_rate(1.0);
        for _ in 0..1000 {
            scripted.ankle_torque(&sense(0.0));
        }
        assert_eq!(scripted.line(), 0.0, "scripted riding cannot spin a stopped wheel");
        // By hand: the line turns at the requested rate at a stop, ahead of a wheel that follows.
        let mut manual = RiderController::new(c);
        manual.set_manual(true);
        assert!(manual.manual() && !scripted.manual());
        manual.set_turn_rate(1.0);
        for tick in 0..1000 {
            let following = Sense { heading: f64::from(tick) * 0.001, ..sense(0.0) };
            manual.ankle_torque(&following);
        }
        assert!((manual.line() - 1.0).abs() < 1e-9, "{}", manual.line());
        // A wheel held back does not wind the line up: it stays within the slack ahead of it.
        let mut held = RiderController::new(c);
        held.set_manual(true);
        held.set_turn_rate(2.5);
        for _ in 0..3000 {
            held.ankle_torque(&sense(0.0));
        }
        assert!((held.line() - MANUAL_HEADING_SLACK_RAD).abs() < 1e-9, "{}", held.line());
        // The cap: the full rate at a stop, the rate whose lean into the turn is the manual limit
        // at speed (either direction).
        let lateral = 9.81 * c.manual_turn_lean_max_deg.to_radians().tan();
        assert_eq!(manual.manual_turn_cap(0.0, 9.81), c.manual_turn_rate_max_rad_s);
        assert!((manual.manual_turn_cap(-9.0, 9.81) - lateral / 9.0).abs() < 1e-12);
        manual.set_turn_rate(10.0);
        let before = manual.line();
        for _ in 0..1000 {
            // The wheel follows the line, as a carving wheel does.
            let following = Sense { heading: manual.line(), ..sense(9.0) };
            manual.ankle_torque(&following);
            manual.steer(&following);
        }
        let turned = wrap_angle(manual.line() - before);
        assert!((turned - lateral / 9.0).abs() < 1e-9, "{turned}");
        // At the cap the rider leans into the turn at the manual limit, not the scripted one.
        assert!(
            (manual.turn_lean() + c.manual_turn_lean_max_deg.to_radians()).abs() < 1e-9,
            "{}",
            manual.turn_lean().to_degrees()
        );
        assert!(c.manual_turn_lean_max_deg > c.turn_lean_max_deg);
        // By hand the forward lean slews at the manual rate.
        let mut quick = RiderController::new(c);
        quick.set_manual(true);
        quick.set_intent(Intent::Lean(0.2));
        quick.ankle_torque(&sense(0.0));
        assert!((quick.lean_command() - c.manual_lean_rate_deg_s.to_radians() * 0.001).abs() < 1e-12);
        assert!(c.manual_lean_rate_deg_s > c.lean_rate_deg_s);
    }

    #[test]
    fn scripted_turn_at_rest_does_not_spin_the_wheel_or_wind_up_the_heading() {
        // Scripted riding only; by hand the rider turns at a stop (the test above).
        let mut rider = RiderController::new(RiderControl::default());
        rider.set_turn_rate(0.5);
        let stopped = sense(0.0);
        for _ in 0..1000 {
            rider.ankle_torque(&stopped);
            let steer = rider.steer(&stopped);
            assert_eq!(steer.yaw_nm, 0.0, "scripted turning needs forward or backward motion");
            assert_eq!(steer.roll, 0.0);
        }
        assert_eq!(rider.line(), 0.0, "a stopped rider must not store a future turn");
    }

    #[test]
    fn manual_turn_obeys_command_delay_and_clears_with_input() {
        let mut rider = RiderController::new(RiderControl { command_delay_s: 0.01, ..RiderControl::default() });
        rider.set_turn_rate(0.5);
        let moving = Sense { dt: crate::TIMESTEP_S, ..sense(5.0) };
        for _ in 0..20 {
            rider.ankle_torque(&moving);
            let steer = rider.steer(&moving);
            assert_eq!(steer.yaw_nm, 0.0, "turn feed-forward must wait for the command delay");
            assert_eq!(steer.roll, 0.0);
        }
        rider.ankle_torque(&moving);
        assert!(rider.steer(&moving).roll < 0.0);
        rider.clear_input(0.0).unwrap();
        rider.ankle_torque(&moving);
        assert_eq!(rider.steer(&moving).yaw_nm, 0.0, "clear_input must discard queued turns");
    }

    #[test]
    fn high_speed_keeps_yaw_rate_damping_against_wobble() {
        let control = RiderControl::default();
        let mut rider = RiderController::new(control);
        let wobble = Sense { yaw_rate: 0.1, ..sense(20.0) };
        assert_eq!(rider.steer(&wobble).yaw_nm, -control.steer_kd * 0.1);
    }

    #[test]
    fn holds_still_without_winding_up_on_the_stand() {
        let mut rider = RiderController::new(RiderControl::default());
        rider.set_intent(Intent::Speed(1.0));
        let held = Sense { held: true, ..sense(0.0) };
        for _ in 0..10_000 {
            rider.ankle_torque(&held);
        }
        assert_eq!(rider.integral, 0.0);
    }
}
