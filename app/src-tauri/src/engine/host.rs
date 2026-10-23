//! What the runner drives: a deterministic simulation that steps in simulated time.
//!
//! [`Host`] is the seam between the studio and the engine. [`RideHost`] rides a scenario on an
//! [`euc_sim::Session`]: the firmware on the emulated board, the electrical plant and the MuJoCo
//! body. A failed physical model stops the run.
//! [`BoardWatch`] reads the board: BLE frames, boot milestones, display, LEDs
//! and buzzer.

use euc_devices::Buzzer;
use euc_proto_begode::{Assembler, Segment, Telemetry};
use euc_sim::scenario::{Ride, Scenario};
use euc_sim::{Machine, Manual, Report, Session};
use euc_telemetry::{BodyInfo, Event, EventKind, Outputs, Snapshot};
use serde::Serialize;

/// A simulation the runner can step. Nothing in it reads the wall clock or blocks. It is built
/// and used on the sim thread only (CPU backends need not be `Send`).
pub trait Host {
    /// Runs `sim_s` more simulated seconds.
    fn step(&mut self, sim_s: f64);
    /// Simulated seconds since power-on.
    fn time_s(&self) -> f64;
    /// The bodies whose poses snapshots carry, in order.
    fn bodies(&self) -> Vec<BodyInfo>;
    /// The current state.
    fn snapshot(&self, out: &mut Snapshot);
    /// Events since the last call.
    fn drain_events(&mut self, out: &mut Vec<Event>);
    /// BLE bytes the firmware sent since the last call, with their simulated time.
    fn drain_ble_tx(&mut self, out: &mut Vec<(f64, u8)>);
    /// Sends bytes to the firmware's BLE module now.
    fn inject_ble(&mut self, bytes: &[u8]) -> Result<(), String>;
    /// Sends a bounded physical GPIO pulse; firmware interprets its gesture.
    fn pulse_button(&mut self, _button: &str, _duration_ms: u32) -> Result<(), String> {
        Err("This host does not expose physical buttons".into())
    }
    /// Applies a live physical GPIO state at the current simulated cycle.
    fn set_button(&mut self, _button: &str, _pressed: bool) -> Result<(), String> {
        Err("This host does not expose physical buttons".into())
    }
    /// Releases desktop input pulses and live holds without running firmware.
    fn release_button_pulses(&mut self) {}
    /// Initial power hold finished and the latch is currently high.
    fn buttons_ready(&self) -> bool {
        false
    }
    /// Whether Power can start or renew a hold, including waking an unpowered controller.
    fn power_button_ready(&self) -> bool {
        self.buttons_ready()
    }
    /// Active power and handle pulses, respectively.
    fn button_pulses(&self) -> [bool; 2] {
        [false; 2]
    }
    /// Why the run stopped for good, if it did.
    fn halted(&self) -> Option<String>;
    /// The declared scenario duration ended and its checks were judged.
    fn completed(&self) -> bool {
        false
    }
    /// Clears manual requests without changing the measured body pose.
    fn manual_pause(&mut self) {}
    /// The interactive host asks the runner to pause at a control or map boundary.
    fn wants_pause(&self) -> bool {
        false
    }
    /// The host is still booting on its stand, with nothing yet for a person to see or do: the
    /// runner steps it as fast as it can instead of at the wall-clock rate.
    fn booting(&self) -> bool {
        false
    }
    /// What the board shows and sounds (display, LEDs, buzzer), if the host models it.
    fn outputs(&self) -> Option<Outputs> {
        None
    }
    /// Buzzer changes since the last call, at their exact simulated time.
    fn drain_tones(&mut self, _out: &mut Vec<Tone>) {}
    /// What the ride sounds like at this sample (motor, tyre, wind), from `snapshot` and the
    /// host's own state; None for a host whose rides make no sound but the buzzer.
    fn ride_sound(&self, _snapshot: &Snapshot) -> Option<super::native_audio::Ride> {
        None
    }
    /// Takes manual control of the rider (keyboard, gamepad), or gives it back (`None`). A host
    /// without a rider ignores it.
    fn set_manual(&mut self, _manual: Option<Manual>) {}
    /// The rider steps off the pedals.
    fn step_off(&mut self) {}
    /// Exclusive simulated interrupt-cycle counters.
    fn handler_counters(&self) -> Option<super::tune::HandlerCounters> {
        None
    }
    /// Named numeric firmware probes, when this host owns a mapped Session.
    fn firmware_var_names(&self) -> Option<Vec<String>> {
        None
    }
    /// Reads one named value on the simulation thread, without advancing it.
    fn firmware_value(&self, _name: &str) -> Option<f64> {
        None
    }
}

