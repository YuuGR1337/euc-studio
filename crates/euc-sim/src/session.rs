//! A running simulation with a live plant: what the studio, recordings and scenarios drive.
//!
//! A [`Session`] owns a [`Machine`] built with a live plant. It advances in fixed sample periods
//! and, after each, reports a [`Snapshot`] (signals and body poses) and any [`Event`]s: stand
//! released, cutoffs, falls, firmware resets and blockers.

use euc_physics::{Intent, Leaving};
use euc_telemetry::{BodyInfo, Event, EventKind, Signals, Snapshot};

use crate::config::{ROLE_CUTOFF, StockMap};
use crate::machine::Machine;
use crate::plant::Live;

/// The firmware's tiltback offsets (stock map and probe names), degrees, nose up positive: what
/// the rider feels as tiltback when the firmware's map names them.
const TILTBACK_VARS: [&str; 3] = ["tiltback_speed_deg", "tiltback_battery_deg", "tiltback_pwm_deg"];

/// The firmware's commanded q voltage (stock map and probe name), in Q15 of Vbus/√3. Its app
/// frame (Begode frame 07) reports it as PWM: `-vq / 315` in whole percent, so 31 500 is 100 %.
const PWM_VAR: &str = "vq_active";
/// Q15 counts of the firmware's q voltage per percent of its PWM figure.
const Q15_PER_PWM_PERCENT: f64 = 315.0;

/// Manual control: how fast a full turn input turns the rider's line, rad/s (about 46°/s).
const MANUAL_TURN_RAD_S: f64 = 0.8;

/// Manual control of the rider (keyboard or gamepad in the studio): how far to lean and turn, as
/// shares of what the rider can do. While set it replaces what a scenario asks for.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Manual {
    /// Lean, -1 (full back) to 1 (full forward), of the rider's lean limit.
    pub lean: f64,
    /// Turn, -1 (right) to 1 (left), of the studio's full turn rate (0.8 rad/s).
    pub steer: f64,
}

/// A firmware variable the session watches.
#[derive(Debug, Clone)]
struct Watched {
    name: String,
    addr: u32,
    size: u32,
    /// The stock map's `kind`: `u8`, `s16`, `f32`...
    kind: String,
}

/// A simulation with a live plant. See the module docs.
pub struct Session {
    machine: Machine,
    hclk: u64,
    sample_cycles: u64,
    isr_load_pct: f32,
    bodies: Vec<BodyInfo>,
    cutoff_vars: Vec<Watched>,
    /// Every RAM variable of the stock map, for checks that read one.
    ram: Vec<Watched>,
    cutoff: Option<String>,
    fallen: bool,
    /// The rider's leaving the pedals is on the timeline.
    rider_off_logged: bool,
    seen_events: usize,
    events: Vec<Event>,
    /// What the scenario asks the rider for, kept while manual control overrides it.
    scenario_intent: Intent,
    manual: Option<Manual>,
}

impl Session {
    /// A session on `machine`, which must have a live plant, sampling at `sample_hz`. With the
    /// stock map of the running build, the session watches the firmware's cutoff flags.
    ///
    /// # Panics
    ///
    /// If the machine's plant is static.
    pub fn new(machine: Machine, stockmap: Option<&StockMap>, sample_hz: f64) -> Self {
        let hclk = machine.hclk();
        let bodies = machine.plant().live_part().expect("a session needs a live plant").physics().bodies();
        let watched = |v: &crate::config::RamVar| Watched {
            name: v.name.clone(),
            addr: v.addr,
            size: v.size,
            kind: v.kind.clone(),
        };
        let cutoff_vars = stockmap
            .map(|map| map.ram.iter().filter(|v| v.role.as_deref() == Some(ROLE_CUTOFF)).map(watched).collect())
            .unwrap_or_default();
        let ram = stockmap.map(|map| map.ram.iter().map(watched).collect()).unwrap_or_default();
        Self {
            machine,
            hclk,
            sample_cycles: ((hclk as f64 / sample_hz).round() as u64).max(1),
            isr_load_pct: f32::NAN,
            bodies,
            cutoff_vars,
            ram,
            cutoff: None,
            fallen: false,
            rider_off_logged: false,
            seen_events: 0,
            events: Vec::new(),
            scenario_intent: Intent::Speed(0.0),
            manual: None,
        }
    }

