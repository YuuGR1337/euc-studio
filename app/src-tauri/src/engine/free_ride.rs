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

/// The watchdog: an input older than this counts as neutral controls (no lean, turn or crouch).
const INPUT_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_LEAN_RAD: f64 = 10.0 * std::f64::consts::PI / 180.0;
/// The largest drive ask a packet may carry, m/s² either way (the view scales its throttle to the
/// status caps, which stay well below this).
const MAX_ACCEL_MPS2: f64 = 12.0;
/// Time constant of each of the two smoothing stages of the achieved acceleration in the status
/// (the speed, then its change), seconds: the wheel's speed ripples at a few hertz.
const ACCEL_FILTER_S: f64 = 0.3;
/// A full crouch input, radians of knee flexion (`Physics::set_crouch` takes 0 to 1.2).
const CROUCH_MAX_RAD: f64 = 1.2;
/// Simulated seconds the firmware boots on the stand before the stand lets go and the controls
/// are live. Stock, ours-compat and ours-clean all start their balance loop between 1.20 and
/// 1.25 s after power-on (a stand released at 0.5 s drives nothing until then); a release at
/// 1.5 s keeps 0.25 s of margin, and the first ten seconds after it match a 4 s release on both
/// rider models. A respawn (build plus an unthrottled boot) then takes about 0.7 s of wall time.
const BOOT_S: f64 = 1.5;

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
    /// Ignored: play mode has no arming step. Kept (optional) so older frontends' packets still
    /// parse.
    #[serde(default)]
    #[allow(dead_code)]
    pub armed: bool,
    /// Crouch, 0 (standing) to 1 (full: 1.2 rad of knee flexion); 0 when absent. The
    /// articulated rider bends toward it at the physics' crouch rate; the rigid rider ignores it.
    #[serde(default)]
    pub crouch: f64,
    /// Drive mode: the asked acceleration, m/s², forward positive, net of rolling resistance and
    /// air drag, at most 12 either way. When present the rider drives (`Intent::Accel`) and
    /// `desired_lean_rad` must be 0; absent, the rider holds `desired_lean_rad`.
    #[serde(default)]
    pub desired_accel_mps2: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FreeRideStatus {
    pub epoch: u64,
    pub mode: &'static str,
    pub ready: bool,
    /// The same as `ready` (play mode has no arming step); kept for older frontends.
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
    /// Simulated seconds until the firmware has booted on the stand and the controls are live.
    pub boot_remaining_s: Option<f64>,
    /// Drive mode: the strongest forward acceleration the rider's drive lean limit can ask for at
    /// the speed now, m/s², net of drag. The firmware may deliver less; asking more overleans.
    pub accel_cap_mps2: Option<f64>,
    /// Drive mode: the strongest braking at the speed now, m/s² (positive), drag included.
    pub brake_cap_mps2: Option<f64>,
    /// The wheel's achieved acceleration along its heading, m/s², smoothed twice over 0.3 s.
    pub accel_mps2: Option<f64>,
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
            accel_cap_mps2: None,
            brake_cap_mps2: None,
            accel_mps2: None,
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
    /// Drive caps now: forward acceleration and braking, m/s².
    pub drive_caps: (f64, f64),
    /// Achieved acceleration, m/s², smoothed.
    pub accel_mps2: f64,
}

#[derive(Default)]
struct Inner {
    status: FreeRideStatus,
    /// The spec this run was built from, for a quick reset.
    spec: Option<FreeRideSpec>,
    /// Where a quick reset restarts on a world: the last clear spot the wheel passed (world runs).
    world_pose: Option<WorldPose>,
    /// The rider is to step off on the next sample.
    step_off: bool,
    map: Option<Arc<HeightMap>>,
    world: Option<euc_physics::WorldGround>,
    latest: Option<ManualInput>,
    last_seq: Option<u32>,
    received: Option<Instant>,
    next_start: u64,
    pending_start: Option<u64>,
    rider: euc_physics::Rider,
}

impl Inner {
    fn input_expired(&self, now: Instant) -> bool {
        self.received.is_none_or(|at| now.saturating_duration_since(at) >= INPUT_TIMEOUT)
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
            rider_joints: euc_sim::recording::rider_joints(rider.model),
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

    /// Boot is done: the controls are live at once (play mode, no arming step). The `armed`
    /// status stays true from here on for older frontends.
    pub(super) fn ready(&self, epoch: u64) {
        let mut inner = self.lock();
        if inner.status.epoch == epoch {
            inner.status.ready = true;
            inner.status.armed = true;
            inner.status.reason = "Ready. Ride".into();
        }
    }

    /// The run cannot go on (a simulator halt, the test pad's edge).
    pub(super) fn failed(&self, epoch: u64, reason: &str) {
        let mut inner = self.lock();
        if inner.status.epoch == epoch {
            inner.status.ready = false;
            inner.status.armed = false;
            inner.status.reason = reason.into();
            inner.latest = None;
        }
    }

    /// The rider has left the pedals; the run goes on (play mode: a crash never pauses).
    pub(super) fn off_pedals(&self, epoch: u64, reason: &str) {
        let mut inner = self.lock();
        if inner.status.epoch == epoch {
            inner.status.reason = reason.into();
        }
    }

    /// Takes a manual input packet. Play mode: no arming, so its `armed` field is ignored and the
    /// source may change at any time; the host uses the newest packet once boot is done, and none
    /// (neutral controls) when it is older than [`INPUT_TIMEOUT`].
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
        if let Some(accel) = input.desired_accel_mps2 {
            if !accel.is_finite() || accel.abs() > MAX_ACCEL_MPS2 {
                return Err("Drive acceleration is outside ±12 m/s²".into());
            }
            if input.desired_lean_rad != 0.0 {
                return Err("Send a lean or a drive acceleration, not both".into());
            }
        }
        if inner.status.mode != "free_ride" || inner.status.epoch != input.run_epoch {
            return Err("Manual input belongs to an old run".into());
        }
        if inner.last_seq.is_some_and(|seq| input.seq <= seq) {
            return Err("Manual input sequence is stale".into());
        }
        inner.status.source = Some(input.source);
        inner.last_seq = Some(input.seq);
        inner.received = Some(Instant::now());
        inner.latest = Some(input);
        Ok(())
    }

    /// Kept for older frontends: there is no arming step any more, so this only checks that the
    /// run is ready.
    pub(super) fn arm(&self, epoch: u64, _source: InputSource) -> Result<(), String> {
        let inner = self.lock();
        if inner.status.epoch != epoch || inner.status.mode != "free_ride" || !inner.status.ready {
            return Err("Free ride is not ready for this run".into());
        }
        Ok(())
    }

    /// Drops the manual input (the studio paused, a view reconnected): neutral controls until a
    /// new packet arrives.
    pub(super) fn clear_input(&self) {
        let mut inner = self.lock();
        inner.latest = None;
        inner.received = None;
    }

    /// The studio paused: the input clears (neutral controls once the run plays again).
    pub(super) fn pause_epoch(&self, epoch: u64) -> Result<(), String> {
        let mut inner = self.lock();
        if inner.status.epoch != epoch || inner.status.mode != "free_ride" {
            return Err("Pause belongs to an old run".into());
        }
        inner.latest = None;
        inner.received = None;
        Ok(())
    }

    pub(super) fn reason(&self, epoch: u64, reason: String) {
        let mut inner = self.lock();
        if inner.status.epoch == epoch {
            inner.status.reason = reason;
        }
    }

    /// The input the host rides with now: the newest packet once boot is done, unless older than
    /// [`INPUT_TIMEOUT`]; `None` means neutral controls.
    pub(super) fn current(&self, epoch: u64) -> Option<ManualInput> {
        let inner = self.lock();
        (inner.status.epoch == epoch && inner.status.ready && !inner.input_expired(Instant::now()))
            .then_some(inner.latest)
            .flatten()
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
        inner.status.accel_cap_mps2 = number(report.drive_caps.0);
        inner.status.brake_cap_mps2 = number(report.drive_caps.1);
        inner.status.accel_mps2 = number(report.accel_mps2);
        if report.world_pose.is_some() {
            inner.world_pose = report.world_pose;
        }
    }