/// A buzzer change: from `t_s` on, the buzzer sounds `hz` at `duty` of the period high (`hz` 0:
/// silent). The duty sets the loudness; the firmware's beeper volume is the compare value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Tone {
    pub t_s: f64,
    pub hz: f64,
    pub duty: f64,
}

/// Boot markers are watched for the machine's first few seconds; reports stop after that (a
/// report walks the whole run log, so it gets slower as the run grows).
const REPORT_UNTIL_S: f64 = 6.0;

/// What the studio reads from the board, whichever plant it runs with.
pub(super) struct BoardWatch {
    hclk: f64,
    ble_read: usize,
    assembler: Assembler,
    /// The firmware's last Live frame.
    live: Option<euc_proto_begode::Live>,
    first_frame: bool,
    events: Vec<Event>,
    reported_markers: usize,
    next_report_s: f64,
    buzzing: bool,
    /// Buzzer changes read so far, and the new ones.
    buzzer_seen: u64,
    tones: Vec<Tone>,
    halted: Option<String>,
}

impl BoardWatch {
    pub(super) fn new(machine: &Machine) -> Self {
        Self {
            hclk: machine.hclk() as f64,
            ble_read: 0,
            assembler: Assembler::new(),
            live: None,
            first_frame: false,
            events: Vec::new(),
            reported_markers: 0,
            next_report_s: 0.5,
            buzzing: false,
            buzzer_seen: 0,
            tones: Vec::new(),
            halted: None,
        }
    }

    pub(super) fn time_s(&self, machine: &Machine) -> f64 {
        machine.cycles as f64 / self.hclk
    }

    /// Turns what changed on the board since the last call into events.
    pub(super) fn after_step(&mut self, machine: &Machine) {
        let now = self.time_s(machine);
        if self.halted.is_none()
            && let Some(why) = machine.halted()
        {
            self.events.push(Event { t_s: now, kind: EventKind::Fault, text: format!("Halted: {why}") });
            self.halted = Some(why.to_owned());
        }
        if now >= self.next_report_s && self.next_report_s <= REPORT_UNTIL_S {
            self.next_report_s = now + 1.0;
            self.collect_report(machine);
        }
        // Every buzzer change at its exact time; a start is worth a timeline entry (the `b` horn
        // beeps twice).
        let Some(buzzer) = machine.component::<Buzzer>("buzzer") else { return };
        for change in buzzer.changes_since(self.buzzer_seen) {
            let t_s = change.at as f64 / self.hclk;
            let (hz, duty) = change.tone.map_or((0.0, 0.0), |pwm| (pwm.frequency_hz(), pwm.duty()));
            if hz > 0.0 && !self.buzzing {
                let text = format!("Beep, {:.1} kHz", hz / 1000.0);
                self.events.push(Event { t_s, kind: EventKind::Firmware, text });
            }
            self.buzzing = hz > 0.0;
            self.tones.push(Tone { t_s, hz, duty });
        }
        self.buzzer_seen = buzzer.changes();
    }

    /// Turns the boot markers the machine logged into events.
    fn collect_report(&mut self, machine: &Machine) {
        let report = Report::new(machine, &[]);
        let mut reached: Vec<_> = report.markers.iter().filter_map(|m| Some((m.first_ms?, m.name.clone()))).collect();
        reached.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (ms, name) in reached.iter().skip(self.reported_markers) {
            self.events.push(Event { t_s: ms / 1000.0, kind: EventKind::Firmware, text: format!("{name}() reached") });
        }
        self.reported_markers = reached.len();
    }

    pub(super) fn drain_ble_tx(&mut self, machine: &Machine, out: &mut Vec<(f64, u8)>) {
        let tx = &machine.board().ble_tx;
        let new = &tx[self.ble_read.min(tx.len())..];
        self.ble_read = tx.len();
        self.read_ble(new, machine, out);
    }

    /// As [`Self::drain_ble_tx`], but takes the bytes out of the board, so that a run without an
    /// end (Free Ride) does not keep them all. A host uses one of the two, never both.
    pub(super) fn take_ble_tx(&mut self, machine: &mut Machine, out: &mut Vec<(f64, u8)>) {
        let new = machine.take_ble_tx();
        self.ble_read = 0;
        self.read_ble(&new, machine, out);
    }

    fn read_ble(&mut self, new: &[(u64, u8)], machine: &Machine, out: &mut Vec<(f64, u8)>) {
        let bytes: Vec<u8> = new.iter().map(|&(_, b)| b).collect();
        out.extend(new.iter().map(|&(cycle, b)| (cycle as f64 / self.hclk, b)));
        self.assembler.push(&bytes);
        while let Some(segment) = self.assembler.next_segment() {
            if let Segment::Frame(frame) = segment {
                if !self.first_frame {
                    self.first_frame = true;
                    self.events.push(Event {
                        t_s: self.time_s(machine),
                        kind: EventKind::Firmware,
                        text: "First telemetry frame".into(),
                    });
                }
                if let Telemetry::Live(live) = Telemetry::decode(&frame) {
                    self.live = Some(live);
                }
            }
        }
    }