    /// The machine.
    pub const fn machine(&self) -> &Machine {
        &self.machine
    }

    /// The machine, mutably: to send BLE bytes while the session runs.
    pub const fn machine_mut(&mut self) -> &mut Machine {
        &mut self.machine
    }

    /// Simulated seconds since power-on.
    pub fn time_s(&self) -> f64 {
        self.machine.cycles as f64 / self.hclk as f64
    }

    /// Seconds between two samples.
    pub fn sample_period_s(&self) -> f64 {
        self.sample_cycles as f64 / self.hclk as f64
    }

    /// The bodies the snapshots carry poses for, in order.
    pub fn bodies(&self) -> &[BodyInfo] {
        &self.bodies
    }

    fn live(&self) -> &Live {
        self.machine.plant().live_part().expect("a session has a live plant")
    }

    fn live_mut(&mut self) -> &mut Live {
        self.machine.plant_mut().live_part_mut().expect("a session has a live plant")
    }

    /// Changes what the rider wants (under manual control: once the control is given back).
    pub fn set_intent(&mut self, intent: Intent) {
        self.scenario_intent = intent;
        if self.manual.is_none() {
            self.live_mut().physics_mut().rider().set_intent(intent);
        }
    }

    /// Takes manual control of the rider (`Some`), or gives it back to whatever set the intent
    /// (`None`). Lean and turn are clamped to -1..1.
    pub fn set_manual(&mut self, manual: Option<Manual>) {
        let manual = manual.map(|m| Manual { lean: m.lean.clamp(-1.0, 1.0), steer: m.steer.clamp(-1.0, 1.0) });
        match (self.manual.is_some(), manual.is_some()) {
            (false, true) => self.push(EventKind::Scenario, "manual control".to_owned()),
            (true, false) => self.push(EventKind::Scenario, "control back to the scenario".to_owned()),
            _ => {}
        }
        self.manual = manual;
        let lean_max = self.live().physics().setup().rider.control.lean_max_deg.to_radians();
        let intent = manual.map_or(self.scenario_intent, |m| Intent::Lean(m.lean * lean_max));
        let rider = self.live_mut().physics_mut().rider();
        rider.set_intent(intent);
        // Given back, the rider keeps the heading reached (scenarios ride straight on from there).
        rider.set_turn_rate(manual.map_or(0.0, |m| m.steer * MANUAL_TURN_RAD_S));
    }

    /// Manual control, if it is on.
    pub const fn manual(&self) -> Option<Manual> {
        self.manual
    }

    /// What the rider wants.
    pub fn intent(&mut self) -> Intent {
        self.live_mut().physics_mut().rider().intent()
    }

    /// Pushes the rider along the heading, newtons, until changed (see `Physics::set_push`).
    pub fn set_push(&mut self, newtons: f64) {
        self.live_mut().physics_mut().set_push(newtons);
    }

    /// Lets the wheel off its stand (or puts it back).
    pub fn set_held(&mut self, held: bool) {
        if self.live().physics().held() != held {
            let text = if held { "stand holds the wheel" } else { "stand released" };
            self.push(EventKind::Scenario, text.to_owned());
        }
        self.live_mut().physics_mut().set_held(held);
    }

    /// The rider steps off the pedals (see `euc_physics::Physics::step_off`).
    pub fn step_off(&mut self) {
        if !self.live().physics().rider_off() {
            self.push(EventKind::Scenario, "rider steps off".to_owned());
            self.live_mut().physics_mut().step_off();
        }
    }

    /// Records a scenario step on the timeline.
    pub fn note(&mut self, text: impl Into<String>) {
        self.push(EventKind::Scenario, text.into());
    }

    /// Records an event of any kind on the timeline, at the current time.
    pub fn log(&mut self, kind: EventKind, text: impl Into<String>) {
        self.push(kind, text.into());
    }

    /// Drive unavailable: a firmware cutoff name, or `controller_supply_off` while unpowered.
    pub fn cutoff(&self) -> Option<&str> {
        self.cutoff.as_deref()
    }

    /// Current stock-map cutoff flags, or the model reason `controller_supply_off`.
    pub fn cutoff_flags(&self) -> Vec<String> {
        if !self.machine.powered() {
            return vec!["controller_supply_off".into()];
        }
        let set =
            |v: &&Watched| self.machine.memory(v.addr, v.size as usize).is_some_and(|b| b.iter().any(|&x| x != 0));
        self.cutoff_vars.iter().filter(set).map(|v| v.name.clone()).collect()
    }