    /// The rider of run `epoch` steps off the pedals on the next sample (once boot is done).
    pub(super) fn step_off(&self, epoch: u64) -> Result<(), String> {
        let mut inner = self.lock();
        if inner.status.epoch != epoch || inner.status.mode != "free_ride" {
            return Err("Step off belongs to an old run".into());
        }
        if !inner.status.ready {
            return Err("The wheel is still booting".into());
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
    pub(super) fn restart_spec(&self, epoch: u64, to_start: bool) -> Result<FreeRideSpec, String> {
        let inner = self.lock();
        if inner.status.epoch != epoch || inner.status.mode != "free_ride" {
            return Err("Reset belongs to an old run".into());
        }
        let mut spec = inner.spec.clone().ok_or("This run cannot be reset yet")?;
        if spec.world.is_some() {
            // To the start: the world's spawn. Else the last clear spot, or this run's own start.
            spec.start = if to_start { None } else { inner.world_pose.or(spec.start) };
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

/// Respawn: a new run (a new epoch) of the same free ride. By default it starts at the last clear
/// spot the wheel of run `epoch` passed with the rider on its pedals, at its heading; with `to_start`,
/// at the world's spawn (the 32 m test pad always starts again at its origin). The firmware boots
/// again on the stand, unthrottled (`FreeRideStatus::boot_remaining_s` counts it down); nothing is
/// moved while a firmware runs.
#[tauri::command]
pub fn free_ride_restart(
    state: State<'_, super::EngineState>,
    manual: State<'_, FreeRideState>,
    epoch: u64,
    to_start: Option<bool>,
) -> Result<(), String> {
    let spec = manual.restart_spec(epoch, to_start.unwrap_or(false))?;
    start_run(&state, &manual, spec)
}

/// The rider steps off the pedals (Free Ride's X), with the physics' step-off as in the
/// step-off scenarios. The run goes on (a crash never pauses); a respawn starts another.
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

/// Plays a paused free ride again (play mode has no arming step; the name stays for the
/// frontend).
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
    /// The last manual lean (or drive ask), turn rate and crouch noted on the timeline.
    last_motion: Option<(Option<f64>, f64, f64, f64)>,
    /// The smoothed speed, m/s, and the achieved acceleration, m/s², smoothed.
    smooth_speed: Option<f64>,
    accel_mps2: f64,
    /// Samples so far, to space the restart-spot checks.
    samples: u32,
    /// The last spot on a world where a reset may stand the wheel again ([`clear_spot`]).
    last_clear: Option<WorldPose>,
    /// The test pad's edge stopped the run (old pad runs only).
    edge_paused: bool,
    /// The rider's leaving the pedals is on the timeline.
    off_noted: bool,
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
            smooth_speed: None,
            accel_mps2: 0.0,
            samples: 0,
            last_clear: None,
            edge_paused: false,
            off_noted: false,
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
        let drive_caps = physics.drive_caps();
        // The achieved acceleration: the change of the smoothed speed, smoothed again.
        let dt = self.session.sample_period_s();
        let gain = dt / (ACCEL_FILTER_S + dt);
        if let Some(smooth) = self.smooth_speed {
            let next = smooth + (speed - smooth) * gain;
            self.accel_mps2 += ((next - smooth) / dt - self.accel_mps2) * gain;
            self.smooth_speed = Some(next);
        } else {
            self.smooth_speed = Some(speed);
        }
        let physics = Self::physics_of(&mut self.session);
        // Where the wheel stands now on its world, and (every few samples, while the rider is still on
        // the pedals) whether a respawn may stand it there again: never where it fell.
        self.samples = self.samples.wrapping_add(1);
        let check = self.samples.is_multiple_of(RESTART_CHECK_SAMPLES);
        if let Some(ground) = physics.setup().world.as_ref().filter(|_| check && on_pedals) {
            let position = physics.position_m();
            let here = WorldPose {
                xy_m: ground.to_world([position[0], position[1]]),
                heading_rad: euc_physics::wrap_angle(ground.start().heading_rad + physics.heading_rad()),
            };
            if clear_spot(ground.world(), here.xy_m) {
                self.last_clear = Some(here);
            }
        }
        HostReport {
            turn_cap_rad_s,
            crouch,
            on_pedals,
            boot_remaining_s,
            world_pose: self.last_clear,
            drive_caps,
            accel_mps2: self.accel_mps2,
        }
    }

    fn sample(&mut self) {
        if self.halted().is_some() || self.edge_paused {
            return;
        }
        if self.boot_ready {
            if self.state.take_step_off(self.epoch) {
                self.session.step_off();
            }
            match self.state.current(self.epoch) {
                Some(input) => self.apply(input),
                // No input, or a stale one: neutral controls (once), and the wheel goes on.
                None if self.last_motion.is_some() => self.clear_input(),
                None => {}
            }
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
            // Play mode: the stand lets go and the controls are live at once; the idle wheel balances.
            self.boot_ready = true;
            self.session.set_held(false);
            self.session.note("Free ride ready: the stand lets go, ride at once");
            self.state.ready(self.epoch);
        }
        if self.boot_ready {
            let edge = self.map.as_ref().map_or(f64::INFINITY, |map| {
                let clearance =
                    |position: [f64; 3], half| map.edge_clearance_m([position[0], position[1]], half).unwrap_or(-1.0);
                clearance(rider.position_m, [0.5, 0.3]).min(clearance(rider.rider_position_m, [0.8, 0.8]))
            });
            if edge <= euc_physics::map::MAP_EDGE_PAUSE_M {
                // The finite test pad (old runs only): the ground ends, so the run pauses here.
                self.session.note("Map edge guard paused the run");
                self.state.failed(self.epoch, "Map edge. Reset before the finite ground ends");
                self.clear_input();
                self.edge_paused = true;
            } else if !rider.attached && !self.off_noted {
                // A crash never pauses: firmware, motor and world go on.
                self.off_noted = true;
                let stepped = matches!(self.physics().left_pedals(), Some((_, euc_physics::Leaving::SteppedOff)));
                // The Free Ride view toasts these two texts.
                self.session.note(if stepped { "Stepped off" } else { "Rider detached" });
                self.state.off_pedals(self.epoch, "Off the pedals. Press R to respawn");
                self.clear_input();
            }
        }
    }

    /// Rides with one manual input: lean or drive ask, turn and crouch.
    fn apply(&mut self, input: ManualInput) {
        self.session.set_intent(input.desired_accel_mps2.map_or(Intent::Lean(input.desired_lean_rad), Intent::Accel));
        // The rider turns its own line at the requested rate, within its manual turn cap.
        Self::physics_of(&mut self.session).rider().set_turn_rate(input.heading_rate_rad_s);
        if !self.physics().rider_joints().is_empty() && !self.physics().rider_off() {
            // The articulated rider bends at its crouch rate; the rigid one cannot crouch.
            let _ = self.session.set_crouch(input.crouch * CROUCH_MAX_RAD);
        }
        let motion = (input.desired_accel_mps2, input.desired_lean_rad, input.heading_rate_rad_s, input.crouch);
        if self.last_motion != Some(motion) {
            let ask = motion.0.map_or_else(
                || format!("Manual lean {:.2} deg", motion.1.to_degrees()),
                |accel| format!("Drive {accel:.2} m/s²"),
            );
            self.session.note(format!(
                "{ask}, turn {:.2} deg/s, crouch {:.0} %",
                motion.2.to_degrees(),
                motion.3 * 100.0
            ));
            self.last_motion = Some(motion);
        }
    }
}

/// The tread's knob repeats round the tyre: "staggered rectangular knobs, 15 repeats of 24 deg",
/// measured on the scan (profiles/begode-extreme/geometry.toml, [tire] tread).
const KNOB_REPEATS: f64 = 15.0;

/// A sample of the ride's sound from the snapshot: the motor's electrical frequency (pole pairs ×
/// wheel revolutions per second) and phase current, the speed, the tread's knob rate and the
/// ground under the wheel (a world's; asphalt and dry off one). None while a signal is missing.
fn ride_sound(
    snapshot: &Snapshot,
    pole_pairs: u32,
    surface: Option<euc_world::Surface>,
) -> Option<super::native_audio::Ride> {
    let signals = &snapshot.signals;
    let wheel_rev_s = f64::from(signals.wheel_rad_s).abs() / std::f64::consts::TAU;
    let ride = super::native_audio::Ride {
        t_s: snapshot.t_s,
        electrical_hz: f64::from(pole_pairs) * wheel_rev_s,
        phase_current_a: f64::from(signals.iq_a).abs(),
        speed_mps: f64::from(signals.speed_kmh).abs() / 3.6,
        knob_hz: KNOB_REPEATS * wheel_rev_s,
        surface: surface.map_or(euc_world::Material::Asphalt, |s| s.material),
        wet: surface.is_some_and(|s| s.wet),
    };
    ride.valid().then_some(ride)
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
        // Nor does the machine keep its diagnostic log once the session has read it.
        self.session.forget_seen_diagnostics();
        self.watch.drain_events(out);
    }
    fn drain_ble_tx(&mut self, out: &mut Vec<(f64, u8)>) {
        // A free ride has no end: take the bytes, so that the board does not keep them all.
        self.watch.take_ble_tx(self.session.machine_mut(), out);
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
    // No run limit: a free ride goes on until a respawn or another run replaces it.
    fn outputs(&self) -> Option<Outputs> {
        Some(BoardWatch::outputs(self.session.machine()))
    }
    fn drain_tones(&mut self, out: &mut Vec<Tone>) {
        self.watch.drain_tones(out);
    }
    fn ride_sound(&self, snapshot: &Snapshot) -> Option<super::native_audio::Ride> {
        let pole_pairs = self.session.machine().plant().live_part()?.electrical().params().motor.pole_pairs;
        let surface = self.physics().ground_surface();
        ride_sound(snapshot, pole_pairs, surface)
    }
    fn take_contact_digest(&mut self) -> Option<euc_physics::ContactDigest> {
        Some(Self::physics_of(&mut self.session).take_contact_digest())
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
        // Play mode never pauses for want of input or a crash; only the test pad's edge does.
        self.edge_paused
    }
    fn booting(&self) -> bool {
        !self.boot_ready
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

    #[test]
    fn a_ride_sound_sample_carries_the_motor_the_tread_and_the_ground() {
        let mut snapshot = Snapshot { t_s: 3.0, ..Snapshot::default() };
        snapshot.signals.wheel_rad_s = -20.0;
        snapshot.signals.iq_a = -35.0;
        snapshot.signals.speed_kmh = -16.2;
        let wet_setts = euc_world::Surface::of(euc_world::Material::Cobblestones, true);
        let ride = super::ride_sound(&snapshot, 26, Some(wet_setts)).unwrap();
        let rev_s = 20.0 / std::f64::consts::TAU;
        assert!((ride.electrical_hz - 26.0 * rev_s).abs() < 1e-9);
        assert!((ride.knob_hz - 15.0 * rev_s).abs() < 1e-9);
        assert_eq!((ride.t_s, ride.phase_current_a), (3.0, 35.0));
        assert!((ride.speed_mps - 4.5).abs() < 1e-6);
        assert_eq!((ride.surface, ride.wet), (euc_world::Material::Cobblestones, true));
        // Off a world: asphalt, dry. A missing signal: no sample.
        let pad = super::ride_sound(&snapshot, 26, None).unwrap();
        assert_eq!((pad.surface, pad.wet), (euc_world::Material::Asphalt, false));
        snapshot.signals.speed_kmh = f32::NAN;
        assert!(super::ride_sound(&snapshot, 26, None).is_none());
    }

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
            desired_accel_mps2: None,
        }
    }

    /// The manual turn cap at a stop of the default rider: the largest rate a packet may carry.
    fn max_turn() -> f64 {
        max_turn_rad_s(&euc_physics::Rider::default())
    }

    /// A run past its boot with one neutral packet: play mode has no arming step.
    fn ready_state() -> FreeRideState {
        let state = state();
        state.ready(EPOCH);
        state.input(packet(1)).unwrap();
        state
    }

    #[derive(Debug, PartialEq)]
    struct Mailbox {
        status: Value,
        latest: Option<(u64, u32, InputSource, f64, f64, bool)>,
        last_seq: Option<u32>,
        received: Option<Instant>,
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
        let state = ready_state();
        let before = mailbox(&state);
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, MAX_LEAN_RAD + 1e-8, -MAX_LEAN_RAD - 1e-8] {
            assert!(state.input(ManualInput { desired_lean_rad: value, ..packet(2) }).is_err());
            assert_eq!(mailbox(&state), before);
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, max_turn() + 1e-8, -max_turn() - 1e-8] {
            assert!(state.input(ManualInput { heading_rate_rad_s: value, ..packet(2) }).is_err());
            assert_eq!(mailbox(&state), before);
        }
        for value in [f64::NAN, f64::INFINITY, -1e-8, 1.0 + 1e-8] {
            assert!(state.input(ManualInput { crouch: value, ..packet(2) }).is_err());
            assert_eq!(mailbox(&state), before);
        }
    }

    #[test]
    fn exact_motion_limits_are_accepted_in_both_directions() {
        let state = ready_state();
        for (seq, sign) in [(2, 1.0), (3, -1.0)] {
            state
                .input(ManualInput {
                    desired_lean_rad: sign * MAX_LEAN_RAD,
                    heading_rate_rad_s: sign * max_turn(),
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
    fn stepping_off_needs_a_booted_current_run_and_happens_once() {
        let booting = state();
        assert!(booting.step_off(EPOCH).is_err(), "still booting");
        let state = ready_state();
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
    fn input_drives_at_once_after_boot_without_an_arming_step() {
        let state = state();
        state.input(ManualInput { desired_lean_rad: 0.1, ..packet(1) }).unwrap();
        assert!(state.current(EPOCH).is_none(), "the host ignores input while the wheel boots");
        assert!(state.arm(EPOCH, SOURCE).is_err(), "not ready yet");
        assert!(!state.lock().status.armed);
        state.ready(EPOCH);
        let status = state.lock().status.clone();
        assert!(status.ready && status.armed, "ready is live: there is no arming step");
        assert_eq!(state.current(EPOCH).unwrap().desired_lean_rad, 0.1);
        // A packet's own armed flag is ignored (older frontends send it), and arm is a no-op.
        for (seq, armed) in [(2, true), (3, false)] {
            state.input(ManualInput { desired_lean_rad: 0.15, crouch: 0.5, armed, ..packet(seq) }).unwrap();
            let input = state.current(EPOCH).unwrap();
            assert_eq!((input.seq, input.desired_lean_rad, input.crouch), (seq, 0.15, 0.5));
        }
        let before = mailbox(&state);
        state.arm(EPOCH, InputSource::Gamepad).unwrap();
        assert_eq!(mailbox(&state), before);
        assert!(state.arm(EPOCH - 1, SOURCE).is_err());
    }

    #[test]
    fn a_quick_reset_restarts_the_same_ride_where_the_wheel_is() {
        let files = euc_sim::WheelFiles::load(&euc_profile::ProfileSource::Builtin, "begode-extreme").unwrap();
        let rider = Physics::new(Setup::new(files.body.clone())).unwrap().rider_state();
        // The test pad restarts at its origin: the same spec.
        let state = state();
        assert!(state.restart_spec(EPOCH, false).is_err(), "nothing to restart before the build remembers its spec");
        state.set_spec(EPOCH, spec("articulated_sagittal_v1"));
        let pad = state.restart_spec(EPOCH, false).unwrap();
        assert_eq!(state.restart_spec(EPOCH, true).unwrap().start, None);
        assert!(pad.map.is_some() && pad.start.is_none());
        assert!(state.restart_spec(EPOCH + 1, false).is_err(), "a reset belongs to its run");
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
        assert_eq!(state.restart_spec(EPOCH + 1, false).unwrap().start, None);
        let pose = WorldPose { xy_m: [12.5, -3.0], heading_rad: 0.4 };
        let report = HostReport {
            turn_cap_rad_s: 2.5,
            crouch: Some(0.5),
            on_pedals: true,
            boot_remaining_s: Some(1.0),
            world_pose: Some(pose),
            drive_caps: (2.5, 2.8),
            accel_mps2: 0.4,
        };
        state.update_rider(EPOCH + 1, rider, report);
        let restart = state.restart_spec(EPOCH + 1, false).unwrap();
        assert_eq!(restart.start, Some(pose));
        assert_eq!(state.restart_spec(EPOCH + 1, true).unwrap().start, None, "to the start: the world's spawn");
        restart.validate().unwrap();
        let status = state.lock().status.clone();
        assert_eq!(
            (status.turn_cap_rad_s, status.crouch, status.on_pedals, status.boot_remaining_s),
            (Some(2.5), Some(0.5), true, Some(1.0))
        );
        assert_eq!(
            (status.accel_cap_mps2, status.brake_cap_mps2, status.accel_mps2),
            (Some(2.5), Some(2.8), Some(0.4))
        );
        // A start place needs a world, and must be finite.
        assert!(FreeRideSpec { start: Some(pose), ..spec("articulated_sagittal_v1") }.validate().is_err());
        let nan = WorldPose { heading_rad: f64::NAN, ..pose };
        assert!(FreeRideSpec { start: Some(nan), ..world }.validate().is_err());
    }

    #[test]
    fn stale_epoch_and_nonincreasing_sequences_cannot_replace_input() {
        let state = ready_state();
        state.input(ManualInput { seq: 10, desired_lean_rad: 0.1, ..packet(10) }).unwrap();
        let before = mailbox(&state);
        for epoch in [0, EPOCH - 1, EPOCH + 1, u64::MAX] {
            assert!(state.input(ManualInput { run_epoch: epoch, ..packet(11) }).is_err());
            assert!(state.current(epoch).is_none());
            assert_eq!(mailbox(&state), before);
        }
        for seq in [0, 1, 9, 10] {
            assert!(state.input(ManualInput { ..packet(seq) }).is_err());
            assert_eq!(mailbox(&state), before);
        }
        state.input(ManualInput { ..packet(u32::MAX) }).unwrap();
        assert!(state.input(ManualInput { ..packet(0) }).is_err(), "sequence wrap needs a new run");
    }

    #[test]
    fn a_drive_packet_carries_an_ask_within_12_and_no_lean() {
        let state = ready_state();
        let before = mailbox(&state);
        for accel in [f64::NAN, f64::INFINITY, 12.0 + 1e-9, -12.0 - 1e-9] {
            assert!(state.input(ManualInput { desired_accel_mps2: Some(accel), ..packet(2) }).is_err());
            assert_eq!(mailbox(&state), before);
        }
        let both = ManualInput { desired_accel_mps2: Some(1.0), desired_lean_rad: 0.05, ..packet(2) };
        assert!(state.input(both).is_err(), "a lean or an ask, not both");
        assert_eq!(mailbox(&state), before);
        for (seq, accel) in [(2, 12.0), (3, -12.0), (4, 0.0)] {
            state.input(ManualInput { desired_accel_mps2: Some(accel), ..packet(seq) }).unwrap();
            assert_eq!(state.current(EPOCH).unwrap().desired_accel_mps2, Some(accel));
        }
        let json = json!({
            "run_epoch": EPOCH, "seq": 9, "source": "gamepad", "desired_lean_rad": 0.0,
            "heading_rate_rad_s": 0.2, "crouch": 0.0, "desired_accel_mps2": 2.5
        });
        let parsed = serde_json::from_value::<ManualInput>(json).unwrap();
        assert_eq!(parsed.desired_accel_mps2, Some(2.5));
        let status = serde_json::to_value(&state.lock().status).unwrap();
        for field in ["accel_cap_mps2", "brake_cap_mps2", "accel_mps2"] {
            assert!(status.get(field).is_some(), "the status names {field}");
        }
    }

    #[test]
    fn the_input_source_may_change_at_any_time() {
        let state = ready_state();
        state.input(ManualInput { source: InputSource::Gamepad, desired_lean_rad: 0.1, ..packet(2) }).unwrap();
        assert_eq!(state.current(EPOCH).unwrap().source, InputSource::Gamepad);
        assert_eq!(state.lock().status.source, Some(InputSource::Gamepad));
        state.input(packet(3)).unwrap();
        assert_eq!(state.current(EPOCH).unwrap().source, SOURCE);
        assert!(state.lock().status.armed);
    }

    #[test]
    fn watchdog_turns_an_old_input_into_neutral_controls_and_never_disarms() {
        assert_eq!(INPUT_TIMEOUT, Duration::from_millis(250));
        let state = ready_state();
        state.input(ManualInput { desired_lean_rad: 0.1, ..packet(2) }).unwrap();
        assert!(state.current(EPOCH).is_some());
        state.lock().received = Some(Instant::now() - INPUT_TIMEOUT - Duration::from_millis(1));
        assert!(state.current(EPOCH).is_none(), "an old input counts as neutral controls");
        let inner = state.lock();
        assert!(inner.status.ready && inner.status.armed, "the run stays live");
        assert_eq!(inner.last_seq, Some(2));
        drop(inner);
        // The next fresh packet drives at once: no neutral interval, no arming.
        state.input(ManualInput { desired_lean_rad: 0.05, ..packet(3) }).unwrap();
        assert_eq!(state.current(EPOCH).unwrap().desired_lean_rad, 0.05);
    }

    #[test]
    fn watchdog_expires_at_exactly_250_ms_but_not_one_nanosecond_before() {
        let state = ready_state();
        let mut inner = state.lock();
        let received = Instant::now();
        inner.received = Some(received);
        assert!(!inner.input_expired(received + Duration::from_nanos(249_999_999)));
        assert!(inner.input_expired(received + Duration::from_millis(250)));
        inner.received = None;
        assert!(inner.input_expired(received), "no input yet counts as neutral controls");
    }

    #[test]
    fn rejected_epoch_and_sequence_do_not_refresh_an_old_input() {
        let state = ready_state();
        state.lock().received = Some(Instant::now() - INPUT_TIMEOUT);
        let before = mailbox(&state);
        assert!(state.input(ManualInput { run_epoch: EPOCH - 1, ..packet(2) }).is_err());
        assert_eq!(mailbox(&state), before);
        assert!(state.input(packet(1)).is_err());
        assert_eq!(mailbox(&state), before);
        assert!(state.current(EPOCH).is_none(), "invalid packets cannot refresh the watchdog");
    }

    #[test]
    fn pause_and_clear_drop_motion_but_keep_sequence_and_build_ticket() {
        for pause in [true, false] {
            let state = ready_state();
            state.input(ManualInput { desired_lean_rad: 0.1, ..packet(2) }).unwrap();
            let ticket = state.reserve_start().unwrap();
            if pause {
                state.pause_epoch(EPOCH).unwrap();
            } else {
                state.clear_input();
            }
            assert!(state.current(EPOCH).is_none());
            let inner = state.lock();
            assert!(inner.latest.is_none() && inner.received.is_none());
            assert!(inner.status.ready && inner.status.armed, "a pause is not a disarm");
            assert_eq!(inner.last_seq, Some(2));
            assert_eq!(inner.pending_start, Some(ticket));
            drop(inner);
            assert!(state.input(packet(2)).is_err());
            assert!(state.reserve_start().is_err());
            state.finish_start(ticket);
            assert!(state.reserve_start().unwrap() > ticket);
            state.input(ManualInput { desired_lean_rad: 0.05, ..packet(3) }).unwrap();
            assert_eq!(state.current(EPOCH).unwrap().desired_lean_rad, 0.05, "the next packet drives again");
        }
        assert!(ready_state().pause_epoch(EPOCH - 1).is_err());
    }

    #[test]
    fn off_the_pedals_keeps_the_run_live_and_a_failure_stops_it() {
        let state = ready_state();
        state.off_pedals(EPOCH - 1, "old");
        state.off_pedals(EPOCH, "Off the pedals. Press R to respawn");
        let status = state.lock().status.clone();
        assert!(status.ready && status.armed);
        assert_eq!(status.reason, "Off the pedals. Press R to respawn");
        state.input(ManualInput { desired_lean_rad: 0.1, ..packet(2) }).unwrap();
        assert!(state.current(EPOCH).is_some());
        state.failed(EPOCH, "Map edge");
        let status = state.lock().status.clone();
        assert!(!status.ready && !status.armed);
        assert!(state.current(EPOCH).is_none());
        assert!(state.step_off(EPOCH).is_err());
    }

    #[test]
    fn stale_lifecycle_callbacks_cannot_change_the_current_run() {
        let state = ready_state();
        let before = mailbox(&state);
        state.ready(EPOCH - 1);
        state.failed(EPOCH - 1, "old failure");
        state.reason(EPOCH - 1, "old reason".into());
        state.off_pedals(EPOCH - 1, "old off");
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
        let state = ready_state();
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
        assert!(inner.received.is_none());
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
        let state = ready_state();
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

    fn packet(seq: u32, lean: f64) -> ManualInput {
        ManualInput {
            run_epoch: EPOCH,
            seq,
            source: InputSource::KeyboardSlider,
            desired_lean_rad: lean,
            heading_rate_rad_s: 0.0,
            armed: false,
            crouch: 0.0,
            desired_accel_mps2: None,
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

        fn input(&mut self, lean: f64) {
            self.motion(lean, 0.0);
        }

        fn motion(&mut self, lean: f64, turn: f64) {
            self.seq += 1;
            self.state
                .input(ManualInput { heading_rate_rad_s: turn, source: self.source, ..packet(self.seq, lean) })
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

        /// Play mode: the stand lets go at the boot time and the controls are live at once, with
        /// no pause and no arming step.
        fn boot(&mut self) {
            assert!(self.state.arm(EPOCH, InputSource::KeyboardSlider).is_err(), "not ready while booting");
            for _ in 0..490 {
                assert!(!self.host.wants_pause(), "booting never pauses");
                if self.state.lock().status.ready {
                    break;
                }
                assert!(self.host.booting());
                self.tick();
            }
            assert!((BOOT_S..BOOT_S + 0.02).contains(&self.host.time_s()), "{}", self.host.time_s());
            let status = self.state.lock().status.clone();
            assert!(status.ready && status.armed, "ready is live: {status:?}");
            assert!(!self.host.booting() && !self.host.wants_pause());
            let before = self.snapshot();
            self.host.step(STEP_S);
            assert!(self.snapshot().t_s > before.t_s, "boot-ready never holds the run");
        }

        fn phase(&mut self, count: usize, lean: f64, reference: [f64; 6]) -> Peaks {
            let mut peaks = Peaks::default();
            for _ in 0..count {
                self.input(lean);
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
        rig.boot();
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
        rig.motion(((speed_m_s - speed) * 0.05).clamp(-0.04, 0.04), turn);
        rig.tick()
    }

    fn sustained_steering(model: &str) {
        let spec = GeneratedMapSpec { grade_pct: [1.0, 0.0], ..GeneratedMapSpec::flat(17) };
        let mut rig = Rig::on_map(model, spec);
        rig.boot();
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
        let input = ManualInput { heading_rate_rad_s: turn, crouch, source: rig.source, ..packet(rig.seq, lean) };
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
        rig.boot();
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
        // X: the rider steps off, and the run goes on (play mode: a crash never pauses).
        rig.state.step_off(EPOCH).unwrap();
        let before = rig.host.time_s();
        for _ in 0..12 {
            rig.host.step(STEP_S);
        }
        let status = rig.state.lock().status.clone();
        assert!(!status.on_pedals && status.ready && status.reason.starts_with("Off the pedals"), "{status:?}");
        assert!(rig.host.time_s() > before + 0.09 && !rig.host.wants_pause());
    }

    /// FR2b quick reset on a world: a new run starts where the wheel stood, at its heading, with
    /// the firmware booting again on the stand.
    fn quick_reset(model: &str) {
        let mut rig = Rig::on_world(model, 42);
        rig.boot();
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
        let spec = rig.state.restart_spec(EPOCH, false).unwrap();
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
        let again = state.restart_spec(EPOCH + 1, false).unwrap().start.unwrap();
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

    fn pause_and_resume(model: &str) {
        let mut rig = Rig::new(model);
        rig.boot();
        for _ in 0..360 {
            drive_sample(&mut rig, 0.6, 0.05);
        }
        let before = rig.snapshot();
        rig.state.pause_epoch(EPOCH).unwrap();
        rig.host.manual_pause();
        let cleared = rig.snapshot();
        assert_eq!(cleared.t_s, before.t_s);
        assert_eq!(cleared.bodies, before.bodies, "pause must not relocate the actual physical bodies");
        assert!(rig.state.current(EPOCH).is_none(), "a pause drops the old steering");
        assert!(rig.state.input(packet(rig.seq, 0.04)).is_err(), "the sequence still rises");
        // Resume (the runner plays again) with the other source: no neutral interval, no arming.
        rig.state.arm(EPOCH, InputSource::Gamepad).unwrap();
        rig.source = InputSource::Gamepad;
        rig.input(0.0);
        let resumed = rig.state.current(EPOCH).unwrap();
        assert_eq!(resumed.source, InputSource::Gamepad);
        assert_eq!((resumed.desired_lean_rad, resumed.heading_rate_rad_s), (0.0, 0.0));
        for _ in 0..360 {
            let snapshot = drive_sample(&mut rig, 0.6, 0.0);
            assert!(rig.state.lock().status.armed);
            assert!(angle_delta(heading(&snapshot), heading(&before)).abs() < 0.1, "old steering must not resume");
        }
        let end = rig.snapshot();
        println!(
            "pause-resume {model}: paused_t_s={:.6}, end_t_s={:.6}, paused_heading_deg={:.6}, end_heading_deg={:.6}, paused_pose={:?}, end_pose={:?}, end_speed_kmh={:.6}",
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
        rig.boot();
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
        rig.boot();
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
    #[ignore = "needs cached pinned stock firmware; real-host rigid steering for 14 s after the boot"]
    fn stock_rigid_sustained_steering_on_generated_terrain() {
        sustained_steering("rigid_ankle_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; real-host articulated steering for 14 s after the boot"]
    fn stock_articulated_sustained_steering_on_generated_terrain() {
        sustained_steering("articulated_sagittal_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; real-host rigid pause and Gamepad resume"]
    fn stock_rigid_pause_and_resume() {
        pause_and_resume("rigid_ankle_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; real-host articulated pause and Gamepad resume"]
    fn stock_articulated_pause_and_resume() {
        pause_and_resume("articulated_sagittal_v1");
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

    /// One sample with no angle or cut-off checks (a crash, a dead wheel): the host must only never
    /// halt or pause.
    fn free_step(rig: &mut Rig, events: &mut Vec<euc_telemetry::Event>) -> Snapshot {
        rig.host.step(STEP_S);
        assert!(rig.host.halted().is_none(), "native halt: {:?}", rig.host.halted());
        assert!(!rig.host.wants_pause(), "play mode never pauses");
        rig.host.drain_events(events);
        rig.host.drain_ble_tx(&mut Vec::new());
        rig.host.drain_tones(&mut Vec::new());
        rig.snapshot()
    }

    fn moved_m(a: &Snapshot, b: &Snapshot) -> f64 {
        f64::from(a.bodies[0].pos[0] - b.bodies[0].pos[0]).hypot(f64::from(a.bodies[0].pos[1] - b.bodies[0].pos[1]))
    }

    /// Play mode: no input after boot. The idle wheel balances with neutral controls and the run
    /// never pauses for want of input; an input left behind times out to neutral controls.
    fn idle_balances(model: &str) {
        let mut rig = Rig::on_world(model, 42);
        rig.boot();
        let start = rig.snapshot();
        let (mut peak_speed, mut peak_pitch) = (0.0_f32, 0.0_f32);
        for _ in 0..1200 {
            assert!(!rig.host.wants_pause(), "no input never pauses");
            let snapshot = rig.tick();
            peak_speed = peak_speed.max(snapshot.signals.speed_kmh.abs());
            peak_pitch = peak_pitch.max(snapshot.signals.pitch_deg.abs());
        }
        let end = rig.snapshot();
        let status = rig.state.lock().status.clone();
        println!(
            "idle {model}: 10 s with no input: peak speed {peak_speed:.2} km/h, peak pitch {peak_pitch:.2} deg, drift {:.3} m",
            moved_m(&end, &start)
        );
        assert!((end.t_s - start.t_s - 10.0).abs() < 1e-6, "the run went on");
        assert!(status.ready && status.on_pedals, "{status:?}");
        assert!(peak_speed < 1.0 && peak_pitch < 3.0 && moved_m(&end, &start) < 0.3);
        // A lean, then silence: after the 250 ms wall-clock watchdog the controls are neutral and
        // the wheel rides on and stays up.
        for _ in 0..120 {
            rig.input(LEAN_RAD);
            rig.tick();
        }
        let leaned = rig.snapshot().signals.speed_kmh;
        std::thread::sleep(INPUT_TIMEOUT + Duration::from_millis(20));
        rig.tick();
        assert_eq!(rig.state.lock().status.desired_lean_deg, Some(0.0), "neutral controls");
        for _ in 0..480 {
            assert!(!rig.host.wants_pause());
            rig.tick();
        }
        let status = rig.state.lock().status.clone();
        println!(
            "idle {model}: a 1 s lean to {leaned:.2} km/h, then no input: {:.2} km/h after 4 s",
            rig.snapshot().signals.speed_kmh
        );
        assert!(leaned > 0.5 && status.ready && status.on_pedals, "{status:?}");
    }

    /// Play mode: a crash never pauses. The rider leaves the pedals at speed; firmware, motor and
    /// world go on, the crash is a timeline note, and the status says how to respawn.
    fn crash_goes_on(model: &str) {
        let mut rig = Rig::on_world(model, 42);
        rig.boot();
        for _ in 0..480 {
            let lean = lean_for(&rig, 3.0);
            hand_sample(&mut rig, lean, 0.0, 0.0);
        }
        let speed = rig.snapshot().signals.speed_kmh;
        rig.state.step_off(EPOCH).unwrap();
        let start = rig.snapshot();
        let mut events = Vec::new();
        let mut ble_late = 0;
        for sample in 0..600 {
            // The player still holds a lean: a rider off the pedals ignores it.
            rig.input(LEAN_RAD);
            rig.host.step(STEP_S);
            assert!(rig.host.halted().is_none(), "native halt: {:?}", rig.host.halted());
            assert!(!rig.host.wants_pause(), "a crash never pauses");
            rig.host.drain_events(&mut events);
            let mut ble = Vec::new();
            rig.host.drain_ble_tx(&mut ble);
            if sample >= 360 {
                ble_late += ble.len();
            }
            rig.host.drain_tones(&mut Vec::new());
        }
        let end = rig.snapshot();
        let status = rig.state.lock().status.clone();
        let cuts: Vec<_> = events.iter().filter(|e| matches!(e.kind, EventKind::Fault | EventKind::Cutoff)).collect();
        println!(
            "crash {model}: stepped off at {speed:.2} km/h; 5 s on: wheel moved {:.2} m, end speed {:.2} km/h, pitch {:.1} deg, {ble_late} BLE bytes in the last 2 s, cut events {:?}",
            moved_m(&end, &start),
            end.signals.speed_kmh,
            end.signals.pitch_deg,
            cuts.iter().map(|e| (e.t_s, e.text.as_str())).collect::<Vec<_>>()
        );
        assert!((end.t_s - start.t_s - 5.0).abs() < 1e-6, "the sim kept stepping");
        assert!(status.ready && !status.on_pedals && status.reason.starts_with("Off the pedals"), "{status:?}");
        assert!(events.iter().any(|e| e.text == "Stepped off"), "the crash is a timeline note");
        assert!(ble_late > 0, "the firmware still runs: it still talks BLE 3 s after the crash");
        assert!(moved_m(&end, &start) > 0.1, "the wheel moves on");
    }

    /// Play mode respawn on a world. After a crash, R stands the wheel up at the last clear spot the
    /// rider rode through, and Shift+R at the world's start. Each respawn boots the firmware again;
    /// this measures the wall time from the call to riding, stepping the boot unthrottled the way
    /// the runner does.
    fn respawn(model: &str) {
        let mut rig = Rig::on_world(model, 42);
        rig.boot();
        for _ in 0..480 {
            let lean = lean_for(&rig, 3.0);
            hand_sample(&mut rig, lean, 0.1, 0.0);
        }
        let ground = rig.state.lock().world.clone().unwrap();
        let ridden = rig.snapshot();
        let rode_to = ground.to_world([f64::from(ridden.bodies[0].pos[0]), f64::from(ridden.bodies[0].pos[1])]);
        rig.state.step_off(EPOCH).unwrap();
        let mut events = Vec::new();
        for _ in 0..360 {
            free_step(&mut rig, &mut events);
        }
        let spawn = super::super::world::shared(WorldChoice { seed: 42, densities: Default::default() }.spec())
            .unwrap()
            .spawn()
            .position;
        let mut epoch = EPOCH;
        for to_start in [false, true] {
            let spec = rig.state.restart_spec(epoch, to_start).unwrap();
            let target = spec.start.map_or([spawn[0], spawn[1]], |pose| pose.xy_m);
            if to_start {
                assert_eq!(spec.start, None, "Shift+R: the world's own start");
            } else {
                let spot = spec.start.expect("R: the last clear spot");
                let gap = (spot.xy_m[0] - rode_to[0]).hypot(spot.xy_m[1] - rode_to[1]);
                assert!(gap < 1.0, "the last clear spot is where the rider rode, not where the wheel fell: {gap} m");
            }
            epoch += 1;
            let wall = Instant::now();
            let built = super::super::firmware::build_free_host(&spec, "stock", epoch, rig.state.clone(), &mut |_| {})
                .expect("the respawn builds");
            let build_s = wall.elapsed().as_secs_f64();
            let mut host = built.host;
            while host.booting() {
                host.step(STEP_S);
                assert!(host.halted().is_none() && !host.wants_pause());
                host.drain_events(&mut Vec::new());
                host.drain_ble_tx(&mut Vec::new());
                host.drain_tones(&mut Vec::new());
            }
            let riding_s = wall.elapsed().as_secs_f64();
            let status = rig.state.lock().status.clone();
            assert!(status.ready && status.on_pedals && status.epoch == epoch, "{status:?}");
            let mut snapshot = Snapshot::default();
            host.snapshot(&mut snapshot);
            let restarted = rig.state.lock().world.clone().unwrap();
            let here = restarted.to_world([f64::from(snapshot.bodies[0].pos[0]), f64::from(snapshot.bodies[0].pos[1])]);
            let (pitch, roll) = (snapshot.signals.pitch_deg, snapshot.signals.roll_deg);
            println!(
                "respawn {model} to_start={to_start}: at {here:?} (asked {target:?}), pitch {pitch:.2} deg, roll {roll:.2} deg; wall: build {build_s:.3} s, boot {:.3} s, call to riding {riding_s:.3} s",
                riding_s - build_s
            );
            assert!((here[0] - target[0]).hypot(here[1] - target[1]) < 0.05, "{here:?} vs {target:?}");
            assert!(pitch.abs() < 2.0 && roll.abs() < 2.0, "upright");
            // It rides at once: two seconds with no input, balanced.
            for _ in 0..240 {
                host.step(STEP_S);
                assert!(host.halted().is_none() && !host.wants_pause());
                host.drain_events(&mut Vec::new());
                host.drain_ble_tx(&mut Vec::new());
                host.drain_tones(&mut Vec::new());
                host.snapshot(&mut snapshot);
                assert!(snapshot.signals.pitch_deg.abs() < 3.0);
            }
            rig.host = host;
        }
    }

    /// Play mode: power off is not a halt. Held 0.81 s at a standstill, the power button makes the
    /// stock firmware drop its PB14 latch; on release the board loses power, the motor goes free
    /// and the rider falls, and the run goes on. A press of the button boots the firmware again.
    /// At speed the firmware ignores the hold.
    fn power_off_and_on(model: &str) {
        let mut rig = Rig::on_world(model, 42);
        rig.boot();
        let mut events = Vec::new();
        // At 2 m/s a 1.5 s hold does not turn the wheel off (|v| > 0.5 m/s).
        for _ in 0..360 {
            let lean = lean_for(&rig, 2.0);
            hand_sample(&mut rig, lean, 0.0, 0.0);
        }
        assert!(rig.host.power_button_ready());
        rig.host.set_button("power", true).unwrap();
        for _ in 0..180 {
            let lean = lean_for(&rig, 2.0);
            hand_sample(&mut rig, lean, 0.0, 0.0);
        }
        assert_eq!(rig.host.outputs().unwrap().power_latched, Some(true), "no power-off while riding");
        rig.host.set_button("power", false).unwrap();
        assert_eq!(rig.host.outputs().unwrap().controller_powered, Some(true));
        // Stop, then hold the button at a standstill.
        for _ in 0..600 {
            let lean = lean_for(&rig, 0.0);
            hand_sample(&mut rig, lean, 0.0, 0.0);
        }
        rig.host.set_button("power", true).unwrap();
        let pressed = rig.host.time_s();
        while rig.host.outputs().unwrap().power_latched != Some(false) && rig.host.time_s() - pressed < 5.0 {
            free_step(&mut rig, &mut events);
        }
        assert_eq!(rig.host.outputs().unwrap().power_latched, Some(false), "stock drops PB14 on a long press");
        let latch_s = rig.host.time_s() - pressed;
        rig.host.set_button("power", false).unwrap();
        free_step(&mut rig, &mut events);
        assert_eq!(rig.host.outputs().unwrap().controller_powered, Some(false), "the rail goes with the latch");
        let off_at = rig.host.time_s();
        let mut fell = None;
        for _ in 0..600 {
            free_step(&mut rig, &mut events);
            if fell.is_none() && !rig.state.lock().status.on_pedals {
                fell = Some(rig.host.time_s() - off_at);
            }
        }
        let status = rig.state.lock().status.clone();
        assert!(rig.host.time_s() >= off_at + 4.99, "the run went on");
        assert!(status.ready, "power off is not a failure: {status:?}");
        assert_eq!(rig.host.outputs().unwrap().controller_powered, Some(false));
        let fell = fell.expect("the rider falls off the dead wheel");
        // A press boots the firmware again.
        assert!(rig.host.power_button_ready());
        rig.host.set_button("power", true).unwrap();
        let wake = rig.host.time_s();
        while rig.host.outputs().unwrap().power_latched != Some(true) && rig.host.time_s() - wake < 3.0 {
            free_step(&mut rig, &mut events);
        }
        assert_eq!(rig.host.outputs().unwrap().power_latched, Some(true), "the firmware latches the rail again");
        let relatch_s = rig.host.time_s() - wake;
        for _ in 0..18 {
            free_step(&mut rig, &mut events);
        }
        rig.host.set_button("power", false).unwrap();
        for _ in 0..360 {
            free_step(&mut rig, &mut events);
        }
        let outputs = rig.host.outputs().unwrap();
        println!(
            "power {model}: latch dropped {latch_s:.3} s into the hold; rider off {fell:.2} s after the rail went; latched again {relatch_s:.3} s into the press; display {} 3.2 s later",
            outputs.display_on
        );
        assert_eq!(outputs.controller_powered, Some(true));
        assert!(outputs.display_on, "the firmware runs its display again");
        assert!(rig.host.buttons_ready());
        assert!(rig.host.firmware_value("drive_command").is_some(), "firmware RAM is live again");
    }

    /// Play mode: the handle button (PA12) and the power button (PB15) reach the firmware while
    /// riding, as on the wheel. At speed the firmware ignores the handle gesture and a short power
    /// press; at a standstill two handle presses soft-disable the balance and a third restores it.
    fn buttons_while_riding(model: &str) {
        let mut rig = Rig::on_world(model, 42);
        rig.boot();
        for _ in 0..360 {
            let lean = lean_for(&rig, 2.0);
            hand_sample(&mut rig, lean, 0.0, 0.0);
        }
        assert!(rig.host.buttons_ready());
        let ride = |rig: &mut Rig, samples: usize| {
            for _ in 0..samples {
                let lean = lean_for(rig, 2.0);
                hand_sample(rig, lean, 0.0, 0.0);
            }
        };
        // Held handle, then two handle presses and a short power press, all at 2 m/s.
        rig.host.set_button("handle", true).unwrap();
        ride(&mut rig, 60);
        rig.host.set_button("handle", false).unwrap();
        ride(&mut rig, 60);
        for _ in 0..2 {
            rig.host.pulse_button("handle", 120).unwrap();
            ride(&mut rig, 36);
        }
        rig.host.pulse_button("power", 150).unwrap();
        ride(&mut rig, 120);
        let snapshot = rig.snapshot();
        let disabled = rig.host.firmware_value("balance_disabled").unwrap();
        println!(
            "buttons {model}: at speed after handle hold, two handle presses and a power press: {:.2} km/h, balance_disabled {disabled}",
            snapshot.signals.speed_kmh
        );
        assert!(snapshot.signals.speed_kmh > 5.0 && disabled == 0.0, "the firmware ignores them at speed");
        assert!(rig.state.lock().status.on_pedals);
        // A phone command reaches the firmware as well: the horn ("b") sounds while riding.
        rig.host.inject_ble(b"b").unwrap();
        let mut tones = Vec::new();
        for _ in 0..60 {
            let lean = lean_for(&rig, 2.0);
            rig.input(lean);
            rig.host.step(STEP_S);
            assert!(rig.host.halted().is_none());
            rig.host.drain_tones(&mut tones);
            rig.host.drain_events(&mut Vec::new());
            rig.host.drain_ble_tx(&mut Vec::new());
        }
        println!("buttons {model}: the horn command while riding: {} tone changes", tones.len());
        assert!(tones.iter().any(|tone| tone.hz > 0.0), "the horn sounds");
        // At a standstill: a handle press counts after 0.1 s, and the release of the second one cuts
        // the drive to one duty unit (the firmware read PA12).
        for _ in 0..600 {
            let lean = lean_for(&rig, 0.0);
            hand_sample(&mut rig, lean, 0.0, 0.0);
        }
        let mut events = Vec::new();
        let mut peaks = Vec::new();
        for _ in 0..2 {
            rig.host.pulse_button("handle", 150).unwrap();
            let mut peak = 0.0_f64;
            for sample in 0..60 {
                free_step(&mut rig, &mut events);
                if sample >= 24 {
                    peak = peak.max(rig.host.firmware_value("drive_command").unwrap().abs());
                }
            }
            peaks.push(peak);
        }
        println!(
            "buttons {model}: at a standstill, the largest drive command after each handle press {peaks:?}; speed {:.2} km/h",
            rig.snapshot().signals.speed_kmh
        );
        assert!(peaks[0] > 1.0, "one press alone does not cut: the wheel still balances");
        assert!(peaks[1] <= 1.0 + 1e-6, "the second press cuts the drive");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; play mode: 10 s with no input on the world"]
    fn stock_articulated_idle_after_boot_balances_without_a_pause() {
        idle_balances("articulated_sagittal_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; play mode: 10 s with no input on the world"]
    fn stock_rigid_idle_after_boot_balances_without_a_pause() {
        idle_balances("rigid_ankle_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; play mode: a step-off at speed goes on 5 s"]
    fn stock_articulated_crash_never_pauses() {
        crash_goes_on("articulated_sagittal_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; play mode: a step-off at speed goes on 5 s"]
    fn stock_rigid_crash_never_pauses() {
        crash_goes_on("rigid_ankle_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; play mode: respawn at the last clear spot and at the start"]
    fn stock_articulated_respawns_upright_at_the_last_clear_spot_and_at_the_start() {
        respawn("articulated_sagittal_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; play mode: power off with the button, then on again"]
    fn stock_articulated_power_off_then_on() {
        power_off_and_on("articulated_sagittal_v1");
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; play mode: handle and power buttons while riding"]
    fn stock_articulated_buttons_while_riding() {
        buttons_while_riding("articulated_sagittal_v1");
    }

    /// The seed-42 city with an ideal road (no potholes, manholes, cracks, rough or wet patches,
    /// speed bumps): the drive tests measure firmware and rider, not the road. The spawn is on the
    /// arterial, heading along it.
    fn ideal_city() -> WorldChoice {
        let densities = euc_world::Densities {
            potholes: 0.0,
            manholes: 0.0,
            cracks: 0.0,
            rough_patches: 0.0,
            wet_patches: 0.0,
            speed_bumps: 0.0,
            ..euc_world::Densities::default()
        };
        WorldChoice { seed: 42, densities }
    }

    /// A Free Ride on [`ideal_city`] driven by drive packets, with the firmware, settings preset,
    /// battery charge and largest CPU quantum under test.
    struct Drive {
        host: FreeRideHost,
        state: FreeRideState,
        seq: u32,
        events: Vec<euc_telemetry::Event>,
    }

    impl Drive {
        fn new(model: &str, firmware: &str, settings: &str, soc: f64, quantum: u32) -> Self {
            require_cached_stock();
            let state = FreeRideState::default();
            let choice = ideal_city();
            let spec = FreeRideSpec {
                wheel: "begode-extreme".into(),
                settings: settings.into(),
                rider_model: model.into(),
                map: None,
                world: Some(choice),
                start: None,
            };
            let rider = spec.rider().unwrap();
            let files = euc_sim::WheelFiles::load(&ProfileSource::Builtin, "begode-extreme").unwrap();
            let board =
                if settings == "blank" { files.board.clone() } else { files.board_with_settings(settings).unwrap() };
            let loaded = super::super::images::load(&files, firmware, &mut |_| {}).unwrap();
            let world = super::super::world::shared(choice.spec()).unwrap();
            let spawn = world.spawn();
            let start = euc_physics::WorldStart {
                xy_m: [spawn.position[0], spawn.position[1]],
                heading_rad: spawn.heading[1].atan2(spawn.heading[0]),
            };
            let ground = euc_physics::WorldGround::with_world(world, start).unwrap();
            state.begin_world(EPOCH, ground.clone(), rider);
            let live = files.live_on_world(rider, Default::default(), ground).unwrap();
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
                exception_timing: loaded.exception_timing(),
                max_quantum: quantum,
                marker_window_s: 4.0,
            };
            let machine =
                euc_sim::Machine::new_live(&config, &euc_sim::registry(), loaded.factory().unwrap(), live).unwrap();
            let mut session = Session::new(machine, loaded.map.as_ref(), 120.0);
            if soc < 1.0 {
                // The scenarios' own battery_soc fault, from a one-entry timeline.
                let text = format!(
                    "schema = 1\nname = \"soc\"\ndescription = \"soc\"\nwheel = \"begode-extreme\"\nseconds = 10.0\n\
                     release_s = 1.0\nspeed_kmh = [[0, 0]]\n[[timeline]]\nat_s = 0.0\nfault = \"battery_soc\"\nsoc = {soc}\n"
                );
                let scenario = euc_sim::scenario::Scenario::from_toml(&text).unwrap();
                let euc_sim::scenario::Fault::Plant(fault) = scenario.faults[0].fault.clone() else {
                    unreachable!("battery_soc is a plant fault")
                };
                let live = session.machine_mut().plant_mut().live_part_mut().unwrap();
                live.electrical_mut().add_fault(0, None, fault).unwrap();
            }
            let host = FreeRideHost::new(session, state.clone(), EPOCH, None);
            let mut drive = Self { host, state, seq: 0, events: Vec::new() };
            while drive.host.booting() {
                drive.host.step(STEP_S);
                drive.drain();
            }
            drive
        }

        fn drain(&mut self) {
            assert!(self.host.halted().is_none(), "native halt: {:?}", self.host.halted());
            self.host.drain_events(&mut self.events);
            self.host.drain_ble_tx(&mut Vec::new());
            self.host.drain_tones(&mut Vec::new());
        }

        fn status(&self) -> FreeRideStatus {
            self.state.lock().status.clone()
        }

        /// One 1/120 s sample asking `accel` (m/s²), straight on.
        fn step(&mut self, accel: f64) -> Snapshot {
            self.seq += 1;
            let input = ManualInput {
                run_epoch: EPOCH,
                seq: self.seq,
                source: InputSource::KeyboardSlider,
                desired_lean_rad: 0.0,
                heading_rate_rad_s: 0.0,
                armed: false,
                crouch: 0.0,
                desired_accel_mps2: Some(accel),
            };
            self.state.input(input).unwrap();
            self.host.step(STEP_S);
            self.drain();
            let mut snapshot = Snapshot::default();
            self.host.snapshot(&mut snapshot);
            snapshot
        }

        /// The rider fell or the firmware cut out: the run is over for this ask.
        fn crashed(&self) -> bool {
            !self.status().on_pedals
                || self.events.iter().any(|e| matches!(e.kind, EventKind::Cutoff) || e.text.contains("fell over"))
        }
    }

    /// Drive mode at a fraction of the caps until `done` (speed in km/h) or `limit_s`: the time it
    /// took, the speeds seen, and whether the rider fell.
    fn drive_until(
        d: &mut Drive,
        fraction: f64,
        limit_s: f64,
        done: impl Fn(f64) -> bool,
    ) -> (f64, f64, f64, Option<f64>) {
        let start = d.host.time_s();
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        loop {
            let status = d.status();
            let cap = if fraction >= 0.0 { status.accel_cap_mps2 } else { status.brake_cap_mps2 };
            let speed = f64::from(d.step(fraction * cap.unwrap()).signals.speed_kmh);
            (low, high) = (low.min(speed), high.max(speed));
            if d.crashed() {
                return (d.host.time_s() - start, low, high, Some(speed));
            }
            if done(speed) || d.host.time_s() - start >= limit_s {
                return (d.host.time_s() - start, low, high, None);
            }
        }
    }

    /// Drive mode, the speed step on the owner's preset (tiltback 69 km/h): half throttle from rest
    /// to 20 km/h, 6 s with no ask, then a half brake to a clean stop.
    fn drive_speed_step(firmware: &str, quantum: u32) {
        let mut d = Drive::new("articulated_sagittal_v1", firmware, "tarik-2026-10-15", 1.0, quantum);
        let (rise, _, _, fell) = drive_until(&mut d, 0.5, 8.0, |v| v >= 20.0);
        assert!(fell.is_none() && rise < 5.0, "{firmware} q{quantum}: 20 km/h in {rise:.2} s, fell {fell:?}");
        let (_, low, high, fell) = drive_until(&mut d, 0.0, 6.0, |_| false);
        assert!(fell.is_none(), "{firmware} q{quantum}: fell cruising at {fell:?}");
        let (stop, _, _, fell) = drive_until(&mut d, -0.5, 10.0, |v| v.abs() < 0.3);
        assert!(fell.is_none() && stop < 8.0, "{firmware} q{quantum}: stop in {stop:.2} s, fell {fell:?}");
        // The brake held 1 s more, then nothing asked: the clean stop holds.
        let (_, after_low, _, _) = drive_until(&mut d, -0.5, 1.0, |_| false);
        let (_, rest_low, rest_high, fell) = drive_until(&mut d, 0.0, 3.0, |_| false);
        println!(
            "drive speed step {firmware} q{quantum}: 0-20 km/h in {rise:.2} s; no ask for 6 s: {low:.1}-{high:.1} km/h; \
             stop in {stop:.2} s; after: lowest {:.2} km/h, then {rest_low:.2}..{rest_high:.2} km/h",
            after_low.min(rest_low)
        );
        assert!(fell.is_none());
        assert!(low > 17.0 && high < 27.0, "{firmware} q{quantum}: cruise {low:.1}-{high:.1} km/h");
        // The pedals settle after the brake: the wheel may rock a little, never ride away.
        assert!(after_low.min(rest_low) > -1.0 && rest_high < 1.0, "{firmware} q{quantum}: not a clean stop");
    }

    /// Drive mode, the tiltback: full throttle on the blank preset, whose speed tiltback starts at
    /// 20 km/h. The firmware tilts the pedals back, the rider eases off, and nobody falls.
    fn drive_into_tiltback(firmware: &str, quantum: u32) {
        let mut d = Drive::new("articulated_sagittal_v1", firmware, "blank", 1.0, quantum);
        let (mut top, mut eased) = (0.0_f64, false);
        for _ in 0..2400 {
            let cap = d.status().accel_cap_mps2.unwrap();
            top = top.max(f64::from(d.step(cap).signals.speed_kmh));
            eased |= d.status().desired_lean_deg.is_some_and(|lean| lean < 0.0);
            assert!(!d.crashed(), "{firmware} q{quantum}: fell at {top:.1} km/h");
        }
        println!("drive into tiltback {firmware} q{quantum}: full throttle 20 s, top {top:.1} km/h, eased off {eased}");
        assert!(eased, "the rider eases off in the tiltback");
        assert!((20.0..45.0).contains(&top), "{firmware} q{quantum}: top {top:.1} km/h");
    }

    fn drive_matrix(firmware: &str) {
        for quantum in [4000, 3000, 2000] {
            drive_speed_step(firmware, quantum);
            drive_into_tiltback(firmware, quantum);
        }
    }

    #[test]
    #[ignore = "needs cached pinned stock firmware; drive mode speed step and tiltback at three quanta"]
    fn stock_drive_speed_step_and_tiltback() {
        drive_matrix("stock");
    }

    #[test]
    #[ignore = "needs firmware/build/target-compat (EUC_FIRMWARE_DIR); drive mode speed step and tiltback at three quanta"]
    fn compat_drive_speed_step_and_tiltback() {
        drive_matrix("ours-compat");
    }

    #[test]
    #[ignore = "needs firmware/build/target-clean (EUC_FIRMWARE_DIR); drive mode speed step and tiltback at three quanta"]
    fn clean_drive_speed_step_and_tiltback() {
        drive_matrix("ours-clean");
    }

    /// Drive mode, normal riding at half throttle on stock, at three charges: to 40 km/h, 10 s
    /// with no ask, a half brake to a clean stop. The rider never falls.
    #[test]
    #[ignore = "needs cached pinned stock firmware; drive mode half throttle at 100, 50 and 20 % charge"]
    fn stock_drive_half_throttle_normal_ride_at_three_charges() {
        for soc in [1.0, 0.5, 0.2] {
            let mut d = Drive::new("articulated_sagittal_v1", "stock", "tarik-2026-10-15", soc, 4000);
            let (rise, _, _, fell) = drive_until(&mut d, 0.5, 12.0, |v| v >= 40.0);
            assert!(fell.is_none() && rise < 10.0, "soc {soc}: 40 km/h in {rise:.2} s, fell {fell:?}");
            let (_, low, high, fell) = drive_until(&mut d, 0.0, 10.0, |_| false);
            assert!(fell.is_none(), "soc {soc}: fell cruising at {fell:?}");
            let (stop, _, _, fell) = drive_until(&mut d, -0.5, 15.0, |v| v.abs() < 0.3);
            assert!(fell.is_none() && stop < 12.0, "soc {soc}: stop in {stop:.2} s, fell {fell:?}");
            let (_, rest_low, rest_high, fell) = drive_until(&mut d, 0.0, 3.0, |_| false);
            println!(
                "drive half throttle soc {soc}: 0-40 km/h in {rise:.2} s; no ask for 10 s: {low:.1}-{high:.1} km/h; \
                 stop in {stop:.2} s; then {rest_low:.2}..{rest_high:.2} km/h"
            );
            // A hard stop from 40 km/h leaves the pedals pitched; the wheel rocks a little as they settle.
            assert!(fell.is_none() && rest_low > -1.0 && rest_high < 1.0, "soc {soc}: not a clean stop");
        }
    }

    /// Drive mode, a release: throttle at `fraction` of the cap from rest to 20 km/h, then no ask
    /// for 4 s. The wheel runs on as it catches the rider (it drives forward under the body as the
    /// lean comes back), then settles at the speed let go: the reference does not run ahead. The
    /// peak over the release speed and where the speed settles, km/h.
    fn drive_release(firmware: &str, fraction: f64) -> (f64, f64) {
        let mut d = Drive::new("articulated_sagittal_v1", firmware, "tarik-2026-10-15", 1.0, 4000);
        let (rise, _, released, fell) = drive_until(&mut d, fraction, 10.0, |v| v >= 20.0);
        assert!(fell.is_none() && rise < 8.0, "{firmware} {fraction}: 20 km/h in {rise:.2} s, fell {fell:?}");
        let (_, low, high, fell) = drive_until(&mut d, 0.0, 4.0, |_| false);
        assert!(fell.is_none(), "{firmware} {fraction}: fell after the release at {fell:?}");
        let settled = f64::from(d.step(0.0).signals.speed_kmh);
        println!(
            "drive release {firmware} at {fraction} of the cap: released at {released:.2} km/h, then \
             {low:.2}-{high:.2} km/h, {settled:.2} km/h after 4 s"
        );
        (high - released, settled - released)
    }

    /// The owner's ride log (2026-10-17): after pushes of about 2.2 m/s² the speed rose 0.6-7.9 km/h
    /// (median 3.0) from the let-go to the peak.
    #[test]
    #[ignore = "needs cached pinned stock firmware; drive mode release at 20 km/h"]
    fn stock_drive_release_at_20_kmh_settles_at_the_speed_let_go() {
        for (fraction, peak_max) in [(0.5, 6.0), (1.0, 10.0)] {
            let (peak, settled) = drive_release("stock", fraction);
            assert!(peak < peak_max, "{fraction} of the cap: peak {peak:.2} km/h over the release");
            assert!(settled.abs() < 1.0, "{fraction} of the cap: settled {settled:.2} km/h from the release");
        }
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

    /// A free ride has no end, so the board keeps no BLE backlog once the runner has read it.
    #[test]
    #[ignore = "needs cached pinned stock firmware; 20 simulated seconds on the native FreeRideHost"]
    fn stock_free_ride_keeps_no_ble_backlog() {
        let state = FreeRideState::default();
        let mut host = diagnostic_host("rigid_ankle_v1", state);
        let (mut bytes, mut events) = (0, 0);
        for _ in 0..2400 {
            host.step(STEP_S);
            assert!(host.halted().is_none());
            let mut out = Vec::new();
            host.drain_ble_tx(&mut out);
            bytes += out.len();
            let mut drained = Vec::new();
            host.drain_events(&mut drained);
            events += drained.len();
            host.drain_tones(&mut Vec::new());
        }
        let left = host.session.machine().board().ble_tx.len();
        println!("free ride 20 s: {bytes} BLE bytes and {events} events read; {left} BLE bytes left on the board");
        assert!(bytes > 1000, "the firmware streams telemetry");
        assert_eq!(left, 0, "nothing piles up on the board");
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
                assert!(host.time_s() < BOOT_S + 0.02);
            }
            trace(&host, model, "stand-ready");
            let mut seq = 0;
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
                state.input(packet(seq, lean)).unwrap();
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