    pub(super) fn outputs(machine: &Machine) -> Outputs {
        euc_sim::recording::outputs(machine)
    }

    pub(super) fn drain_events(&mut self, out: &mut Vec<Event>) {
        out.append(&mut self.events);
    }

    pub(super) fn drain_tones(&mut self, out: &mut Vec<Tone>) {
        out.append(&mut self.tones);
    }

    pub(super) fn halted(&self) -> Option<String> {
        self.halted.clone()
    }
}

/// A scenario ridden on a session by `euc_sim::scenario::Ride` (the same driver as `euc sim run`):
/// the rider follows the target speed, the stand lets go at the release time, faults and pushes
/// come on cue, and the checks are judged (and posted as events) when the scenario's time is up.
/// The run stops after its declared duration.
pub struct RideHost {
    session: Session,
    scenario: Scenario,
    ride: Ride,
    watch: BoardWatch,
    bodies: Vec<BodyInfo>,
    judged: bool,
}

impl RideHost {
    /// Rides `scenario` on `session` (built for it, at power-on).
    pub fn new(mut session: Session, scenario: Scenario) -> Result<Self, String> {
        let ride = Ride::new(&scenario, &mut session).map_err(|e| format!("scenario {}: {e}", scenario.name))?;
        Ok(Self {
            watch: BoardWatch::new(session.machine()),
            bodies: session.bodies().to_vec(),
            session,
            scenario,
            ride,
            judged: false,
        })
    }

    fn ride_sample(&mut self) {
        self.ride.sample(&self.scenario, &mut self.session);
        self.watch.after_step(self.session.machine());
        if !self.judged && self.watch.halted.is_none() && self.ride.finished(&self.scenario, &self.session) {
            self.judge();
        }
    }

    /// Posts the scenario's checks as events.
    fn judge(&mut self) {
        self.judged = true;
        let outcome = self.ride.outcome(&self.scenario);
        let t_s = self.session.time_s();
        for check in &outcome.checks {
            let (kind, verdict) =
                if check.passed { (EventKind::Scenario, "passed") } else { (EventKind::Fault, "failed") };
            self.watch.events.push(Event {
                t_s,
                kind,
                text: format!("Check {verdict}: {} ({})", check.what, check.measured),
            });
        }
        for phase in &outcome.phases {
            let value =
                |v: Option<f64>, unit: &str| v.map_or_else(|| "unavailable".into(), |n| format!("{n:.2} {unit}"));
            self.watch.events.push(Event {
                t_s,
                kind: if phase.status == euc_sim::scenario::PhaseStatus::Reached {
                    EventKind::Scenario
                } else {
                    EventKind::Fault
                },
                text: format!(
                    "Phase {} {:?}{}: elapsed {}, forward distance {}, end speed {}, peak phase current {}",
                    phase.name,
                    phase.status,
                    phase.reason.as_ref().map_or(String::new(), |reason| format!(" ({reason})")),
                    value(phase.elapsed_s, "s"),
                    value(phase.distance_m, "m"),
                    value(phase.end_speed_kmh, "km/h"),
                    value(phase.max_abs_phase_current_a, "A"),
                ),
            });
        }
        let passed = outcome.checks.iter().filter(|c| c.passed).count();
        let (kind, verdict) =
            if outcome.passed() { (EventKind::Scenario, "passed") } else { (EventKind::Fault, "failed") };
        self.watch.events.push(Event {
            t_s,
            kind,
            text: format!("Scenario {} {verdict}: {passed} of {} checks", self.scenario.name, outcome.checks.len()),
        });
    }
}

impl Host for RideHost {
    fn handler_counters(&self) -> Option<super::tune::HandlerCounters> {
        self.session.machine().isr_metrics_available().then(|| super::tune::handler_counters(self.session.machine()))
    }

    fn completed(&self) -> bool {
        self.judged
    }

    fn firmware_var_names(&self) -> Option<Vec<String>> {
        Some(self.session.firmware_var_names())
    }

    fn firmware_value(&self, name: &str) -> Option<f64> {
        self.session.firmware_value(name)
    }

    fn step(&mut self, sim_s: f64) {
        let period = self.session.sample_period_s();
        let samples = (sim_s / period).round().max(1.0) as u32;
        for _ in 0..samples {
            if self.watch.halted.is_some() || self.judged {
                return;
            }
            self.ride_sample();
        }
    }

    fn time_s(&self) -> f64 {
        self.session.time_s()
    }

    fn bodies(&self) -> Vec<BodyInfo> {
        self.bodies.clone()
    }