    /// The stock-map RAM variable `name` now, little-endian, as an unsigned number of up to
    /// 8 bytes; `None` without power, without a stock map, or for an unknown name.
    pub fn firmware_var(&self, name: &str) -> Option<u64> {
        if !self.machine.powered() {
            return None;
        }
        let var = self.ram.iter().find(|v| v.name == name)?;
        let bytes = self.machine.memory(var.addr, var.size.min(8) as usize)?;
        Some(bytes.iter().rev().fold(0, |value, &b| value << 8 | u64::from(b)))
    }

    /// Names of the firmware's numeric RAM variables (not text), as its map gives them.
    pub fn firmware_var_names(&self) -> Vec<String> {
        self.ram.iter().filter(|v| v.kind != "ascii").map(|v| v.name.clone()).collect()
    }

    /// The stock-map RAM variable `name` now as a number, read as its `kind` says: `s8`, `s16`,
    /// `s32` signed, `f32` floating point, anything else unsigned.
    pub fn firmware_value(&self, name: &str) -> Option<f64> {
        let raw = self.firmware_var(name)?;
        let var = self.ram.iter().find(|v| v.name == name)?;
        let bits = 8 * var.size.min(8);
        Some(match var.kind.as_str() {
            "f32" => f64::from(f32::from_bits(raw as u32)),
            "s8" | "s16" | "s32" => ((raw << (64 - bits)) as i64 >> (64 - bits)) as f64,
            _ => raw as f64,
        })
    }

    /// Runs one sample period and returns the snapshot at its end.
    pub fn step(&mut self) -> Snapshot {
        self.feel_tiltback();
        let cut = !self.machine.powered() || self.cutoff.is_some();
        self.live_mut().physics_mut().set_drive_cut(cut);
        let cycle_start = self.machine.cycles;
        let isr_start = self.machine.isr_cycles();
        self.machine.run(self.sample_cycles);
        let elapsed = self.machine.cycles - cycle_start;
        self.isr_load_pct = if elapsed == 0 || !self.machine.isr_metrics_available() {
            f32::NAN
        } else {
            (100.0 * (self.machine.isr_cycles() - isr_start) as f64 / elapsed as f64).clamp(0.0, 100.0) as f32
        };
        self.watch();
        self.snapshot()
    }

    /// Tells the rider the firmware's tiltback, the sum of its nose-up offsets, when the map names
    /// any of them; without them the rider judges the tiltback by the pedals.
    fn feel_tiltback(&mut self) {
        let offsets: Vec<f64> =
            TILTBACK_VARS.iter().filter_map(|v| self.firmware_value(v)).filter(|d| d.is_finite()).collect();
        let felt = (!offsets.is_empty()).then(|| offsets.iter().map(|d| d.max(0.0)).sum::<f64>().to_radians());
        self.live_mut().physics_mut().rider().set_commanded_tiltback(felt);
    }

    /// The state now.
    pub fn snapshot(&self) -> Snapshot {
        let live = self.live();
        let body = live.physics().state();
        let plant = live.electrical().snapshot();
        let phase = plant.phase_current.map(|i| i as f32);
        // As the frame carries it: C integer division (truncation), then the magnitude.
        let pwm = self.firmware_value(PWM_VAR).map_or(f32::NAN, |vq| (-vq / Q15_PER_PWM_PERCENT).trunc().abs() as f32);
        let full_modulation = plant.bus_v / 3f64.sqrt() * Q15_PER_PWM_PERCENT * 100.0 / 32768.0;
        let modulation = plant.voltage_dq.d.hypot(plant.voltage_dq.q) / full_modulation * 100.0;
        let signals = Signals {
            speed_kmh: (body.speed_mps * 3.6) as f32,
            wheel_rad_s: body.wheel_rad_s as f32,
            pwm_pct: pwm,
            phase_a_a: phase[0],
            phase_b_a: phase[1],
            phase_c_a: phase[2],
            // Forward torque positive, like the torque channel (the motor's own q axis may run the
            // other way).
            iq_a: (plant.current_dq.q * f64::from(live.electrical().params().motor.direction)) as f32,
            torque_nm: plant.wheel_torque as f32,
            battery_v: plant.bus_v as f32,
            battery_a: plant.bus_a as f32,
            soc_pct: (plant.soc * 100.0) as f32,
            isr_load_pct: self.isr_load_pct,
            pitch_deg: body.pitch_rad.to_degrees() as f32,
            roll_deg: body.roll_rad.to_degrees() as f32,
            board_temp_c: plant.stage_c as f32,
            motor_temp_c: plant.winding_c as f32,
            distance_m: body.distance_m as f32,
            rider_lean_deg: body.rider_lean_rad.to_degrees() as f32,
            suspension_mm: (body.suspension_m * 1000.0) as f32,
            shock_mm: (body.shock_m * 1000.0) as f32,
            modulation_pct: if plant.bus_v > 0.0 { modulation as f32 } else { f32::NAN },
            ..Signals::default()
        };
        Snapshot { t_s: self.time_s(), signals, bodies: live.physics().poses() }
    }

