//! Manual rider inputs stay on the simulation thread. This module bounds their shared mailbox.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use euc_physics::map::{GeneratedMapSpec, HeightMap};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::host::{BoardWatch, Host, Tone};
use euc_physics::Intent;
use euc_sim::Session;
use euc_telemetry::{BodyInfo, Event, Outputs, Snapshot};

const INPUT_TIMEOUT: Duration = Duration::from_millis(250);
const NEUTRAL_TIME: Duration = Duration::from_millis(250);
const INPUT_TIMEOUT_REASON: &str = "Input timed out. Controls are disarmed";
const MAX_LEAN_RAD: f64 = 10.0 * std::f64::consts::PI / 180.0;
const MAX_TURN_RAD_S: f64 = 30.0 * std::f64::consts::PI / 180.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputSource {
    KeyboardSlider,
    Gamepad,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreeRideSpec {
    pub wheel: String,
    pub settings: String,
    pub rider_model: String,
    pub map: GeneratedMapSpec,
}

impl FreeRideSpec {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.wheel != "begode-extreme" {
            return Err("Free ride currently supports Begode Extreme".into());
        }
        self.rider()?;
        if self.settings.len() > 80 || self.settings.is_empty() {
            return Err("Select a settings preset".into());
        }
        self.map.validate()
    }

    pub(super) fn rider(&self) -> Result<euc_physics::Rider, String> {
        let model = match self.rider_model.as_str() {
            "rigid_ankle_v1" => euc_physics::RiderModel::RigidAnkleV1,
            "articulated_sagittal_v1" => euc_physics::RiderModel::ArticulatedSagittalV1,
            _ => return Err("Select a supported rider model".into()),
        };
        Ok(euc_physics::Rider { model, ..Default::default() })
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualInput {
    pub run_epoch: u64,
    pub seq: u32,
    pub source: InputSource,
    pub desired_lean_rad: f64,
    pub heading_rate_rad_s: f64,
    pub armed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FreeRideStatus {
    pub epoch: u64,
    pub mode: &'static str,
    pub ready: bool,
    pub armed: bool,
    pub source: Option<InputSource>,
    pub reason: String,
    pub desired_lean_deg: Option<f64>,
    pub commanded_lean_deg: Option<f64>,
    pub actual_lean_deg: Option<f64>,
    pub heading_deg: Option<f64>,
    pub path_length_m: Option<f64>,
    pub yaw_aid_work_j: Option<f64>,
    pub roll_aid_work_j: Option<f64>,
    pub rider_model: &'static str,
}

impl Default for FreeRideStatus {
    fn default() -> Self {
        Self {
            epoch: 0,
            mode: "scenario",
            ready: false,
            armed: false,
            source: None,
            reason: "Select Free ride to use manual controls".into(),
            desired_lean_deg: None,
            commanded_lean_deg: None,
            actual_lean_deg: None,
            heading_deg: None,
            path_length_m: None,
            yaw_aid_work_j: None,
            roll_aid_work_j: None,
            rider_model: "rigid_ankle_v1",
        }
    }
}

#[derive(Default)]
struct Inner {
    status: FreeRideStatus,
    map: Option<Arc<HeightMap>>,
    latest: Option<ManualInput>,
    last_seq: Option<u32>,
    received: Option<Instant>,
    neutral_since: Option<Instant>,
    next_start: u64,
    pending_start: Option<u64>,
    rider: euc_physics::Rider,
}

impl Inner {
    fn disarm(&mut self, reason: &str) {
        self.status.armed = false;
        self.status.reason = reason.into();
        self.latest = None;
        self.received = None;
        self.neutral_since = None;
    }

    fn input_expired(&self, now: Instant) -> bool {
        self.received.is_none_or(|at| now.saturating_duration_since(at) >= INPUT_TIMEOUT)
    }

    fn expire_input(&mut self, now: Instant) -> bool {
        if self.status.mode != "free_ride" || !self.status.armed || !self.input_expired(now) {
            return false;
        }
        self.disarm(INPUT_TIMEOUT_REASON);
        true
    }
}

#[derive(Default, Clone)]
pub struct FreeRideState(Arc<Mutex<Inner>>);

impl FreeRideState {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn recording_context(&self, epoch: u64) -> Result<Option<serde_json::Value>, String> {
        let inner = self.lock();
        if inner.status.mode != "free_ride" {
            return Ok(None);
        }
        if inner.status.epoch != epoch {
            return Err("Recording map belongs to an old run".into());
        }
        let map = inner.map.as_deref().cloned().ok_or("Free ride recording has no map")?;
        let rider = inner.rider;
        let assist = euc_physics::Assist::default();
        let context = euc_sim::recording::RecordingContext {
            schema: 1,
            run_mode: euc_sim::recording::RunMode::FreeRide,
            rider_model: match rider.model {
                euc_physics::RiderModel::RigidAnkleV1 => euc_sim::recording::RiderModel::RigidAnkleV1,
                euc_physics::RiderModel::ArticulatedSagittalV1 => euc_sim::recording::RiderModel::ArticulatedSagittalV1,
            },
            height_map: Some(map),
            model_aids: euc_sim::recording::ModelAids {
                roll_stiffness: assist.roll_stiffness,
                roll_damping: assist.roll_damping,
                steer_kp: rider.control.steer_kp,
                steer_kd: rider.control.steer_kd,
                steer_max_nm: rider.control.steer_max_nm,
            },
        };
        context.validate()?;
        serde_json::to_value(context).map(Some).map_err(|error| error.to_string())
    }

    pub(super) fn begin(&self, epoch: u64, map: Option<Arc<HeightMap>>, rider: euc_physics::Rider) {
        let mut inner = self.lock();
        let (next_start, pending_start) = (inner.next_start, inner.pending_start);
        *inner = Inner {
            status: FreeRideStatus {
                epoch,
                rider_model: rider.model.id(),
                mode: if map.is_some() { "free_ride" } else { "scenario" },
                reason: if map.is_some() {
                    "Starting firmware on the stand".into()
                } else {
                    "Scenario controls the rider".into()
                },
                ..FreeRideStatus::default()
            },
            map,
            rider,
            next_start,
            pending_start,
            ..Inner::default()
        };
    }

    fn reserve_start(&self) -> Result<u64, String> {
        let mut inner = self.lock();
        if inner.pending_start.is_some() {
            return Err("Wait for the current free ride build".into());
        }
        inner.next_start = inner.next_start.checked_add(1).ok_or("Restart the app before starting another run")?;
        let ticket = inner.next_start;
        inner.pending_start = Some(ticket);
        Ok(ticket)
    }

    pub(super) fn finish_start(&self, ticket: u64) {
        let mut inner = self.lock();
        if inner.pending_start == Some(ticket) {
            inner.pending_start = None;
        }
    }

    pub(super) fn disarm(&self, reason: &str) {
        self.lock().disarm(reason);
    }

    pub(super) fn check_timeout(&self) -> bool {
        let mut inner = self.lock();
        inner.expire_input(Instant::now())
    }

    pub(super) fn ready(&self, epoch: u64) {
        let mut inner = self.lock();
        if inner.status.epoch == epoch {
            inner.status.ready = true;
            inner.status.reason = "Ready. Keep controls neutral, then arm".into();
        }
    }

    pub(super) fn failed(&self, epoch: u64, reason: &str) {
        let mut inner = self.lock();
        if inner.status.epoch == epoch {
            inner.status.ready = false;
            inner.status.armed = false;
            inner.status.reason = reason.into();
            inner.latest = None;
        }
    }

    pub(super) fn input(&self, input: ManualInput) -> Result<(), String> {
        if !input.desired_lean_rad.is_finite()
            || input.desired_lean_rad.abs() > MAX_LEAN_RAD
            || !input.heading_rate_rad_s.is_finite()
            || input.heading_rate_rad_s.abs() > MAX_TURN_RAD_S
        {
            return Err("Manual lean or turn rate is outside its limits".into());
        }
        let mut inner = self.lock();
        if inner.status.mode != "free_ride" || inner.status.epoch != input.run_epoch {
            return Err("Manual input belongs to an old run".into());
        }
        if inner.last_seq.is_some_and(|seq| input.seq <= seq) {
            return Err("Manual input sequence is stale".into());
        }
        let now = Instant::now();
        if inner.expire_input(now) {
            // Discard the timeout-triggering packet, even if it claims to be neutral/unarmed.
            // A subsequent fresh neutral packet starts a new interval for explicit arming.
            return Err(INPUT_TIMEOUT_REASON.into());
        }
        if inner.input_expired(now) {
            inner.neutral_since = None;
        }
        if inner.status.source.is_some_and(|source| source != input.source) && inner.status.armed {
            return Err("Pause before changing the input source".into());
        }
        if input.armed && !inner.status.armed {
            return Err("Arm neutral controls before sending motion".into());
        }
        let neutral = input.desired_lean_rad.abs() < 1e-8 && input.heading_rate_rad_s.abs() < 1e-8;
        if inner.status.source != Some(input.source) {
            inner.neutral_since = None;
        }
        inner.status.source = Some(input.source);
        if !inner.status.armed && neutral {
            inner.neutral_since.get_or_insert(now);
        } else if !neutral {
            inner.neutral_since = None;
        }
        if !input.armed && inner.status.armed {
            inner.status.armed = false;
            inner.status.reason = "Controls disarmed".into();
        }
        inner.last_seq = Some(input.seq);
        inner.received = Some(now);
        inner.latest = Some(input);
        Ok(())
    }

    pub(super) fn arm(&self, epoch: u64, source: InputSource) -> Result<(), String> {
        let mut inner = self.lock();
        if inner.status.epoch != epoch || inner.status.mode != "free_ride" || !inner.status.ready {
            return Err("Free ride is not ready for this run".into());
        }
        if inner.status.source != Some(source)
            || inner.neutral_since.is_none_or(|at| at.elapsed() < NEUTRAL_TIME)
            || inner.input_expired(Instant::now())
        {
            return Err("Keep both controls neutral for 250 ms before arming".into());
        }
        inner.status.armed = true;
        inner.status.reason = "Manual controls armed. Rigid rider aids remain active".into();
        inner.received = Some(Instant::now());
        // Arming starts from the neutral packet. A later packet supplies motion.
        if let Some(latest) = inner.latest.as_mut() {
            latest.armed = true;
        }
        Ok(())
    }

    pub(super) fn pause_epoch(&self, epoch: u64) -> Result<(), String> {
        let mut inner = self.lock();
        if inner.status.epoch != epoch || inner.status.mode != "free_ride" {
            return Err("Pause belongs to an old run".into());
        }
        inner.status.armed = false;
        inner.status.reason = "Manual controls paused".into();
        inner.latest = None;
        inner.received = None;
        inner.neutral_since = None;
        Ok(())
    }

    pub(super) fn reason(&self, epoch: u64, reason: String) {
        let mut inner = self.lock();
        if inner.status.epoch == epoch {
            inner.status.reason = reason;
        }
    }

    pub(super) fn current(&self, epoch: u64) -> Option<ManualInput> {
        let inner = self.lock();
        (inner.status.epoch == epoch && inner.status.armed)
            .then_some(inner.latest)
            .flatten()
            .filter(|input| input.armed)
    }

    pub(super) fn update_rider(&self, epoch: u64, rider: euc_physics::RiderState) {
        let mut inner = self.lock();
        if inner.status.epoch != epoch {
            return;
        }
        let number = |value: f64| value.is_finite().then_some(value);
        inner.status.desired_lean_deg = number(rider.desired_lean_rad.to_degrees());
        inner.status.commanded_lean_deg = number(rider.commanded_lean_rad.to_degrees());
        inner.status.actual_lean_deg = number(rider.measured_lean_rad.to_degrees());
        inner.status.heading_deg = number(rider.heading_rad.to_degrees());
        inner.status.path_length_m = number(rider.path_length_m);
        inner.status.yaw_aid_work_j = number(rider.yaw_aid_work_j);
        inner.status.roll_aid_work_j = number(rider.roll_aid_work_j);
    }
}

#[tauri::command]
pub fn free_ride_status(state: State<'_, FreeRideState>) -> FreeRideStatus {
    state.lock().status.clone()
}

#[tauri::command]
pub fn free_ride_map(state: State<'_, FreeRideState>, epoch: u64) -> Result<HeightMap, String> {
    let inner = state.lock();
    if inner.status.epoch != epoch {
        return Err("Map belongs to an old run".into());
    }
    inner.map.as_deref().cloned().ok_or_else(|| "This run has no generated map".into())
}

#[tauri::command]
pub fn free_ride_start(
    state: State<'_, super::EngineState>,
    manual: State<'_, FreeRideState>,
    spec: FreeRideSpec,
) -> Result<(), String> {
    spec.validate()?;
    // Validate the generated surface before replacing any active run.
    spec.map.generate(0)?;
    let ticket = manual.reserve_start()?;
    let result = super::with_runner(&state, |runner| runner.start_free_ride(spec, ticket)).and_then(|result| result);
    if result.is_err() {
        manual.finish_start(ticket);
    }
    result
}

#[tauri::command]
pub fn free_ride_input(state: State<'_, FreeRideState>, input: ManualInput) -> Result<(), String> {
    state.input(input)
}

#[tauri::command]
pub fn free_ride_arm(engine: State<'_, super::EngineState>, epoch: u64, source: InputSource) -> Result<(), String> {
    super::with_runner(&engine, |runner| runner.manual_arm(epoch, source))?
}

#[tauri::command]
pub fn free_ride_pause(
    engine: State<'_, super::EngineState>,
    state: State<'_, FreeRideState>,
    epoch: u64,
) -> Result<(), String> {
    state.pause_epoch(epoch)?;
    super::with_runner(&engine, |runner| runner.manual_pause(epoch))?
}

/// Direct Session stepping avoids the scenario driver's speed requests and growing observation vectors.
pub(super) struct FreeRideHost {
    session: Session,
    watch: BoardWatch,
    state: FreeRideState,
    epoch: u64,
    map: Arc<HeightMap>,
    boot_ready: bool,
    desired_heading: f64,
    last_motion: Option<(f64, f64)>,
}

impl FreeRideHost {
    pub(super) fn new(session: Session, state: FreeRideState, epoch: u64, map: Arc<HeightMap>) -> Self {
        Self {
            watch: BoardWatch::new(session.machine()),
            session,
            state,
            epoch,
            map,
            boot_ready: false,
            desired_heading: 0.0,
            last_motion: None,
        }
    }

    fn physics(&self) -> &euc_physics::Physics {
        self.session.machine().plant().live_part().expect("Free ride has a live plant").physics()
    }

    fn clear_input(&mut self) {
        let physics =
            self.session.machine_mut().plant_mut().live_part_mut().expect("Free ride has a live plant").physics_mut();
        physics.clear_rider_input();
        self.desired_heading = physics.heading_rad();
        self.last_motion = None;
    }

    fn sample(&mut self) {
        if self.completed() || self.halted().is_some() {
            return;
        }
        let period = self.session.sample_period_s();
        if let Some(input) = self.state.current(self.epoch) {
            self.session.set_held(false);
            self.session.set_intent(Intent::Lean(input.desired_lean_rad));
            self.desired_heading += input.heading_rate_rad_s * period;
            let physics = self
                .session
                .machine_mut()
                .plant_mut()
                .live_part_mut()
                .expect("Free ride has a live plant")
                .physics_mut();
            let _ = physics.set_heading_target(self.desired_heading);
            let motion = (input.desired_lean_rad, input.heading_rate_rad_s);
            if self.last_motion != Some(motion) {
                self.session.note(format!(
                    "Manual lean {:.2} deg, turn {:.2} deg/s",
                    motion.0.to_degrees(),
                    motion.1.to_degrees()
                ));
                self.last_motion = Some(motion);
            }
        } else if self.boot_ready {
            return;
        }
        self.session.step();
        self.watch.after_step(self.session.machine());
        let rider = self.physics().rider_state();
        self.state.update_rider(self.epoch, rider);
        if let Some(why) = self.halted() {
            self.state.failed(self.epoch, &why);
            return;
        }
        // This is a simulator boot wait, not a measured user reaction time.
        if !self.boot_ready && self.time_s() >= 4.0 {
            self.boot_ready = true;
            self.session.note("Free ride ready on the stand; manual controls require arming");
            self.state.ready(self.epoch);
        }
        if self.boot_ready {
            let clearance =
                |position: [f64; 3], half| self.map.edge_clearance_m([position[0], position[1]], half).unwrap_or(-1.0);
            let edge = clearance(rider.position_m, [0.5, 0.3]).min(clearance(rider.rider_position_m, [0.8, 0.8]));
            if edge <= euc_physics::map::MAP_EDGE_PAUSE_M {
                self.session.note("Map edge guard paused the run");
                self.state.failed(self.epoch, "Map edge. Reset before the finite ground ends");
                self.clear_input();
            } else if !rider.attached {
                self.session.note("Rider detached; manual controls are disarmed");
                self.state.failed(self.epoch, "Rider detached. Reset to start another free ride");
                self.clear_input();
            }
        }
        if self.completed() {
            self.state.failed(self.epoch, "Free ride reached its 600 s software run limit");
        }
    }
}

impl Host for FreeRideHost {
    fn step(&mut self, sim_s: f64) {
        let count = (sim_s / self.session.sample_period_s()).round().max(1.0) as u32;
        for _ in 0..count {
            self.sample();
        }
    }
    fn time_s(&self) -> f64 {
        self.session.time_s()
    }
    fn bodies(&self) -> Vec<BodyInfo> {
        self.session.bodies().to_vec()
    }
    fn snapshot(&self, out: &mut Snapshot) {
        *out = self.session.snapshot();
    }
    fn drain_events(&mut self, out: &mut Vec<Event>) {
        out.extend(self.session.take_events());
        self.watch.drain_events(out);
    }
    fn drain_ble_tx(&mut self, out: &mut Vec<(f64, u8)>) {
        self.watch.drain_ble_tx(self.session.machine(), out);
    }
    fn inject_ble(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.session.machine_mut().inject_ble(bytes);
        Ok(())
    }
    fn pulse_button(&mut self, button: &str, duration_ms: u32) -> Result<(), String> {
        self.session.machine_mut().pulse_button(button, duration_ms)
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
    fn completed(&self) -> bool {
        self.session.time_s() >= 600.0
    }
    fn outputs(&self) -> Option<Outputs> {
        Some(BoardWatch::outputs(self.session.machine()))
    }
    fn drain_tones(&mut self, out: &mut Vec<Tone>) {
        self.watch.drain_tones(out);
    }
    fn handler_counters(&self) -> Option<super::tune::HandlerCounters> {
        self.session.machine().isr_metrics_available().then(|| super::tune::handler_counters(self.session.machine()))
    }
    fn firmware_var_names(&self) -> Option<Vec<String>> {
        Some(self.session.firmware_var_names())
    }
    fn firmware_value(&self, name: &str) -> Option<f64> {
        self.session.firmware_value(name)
    }
    fn manual_pause(&mut self) {
        self.clear_input();
    }
    fn wants_pause(&self) -> bool {
        self.boot_ready && self.state.current(self.epoch).is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use euc_physics::{Physics, Rider, RiderModel, Setup, WheelBody};
    use euc_sim::recording::{RecordingContext, RunMode};
    use serde_json::{Value, json};

    const EPOCH: u64 = 42;
    const SOURCE: InputSource = InputSource::KeyboardSlider;

    fn spec(model: &str) -> FreeRideSpec {
        FreeRideSpec {
            wheel: "begode-extreme".into(),
            settings: "default".into(),
            rider_model: model.into(),
            map: GeneratedMapSpec::gentle_slope(17),
        }
    }

    fn state() -> FreeRideState {
        let state = FreeRideState::default();
        let map = GeneratedMapSpec::flat(17).generate(EPOCH as u32).unwrap();
        state.begin(EPOCH, Some(Arc::new(map)), Rider::default());
        state
    }

    fn packet(seq: u32) -> ManualInput {
        ManualInput {
            run_epoch: EPOCH,
            seq,
            source: SOURCE,
            desired_lean_rad: 0.0,
            heading_rate_rad_s: 0.0,
            armed: false,
        }
    }

    fn neutral_interval(state: &FreeRideState) {
        let mut inner = state.lock();
        let now = Instant::now();
        inner.neutral_since = Some(now - NEUTRAL_TIME - Duration::from_millis(1));
        inner.received = Some(now);
    }

    fn armed_state() -> FreeRideState {
        let state = state();
        state.ready(EPOCH);
        state.input(packet(1)).unwrap();
        neutral_interval(&state);
        state.arm(EPOCH, SOURCE).unwrap();
        state
    }

    #[derive(Debug, PartialEq)]
    struct Mailbox {
        status: Value,
        latest: Option<(u64, u32, InputSource, f64, f64, bool)>,
        last_seq: Option<u32>,
        received: Option<Instant>,
        neutral_since: Option<Instant>,
        next_start: u64,
        pending_start: Option<u64>,
        map: Option<Arc<HeightMap>>,
        rider: Rider,
    }

    fn mailbox(state: &FreeRideState) -> Mailbox {
        let inner = state.lock();
        Mailbox {
            status: serde_json::to_value(&inner.status).unwrap(),
            latest: inner.latest.map(|input| {
                (
                    input.run_epoch,
                    input.seq,
                    input.source,
                    input.desired_lean_rad,
                    input.heading_rate_rad_s,
                    input.armed,
                )
            }),
            last_seq: inner.last_seq,
            received: inner.received,
            neutral_since: inner.neutral_since,
            next_start: inner.next_start,
            pending_start: inner.pending_start,
            map: inner.map.clone(),
            rider: inner.rider,
        }
    }

    #[test]
    fn spec_accepts_only_the_two_supported_rider_models() {
        for (id, model) in [
            ("rigid_ankle_v1", RiderModel::RigidAnkleV1),
            ("articulated_sagittal_v1", RiderModel::ArticulatedSagittalV1),
        ] {
            let spec = spec(id);
            spec.validate().unwrap();
            assert_eq!(spec.rider().unwrap().model, model);
            spec.map.generate(19).unwrap().validate().unwrap();
        }
        for id in ["", "rigid", "articulated", "RIGID_ANKLE_V1", "unknown"] {
            assert!(spec(id).validate().is_err());
            assert!(spec(id).rider().is_err());
        }
    }

    #[test]
    fn spec_rejects_invalid_wheel_settings_and_map() {
        let valid = spec("rigid_ankle_v1");
        for wheel in ["", "other-wheel", "Begode-Extreme"] {
            assert!(FreeRideSpec { wheel: wheel.into(), ..valid.clone() }.validate().is_err());
        }
        for settings in [String::new(), "x".repeat(81)] {
            assert!(FreeRideSpec { settings, ..valid.clone() }.validate().is_err());
        }
        FreeRideSpec { settings: "x".repeat(80), ..valid.clone() }.validate().unwrap();
        let mut invalid = valid;
        invalid.map.grade_pct[0] = f64::NAN;
        assert!(invalid.validate().is_err());
        invalid.map = GeneratedMapSpec { schema: 99, ..GeneratedMapSpec::default() };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn deserialization_rejects_unknown_fields_and_input_sources() {
        let valid = json!({
            "run_epoch": EPOCH, "seq": 1, "source": "keyboard_slider",
            "desired_lean_rad": 0.0, "heading_rate_rad_s": 0.0, "armed": false
        });
        serde_json::from_value::<ManualInput>(valid.clone()).unwrap();
        let mut extra = valid.clone();
        extra["speed"] = json!(1.0);
        assert!(serde_json::from_value::<ManualInput>(extra).is_err());
        let mut unknown = valid;
        unknown["source"] = json!("remote");
        assert!(serde_json::from_value::<ManualInput>(unknown).is_err());
        let mut spec = json!({
            "wheel": "begode-extreme", "settings": "default", "rider_model": "rigid_ankle_v1",
            "map": GeneratedMapSpec::flat(3)
        });
        serde_json::from_value::<FreeRideSpec>(spec.clone()).unwrap();
        spec["unbounded"] = json!(true);
        assert!(serde_json::from_value::<FreeRideSpec>(spec).is_err());
    }

    #[test]
    fn invalid_motion_never_changes_the_mailbox() {
        let state = armed_state();
        let before = mailbox(&state);
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, MAX_LEAN_RAD + 1e-8, -MAX_LEAN_RAD - 1e-8] {
            assert!(state.input(ManualInput { desired_lean_rad: value, armed: true, ..packet(2) }).is_err());
            assert_eq!(mailbox(&state), before);
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, MAX_TURN_RAD_S + 1e-8, -MAX_TURN_RAD_S - 1e-8] {
            assert!(state.input(ManualInput { heading_rate_rad_s: value, armed: true, ..packet(2) }).is_err());
            assert_eq!(mailbox(&state), before);
        }
    }

    #[test]
    fn exact_motion_limits_are_accepted_in_both_directions() {
        let state = armed_state();
        for (seq, sign) in [(2, 1.0), (3, -1.0)] {
            state
                .input(ManualInput {
                    desired_lean_rad: sign * MAX_LEAN_RAD,
                    heading_rate_rad_s: sign * MAX_TURN_RAD_S,
                    armed: true,
                    ..packet(seq)
                })
                .unwrap();
            let input = state.current(EPOCH).unwrap();
            assert_eq!(input.desired_lean_rad, sign * MAX_LEAN_RAD);
            assert_eq!(input.heading_rate_rad_s, sign * MAX_TURN_RAD_S);
        }
    }

    #[test]
    fn stale_epoch_and_nonincreasing_sequences_cannot_replace_input() {
        let state = armed_state();
        state.input(ManualInput { seq: 10, desired_lean_rad: 0.1, armed: true, ..packet(10) }).unwrap();
        let before = mailbox(&state);
        for epoch in [0, EPOCH - 1, EPOCH + 1, u64::MAX] {
            assert!(state.input(ManualInput { run_epoch: epoch, armed: true, ..packet(11) }).is_err());
            assert!(state.current(epoch).is_none());
            assert_eq!(mailbox(&state), before);
        }
        for seq in [0, 1, 9, 10] {
            assert!(state.input(ManualInput { armed: true, ..packet(seq) }).is_err());
            assert_eq!(mailbox(&state), before);
        }
        state.input(ManualInput { armed: true, ..packet(u32::MAX) }).unwrap();
        assert!(state.input(ManualInput { armed: true, ..packet(0) }).is_err(), "sequence wrap needs a new run");
    }

    #[test]
    fn arm_requires_ready_fresh_neutral_input_and_the_same_source() {
        let state = state();
        state.input(packet(1)).unwrap();
        neutral_interval(&state);
        assert!(state.arm(EPOCH, SOURCE).is_err(), "boot has not finished");
        state.ready(EPOCH);
        assert!(state.arm(EPOCH - 1, SOURCE).is_err());
        assert!(state.arm(EPOCH, InputSource::Gamepad).is_err());
        state.lock().neutral_since = Some(Instant::now() + Duration::from_secs(1));
        assert!(state.arm(EPOCH, SOURCE).is_err(), "neutral interval is shorter than 250 ms");
        neutral_interval(&state);
        state.lock().received = Some(Instant::now() - INPUT_TIMEOUT - Duration::from_millis(1));
        assert!(state.arm(EPOCH, SOURCE).is_err(), "neutral data is stale");
        neutral_interval(&state);
        state.arm(EPOCH, SOURCE).unwrap();
        let current = state.current(EPOCH).unwrap();
        assert!(current.armed);
        assert_eq!((current.desired_lean_rad, current.heading_rate_rad_s), (0.0, 0.0));
    }

    #[test]
    fn motion_and_source_changes_restart_the_neutral_interval() {
        let state = state();
        state.ready(EPOCH);
        state.input(packet(1)).unwrap();
        neutral_interval(&state);
        state.input(ManualInput { desired_lean_rad: 0.01, ..packet(2) }).unwrap();
        assert!(state.lock().neutral_since.is_none());
        assert!(state.arm(EPOCH, SOURCE).is_err());
        state.input(packet(3)).unwrap();
        neutral_interval(&state);
        state.input(ManualInput { source: InputSource::Gamepad, ..packet(4) }).unwrap();
        assert!(state.arm(EPOCH, InputSource::Gamepad).is_err());
        neutral_interval(&state);
        state.arm(EPOCH, InputSource::Gamepad).unwrap();
        assert_eq!(state.current(EPOCH).unwrap().source, InputSource::Gamepad);
    }

    #[test]
    fn input_cannot_arm_itself_or_switch_an_armed_source() {
        let state = state();
        state.ready(EPOCH);
        assert!(state.input(ManualInput { armed: true, ..packet(1) }).is_err());
        assert!(state.current(EPOCH).is_none());
        state.input(packet(1)).unwrap();
        neutral_interval(&state);
        state.arm(EPOCH, SOURCE).unwrap();
        let before = mailbox(&state);
        for armed in [false, true] {
            assert!(state.input(ManualInput { source: InputSource::Gamepad, armed, ..packet(2) }).is_err());
            assert_eq!(mailbox(&state), before);
        }
        state.pause_epoch(EPOCH).unwrap();
        state.input(ManualInput { source: InputSource::Gamepad, ..packet(2) }).unwrap();
        assert!(state.arm(EPOCH, InputSource::Gamepad).is_err());
        neutral_interval(&state);
        state.arm(EPOCH, InputSource::Gamepad).unwrap();
    }

    #[test]
    fn watchdog_disarms_after_250_ms_and_requires_new_neutral_input() {
        assert_eq!(INPUT_TIMEOUT, Duration::from_millis(250));
        assert_eq!(NEUTRAL_TIME, Duration::from_millis(250));
        let state = armed_state();
        assert!(!state.check_timeout());
        state.lock().received = Some(Instant::now() - INPUT_TIMEOUT - Duration::from_millis(1));
        assert!(state.check_timeout());
        assert!(state.current(EPOCH).is_none());
        let inner = state.lock();
        assert!(!inner.status.armed);
        assert!(inner.status.ready);
        assert!(inner.latest.is_none() && inner.received.is_none() && inner.neutral_since.is_none());
        assert_eq!(inner.last_seq, Some(1));
        drop(inner);
        assert!(!state.check_timeout());
        assert!(state.arm(EPOCH, SOURCE).is_err());
        assert!(state.input(ManualInput { armed: true, ..packet(2) }).is_err());
        state.input(packet(2)).unwrap();
        assert!(state.arm(EPOCH, SOURCE).is_err());
        neutral_interval(&state);
        state.arm(EPOCH, SOURCE).unwrap();
    }

    #[test]
    fn watchdog_expires_at_exactly_250_ms_but_not_one_nanosecond_before() {
        let state = armed_state();
        let mut inner = state.lock();
        let received = Instant::now();
        inner.received = Some(received);
        assert!(!inner.expire_input(received + Duration::from_nanos(249_999_999)));
        assert!(inner.status.armed && inner.latest.is_some());
        assert!(inner.expire_input(received + Duration::from_millis(250)));
        assert!(!inner.status.armed);
        assert!(inner.latest.is_none() && inner.received.is_none() && inner.neutral_since.is_none());
        assert_eq!((inner.status.epoch, inner.last_seq), (EPOCH, Some(1)));
        assert!(!inner.expire_input(received + Duration::from_secs(1)));
    }

    #[test]
    fn late_armed_packet_cannot_hide_an_expired_watchdog_interval() {
        for gap in [INPUT_TIMEOUT, INPUT_TIMEOUT + Duration::from_millis(1), Duration::from_secs(1)] {
            for lean in [0.0, 0.1] {
                let state = armed_state();
                state.lock().received = Some(Instant::now() - gap);
                assert!(state.input(ManualInput { desired_lean_rad: lean, armed: true, ..packet(2) }).is_err());
                assert!(state.current(EPOCH).is_none());
                let inner = state.lock();
                assert!(!inner.status.armed);
                assert!(inner.status.ready);
                assert_eq!(inner.status.epoch, EPOCH);
                assert_eq!(inner.status.source, Some(SOURCE));
                assert_eq!(inner.last_seq, Some(1), "rejected packets do not advance the accepted sequence");
                assert!(inner.latest.is_none() && inner.received.is_none() && inner.neutral_since.is_none());
                drop(inner);
                assert!(state.arm(EPOCH, SOURCE).is_err(), "a late neutral armed packet cannot start rearming");
            }
        }
    }

    #[test]
    fn timeout_packet_cannot_count_as_the_new_neutral_arm_interval() {
        let state = armed_state();
        state.lock().received = Some(Instant::now() - INPUT_TIMEOUT);
        assert!(state.input(packet(2)).is_err(), "discard the packet that first detects the timeout");
        assert!(state.lock().neutral_since.is_none());
        assert!(state.arm(EPOCH, SOURCE).is_err());
        let fresh = Instant::now();
        state.input(packet(3)).unwrap();
        assert!(state.lock().neutral_since.unwrap() >= fresh);
        assert!(state.arm(EPOCH, SOURCE).is_err());
        assert!(state.input(ManualInput { armed: true, ..packet(4) }).is_err());
        neutral_interval(&state);
        state.arm(EPOCH, SOURCE).unwrap();
        let input = state.current(EPOCH).unwrap();
        assert_eq!(input.seq, 3);
        assert_eq!((input.desired_lean_rad, input.heading_rate_rad_s), (0.0, 0.0));
        state.input(ManualInput { desired_lean_rad: 0.1, armed: true, ..packet(4) }).unwrap();
        assert_eq!(state.current(EPOCH).unwrap().desired_lean_rad, 0.1);
    }

    #[test]
    fn rejected_epoch_and_sequence_do_not_change_an_expired_run() {
        let state = armed_state();
        state.lock().received = Some(Instant::now() - INPUT_TIMEOUT);
        let before = mailbox(&state);
        assert!(state.input(ManualInput { run_epoch: EPOCH - 1, armed: true, ..packet(2) }).is_err());
        assert_eq!(mailbox(&state), before);
        assert!(state.input(ManualInput { armed: true, ..packet(1) }).is_err());
        assert_eq!(mailbox(&state), before);
        assert!(state.check_timeout(), "invalid packets cannot refresh the watchdog");
    }

    #[test]
    fn missing_timestamp_times_out_only_an_armed_free_ride() {
        let state = armed_state();
        state.lock().received = None;
        assert!(state.check_timeout());
        state.lock().status.mode = "scenario";
        state.lock().status.armed = true;
        assert!(!state.check_timeout());
        state.lock().status.mode = "free_ride";
        state.lock().status.armed = false;
        assert!(!state.check_timeout());
    }

    #[test]
    fn pause_and_disarm_clear_motion_but_keep_sequence_and_build_ticket() {
        for pause in [true, false] {
            let state = armed_state();
            let ticket = state.reserve_start().unwrap();
            if pause {
                state.pause_epoch(EPOCH).unwrap();
            } else {
                state.disarm("View disconnected");
            }
            assert!(state.current(EPOCH).is_none());
            let inner = state.lock();
            assert!(inner.latest.is_none() && inner.received.is_none() && inner.neutral_since.is_none());
            assert_eq!(inner.last_seq, Some(1));
            assert_eq!(inner.pending_start, Some(ticket));
            drop(inner);
            assert!(state.input(packet(1)).is_err());
            assert!(state.reserve_start().is_err());
            state.finish_start(ticket);
            assert!(state.reserve_start().unwrap() > ticket);
        }
    }

    #[test]
    fn explicit_unarmed_packet_cancels_motion() {
        let state = armed_state();
        state.input(ManualInput { desired_lean_rad: 0.1, armed: true, ..packet(2) }).unwrap();
        state.input(packet(3)).unwrap();
        assert!(!state.lock().status.armed);
        assert!(state.current(EPOCH).is_none());
        assert!(state.input(ManualInput { armed: true, ..packet(4) }).is_err());
    }

    #[test]
    fn stale_lifecycle_callbacks_cannot_change_the_current_run() {
        let state = armed_state();
        let before = mailbox(&state);
        state.ready(EPOCH - 1);
        state.failed(EPOCH - 1, "old failure");
        state.reason(EPOCH - 1, "old reason".into());
        assert!(state.pause_epoch(EPOCH - 1).is_err());
        assert!(state.arm(EPOCH - 1, SOURCE).is_err());
        assert_eq!(mailbox(&state), before);
        state.failed(EPOCH, "current failure");
        assert!(!state.lock().status.ready);
        assert!(state.current(EPOCH).is_none());
        assert!(state.arm(EPOCH, SOURCE).is_err());
    }

    #[test]
    fn begin_clears_previous_input_and_replaces_map_and_rider_across_clones() {
        let state = armed_state();
        let shared = state.clone();
        let map = Arc::new(GeneratedMapSpec::gentle_slope(9).generate(99).unwrap());
        let rider = spec("articulated_sagittal_v1").rider().unwrap();
        shared.begin(EPOCH + 1, Some(map.clone()), rider);
        let inner = state.lock();
        assert_eq!(inner.status.epoch, EPOCH + 1);
        assert_eq!(inner.status.rider_model, "articulated_sagittal_v1");
        assert!(!inner.status.armed && !inner.status.ready);
        assert!(inner.status.source.is_none());
        assert!(inner.latest.is_none() && inner.last_seq.is_none());
        assert!(inner.received.is_none() && inner.neutral_since.is_none());
        assert!(Arc::ptr_eq(inner.map.as_ref().unwrap(), &map));
        assert_eq!(inner.rider, rider);
        drop(inner);
        assert!(state.current(EPOCH).is_none());
        assert!(state.recording_context(EPOCH).is_err());
        assert!(state.input(packet(2)).is_err());
        state.input(ManualInput { run_epoch: EPOCH + 1, ..packet(0) }).unwrap();
    }

    #[test]
    fn pending_build_survives_begin_and_only_its_own_finish_releases_it() {
        let state = state();
        let ticket = state.reserve_start().unwrap();
        assert!(state.reserve_start().is_err());
        state.begin(EPOCH + 1, None, Rider::default());
        assert_eq!(state.lock().pending_start, Some(ticket));
        state.finish_start(ticket + 1);
        assert!(state.reserve_start().is_err());
        state.finish_start(ticket);
        let next = state.reserve_start().unwrap();
        assert!(next > ticket);
        state.finish_start(ticket);
        assert_eq!(state.lock().pending_start, Some(next), "a late old build cannot release the next build");
        state.finish_start(next);
        state.finish_start(next);
        assert!(state.lock().pending_start.is_none());
    }

    #[test]
    fn build_ticket_overflow_fails_without_reusing_a_ticket() {
        let state = state();
        state.lock().next_start = u64::MAX;
        let before = mailbox(&state);
        assert!(state.reserve_start().is_err());
        assert_eq!(mailbox(&state), before);
    }

    #[test]
    fn scenario_replacement_drops_manual_map_and_recording_context() {
        let state = armed_state();
        let old_context = state.recording_context(EPOCH).unwrap().unwrap();
        state.begin(EPOCH + 1, None, Rider::default());
        assert_eq!(state.lock().status.mode, "scenario");
        assert!(state.lock().map.is_none());
        assert!(state.current(EPOCH).is_none());
        assert!(state.recording_context(EPOCH + 1).unwrap().is_none());
        assert!(state.input(ManualInput { run_epoch: EPOCH + 1, ..packet(2) }).is_err());
        assert!(state.arm(EPOCH + 1, SOURCE).is_err());
        assert_eq!(old_context["height_map"]["revision"], EPOCH as u32);
    }

    #[test]
    fn recording_context_rejects_missing_invalid_and_old_maps() {
        let state = state();
        assert!(state.recording_context(EPOCH - 1).is_err());
        state.lock().map = None;
        assert!(state.recording_context(EPOCH).is_err());
        let mut invalid = GeneratedMapSpec::flat(1).generate(3).unwrap();
        invalid.heights_mm.pop();
        state.lock().map = Some(Arc::new(invalid));
        assert!(state.recording_context(EPOCH).is_err());
    }

    #[test]
    fn recording_context_matches_actual_rigid_and_articulated_models_and_maps() {
        let plant: toml::Value = toml::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../profiles/begode-extreme/plant.toml"
        )))
        .unwrap();
        let wheel: WheelBody = plant["body"].clone().try_into().unwrap();
        for (id, recorded_model, joint_count) in [
            ("rigid_ankle_v1", euc_sim::recording::RiderModel::RigidAnkleV1, 0),
            ("articulated_sagittal_v1", euc_sim::recording::RiderModel::ArticulatedSagittalV1, 6),
        ] {
            let spec = spec(id);
            let map = Arc::new(spec.map.generate(37).unwrap());
            let mut rider = spec.rider().unwrap();
            rider.control.steer_kp = 13.0;
            rider.control.steer_kd = 7.0;
            rider.control.steer_max_nm = 11.0;
            let mut setup = Setup::new(wheel.clone());
            setup.rider = rider;
            setup.height_map = Some(map.as_ref().clone());
            let physics = Physics::new(setup).unwrap();
            let state = FreeRideState::default();
            state.begin(EPOCH, Some(map.clone()), physics.setup().rider);
            let value = state.recording_context(EPOCH).unwrap().unwrap();
            let context = RecordingContext::from_json(&value).unwrap();
            assert_eq!(context.schema, 1);
            assert_eq!(context.run_mode, RunMode::FreeRide);
            assert_eq!(context.rider_model, recorded_model);
            assert_eq!(context.height_map.as_ref(), physics.setup().height_map.as_ref());
            assert_eq!(context.height_map.as_ref(), Some(map.as_ref()));
            assert_eq!(physics.rider_joints().len(), joint_count);
            assert_eq!(physics.rider_state().model, id);
            assert_eq!(state.lock().status.rider_model, id);
            assert_eq!(value["rider_model"], id);
            assert_eq!(context.model_aids.steer_kp, rider.control.steer_kp);
            assert_eq!(context.model_aids.steer_kd, rider.control.steer_kd);
            assert_eq!(context.model_aids.steer_max_nm, rider.control.steer_max_nm);
            let assist = euc_physics::Assist::default();
            assert_eq!(context.model_aids.roll_stiffness, assist.roll_stiffness);
            assert_eq!(context.model_aids.roll_damping, assist.roll_damping);
            state.begin(EPOCH + 1, None, Rider::default());
            assert_eq!(RecordingContext::from_json(&value).unwrap(), context, "saved context owns its grid");
        }
    }
}

/// Real native factory and cached stock firmware. Separate from the input/watchdog unit tests.
#[cfg(test)]
mod stock_rider_acceptance {
    use super::*;
    use euc_fwsource::FirmwareCache;
    use euc_profile::{Channel, FirmwareSource, ProfileSource};
    use euc_telemetry::EventKind;

    const EPOCH: u64 = 700;
    const STEP_S: f64 = 1.0 / 120.0;
    const LEAN_RAD: f64 = 0.04;
    const STOCK_SHA256: &str = "cbfb328cab1e0b8219790dd3ef710bb266569c70b6ac396ccd06c247da9fd27d";

    struct Rig {
        host: Box<dyn Host>,
        state: FreeRideState,
        seq: u32,
        articulated: bool,
        source: InputSource,
    }

    #[derive(Debug, Default)]
    struct Peaks {
        pitch_deg: f32,
        roll_deg: f32,
        torque_nm: f32,
        iq_a: f32,
        phase_a: f32,
        drive_command: f64,
        joint_change_rad: [f64; 6],
    }

    fn require_cached_stock() {
        let files = euc_sim::WheelFiles::load(&ProfileSource::Builtin, "begode-extreme").unwrap();
        let build = files
            .profile
            .firmware
            .iter()
            .find_map(|source| match source {
                FirmwareSource::BegodeApi(build) if build.channel == Channel::Current => Some(build),
                _ => None,
            })
            .unwrap();
        // Check the exact image identity before accepting named probe/map addresses.
        assert_eq!(build.sha256.to_string(), STOCK_SHA256);
        assert_eq!(build.label, "GW1825007");
        assert_eq!(files.stockmap.build, build.label);
        let root = std::env::var_os("EUC_CACHE_DIR")
            .map(Into::into)
            .or_else(FirmwareCache::default_root)
            .expect("firmware cache directory");
        assert!(
            FirmwareCache::new(root).verified(&build.sha256).is_some(),
            "cache the pinned stock firmware first; this acceptance test must not download it"
        );
        println!("stock pin: {} sha256={STOCK_SHA256}; stockmap={}", build.label, files.stockmap.build);
    }

    fn packet(seq: u32, lean: f64, armed: bool) -> ManualInput {
        ManualInput {
            run_epoch: EPOCH,
            seq,
            source: InputSource::KeyboardSlider,
            desired_lean_rad: lean,
            heading_rate_rad_s: 0.0,
            armed,
        }
    }

    // Relative body orientations expose the actual six sagittal hinge angles through Host.
    // No diagnostic method or test-only downcast is added to the production Host contract.
    fn joints(snapshot: &Snapshot) -> [f64; 6] {
        if snapshot.bodies.len() == 3 {
            return [0.0; 6];
        }
        [[2, 3], [3, 4], [4, 5], [2, 6], [6, 7], [7, 8]].map(|[parent, child]| {
            let [pw, px, py, pz] = snapshot.bodies[parent].quat.map(f64::from);
            let [cw, cx, cy, cz] = snapshot.bodies[child].quat.map(f64::from);
            let w = pw * cw + px * cx + py * cy + pz * cz;
            let y = pw * cy + px * cz - py * cw - pz * cx;
            let angle = 2.0 * y.atan2(w);
            (angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
        })
    }

    impl Rig {
        fn new(model: &str) -> Self {
            Self::on_map(model, GeneratedMapSpec::flat(17))
        }

        fn on_map(model: &str, map: GeneratedMapSpec) -> Self {
            require_cached_stock();
            let state = FreeRideState::default();
            let spec = FreeRideSpec {
                wheel: "begode-extreme".into(),
                settings: "neutral".into(),
                rider_model: model.into(),
                map,
            };
            let built = super::super::firmware::build_free_host(&spec, "stock", EPOCH, state.clone(), &mut |_| {})
                .expect("real native FreeRideHost factory");
            println!(
                "stock rider acceptance: model={model}, firmware={}, CPU={:?}",
                built.build,
                super::super::cpu::backend()
            );
            let names: Vec<_> = built.host.bodies().into_iter().map(|b| b.name).collect();
            let articulated = model == "articulated_sagittal_v1";
            if articulated {
                assert_eq!(
                    names,
                    [
                        "wheel",
                        "shell",
                        "rider",
                        "left_thigh",
                        "left_shank",
                        "left_foot",
                        "right_thigh",
                        "right_shank",
                        "right_foot"
                    ]
                );
            } else {
                assert_eq!(names, ["wheel", "shell", "rider"]);
            }
            assert_eq!(state.lock().status.rider_model, model);
            assert_eq!(state.recording_context(EPOCH).unwrap().unwrap()["rider_model"], model);
            Self { host: built.host, state, seq: 0, articulated, source: InputSource::KeyboardSlider }
        }

        fn input(&mut self, lean: f64, armed: bool) {
            self.motion(lean, 0.0, armed);
        }

        fn motion(&mut self, lean: f64, turn: f64, armed: bool) {
            self.seq += 1;
            self.state
                .input(ManualInput { heading_rate_rad_s: turn, source: self.source, ..packet(self.seq, lean, armed) })
                .unwrap();
        }

        fn snapshot(&self) -> Snapshot {
            let mut snapshot = Snapshot::default();
            self.host.snapshot(&mut snapshot);
            snapshot
        }

        fn tick(&mut self) -> Snapshot {
            self.host.step(STEP_S);
            assert!(self.host.halted().is_none(), "native halt: {:?}", self.host.halted());
            assert!(!self.host.completed());
            let mut events = Vec::new();
            self.host.drain_events(&mut events);
            assert!(!events.iter().any(|e| matches!(e.kind, EventKind::Fault | EventKind::Cutoff)), "{events:?}");
            // Consume normal backend outputs without opening audio or storing a recording.
            self.host.drain_ble_tx(&mut Vec::new());
            self.host.drain_tones(&mut Vec::new());
            let snapshot = self.snapshot();
            assert_eq!(snapshot.bodies.len(), if self.articulated { 9 } else { 3 });
            assert!(snapshot.bodies.iter().all(|p| p.to_array().iter().all(|v| v.is_finite())));
            assert!(
                snapshot.signals.pitch_deg.abs() < 10.0 && snapshot.signals.roll_deg.abs() < 10.0,
                "unexpected large wheel angle at t={:.6}: {:?}",
                snapshot.t_s,
                snapshot.signals
            );
            snapshot
        }

        fn boot_and_arm(&mut self) {
            assert!(self.state.arm(EPOCH, InputSource::KeyboardSlider).is_err());
            for _ in 0..490 {
                if self.state.lock().status.ready {
                    break;
                }
                self.tick();
            }
            assert!((4.0..4.02).contains(&self.host.time_s()));
            let status = self.state.lock().status.clone();
            assert!(status.ready && !status.armed);
            assert!(self.host.wants_pause());
            let before = self.snapshot();
            self.host.step(STEP_S);
            let paused = self.snapshot();
            assert_eq!(paused.t_s, before.t_s, "boot-ready must hold until explicit arming");
            assert_eq!(paused.bodies, before.bodies);
            self.input(0.0, false);
            assert!(self.state.arm(EPOCH, InputSource::KeyboardSlider).is_err());
            // Exercise the real wall-clock neutral gate. Do not alter internal timestamps.
            for _ in 0..6 {
                std::thread::sleep(Duration::from_millis(50));
                self.input(0.0, false);
            }
            self.state.arm(EPOCH, InputSource::KeyboardSlider).unwrap();
            assert!(self.state.lock().status.armed);
            assert!(!self.host.wants_pause());
        }

        fn phase(&mut self, count: usize, lean: f64, reference: [f64; 6]) -> Peaks {
            let mut peaks = Peaks::default();
            for _ in 0..count {
                self.input(lean, true);
                let snapshot = self.tick();
                let status = self.state.lock().status.clone();
                assert!(status.ready && status.armed, "manual run lost readiness: {status:?}");
                let s = snapshot.signals;
                for value in [s.speed_kmh, s.torque_nm, s.iq_a, s.phase_a_a, s.phase_b_a, s.phase_c_a] {
                    assert!(value.is_finite());
                }
                peaks.pitch_deg = peaks.pitch_deg.max(s.pitch_deg.abs());
                peaks.roll_deg = peaks.roll_deg.max(s.roll_deg.abs());
                peaks.torque_nm = peaks.torque_nm.max(s.torque_nm.abs());
                peaks.iq_a = peaks.iq_a.max(s.iq_a.abs());
                peaks.phase_a = peaks.phase_a.max(s.phase_a_a.abs()).max(s.phase_b_a.abs()).max(s.phase_c_a.abs());
                peaks.drive_command = peaks.drive_command.max(self.host.firmware_value("drive_command").unwrap().abs());
                for (index, angle) in joints(&snapshot).iter().enumerate() {
                    peaks.joint_change_rad[index] = peaks.joint_change_rad[index].max((angle - reference[index]).abs());
                }
            }
            peaks
        }
    }

    fn accept(model: &str) {
        let mut rig = Rig::new(model);
        rig.boot_and_arm();
        let neutral = rig.phase(60, 0.0, joints(&rig.snapshot()));
        let start = rig.snapshot();
        let lean = rig.phase(120, LEAN_RAD, joints(&start));
        let leaned = rig.snapshot();
        let lean_status = rig.state.lock().status.clone();
        let recovery = rig.phase(60, 0.0, joints(&leaned));
        let end = rig.snapshot();
        let status = rig.state.lock().status.clone();
        println!("{model}: neutral={neutral:?}\nlean={lean:?}\nrecovery={recovery:?}");
        println!(
            "{model}: lean-end desired={:?}deg, commanded={:?}deg, actual={:?}deg, joint_angles={:?}rad",
            lean_status.desired_lean_deg,
            lean_status.commanded_lean_deg,
            lean_status.actual_lean_deg,
            joints(&leaned)
        );
        println!(
            "{model}: t={:.4}s, start_speed={:.4}km/h, lean_speed={:.4}km/h, end_speed={:.4}km/h, dx={:.4}m, desired={:?}, commanded={:?}, actual={:?}",
            end.t_s,
            start.signals.speed_kmh,
            leaned.signals.speed_kmh,
            end.signals.speed_kmh,
            leaned.bodies[1].pos[0] - start.bodies[1].pos[0],
            status.desired_lean_deg,
            status.commanded_lean_deg,
            status.actual_lean_deg
        );
        assert!((lean_status.desired_lean_deg.unwrap().to_radians() - LEAN_RAD).abs() < 1e-8);
        assert!((lean_status.commanded_lean_deg.unwrap().to_radians() - LEAN_RAD).abs() < 1e-8);
        assert!(
            lean.torque_nm > 1.0 && lean.iq_a > 0.1 && lean.phase_a > 0.1 && lean.drive_command > 1.0,
            "stock controller must drive the electrical motor, not a kinematic pose: {lean:?}"
        );
        if rig.articulated {
            assert!(lean.joint_change_rad.iter().all(|change| *change > 0.001), "physical hinge response: {lean:?}");
        }
        assert!(leaned.signals.speed_kmh - start.signals.speed_kmh > 0.1, "lean must produce forward acceleration");
        assert!(leaned.bodies[1].pos[0] - start.bodies[1].pos[0] > 0.005, "wheel must physically travel");
        assert_eq!(rig.state.lock().status.desired_lean_deg, Some(0.0));
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; runs six simulated seconds on the native FreeRideHost"]
    fn stock_articulated_stand_arm_lean_and_neutral() {
        accept("articulated_sagittal_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; runs six simulated seconds on the native FreeRideHost"]
    fn stock_rigid_stand_arm_lean_and_neutral() {
        accept("rigid_ankle_v1");
    }

    fn heading(snapshot: &Snapshot) -> f64 {
        let [w, x, y, z] = snapshot.bodies[1].quat.map(f64::from);
        (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z))
    }

    fn angle_delta(a: f64, b: f64) -> f64 {
        (a - b + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
    }

    // Independent footprint calculation from actual streamed poses, not a body relocation or
    // a call back into the map's edge-clearance helper under test.
    fn footprint_clearance(snapshot: &Snapshot, map: &HeightMap) -> f64 {
        let [lo, hi] = map.bounds_xy_m().unwrap();
        let mut clearance = f64::INFINITY;
        for (body, half) in [(1, [0.5, 0.3]), (2, [0.8, 0.8])] {
            for axis in 0..2 {
                let position = f64::from(snapshot.bodies[body].pos[axis]);
                clearance = clearance.min(position - half[axis] - lo[axis]);
                clearance = clearance.min(hi[axis] - position - half[axis]);
            }
        }
        clearance
    }

    // A bounded test-side speed-feedback policy sends ordinary accepted ManualInput packets.
    // It never writes firmware memory, body poses, wheel velocity, or physics state.
    fn drive_sample(rig: &mut Rig, speed_m_s: f64, turn: f64) -> Snapshot {
        let speed = f64::from(rig.snapshot().signals.speed_kmh) / 3.6;
        rig.motion(((speed_m_s - speed) * 0.05).clamp(-0.04, 0.04), turn, true);
        rig.tick()
    }

    fn sustained_steering(model: &str) {
        let spec = GeneratedMapSpec { grade_pct: [1.0, 0.0], ..GeneratedMapSpec::flat(17) };
        let mut rig = Rig::on_map(model, spec);
        rig.boot_and_arm();
        let map = rig.state.lock().map.as_ref().unwrap().clone();
        let start = rig.snapshot();
        let mut maximum_ground_height = 0.0_f64;
        let mut minimum_clearance = f64::INFINITY;
        let mut peak_roll = 0.0_f32;
        let mut peak_current = 0.0_f32;
        // Four seconds straight, six seconds turning, four seconds holding heading.
        for sample in 0..1680 {
            let turn = if (480..1200).contains(&sample) { 0.08 } else { 0.0 };
            let snapshot = drive_sample(&mut rig, 0.6, turn);
            let status = rig.state.lock().status.clone();
            assert!(status.ready && status.armed, "{model} steering stopped at {:.4}: {status:?}", snapshot.t_s);
            let [x, y, _] = snapshot.bodies[1].pos.map(f64::from);
            let ground = map.sample(x, y).expect("actual shell remains on the finite generated map");
            maximum_ground_height = maximum_ground_height.max(ground.height_m.abs());
            minimum_clearance = minimum_clearance.min(footprint_clearance(&snapshot, &map));
            peak_roll = peak_roll.max(snapshot.signals.roll_deg.abs());
            peak_current = peak_current.max(snapshot.signals.iq_a.abs());
            if sample % 360 == 359 {
                println!(
                    "sustained-steer {model} t={:.3} pose={:?} heading_deg={:.3} speed_kmh={:.3} ground_m={:.5}",
                    snapshot.t_s,
                    snapshot.bodies[1].pos,
                    heading(&snapshot).to_degrees(),
                    snapshot.signals.speed_kmh,
                    ground.height_m
                );
            }
        }
        let end = rig.snapshot();
        let status = rig.state.lock().status.clone();
        let yaw = angle_delta(heading(&end), heading(&start));
        let path = status.path_length_m.unwrap();
        println!(
            "sustained-steer result {model}: elapsed_s={:.6}, path_m={path:.6}, yaw_deg={:.6}, dy_m={:.6}, max_ground_m={maximum_ground_height:.6}, min_clearance_m={minimum_clearance:.6}, peak_roll_deg={peak_roll:.6}, peak_iq_a={peak_current:.6}, yaw_work_j={:?}",
            end.t_s - start.t_s,
            yaw.to_degrees(),
            end.bodies[1].pos[1] - start.bodies[1].pos[1],
            status.yaw_aid_work_j
        );
        assert!((end.t_s - start.t_s - 14.0).abs() < 1e-6);
        assert!(path > 2.0, "sustained run must physically travel: {path}");
        assert!(yaw > 0.15, "the actual shell must turn left: {yaw}");
        assert!(end.bodies[1].pos[1] - start.bodies[1].pos[1] > 0.1, "actual path must leave the x axis");
        assert!(maximum_ground_height > 0.005, "the physical run must leave the level spawn patch");
        assert!(minimum_clearance > 1.0);
        assert!(status.yaw_aid_work_j.unwrap().abs() > 0.01, "external yaw work stays explicit");
        assert!(angle_delta(heading(&end), heading(&start) + 0.48).abs() < 0.1);
    }

    fn pause_and_rearm(model: &str) {
        let mut rig = Rig::new(model);
        rig.boot_and_arm();
        for _ in 0..360 {
            drive_sample(&mut rig, 0.6, 0.05);
        }
        let before = rig.snapshot();
        rig.state.pause_epoch(EPOCH).unwrap();
        rig.host.manual_pause();
        let cleared = rig.snapshot();
        assert_eq!(cleared.t_s, before.t_s);
        assert_eq!(cleared.bodies, before.bodies, "pause must not relocate the actual physical bodies");
        assert!(rig.host.wants_pause());
        for _ in 0..120 {
            rig.host.step(STEP_S);
        }
        let frozen = rig.snapshot();
        assert_eq!(frozen.t_s, before.t_s);
        assert_eq!(frozen.bodies, before.bodies);
        assert!(rig.state.arm(EPOCH, InputSource::KeyboardSlider).is_err());
        assert!(rig.state.input(packet(rig.seq + 1, 0.04, true)).is_err());
        rig.source = InputSource::Gamepad;
        rig.input(0.0, false);
        assert!(rig.state.arm(EPOCH, rig.source).is_err());
        for _ in 0..6 {
            std::thread::sleep(Duration::from_millis(50));
            rig.input(0.0, false);
        }
        rig.state.arm(EPOCH, rig.source).unwrap();
        let armed = rig.state.current(EPOCH).unwrap();
        assert_eq!(armed.source, InputSource::Gamepad);
        assert_eq!((armed.desired_lean_rad, armed.heading_rate_rad_s), (0.0, 0.0));
        assert_eq!(rig.snapshot().bodies, before.bodies);
        for _ in 0..360 {
            let snapshot = drive_sample(&mut rig, 0.6, 0.0);
            assert!(rig.state.lock().status.armed);
            assert!(angle_delta(heading(&snapshot), heading(&before)).abs() < 0.1, "old steering must not resume");
        }
        let end = rig.snapshot();
        println!(
            "pause-rearm {model}: paused_t_s={:.6}, end_t_s={:.6}, paused_heading_deg={:.6}, end_heading_deg={:.6}, paused_pose={:?}, end_pose={:?}, end_speed_kmh={:.6}",
            before.t_s,
            end.t_s,
            heading(&before).to_degrees(),
            heading(&end).to_degrees(),
            before.bodies[1].pos,
            end.bodies[1].pos,
            end.signals.speed_kmh
        );
        assert!((end.t_s - before.t_s - 3.0).abs() < 1e-6);
        assert!(end.bodies[1].pos != before.bodies[1].pos);
    }

    fn finite_edge_admission(model: &str) {
        let mut rig = Rig::new(model);
        rig.boot_and_arm();
        let map = rig.state.lock().map.as_ref().unwrap().clone();
        let started = rig.snapshot().t_s;
        let mut previous_clearance = footprint_clearance(&rig.snapshot(), &map);
        let mut warning_seen = false;
        for sample in 0..5400 {
            let snapshot = drive_sample(&mut rig, 0.8, 0.0);
            let clearance = footprint_clearance(&snapshot, &map);
            let status = rig.state.lock().status.clone();
            // Streamed positions are f32; reserve 0.1 mm at the exact pause threshold.
            if clearance > 1.0001 && clearance <= 2.0 {
                warning_seen = true;
                assert!(status.ready && status.armed, "the warning band must not pause admission early");
            }
            if sample % 600 == 599 {
                println!(
                    "edge-approach {model}: t={:.3}, pose={:?}, clearance_m={clearance:.6}, speed_kmh={:.6}",
                    snapshot.t_s, snapshot.bodies[1].pos, snapshot.signals.speed_kmh
                );
            }
            if !status.ready {
                println!(
                    "edge-stop {model}: elapsed_s={:.6}, pose={:?}, previous_clearance_m={previous_clearance:.6}, clearance_m={clearance:.6}, reason={}",
                    snapshot.t_s - started,
                    snapshot.bodies[1].pos,
                    status.reason
                );
                assert!(status.reason.starts_with("Map edge."), "edge admission was not reached: {status:?}");
                assert!(warning_seen, "the physical run must cross the warning band before admission stops");
                assert!(!status.armed);
                assert!(rig.state.current(EPOCH).is_none());
                assert!(previous_clearance > 1.0 - 1e-4);
                assert!((0.5..=1.0001).contains(&clearance), "stop while both footprints remain inside: {clearance}");
                for body in [1, 2] {
                    let [x, y, _] = snapshot.bodies[body].pos.map(f64::from);
                    assert!(map.sample(x, y).is_some());
                }
                assert!(rig.host.wants_pause());
                assert!(rig.state.arm(EPOCH, rig.source).is_err());
                rig.host.step(1.0);
                let frozen = rig.snapshot();
                assert_eq!(frozen.t_s, snapshot.t_s);
                assert_eq!(frozen.bodies, snapshot.bodies);
                return;
            }
            assert!(clearance > 1.0 - 1e-4);
            previous_clearance = clearance;
        }
        panic!("{model} did not physically reach the edge guard within 45 simulated seconds");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; real-host rigid steering for 14 s after 4 s boot"]
    fn stock_rigid_sustained_steering_on_generated_terrain() {
        sustained_steering("rigid_ankle_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; real-host articulated steering for 14 s after 4 s boot"]
    fn stock_articulated_sustained_steering_on_generated_terrain() {
        sustained_steering("articulated_sagittal_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; real-host rigid pause and neutral Gamepad rearm"]
    fn stock_rigid_pause_and_neutral_rearm() {
        pause_and_rearm("rigid_ankle_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; real-host articulated pause and neutral Gamepad rearm"]
    fn stock_articulated_pause_and_neutral_rearm() {
        pause_and_rearm("articulated_sagittal_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; real poses must reach finite map edge within 45 s"]
    fn stock_rigid_finite_map_edge_admission() {
        finite_edge_admission("rigid_ankle_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; real articulated poses must reach finite map edge within 45 s"]
    fn stock_articulated_finite_map_edge_admission() {
        finite_edge_admission("articulated_sagittal_v1");
    }

    // A concrete host permits exact read-only physics diagnostics, without adding downcasts to
    // Host. Configuration matches build_free_host; factory acceptance above remains the gate.
    fn diagnostic_host(model: &str, state: FreeRideState) -> FreeRideHost {
        require_cached_stock();
        let files = euc_sim::WheelFiles::load(&ProfileSource::Builtin, "begode-extreme").unwrap();
        let spec = FreeRideSpec {
            wheel: "begode-extreme".into(),
            settings: "neutral".into(),
            rider_model: model.into(),
            map: GeneratedMapSpec::flat(17),
        };
        let map = Arc::new(spec.map.generate(EPOCH as u32).unwrap());
        let rider = spec.rider().unwrap();
        state.begin(EPOCH, Some(Arc::clone(&map)), rider);
        let board = files.board_with_settings(&spec.settings).unwrap();
        let loaded = super::super::images::load(&files, "stock", &mut |_| {}).unwrap();
        let live = files.live_on_map(rider, Default::default(), map.as_ref().clone()).unwrap();
        let config = euc_sim::MachineConfig {
            board: &board,
            stockmap: loaded.map.as_ref(),
            image: &loaded.image,
            load_addr: files.profile.board.app.base,
            hclk: files.hclk(),
            plant: euc_sim::PlantState::default(),
            injections: Vec::new(),
            power_cycles: Vec::new(),
            cpi: euc_sim::DEFAULT_CPI,
            exception_timing: euc_sim::ExceptionTiming::default(),
            max_quantum: 4000,
            marker_window_s: 4.0,
        };
        let machine =
            euc_sim::Machine::new_live(&config, &euc_sim::registry(), super::super::cpu::factory().unwrap(), live)
                .unwrap();
        FreeRideHost::new(Session::new(machine, loaded.map.as_ref(), 120.0), state, EPOCH, map)
    }

    fn trace(host: &FreeRideHost, model: &str, phase: &str) {
        let physics = host.physics();
        let state = physics.rider_state();
        let poses = physics.poses();
        let ankle = if poses.len() == 9 {
            std::array::from_fn(|axis| f64::midpoint(f64::from(poses[5].pos[axis]), f64::from(poses[8].pos[axis])))
        } else {
            state.rider_position_m
        };
        let relative: [f64; 3] = std::array::from_fn(|axis| physics.rider_com_m()[axis] - ankle[axis]);
        let joints: Vec<_> = physics
            .rider_joints()
            .into_iter()
            .map(|joint| (joint.posture_target_rad, joint.angle_rad, joint.rate_rad_s, joint.torque_nm))
            .collect();
        let mut snapshot = Snapshot::default();
        host.snapshot(&mut snapshot);
        println!(
            "stock-trace {model} {phase} t={:.4}: COM-ankle_m={relative:?}, desired/commanded/actual_lean_rad={:?}, joint_posture/angle/rate/torque={joints:?}, shell_pitch_deg={:.5}, wheel_torque_nm={:.5}, speed_kmh={:.5}",
            host.time_s(),
            [state.desired_lean_rad, state.commanded_lean_rad, state.measured_lean_rad],
            snapshot.signals.pitch_deg,
            snapshot.signals.torque_nm,
            snapshot.signals.speed_kmh
        );
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; exact native COM/joint/motor trace, no hardware or audio"]
    fn stock_exact_com_joint_and_motor_trace() {
        for model in ["rigid_ankle_v1", "articulated_sagittal_v1"] {
            let state = FreeRideState::default();
            let mut host = diagnostic_host(model, state.clone());
            trace(&host, model, "initial-stand");
            while !state.lock().status.ready {
                host.step(STEP_S);
                assert!(host.halted().is_none());
                assert!(host.time_s() < 4.02);
            }
            trace(&host, model, "stand-ready");
            let mut seq = 1;
            state.input(packet(seq, 0.0, false)).unwrap();
            for _ in 0..6 {
                std::thread::sleep(Duration::from_millis(50));
                seq += 1;
                state.input(packet(seq, 0.0, false)).unwrap();
            }
            state.arm(EPOCH, InputSource::KeyboardSlider).unwrap();
            for sample in 0..240 {
                let phase = if sample < 60 {
                    "neutral"
                } else if sample < 180 {
                    "lean"
                } else {
                    "recover"
                };
                let lean = if phase == "lean" { LEAN_RAD } else { 0.0 };
                seq += 1;
                state.input(packet(seq, lean, true)).unwrap();
                host.step(STEP_S);
                assert!(host.halted().is_none() && !host.physics().rider_off());
                if [0, 59, 89, 119, 149, 179, 239].contains(&sample) {
                    trace(&host, model, phase);
                }
                host.drain_events(&mut Vec::new());
                host.drain_ble_tx(&mut Vec::new());
                host.drain_tones(&mut Vec::new());
            }
        }
    }
}