    fn snapshot(&self, out: &mut Snapshot) {
        *out = self.session.snapshot();
        let s = &mut out.signals;
        s.target_speed_kmh = self.scenario.target_kmh(out.t_s) as f32;
    }

    fn drain_events(&mut self, out: &mut Vec<Event>) {
        // The session writes its notes in lower case ("stand released"); the timeline starts
        // every entry with a capital.
        out.extend(self.session.take_events().into_iter().map(|mut e| {
            if let Some(first) = e.text.get(..1) {
                e.text = first.to_uppercase() + &e.text[1..];
            }
            e
        }));
        self.watch.drain_events(out);
    }

    fn drain_ble_tx(&mut self, out: &mut Vec<(f64, u8)>) {
        self.watch.drain_ble_tx(self.session.machine(), out);
    }

    fn drain_tones(&mut self, out: &mut Vec<Tone>) {
        self.watch.drain_tones(out);
    }

    fn inject_ble(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.session.machine_mut().inject_ble(bytes);
        Ok(())
    }

    fn pulse_button(&mut self, button: &str, duration_ms: u32) -> Result<(), String> {
        self.session.machine_mut().pulse_button(button, duration_ms)?;
        self.session.log(EventKind::Command, format!("Physical {button} button pressed for {duration_ms} ms"));
        Ok(())
    }

    fn release_button_pulses(&mut self) {
        self.session.machine_mut().release_button_pulses();
    }

    fn set_button(&mut self, button: &str, pressed: bool) -> Result<(), String> {
        self.session.machine_mut().set_button(button, pressed)
    }

    fn buttons_ready(&self) -> bool {
        self.session.machine().board().buttons_ready()
    }

    fn power_button_ready(&self) -> bool {
        self.session.machine().board().power_button_ready()
    }

    fn button_pulses(&self) -> [bool; 2] {
        self.session.machine().board().button_pulses()
    }

    fn halted(&self) -> Option<String> {
        self.watch.halted()
    }

    fn outputs(&self) -> Option<Outputs> {
        Some(BoardWatch::outputs(self.session.machine()))
    }

    fn set_manual(&mut self, manual: Option<Manual>) {
        self.session.set_manual(manual);
    }

    fn step_off(&mut self) {
        self.session.step_off();
    }
}

#[cfg(test)]
#[path = "power_tests.rs"]
mod power_tests;

#[cfg(test)]
mod tests {
    use euc_sim::Manual;
    use euc_telemetry::Snapshot;

    use super::super::firmware::build_host;
    use super::Host;

    fn run(host: &mut dyn Host, seconds: f64) -> Snapshot {
        for _ in 0..(seconds * 120.0).round() as u32 {
            host.step(1.0 / 120.0);
        }
        let mut snap = Snapshot::default();
        host.snapshot(&mut snap);
        snap
    }

    /// The studio's keyboard on the stock firmware: lean forward and the wheel rides off, turn
    /// and it turns, lean back and it stops, give the rider back and the scenario's stand-still
    /// holds.
    #[test]
    #[ignore = "needs the stock image in the local firmware cache"]
    fn manual_lean_rides_the_stock_firmware() {
        let mut built = build_host("balance-60s", "stock", &mut |_| {}).expect("host");
        let host = built.host.as_mut();
        run(host, 4.0);
        host.set_manual(Some(Manual { lean: 0.6, steer: 0.0 }));
        let fast = run(host, 3.0).signals.speed_kmh;
        // A full left turn at a gentle lean: the shell's heading swings left.
        host.set_manual(Some(Manual { lean: 0.2, steer: 1.0 }));
        let yaw = |snap: &Snapshot| {
            let [w, x, y, z] = snap.bodies[1].quat.map(f64::from);
            (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z))
        };
        let turned = yaw(&run(host, 1.5));
        host.set_manual(Some(Manual { lean: -0.6, steer: 0.0 }));
        let mut slow = f32::MAX;
        for _ in 0..30 {
            slow = slow.min(run(host, 0.1).signals.speed_kmh.abs());
        }
        host.set_manual(None);
        let after = run(host, 4.0).signals.speed_kmh;
        let mut events = Vec::new();
        host.drain_events(&mut events);
        println!("fast {fast:.1} km/h, turned {turned:.2} rad, slowest {slow:.1} km/h, after {after:.1} km/h");
        assert!(turned > 0.5, "a full left turn for 1.5 s turns left: {turned} rad");
        assert!(host.halted().is_none());
        assert!(fast > 4.0, "leaning forward rides off: {fast}");
        assert!(slow < fast / 2.0, "leaning back brakes: {slow}");
        assert!(after.abs() < 2.0, "the scenario stands still again: {after}");
        let texts: Vec<_> = events.iter().map(|e| e.text.as_str()).collect();
        assert!(texts.contains(&"Manual control") && texts.contains(&"Control back to the scenario"), "{texts:?}");
    }
}