    /// Events since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    fn push(&mut self, kind: EventKind, text: String) {
        self.events.push(Event { t_s: self.time_s(), kind, text });
    }

    /// Turns what changed in the last period into events.
    fn watch(&mut self) {
        let cutoff = if self.machine.powered() {
            self.cutoff_vars.iter().find_map(|v| {
                let bytes = self.machine.memory(v.addr, v.size as usize)?;
                bytes.iter().any(|&b| b != 0).then(|| v.name.clone())
            })
        } else {
            Some("controller_supply_off".into())
        };
        if cutoff != self.cutoff {
            match &cutoff {
                Some(name) if name == "controller_supply_off" => {
                    self.push(EventKind::Cutoff, "controller supply is off".into());
                }
                Some(name) => self.push(EventKind::Cutoff, format!("firmware cut the motor ({name})")),
                None if self.cutoff.as_deref() == Some("controller_supply_off") => self
                    .push(EventKind::Cutoff, "controller supply restored; motor drive follows firmware output".into()),
                None => self.push(EventKind::Cutoff, "firmware drives the motor again".to_owned()),
            }
            self.cutoff = cutoff;
        }
        let fallen = self.live().physics().state().fallen;
        if fallen && !self.fallen {
            self.push(EventKind::Fault, "the wheel fell over".to_owned());
        }
        self.fallen = fallen;
        if let Some((t_s, how)) = self.live().physics().left_pedals()
            && !self.rider_off_logged
        {
            self.rider_off_logged = true;
            let text = match how {
                Leaving::SteppedOff => None,
                Leaving::LetGoAfterCut => Some("rider off: lets go of the pedals (drive cut, the wheel tips)"),
                Leaving::LetGoFalling => Some("rider off: lets go of the pedals (the wheel falls)"),
                Leaving::LostFooting => Some("rider off: both feet off the pedals (the wheel stays up)"),
            };
            if let Some(text) = text {
                self.events.push(Event { t_s, kind: EventKind::Fault, text: text.to_owned() });
            }
        }
        let hclk = self.hclk as f64;
        let new: Vec<Event> = self.machine.sys.diag.events[self.seen_events..]
            .iter()
            .filter(|(_, text)| {
                text.starts_with("BLOCKER")
                    || text.contains("RESET")
                    || text.contains("POWER CYCLE")
                    || text.starts_with("CONTROLLER POWER")
            })
            .map(|(cycle, text)| {
                let kind = if text.starts_with("BLOCKER") { EventKind::Fault } else { EventKind::Firmware };
                Event { t_s: *cycle as f64 / hclk, kind, text: text.clone() }
            })
            .collect();
        self.seen_events = self.machine.sys.diag.events.len();
        self.events.extend(new);
    }
}

#[cfg(test)]
mod tests {
    use euc_physics::{Assist, Intent, Rider, RiderControl, TIMESTEP_S, Terrain};
    use euc_profile::ProfileSource;

    use super::{Manual, Session};
    use crate::{Machine, MachineConfig, PlantState, WheelFiles};

