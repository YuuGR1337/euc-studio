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
/// A full crouch input, radians of knee flexion (`Physics::set_crouch` takes 0 to 1.2).
const CROUCH_MAX_RAD: f64 = 1.2;
/// Simulated seconds the firmware boots on the stand before manual controls can arm. This is a
/// simulator boot wait, not a measured user reaction time.
const BOOT_S: f64 = 4.0;

/// The largest manual turn rate a packet may ask for, rad/s: the rider's cap at a stop. The
/// physics caps it further with speed (`RiderController::manual_turn_cap`).
fn max_turn_rad_s(rider: &euc_physics::Rider) -> f64 {
    rider.control.manual_turn_rate_max_rad_s
}

/// A quick reset stands the wheel on its stand again only this far from every solid, metres:
/// stand, wheel and rider fit, and nothing is pressed into a box.
const RESTART_CLEAR_M: f64 = 1.5;
/// ...and only on ground this level: within 0.5 m of the spot it varies by less, metres (no curb,
/// step or steep slope under the stand).
const RESTART_LEVEL_M: f64 = 0.03;
/// Samples between two checks of the wheel's spot.
const RESTART_CHECK_SAMPLES: u32 = 12;

/// Whether a quick reset may stand the wheel at `xy` (world metres): no solid within
/// [`RESTART_CLEAR_M`] and level ground under it. A spot that is not clear restarts at the last
/// clear one the wheel passed, or the run's own start.
fn clear_spot(world: &euc_world::World, xy: [f64; 2]) -> bool {
    let reach = RESTART_CLEAR_M;
    let around = euc_world::Rect::new([xy[0] - reach, xy[1] - reach], [xy[0] + reach, xy[1] + reach]);
    if !world.solids_in(around).is_empty() {
        return false;
    }
    let heights = [[0.0, 0.0], [0.5, 0.0], [-0.5, 0.0], [0.0, 0.5], [0.0, -0.5]]
        .map(|d: [f64; 2]| world.height_at(xy[0] + d[0], xy[1] + d[1]));
    let (low, high) = heights.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &z| (l.min(z), h.max(z)));
    high - low < RESTART_LEVEL_M
}

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
    /// The finite 32 m test pad; absent when the ground is a streaming world.
    #[serde(default)]
    pub map: Option<GeneratedMapSpec>,
    /// The streaming world (docs/models/world.md), Free Ride's default ground.
    #[serde(default)]
    pub world: Option<WorldChoice>,
    /// Where on the world the run starts, instead of the world's spawn (a quick reset restarts
    /// where the wheel was). Worlds only.
    #[serde(default)]
    pub start: Option<WorldPose>,
}

/// A place and heading on a world, world coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldPose {
    /// Ground point under the wheel, metres.
    pub xy_m: [f64; 2],
    /// Heading, radians, left positive from world +x.
    pub heading_rad: f64,
}

/// The world a free ride rides: its seed and densities (city biome, this build's generator).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldChoice {
    pub seed: u64,
    #[serde(default)]
    pub densities: euc_world::Densities,
}

impl WorldChoice {
    pub(super) fn spec(&self) -> euc_world::WorldSpec {
        euc_world::WorldSpec { densities: self.densities, ..euc_world::WorldSpec::city(self.seed) }
    }
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
        if let Some(start) = &self.start {
            if self.world.is_none() {
                return Err("A start place needs a world".into());
            }
            if !start.xy_m.iter().chain([&start.heading_rad]).all(|v| v.is_finite()) {
                return Err("The start place must be finite".into());
            }
        }
        match (&self.map, &self.world) {
            (Some(map), None) => map.validate(),
            (None, Some(world)) => world.spec().validate(),
            _ => Err("Choose either the test pad or a world".into()),
        }
    }

    pub(super) fn rider(&self) -> Result<euc_physics::Rider, String> {
        let model = match self.rider_model.as_str() {
            "rigid_ankle_v1" => euc_physics::RiderModel::RigidAnkleV1,
            "articulated_sagittal_v1" => euc_physics::RiderModel::ArticulatedSagittalV1,
            "articulated_contact_v1" => euc_physics::RiderModel::ArticulatedContactV1,
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
    /// Manual turn rate, rad/s, left positive, up to the rider's cap at a stop; the physics caps
    /// it further with speed (`FreeRideStatus::turn_cap_rad_s`).
    pub heading_rate_rad_s: f64,
    pub armed: bool,
    /// Crouch, 0 (standing) to 1 (full: 1.2 rad of knee flexion); 0 when absent. The
    /// articulated rider bends toward it at the physics' crouch rate; the rigid rider ignores it.
    #[serde(default)]
    pub crouch: f64,
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
    /// The ground of this run: "map" (the 32 m test pad), "world" or "none".
    pub ground: &'static str,
    /// The largest manual turn rate at the current speed, rad/s: what a full turn input gets.
    pub turn_cap_rad_s: Option<f64>,
    /// The crouch the legs hold, 0 to 1 of a full crouch; `None` when the rider cannot crouch
    /// (the rigid rider, or a rider off the pedals).
    pub crouch: Option<f64>,
    /// The rider stands on the pedals (false once stepped off, let go or lost footing).
    pub on_pedals: bool,
    /// Simulated seconds until the firmware has booted on the stand and controls can arm.
    pub boot_remaining_s: Option<f64>,
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
            rider_model: euc_physics::RIDER_MODEL,
            ground: "none",
            turn_cap_rad_s: None,
            crouch: None,
            on_pedals: false,
            boot_remaining_s: None,
        }
    }
}