    /// The Extreme with its live plant and an image that only spins: enough to steer the rider.
    fn session() -> Session {
        let files = WheelFiles::load(&ProfileSource::Builtin, "begode-extreme").unwrap();
        let mut image = vec![0_u8; 0x200];
        let base = files.profile.board.app.base;
        image[..4].copy_from_slice(&0x2000_1000_u32.to_le_bytes());
        image[4..8].copy_from_slice(&(base + 0x101).to_le_bytes());
        image[0x100..0x102].copy_from_slice(&0xE7FE_u16.to_le_bytes()); // b .
        let config = MachineConfig {
            board: &files.board,
            stockmap: None,
            image: &image,
            load_addr: base,
            hclk: files.hclk(),
            plant: PlantState::default(),
            injections: Vec::new(),
            power_cycles: Vec::new(),
            cpi: crate::DEFAULT_CPI,
            exception_timing: crate::ExceptionTiming::default(),
            max_quantum: 4000,
            marker_window_s: 0.0,
        };
        let rider =
            Rider { control: RiderControl { command_delay_s: 0.02, ..RiderControl::default() }, ..Rider::default() };
        let live = files.live_on(rider, Assist::default(), Terrain::default()).unwrap();
        let factory = crate::cpu::Backend::Own.factory().unwrap();
        Session::new(Machine::new_live(&config, &crate::registry(), factory, live).unwrap(), None, 100.0)
    }

    #[test]
    fn manual_control_overrides_the_scenario_and_gives_it_back() {
        let mut s = session();
        s.set_intent(Intent::Speed(2.0));
        assert_eq!(s.intent(), Intent::Speed(2.0));
        let lean_max = s.live().physics().setup().rider.control.lean_max_deg.to_radians();

        // This image has no balance loop. Prepare actual forward motion with the physics test
        // balance law, without replacing any pose or velocity. Manual turns need a moving wheel.
        s.set_held(false);
        let physics = s.live_mut().physics_mut();
        for _ in 0..(12.0 / TIMESTEP_S).round() as usize {
            let torque =
                (-2500.0 * physics.state().pitch_rad + 250.0 * physics.imu().gyro_rad_s[1]).clamp(-300.0, 300.0);
            physics.step(torque);
            if physics.state().speed_mps > 1.6 {
                break;
            }
        }
        assert!(physics.state().speed_mps > 1.6, "prepare real forward motion: {:?}", physics.state());
        let initial = physics.rider_state();
        s.take_events(); // The stand-release event belongs to fixture preparation.

        // Full forward (clamped from 3), then a scenario step that must not win.
        s.set_manual(Some(Manual { lean: 3.0, steer: -0.5 }));
        assert_eq!(s.manual(), Some(Manual { lean: 1.0, steer: -0.5 }));
        assert_eq!(s.intent(), Intent::Lean(lean_max));
        // The 20 ms command delay also applies to turns. One 10 ms sample cannot apply it yet.
        s.step();
        let delayed = s.live().physics().rider_state();
        assert!(
            (delayed.applied_heading_rad - initial.heading_rad).abs() < 1e-10,
            "turn keeps the measured heading until its command delay expires: {delayed:?}"
        );
        assert!(delayed.desired_heading_rad < initial.desired_heading_rad - 0.003);
        s.set_intent(Intent::Speed(5.0));
        assert_eq!(s.intent(), Intent::Lean(lean_max), "the scenario waits while the user rides");
        for _ in 0..9 {
            s.step();
        }
        let moving = s.live().physics().rider_state();
        assert!(moving.applied_heading_rad < initial.applied_heading_rad - 0.02, "delayed turn applies: {moving:?}");
        assert!(
            moving.heading_rad < initial.heading_rad - 0.001,
            "the moving wheel physically turns right: {moving:?}"
        );
        assert!(!s.live().physics().state().fallen && moving.attached);
        let turned = s.live_mut().physics_mut().rider().line();
        s.set_manual(Some(Manual { lean: -0.5, steer: 0.0 }));
        assert_eq!(s.intent(), Intent::Lean(-0.5 * lean_max));

        // Given back: the scenario's latest wish, at once.
        s.set_manual(None);
        assert_eq!(s.intent(), Intent::Speed(5.0));
        for _ in 0..4 {
            s.step();
        }
        assert_eq!(s.live_mut().physics_mut().rider().applied_intent(), Intent::Speed(5.0));
        assert_eq!(s.live_mut().physics_mut().rider().line(), turned, "keeps the heading reached");
        let notes: Vec<String> = s.take_events().into_iter().map(|e| e.text).collect();
        assert_eq!(notes, ["manual control", "control back to the scenario"]);
    }
}