/// What the free ride host reports after each sample, besides the rider's state.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct HostReport {
    pub turn_cap_rad_s: f64,
    pub crouch: Option<f64>,
    pub on_pedals: bool,
    pub boot_remaining_s: Option<f64>,
    /// Where the wheel is on its world (world runs only), for a quick reset.
    pub world_pose: Option<WorldPose>,
}

#[derive(Default)]
struct Inner {
    status: FreeRideStatus,
    /// The spec this run was built from, for a quick reset.
    spec: Option<FreeRideSpec>,
    /// Where a quick reset restarts on a world: the last clear spot the wheel passed (world runs).
    world_pose: Option<WorldPose>,
    /// The armed rider is to step off on the next sample.
    step_off: bool,
    map: Option<Arc<HeightMap>>,
    world: Option<euc_physics::WorldGround>,
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
        let (height_map, world) = match (&inner.map, &inner.world) {
            (Some(map), _) => (Some(map.as_ref().clone()), None),
            (None, Some(ground)) => (None, Some(euc_sim::recording::WorldRecord::of(ground))),
            (None, None) => return Err("Free ride recording has no ground".into()),
        };
        let rider = inner.rider;
        let assist = euc_physics::Assist::default();
        let context = euc_sim::recording::RecordingContext {
            schema: 1,
            run_mode: euc_sim::recording::RunMode::FreeRide,
            rider_model: rider.model.into(),
            height_map,
            world,
            model_aids: euc_sim::recording::ModelAids::of(&rider, &assist),
        };
        context.validate()?;
        serde_json::to_value(context).map(Some).map_err(|error| error.to_string())
    }

    pub(super) fn begin(&self, epoch: u64, map: Option<Arc<HeightMap>>, rider: euc_physics::Rider) {
        self.start(epoch, map, None, rider);
    }

    /// A free ride on a streaming world.
    pub(super) fn begin_world(&self, epoch: u64, ground: euc_physics::WorldGround, rider: euc_physics::Rider) {
        self.start(epoch, None, Some(ground), rider);
    }

    fn start(
        &self,
        epoch: u64,
        map: Option<Arc<HeightMap>>,
        world: Option<euc_physics::WorldGround>,
        rider: euc_physics::Rider,
    ) {
        let mut inner = self.lock();
        let (next_start, pending_start) = (inner.next_start, inner.pending_start);
        let free = map.is_some() || world.is_some();
        *inner = Inner {
            status: FreeRideStatus {
                epoch,
                rider_model: rider.model.id(),
                mode: if free { "free_ride" } else { "scenario" },
                reason: if free {
                    "Starting firmware on the stand".into()
                } else {
                    "Scenario controls the rider".into()
                },
                ground: if map.is_some() {
                    "map"
                } else if world.is_some() {
                    "world"
                } else {
                    "none"
                },
                ..FreeRideStatus::default()
            },
            map,
            world,
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
        if !input.crouch.is_finite() || !(0.0..=1.0).contains(&input.crouch) {
            return Err("Manual crouch is outside 0 to 1".into());
        }
        let mut inner = self.lock();
        if !input.desired_lean_rad.is_finite()
            || input.desired_lean_rad.abs() > MAX_LEAN_RAD
            || !input.heading_rate_rad_s.is_finite()
            || input.heading_rate_rad_s.abs() > max_turn_rad_s(&inner.rider)
        {
            return Err("Manual lean or turn rate is outside its limits".into());
        }
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
        let neutral =
            input.desired_lean_rad.abs() < 1e-8 && input.heading_rate_rad_s.abs() < 1e-8 && input.crouch < 1e-8;
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

    pub(super) fn update_rider(&self, epoch: u64, rider: euc_physics::RiderState, report: HostReport) {
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
        inner.status.turn_cap_rad_s = number(report.turn_cap_rad_s);
        inner.status.crouch = report.crouch.and_then(number);
        inner.status.on_pedals = report.on_pedals;
        inner.status.boot_remaining_s = report.boot_remaining_s.and_then(number);
        if report.world_pose.is_some() {
            inner.world_pose = report.world_pose;
        }
    }

    /// The armed rider of run `epoch` steps off the pedals on the next sample.
    pub(super) fn step_off(&self, epoch: u64) -> Result<(), String> {
        let mut inner = self.lock();
        if inner.status.epoch != epoch || inner.status.mode != "free_ride" {
            return Err("Step off belongs to an old run".into());
        }
        if !inner.status.armed {
            return Err("Arm the controls before stepping off".into());
        }
        inner.step_off = true;
        Ok(())
    }

    fn take_step_off(&self, epoch: u64) -> bool {
        let mut inner = self.lock();
        inner.status.epoch == epoch && std::mem::take(&mut inner.step_off)
    }

    /// Remembers the spec a run was built from, for a quick reset.
    pub(super) fn set_spec(&self, epoch: u64, spec: FreeRideSpec) {
        let mut inner = self.lock();
        if inner.status.epoch == epoch {
            inner.spec = Some(spec);
        }
    }

    /// The spec for a quick reset of run `epoch`: the same wheel, settings, rider and ground,
    /// starting where the wheel is now on a world (the 32 m test pad starts again at its origin).
    pub(super) fn restart_spec(&self, epoch: u64) -> Result<FreeRideSpec, String> {
        let inner = self.lock();
        if inner.status.epoch != epoch || inner.status.mode != "free_ride" {
            return Err("Reset belongs to an old run".into());
        }
        let mut spec = inner.spec.clone().ok_or("This run cannot be reset yet")?;
        if spec.world.is_some() {
            spec.start = inner.world_pose.or(spec.start);
        }
        Ok(spec)
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

/// Where a world free ride is: its world and the frame its physics uses.
#[tauri::command]
pub fn free_ride_world(state: State<'_, FreeRideState>, epoch: u64) -> Result<super::world::WorldFrame, String> {
    let inner = state.lock();
    if inner.status.epoch != epoch {
        return Err("World belongs to an old run".into());
    }
    let ground = inner.world.as_ref().ok_or("This run has no world")?;
    Ok(super::world::WorldFrame::of(ground))
}

#[tauri::command]
pub fn free_ride_start(
    state: State<'_, super::EngineState>,
    manual: State<'_, FreeRideState>,
    spec: FreeRideSpec,
) -> Result<(), String> {
    start_run(&state, &manual, spec)
}

/// Quick reset: a new run (a new epoch) of the same free ride, at the spot and heading where the
/// wheel of run `epoch` is now on a world (the 32 m test pad starts again at its origin). The
/// firmware boots again on the stand (`FreeRideStatus::boot_remaining_s` counts it down); nothing
/// is moved while a firmware runs.
#[tauri::command]
pub fn free_ride_restart(
    state: State<'_, super::EngineState>,
    manual: State<'_, FreeRideState>,
    epoch: u64,
) -> Result<(), String> {
    let spec = manual.restart_spec(epoch)?;
    start_run(&state, &manual, spec)
}

/// The armed rider steps off the pedals (Free Ride's X), with the physics' step-off as in the
/// step-off scenarios. The run then ends; a reset starts another.
#[tauri::command]
pub fn free_ride_step_off(state: State<'_, FreeRideState>, epoch: u64) -> Result<(), String> {
    state.step_off(epoch)
}

fn start_run(state: &State<'_, super::EngineState>, manual: &FreeRideState, spec: FreeRideSpec) -> Result<(), String> {
    spec.validate()?;
    // Validate the generated surface before replacing any active run.
    if let Some(map) = &spec.map {
        map.generate(0)?;
    }
    let ticket = manual.reserve_start()?;
    let result = super::with_runner(state, |runner| runner.start_free_ride(spec, ticket)).and_then(|result| result);
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
    /// The finite map's edges pause the run; a world has none.
    map: Option<Arc<HeightMap>>,
    boot_ready: bool,
    /// The last manual lean, turn rate and crouch noted on the timeline.
    last_motion: Option<(f64, f64, f64)>,
    /// Samples so far, to space the restart-spot checks.
    samples: u32,
    /// The last spot on a world where a reset may stand the wheel again ([`clear_spot`]).
    last_clear: Option<WorldPose>,
}

impl FreeRideHost {
    pub(super) fn new(mut session: Session, state: FreeRideState, epoch: u64, map: Option<Arc<HeightMap>>) -> Self {
        // A person rides: the manual limits apply (turning at a stop, the speed-scheduled turn cap).
        Self::physics_of(&mut session).rider().set_manual(true);
        Self {
            watch: BoardWatch::new(session.machine()),
            session,
            state,
            epoch,
            map,
            boot_ready: false,
            last_motion: None,
            samples: 0,
            last_clear: None,
        }
    }

    fn physics(&self) -> &euc_physics::Physics {
        self.session.machine().plant().live_part().expect("Free ride has a live plant").physics()
    }

    fn physics_of(session: &mut Session) -> &mut euc_physics::Physics {
        session.machine_mut().plant_mut().live_part_mut().expect("Free ride has a live plant").physics_mut()
    }

    /// Neutral controls: no lean, no turn, standing up.
    fn clear_input(&mut self) {
        let physics = Self::physics_of(&mut self.session);
        physics.clear_rider_input();
        if physics.crouch_rad() > 0.0 {
            // Only a rider still on the pedals can stand up; otherwise there is nothing to undo.
            let _ = physics.set_crouch(0.0);
        }
        self.last_motion = None;
    }

    /// What the status reports besides the rider's state.
    fn report(&mut self) -> HostReport {
        let boot_remaining_s = (!self.boot_ready).then(|| (BOOT_S - self.time_s()).max(0.0));
        let physics = Self::physics_of(&mut self.session);
        let (speed, gravity) = (physics.state().speed_mps, physics.gravity());
        let turn_cap_rad_s = physics.rider().manual_turn_cap(speed, gravity);
        let on_pedals = !physics.rider_off();
        let articulated = !physics.rider_joints().is_empty();
        let crouch = (articulated && on_pedals).then(|| physics.crouch_rad() / CROUCH_MAX_RAD);
        // Where the wheel stands now on its world, and (every few samples) whether a reset may
        // stand it there again.
        self.samples = self.samples.wrapping_add(1);
        let check = self.samples.is_multiple_of(RESTART_CHECK_SAMPLES);
        if let Some(ground) = physics.setup().world.as_ref().filter(|_| check) {
            let position = physics.position_m();
            let here = WorldPose {
                xy_m: ground.to_world([position[0], position[1]]),
                heading_rad: euc_physics::wrap_angle(ground.start().heading_rad + physics.heading_rad()),
            };
            if clear_spot(ground.world(), here.xy_m) {
                self.last_clear = Some(here);
            }
        }
        HostReport { turn_cap_rad_s, crouch, on_pedals, boot_remaining_s, world_pose: self.last_clear }
    }

    fn sample(&mut self) {
        if self.completed() || self.halted().is_some() {
            return;
        }
        if let Some(input) = self.state.current(self.epoch) {
            if self.state.take_step_off(self.epoch) {
                self.session.step_off();
            }
            self.session.set_held(false);
            self.session.set_intent(Intent::Lean(input.desired_lean_rad));
            // The rider turns its own line at the requested rate, within its manual turn cap.
            Self::physics_of(&mut self.session).rider().set_turn_rate(input.heading_rate_rad_s);
            if !self.physics().rider_joints().is_empty() && !self.physics().rider_off() {
                // The articulated rider bends at its crouch rate; the rigid one cannot crouch.
                let _ = self.session.set_crouch(input.crouch * CROUCH_MAX_RAD);
            }
            let motion = (input.desired_lean_rad, input.heading_rate_rad_s, input.crouch);
            if self.last_motion != Some(motion) {
                self.session.note(format!(
                    "Manual lean {:.2} deg, turn {:.2} deg/s, crouch {:.0} %",
                    motion.0.to_degrees(),
                    motion.1.to_degrees(),
                    motion.2 * 100.0
                ));
                self.last_motion = Some(motion);
            }
        } else if self.boot_ready {
            return;
        }
        self.session.step();
        self.watch.after_step(self.session.machine());
        let rider = self.physics().rider_state();
        let report = self.report();
        self.state.update_rider(self.epoch, rider, report);
        if let Some(why) = self.halted() {
            self.state.failed(self.epoch, &why);
            return;
        }
        if !self.boot_ready && self.time_s() >= BOOT_S {
            self.boot_ready = true;
            self.session.note("Free ride ready on the stand; manual controls require arming");
            self.state.ready(self.epoch);
        }
        if self.boot_ready {
            let edge = self.map.as_ref().map_or(f64::INFINITY, |map| {
                let clearance =
                    |position: [f64; 3], half| map.edge_clearance_m([position[0], position[1]], half).unwrap_or(-1.0);
                clearance(rider.position_m, [0.5, 0.3]).min(clearance(rider.rider_position_m, [0.8, 0.8]))
            });
            if edge <= euc_physics::map::MAP_EDGE_PAUSE_M {
                self.session.note("Map edge guard paused the run");
                self.state.failed(self.epoch, "Map edge. Reset before the finite ground ends");
                self.clear_input();
            } else if !rider.attached {
                if matches!(self.physics().left_pedals(), Some((_, euc_physics::Leaving::SteppedOff))) {
                    self.session.note("Rider stepped off; manual controls are disarmed");
                    self.state.failed(self.epoch, "Stepped off. Reset to start another free ride");
                } else {
                    self.session.note("Rider detached; manual controls are disarmed");
                    self.state.failed(self.epoch, "Rider detached. Reset to start another free ride");
                }
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
            map: Some(GeneratedMapSpec::gentle_slope(17)),
            world: None,
            start: None,
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
            crouch: 0.0,
        }
    }

    /// The manual turn cap at a stop of the default rider: the largest rate a packet may carry.
    fn max_turn() -> f64 {
        max_turn_rad_s(&euc_physics::Rider::default())
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
    fn spec_accepts_only_the_three_supported_rider_models() {
        for (id, model) in [
            ("rigid_ankle_v1", RiderModel::RigidAnkleV1),
            ("articulated_sagittal_v1", RiderModel::ArticulatedSagittalV1),
            ("articulated_contact_v1", RiderModel::ArticulatedContactV1),
        ] {
            let spec = spec(id);
            spec.validate().unwrap();
            assert_eq!(spec.rider().unwrap().model, model);
            spec.map.as_ref().unwrap().generate(19).unwrap().validate().unwrap();
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
        invalid.map.as_mut().unwrap().grade_pct[0] = f64::NAN;
        assert!(invalid.validate().is_err());
        invalid.map = Some(GeneratedMapSpec { schema: 99, ..GeneratedMapSpec::default() });
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
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, max_turn() + 1e-8, -max_turn() - 1e-8] {
            assert!(state.input(ManualInput { heading_rate_rad_s: value, armed: true, ..packet(2) }).is_err());
            assert_eq!(mailbox(&state), before);
        }
        for value in [f64::NAN, f64::INFINITY, -1e-8, 1.0 + 1e-8] {
            assert!(state.input(ManualInput { crouch: value, armed: true, ..packet(2) }).is_err());
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
                    heading_rate_rad_s: sign * max_turn(),
                    armed: true,
                    crouch: 1.0,
                    ..packet(seq)
                })
                .unwrap();
            let input = state.current(EPOCH).unwrap();
            assert_eq!(input.desired_lean_rad, sign * MAX_LEAN_RAD);
            assert_eq!(input.heading_rate_rad_s, sign * max_turn());
            assert_eq!(input.crouch, 1.0);
        }
        // The cap at a stop is the rider's: well above the old fixed 30°/s.
        assert!(max_turn() > 30_f64.to_radians());
    }

    #[test]
    fn stepping_off_needs_armed_controls_of_the_current_run_and_happens_once() {
        let unarmed = state();
        assert!(unarmed.step_off(EPOCH).is_err(), "not armed");
        let state = armed_state();
        assert!(state.step_off(EPOCH + 1).is_err(), "an old run");
        assert!(!state.take_step_off(EPOCH));
        state.step_off(EPOCH).unwrap();
        assert!(!state.take_step_off(EPOCH + 1), "another run's host does not take it");
        assert!(state.take_step_off(EPOCH));
        assert!(!state.take_step_off(EPOCH), "once");
    }

    #[test]
    fn a_reset_stands_the_wheel_only_on_a_clear_level_spot() {
        let world = euc_world::World::new(WorldChoice { seed: 3, densities: Default::default() }.spec()).unwrap();
        let spawn = world.spawn().position;
        assert!(clear_spot(&world, [spawn[0], spawn[1]]), "the spawn is clear");
        let near = world.solids_in(euc_world::Rect::new([-60.0, -60.0], [60.0, 60.0]));
        let solid = near.first().expect("a city has solids near the spawn");
        assert!(!clear_spot(&world, [solid.centre[0], solid.centre[1]]), "not inside a solid: {solid:?}");
        let beside = [solid.centre[0] + solid.half[0].max(solid.half[1]) + 0.5, solid.centre[1]];
        assert!(!clear_spot(&world, beside), "nor right beside one");
    }

    #[test]
    fn a_crouch_is_motion_and_blocks_the_neutral_gate() {
        let state = state();
        state.input(ManualInput { crouch: 0.5, ..packet(1) }).unwrap();
        assert!(state.lock().neutral_since.is_none(), "a crouch is not neutral");
        state.input(packet(2)).unwrap();
        assert!(state.lock().neutral_since.is_some());
    }

    #[test]
    fn a_quick_reset_restarts_the_same_ride_where_the_wheel_is() {
        let files = euc_sim::WheelFiles::load(&euc_profile::ProfileSource::Builtin, "begode-extreme").unwrap();
        let rider = Physics::new(Setup::new(files.body.clone())).unwrap().rider_state();
        // The test pad restarts at its origin: the same spec.
        let state = state();
        assert!(state.restart_spec(EPOCH).is_err(), "nothing to restart before the build remembers its spec");
        state.set_spec(EPOCH, spec("articulated_sagittal_v1"));
        let pad = state.restart_spec(EPOCH).unwrap();
        assert!(pad.map.is_some() && pad.start.is_none());
        assert!(state.restart_spec(EPOCH + 1).is_err(), "a reset belongs to its run");
        // A world run restarts where the host last reported the wheel; before that, at the spawn.
        let choice = WorldChoice { seed: 3, densities: Default::default() };
        let world = FreeRideSpec { map: None, world: Some(choice), ..spec("articulated_sagittal_v1") };
        let ground = euc_physics::WorldGround::new(
            choice.spec(),
            euc_physics::WorldStart { xy_m: [0.0, 0.0], heading_rad: 0.0 },
        )
        .unwrap();
        state.begin_world(EPOCH + 1, ground, Rider::default());
        state.set_spec(EPOCH + 1, world.clone());
        assert_eq!(state.restart_spec(EPOCH + 1).unwrap().start, None);
        let pose = WorldPose { xy_m: [12.5, -3.0], heading_rad: 0.4 };
        let report = HostReport {
            turn_cap_rad_s: 2.5,
            crouch: Some(0.5),
            on_pedals: true,
            boot_remaining_s: Some(1.0),
            world_pose: Some(pose),
        };
        state.update_rider(EPOCH + 1, rider, report);
        let restart = state.restart_spec(EPOCH + 1).unwrap();
        assert_eq!(restart.start, Some(pose));
        restart.validate().unwrap();
        let status = state.lock().status.clone();
        assert_eq!(
            (status.turn_cap_rad_s, status.crouch, status.on_pedals, status.boot_remaining_s),
            (Some(2.5), Some(0.5), true, Some(1.0))
        );
        // A start place needs a world, and must be finite.
        assert!(FreeRideSpec { start: Some(pose), ..spec("articulated_sagittal_v1") }.validate().is_err());
        let nan = WorldPose { heading_rad: f64::NAN, ..pose };
        assert!(FreeRideSpec { start: Some(nan), ..world }.validate().is_err());
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
            ("articulated_contact_v1", euc_sim::recording::RiderModel::ArticulatedContactV1, 6),
        ] {
            let spec = spec(id);
            let map = Arc::new(spec.map.as_ref().unwrap().generate(37).unwrap());
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
            // Contact feet add the roll aid on the upper body; the welded models carry none.
            let upper = if recorded_model == euc_sim::recording::RiderModel::ArticulatedContactV1 {
                (assist.roll_stiffness, assist.roll_damping)
            } else {
                (0.0, 0.0)
            };
            assert_eq!(
                (context.model_aids.upper_body_roll_stiffness, context.model_aids.upper_body_roll_damping),
                upper
            );
            assert_eq!(value["model_aids"].get("upper_body_roll_stiffness").is_some(), upper.0 > 0.0);
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
            crouch: 0.0,
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
            Self::on_ground(model, Some(map), None)
        }

        fn on_world(model: &str, seed: u64) -> Self {
            Self::on_ground(model, None, Some(WorldChoice { seed, densities: Default::default() }))
        }

        fn on_ground(model: &str, map: Option<GeneratedMapSpec>, world: Option<WorldChoice>) -> Self {
            require_cached_stock();
            let state = FreeRideState::default();
            let spec = FreeRideSpec {
                wheel: "begode-extreme".into(),
                settings: "neutral".into(),
                rider_model: model.into(),
                map,
                world,
                start: None,
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

    /// One sample ridden by hand, without the rig's 10° roll guard (a full turn leans 35°): the
    /// firmware must neither halt, fault nor cut out.
    fn hand_sample(rig: &mut Rig, lean: f64, turn: f64, crouch: f64) -> Snapshot {
        rig.seq += 1;
        let input = ManualInput { heading_rate_rad_s: turn, crouch, source: rig.source, ..packet(rig.seq, lean, true) };
        rig.state.input(input).unwrap();
        rig.host.step(STEP_S);
        assert!(rig.host.halted().is_none(), "native halt: {:?}", rig.host.halted());
        let mut events = Vec::new();
        rig.host.drain_events(&mut events);
        assert!(!events.iter().any(|e| matches!(e.kind, EventKind::Fault | EventKind::Cutoff)), "{events:?}");
        rig.host.drain_ble_tx(&mut Vec::new());
        rig.host.drain_tones(&mut Vec::new());
        let status = rig.state.lock().status.clone();
        assert!(status.ready && status.armed, "manual run lost readiness: {status:?}");
        rig.snapshot()
    }

    /// The manual lean that rides at `target` m/s (a rider's speed sense), within the packet limit.
    fn lean_for(rig: &Rig, target: f64) -> f64 {
        let speed = f64::from(rig.snapshot().signals.speed_kmh) / 3.6;
        ((target - speed) * 0.1).clamp(-MAX_LEAN_RAD, MAX_LEAN_RAD)
    }

    /// FR2b on the stock firmware: a pivot at a stop, a full turn at 3 m/s at the manual turn cap,
    /// and (articulated) a full crouch while riding.
    fn by_hand(model: &str) {
        let mut rig = Rig::new(model);
        rig.boot_and_arm();
        let full = max_turn_rad_s(&euc_physics::Rider::default());
        assert!((rig.state.lock().status.turn_cap_rad_s.unwrap() - full).abs() < 1e-9, "the full cap at a stop");
        // A pivot at a stop: 1.5 s of full left turn input.
        let (mut drift, mut pivot, mut last) = (0.0_f32, 0.0, heading(&rig.snapshot()));
        for _ in 0..180 {
            let snapshot = hand_sample(&mut rig, 0.0, full, 0.0);
            drift = drift.max(snapshot.signals.speed_kmh.abs());
            pivot += angle_delta(heading(&snapshot), last);
            last = heading(&snapshot);
        }
        println!(
            "{model} by hand: pivot {pivot:.3} rad in 1.5 s (asked {:.2}), speed up to {drift:.2} km/h",
            1.5 * full
        );
        assert!(pivot > 2.0, "{model} turns on the spot: {pivot} rad");
        assert!(drift < 2.0, "{model} stays put while pivoting: {drift} km/h");
        // Up to 3 m/s straight, then a full left turn for 2 s.
        for _ in 0..480 {
            let lean = lean_for(&rig, 3.0);
            hand_sample(&mut rig, lean, 0.0, 0.0);
        }
        let (mut yaw, mut speed, mut roll, n) = (0.0, 0.0, 0.0, 240);
        let mut last = heading(&rig.snapshot());
        for sample in 0..n + 120 {
            let lean = lean_for(&rig, 3.0);
            let snapshot = hand_sample(&mut rig, lean, full, 0.0);
            let now = heading(&snapshot);
            if sample >= 120 {
                // The last 2 s, after the lean into the turn has built up.
                yaw += angle_delta(now, last) / STEP_S / f64::from(n);
                speed += f64::from(snapshot.signals.speed_kmh) / 3.6 / f64::from(n);
                roll += f64::from(snapshot.signals.roll_deg) / f64::from(n);
            }
            last = now;
        }
        let status = rig.state.lock().status.clone();
        let lateral = 9.81 * euc_physics::Rider::default().control.manual_turn_lean_max_deg.to_radians().tan();
        println!(
            "{model} by hand: full turn at {speed:.2} m/s: yaw {yaw:.3} rad/s, lateral {:.2} m/s² (manual limit {lateral:.2}), roll {roll:.1}°, status cap {:?}",
            yaw * speed,
            status.turn_cap_rad_s
        );
        assert!(speed > 2.0, "{model} keeps riding through the turn: {speed} m/s");
        assert!((yaw * speed - lateral).abs() < 0.2 * lateral, "{model} turns at the lateral limit: {}", yaw * speed);
        assert!(roll.abs() > 25.0, "{model} leans into the turn: {roll}°");
        // (Articulated) a full crouch while riding on, still turning at half the cap so that the
        // run stays on the 32 m pad.
        if rig.articulated {
            let mut pitch = 0.0_f32;
            for _ in 0..120 {
                let lean = lean_for(&rig, 3.0);
                pitch = pitch.max(hand_sample(&mut rig, lean, full / 2.0, 1.0).signals.pitch_deg.abs());
            }
            let crouch = rig.state.lock().status.crouch.unwrap();
            println!("{model} by hand: crouch {crouch:.2} while riding, largest pitch {pitch:.2}°");
            assert!((crouch - 1.0).abs() < 1e-6, "the legs reach a full crouch in a second: {crouch}");
            assert!(pitch < 10.0);
        } else {
            assert_eq!(rig.state.lock().status.crouch, None, "the rigid rider cannot crouch");
        }
        assert!(rig.state.lock().status.on_pedals);
        // X: the armed rider steps off, and the run ends with its own reason.
        rig.state.step_off(EPOCH).unwrap();
        for _ in 0..12 {
            rig.host.step(STEP_S);
        }
        let status = rig.state.lock().status.clone();
        assert!(!status.on_pedals && !status.armed && status.reason.starts_with("Stepped off"), "{status:?}");
    }

    /// FR2b quick reset on a world: a new run starts where the wheel stood, at its heading, with
    /// the firmware booting again on the stand.
    fn quick_reset(model: &str) {
        let mut rig = Rig::on_world(model, 42);
        rig.boot_and_arm();
        for _ in 0..120 {
            hand_sample(&mut rig, 0.0, max_turn_rad_s(&euc_physics::Rider::default()), 0.0);
        }
        for _ in 0..60 {
            hand_sample(&mut rig, 0.0, 0.0, 0.0);
        }
        let ground = rig.state.lock().world.clone().unwrap();
        let snapshot = rig.snapshot();
        let here = ground.to_world([f64::from(snapshot.bodies[0].pos[0]), f64::from(snapshot.bodies[0].pos[1])]);
        let facing = ground.start().heading_rad + heading(&snapshot);
        let spec = rig.state.restart_spec(EPOCH).unwrap();
        let start = spec.start.expect("a world run restarts at its pose");
        println!("{model} quick reset: from {here:?} facing {facing:.3} to {start:?}");
        assert!((start.xy_m[0] - here[0]).hypot(start.xy_m[1] - here[1]) < 0.05, "{start:?} vs {here:?}");
        assert!(angle_delta(start.heading_rad, facing).abs() < 0.02);
        assert!(angle_delta(start.heading_rad, ground.start().heading_rad).abs() > 1.0, "the pivot turned it");
        let state = rig.state.clone();
        let built = super::super::firmware::build_free_host(&spec, "stock", EPOCH + 1, state.clone(), &mut |_| {})
            .expect("the restarted run builds");
        let mut host = built.host;
        let restarted = state.lock().world.clone().unwrap();
        assert_eq!(restarted.start().xy_m, start.xy_m);
        assert_eq!(restarted.start().heading_rad, start.heading_rad);
        assert!(!state.lock().status.ready, "the firmware boots again");
        let mut countdown = Vec::new();
        while !state.lock().status.ready {
            host.step(0.5);
            countdown.push(state.lock().status.boot_remaining_s);
            assert!(host.halted().is_none() && countdown.len() < 20, "{countdown:?}");
        }
        println!("{model} quick reset: boot countdown {countdown:?}");
        assert!(countdown.windows(2).all(|w| w[1].unwrap_or(0.0) <= w[0].unwrap_or(0.0)), "{countdown:?}");
        assert_eq!(state.lock().status.epoch, EPOCH + 1);
        // On the stand the wheel stays where it was put (it settles by millimetres).
        let again = state.restart_spec(EPOCH + 1).unwrap().start.unwrap();
        assert!((again.xy_m[0] - start.xy_m[0]).hypot(again.xy_m[1] - start.xy_m[1]) < 0.02, "{again:?} vs {start:?}");
        assert!(angle_delta(again.heading_rad, start.heading_rad).abs() < 0.01, "{again:?} vs {start:?}");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; FR2b manual controls on the native FreeRideHost"]
    fn stock_articulated_pivots_carves_and_crouches_by_hand() {
        by_hand("articulated_sagittal_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; FR2b manual controls on the native FreeRideHost"]
    fn stock_rigid_pivots_and_carves_by_hand() {
        by_hand("rigid_ankle_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; FR2b quick reset on the streaming world"]
    fn stock_articulated_quick_reset_restarts_where_the_wheel_is() {
        quick_reset("articulated_sagittal_v1");
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

    // The streaming world: the ground follows the wheel and nothing pauses at an edge. Rides the
    // spawn street straight at 7.2 km/h for 30 s (about 60 m, past the test pad's 32 m) through
    // the real factory, and reports the host's step budget: wall time per 1/120 s sample, which
    // the studio's runner must keep under 8.33 ms for a real-time factor of 1.
    fn world_ride(model: &str) {
        let mut rig = Rig::on_world(model, 42);
        rig.boot_and_arm();
        let ground = rig.state.lock().world.clone().expect("a world run keeps its ground");
        assert_eq!(rig.state.lock().status.ground, "world");
        let context = rig.state.recording_context(EPOCH).unwrap().unwrap();
        assert_eq!(context["world"]["spec"]["seed"], 42);
        let start = rig.snapshot();
        let radius = f64::from(start.bodies[0].pos[2]) - ground.height_m([0.0, 0.0]);
        let (mut lowest, mut highest, mut worst_contact) = (f64::INFINITY, f64::NEG_INFINITY, 0.0_f64);
        let mut stepping = Duration::ZERO;
        let samples = 3600;
        for sample in 0..samples {
            let began = Instant::now();
            let snapshot = drive_sample(&mut rig, 2.0, 0.0);
            stepping += began.elapsed();
            let status = rig.state.lock().status.clone();
            assert!(status.ready && status.armed, "{model} world ride stopped at {:.3}: {status:?}", snapshot.t_s);
            let [x, y, z] = snapshot.bodies[0].pos.map(f64::from);
            let height = ground.height_m([x, y]);
            lowest = lowest.min(height);
            highest = highest.max(height);
            worst_contact = worst_contact.max((z - radius - height).abs());
            if sample % 600 == 599 {
                let w = ground.to_world([x, y]);
                println!(
                    "world-ride {model} t={:.2} physics=({x:.2}, {y:.2}) world=({:.2}, {:.2}) ground_m={height:.4} \
                     speed_kmh={:.2} pitch_deg={:.2}",
                    snapshot.t_s, w[0], w[1], snapshot.signals.speed_kmh, snapshot.signals.pitch_deg
                );
            }
        }
        let end = rig.snapshot();
        let path = rig.state.lock().status.path_length_m.unwrap();
        let simulated = end.t_s - start.t_s;
        let per_sample_ms = stepping.as_secs_f64() * 1e3 / f64::from(samples);
        println!(
            "world-ride result {model}: simulated_s={simulated:.3} path_m={path:.2} ground_range_m=[{lowest:.4}, \
             {highest:.4}] worst_contact_m={worst_contact:.4} host_ms_per_sample={per_sample_ms:.3} \
             budget_used={:.0}% rtf_host_only={:.2}",
            per_sample_ms / (1e3 / 120.0) * 100.0,
            simulated / stepping.as_secs_f64()
        );
        assert!((simulated - 30.0).abs() < 1e-6);
        assert!(path > 40.0, "the wheel must ride past the old 32 m pad: {path}");
        assert!(worst_contact < 0.05, "the tire must stay on the drawn ground: {worst_contact}");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; real-host articulated ride on the streaming world for 30 s"]
    fn stock_articulated_rides_the_streaming_world() {
        world_ride("articulated_sagittal_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; real-host rigid ride on the streaming world for 30 s"]
    fn stock_rigid_rides_the_streaming_world() {
        world_ride("rigid_ankle_v1");
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
            map: Some(GeneratedMapSpec::flat(17)),
            world: None,
            start: None,
        };
        let map = Arc::new(spec.map.as_ref().unwrap().generate(EPOCH as u32).unwrap());
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
        FreeRideHost::new(Session::new(machine, loaded.map.as_ref(), 120.0), state, EPOCH, Some(map))
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
