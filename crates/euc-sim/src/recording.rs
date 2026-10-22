//! Recording a session into an `.eucrec` (format: `euc_telemetry::recording`): the header filled
//! from the wheel's files and the session, the firmware's BLE output decoded into frames, and a
//! [`Recorder`] that takes each sample with the events and frames that came with it.
//!
//! `euc sim run --record` and the Python bindings record through this module; the studio feeds
//! the same writer from its own run loop.
//!
//! [`RecordingContext`] is the header's optional `context`: what kind of run it was (a scenario
//! or a free ride), the rider model, the external aids the model ran with and, for a free ride,
//! the ground it was on. The writer validates it before the file is created and a reader before it
//! trusts it, so bad metadata can never replace the saved ground with a flat plane.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use euc_devices::{Buzzer, Tm1640, ws2812};
use euc_profile::{ProfileSource, Sha256};
use euc_proto_begode::Assembler;
use euc_proto_begode::describe::{describe, segment_bytes};
use euc_stm32f1::Dma;
use euc_telemetry::recording::{BleFrame, Header, OutputsAt, Writer};
use euc_telemetry::{Event, EventKind, Outputs, Snapshot};
use serde::{Deserialize, Serialize};

use crate::machine::Machine;
use crate::scenario::Outcome;
use crate::session::Session;
use crate::wheel::WheelFiles;

/// Context schema this module writes and reads.
pub const CONTEXT_SCHEMA: u32 = 1;
/// Maximum size of the context as compact JSON (its map included), bytes.
pub const MAX_CONTEXT_BYTES: usize = euc_physics::map::MAX_MAP_METADATA_BYTES;

/// Recorded run mode. Metadata only: it never enables live controls during playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    /// A scripted scenario.
    Scenario,
    /// A manual run on a finite generated map.
    FreeRide,
}

/// The human model a context-1 recording names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiderModel {
    /// The rigid ankle model.
    RigidAnkleV1,
    /// The articulated sagittal model, feet welded to the pedals.
    ArticulatedSagittalV1,
    /// The articulated model with each sole on its own pedal through contact.
    ArticulatedContactV1,
}

impl RiderModel {
    /// The id as the context spells it.
    pub const fn id(self) -> &'static str {
        match self {
            Self::RigidAnkleV1 => "rigid_ankle_v1",
            Self::ArticulatedSagittalV1 => "articulated_sagittal_v1",
            Self::ArticulatedContactV1 => "articulated_contact_v1",
        }
    }
}

impl From<euc_physics::RiderModel> for RiderModel {
    fn from(model: euc_physics::RiderModel) -> Self {
        match model {
            euc_physics::RiderModel::RigidAnkleV1 => Self::RigidAnkleV1,
            euc_physics::RiderModel::ArticulatedSagittalV1 => Self::ArticulatedSagittalV1,
            euc_physics::RiderModel::ArticulatedContactV1 => Self::ArticulatedContactV1,
        }
    }
}

/// The external aids the model ran with. Never inferred from the recorded movement, and no
/// personal body measurements.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAids {
    /// External roll stiffness, N·m/rad.
    pub roll_stiffness: f64,
    /// External roll damping, N·m·s/rad.
    pub roll_damping: f64,
    /// External yaw stiffness, N·m/rad.
    pub steer_kp: f64,
    /// External yaw damping, N·m·s/rad.
    pub steer_kd: f64,
    /// External yaw torque cap, N·m.
    pub steer_max_nm: f64,
    /// External roll stiffness on the articulated upper body, N·m/rad: contact feet do not hold
    /// the body to the shell (`articulated_contact_v1` only; absent for the other models).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub upper_body_roll_stiffness: f64,
    /// External roll damping on the articulated upper body, N·m·s/rad (as above).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub upper_body_roll_damping: f64,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if passes a reference.
fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

impl ModelAids {
    /// The aids a rider and its assist settings run with: the shell's roll spring and the yaw
    /// aid for every model, and for contact feet the same roll spring on the upper body.
    pub fn of(rider: &euc_physics::Rider, assist: &euc_physics::Assist) -> Self {
        let contact = rider.model.has_contact_feet();
        Self {
            roll_stiffness: assist.roll_stiffness,
            roll_damping: assist.roll_damping,
            steer_kp: rider.control.steer_kp,
            steer_kd: rider.control.steer_kd,
            steer_max_nm: rider.control.steer_max_nm,
            upper_body_roll_stiffness: if contact { assist.roll_stiffness } else { 0.0 },
            upper_body_roll_damping: if contact { assist.roll_damping } else { 0.0 },
        }
    }
}

/// The header's optional `context` (schema 1). `from_json` and `validate` are the one gate: the
/// studio's capture, the desktop reader and the Python bindings all go through them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordingContext {
    /// Context schema: [`CONTEXT_SCHEMA`]. Omitted on input it defaults to 1; output has it.
    #[serde(default = "context_schema")]
    pub schema: u32,
    /// Scenario or free ride.
    pub run_mode: RunMode,
    /// The physical rider model.
    pub rider_model: RiderModel,
    /// Finite ground, required for a free ride. Absent for a scenario's own ground. The saved
    /// grid is the ground of a free ride's playback; it is never regenerated from a seed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_map: Option<euc_physics::HeightMap>,
    /// External aid strengths.
    pub model_aids: ModelAids,
}

const fn context_schema() -> u32 {
    CONTEXT_SCHEMA
}

impl RecordingContext {
    /// Parses a context: strict fields, known enums, a well-formed map, the schema and the size.
    pub fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        if serde_json::to_vec(value).map_err(|e| e.to_string())?.len() > MAX_CONTEXT_BYTES {
            return Err("recording context exceeds 128 KiB".into());
        }
        let context: Self =
            serde_json::from_value(value.clone()).map_err(|e| format!("invalid recording context: {e}"))?;
        context.validate()?;
        Ok(context)
    }

    /// Checks the context before capture or playback trusts it.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTEXT_SCHEMA {
            return Err(format!("unsupported recording context schema {}", self.schema));
        }
        if self.run_mode == RunMode::FreeRide && self.height_map.is_none() {
            return Err("a free ride recording context needs a height_map".into());
        }
        if let Some(map) = &self.height_map {
            map.validate().map_err(|e| format!("invalid recording map: {e}"))?;
        }
        let aids = self.model_aids;
        if [
            aids.roll_stiffness,
            aids.roll_damping,
            aids.steer_kp,
            aids.steer_kd,
            aids.steer_max_nm,
            aids.upper_body_roll_stiffness,
            aids.upper_body_roll_damping,
        ]
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err("recording model_aids must be finite and not negative".into());
        }
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > MAX_CONTEXT_BYTES {
            return Err("recording context exceeds 128 KiB".into());
        }
        Ok(())
    }

    /// As the header stores it.
    ///
    /// # Panics
    /// Panics if the serializable context cannot be converted to a JSON value.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("a context serializes")
    }

    /// The context of `header`, validated; `None` when it has none.
    pub fn of(header: &Header) -> Result<Option<Self>, String> {
        header.context.as_ref().map(Self::from_json).transpose()
    }

    /// The context of a scenario run on `session`: the rider model and the aids the session's
    /// physics was actually built with (after any command-line override), not the scenario's text.
    ///
    /// # Panics
    /// If the session's plant is static (a session always has a live one).
    pub fn scenario(session: &Session) -> Self {
        let setup = session.machine().plant().live_part().expect("a session has a live plant").physics().setup();
        Self {
            schema: CONTEXT_SCHEMA,
            run_mode: RunMode::Scenario,
            rider_model: setup.rider.model.into(),
            height_map: None,
            model_aids: ModelAids::of(&setup.rider, &setup.assist),
        }
    }
}

/// The board outputs at the current simulated time, shared by live displays and captures.
pub fn outputs(machine: &Machine) -> Outputs {
    const LED_PERIOD: u16 = 30;
    const LED_DMA_CHANNEL: u8 = 2;
    let display = machine.component::<Tm1640>("display");
    let leds = machine
        .component::<Dma>("DMA1")
        .and_then(|dma| dma.memory(LED_DMA_CHANNEL))
        .and_then(|(addr, count)| machine.memory(addr, count as usize))
        .map(|duties| ws2812::decode(duties.iter().map(|&d| u16::from(d)), LED_PERIOD))
        .unwrap_or_default();
    let board = machine.board();
    let powered = machine.powered();
    let headlight_pin_high = if powered { board.headlight_pin_high() } else { None };
    let headlight_duty = if powered { board.headlight_duty(machine.cycles, machine.hclk()) } else { Some(0.0) };
    Outputs {
        display_grid: display.map(|d| d.grid().to_vec()).unwrap_or_default(),
        display_on: display.is_some_and(Tm1640::display_on),
        display_brightness: display.map_or(0, Tm1640::brightness),
        leds: leds.iter().map(|c| [c.r, c.g, c.b]).collect(),
        buzzer_hz: machine
            .component::<Buzzer>("buzzer")
            .and_then(Buzzer::tone)
            .map_or(0, |pwm| pwm.frequency_hz().round() as u32),
        headlight_pin_high,
        headlight_duty,
        headlight_source: if powered {
            (headlight_pin_high.is_some() || headlight_duty.is_some())
                .then(|| "GPIOB.12 edges; up to 40 ms simulated high time".into())
        } else {
            Some("Controller supply off".into())
        },
        power_latched: board.power_latched(),
        controller_powered: Some(powered),
    }
}

/// The git commit the engine was built from, when the build could ask git (or `EUC_GIT_REV` was
/// set).
pub const GIT_REV: Option<&str> = option_env!("EUC_GIT_REV");

/// The profile files the profile hash covers, in hash order: every file the engine and the studio
/// read. A file the wheel does not have is left out.
pub const PROFILE_FILES: [&str; 10] = [
    "wheel.toml",
    "board.toml",
    "plant.toml",
    "geometry.toml",
    "stockmap.toml",
    "probes.toml",
    "settings.toml",
    "display.toml",
    "lights.toml",
    "ble.toml",
];

/// sha256 of wheel `id`'s profile files ([`PROFILE_FILES`]): for each file present, its name, a
/// NUL, its text and a NUL.
pub fn profile_sha256(source: &ProfileSource, id: &str) -> Sha256 {
    let mut all = Vec::new();
    for file in PROFILE_FILES {
        if let Ok(text) = source.read(id, file) {
            all.extend_from_slice(file.as_bytes());
            all.push(0);
            all.extend_from_slice(text.as_bytes());
            all.push(0);
        }
    }
    Sha256::digest(&all)
}

/// A header for a run of `session` on the wheel of `files` (read from `source`) with the firmware
/// `firmware` (a label) whose image hashes to `firmware_sha256`. The caller adds the CPU, the
/// scenario and the settings preset.
pub fn header(
    producer: &str,
    source: &ProfileSource,
    files: &WheelFiles,
    session: &Session,
    firmware: &str,
    firmware_sha256: &Sha256,
) -> Header {
    let mut header = Header::new(producer, &files.profile.id, firmware);
    header.git_rev = GIT_REV.map(str::to_owned);
    header.profile_sha256 = Some(profile_sha256(source, &files.profile.id).to_string());
    header.firmware_sha256 = Some(firmware_sha256.to_string());
    header.seed = Some(files.electrical.seed);
    header.sample_hz = 1.0 / session.sample_period_s();
    header.bodies = session.bodies().to_vec();
    header
}

/// A scenario's checks as timeline entries at `t_s` (the end of the run), as the studio posts
/// them: one per check, then the verdict.
pub fn verdict_events(scenario: &str, outcome: &Outcome, t_s: f64) -> Vec<Event> {
    let verdict = |passed: bool| if passed { (EventKind::Scenario, "passed") } else { (EventKind::Fault, "failed") };
    let mut events: Vec<Event> = outcome
        .checks
        .iter()
        .map(|check| {
            let (kind, word) = verdict(check.passed);
            Event { t_s, kind, text: format!("Check {word}: {} ({})", check.what, check.measured) }
        })
        .collect();
    let (kind, word) = verdict(outcome.passed());
    let passed = outcome.checks.iter().filter(|c| c.passed).count();
    let text = format!("Scenario {scenario} {word}: {passed} of {} checks", outcome.checks.len());
    events.push(Event { t_s, kind, text });
    events
}

/// Decodes the firmware's BLE output into [`BleFrame`]s as the run goes.
#[derive(Default)]
pub struct FrameTap {
    read: usize,
    assembler: Assembler,
}

impl FrameTap {
    /// Appends the segments the firmware completed since the last call. Each carries the time of
    /// the last byte read in this call.
    pub fn drain(&mut self, machine: &Machine, out: &mut Vec<BleFrame>) {
        let tx = &machine.board().ble_tx;
        let new = &tx[self.read.min(tx.len())..];
        self.read = tx.len();
        let Some(&(cycle, _)) = new.last() else { return };
        let t_s = cycle as f64 / machine.hclk() as f64;
        let bytes: Vec<u8> = new.iter().map(|&(_, b)| b).collect();
        self.assembler.push(&bytes);
        while let Some(segment) = self.assembler.next_segment() {
            let d = describe(&segment);
            out.push(BleFrame {
                t_s,
                kind: d.kind.to_owned(),
                summary: d.summary,
                fields: d.fields,
                bytes: segment_bytes(&segment).to_vec(),
                reply: d.reply,
            });
        }
    }
}

/// Records a session sample by sample.
pub struct Recorder<W: Write> {
    writer: Writer<W>,
    tap: FrameTap,
    frames: Vec<BleFrame>,
    outputs: Option<Outputs>,
}

impl Recorder<BufWriter<File>> {
    /// Creates `path` and writes `header`.
    pub fn create(path: &Path, header: &Header) -> io::Result<Self> {
        Ok(Self::new(Writer::create(path, header)?))
    }
}

impl<W: Write> Recorder<W> {
    /// Records into `writer`.
    pub fn new(writer: Writer<W>) -> Self {
        Self { writer, tap: FrameTap::default(), frames: Vec::new(), outputs: None }
    }

    /// Adds a sample, its events, new firmware BLE frames, and changed board outputs.
    pub fn sample(&mut self, snapshot: &Snapshot, events: &[Event], machine: &Machine) -> io::Result<()> {
        for event in events {
            self.writer.event(event);
        }
        self.tap.drain(machine, &mut self.frames);
        for frame in self.frames.drain(..) {
            self.writer.frame(frame);
        }
        let now = outputs(machine);
        if self.outputs.as_ref() != Some(&now) {
            self.writer.outputs(OutputsAt { t_s: snapshot.t_s, outputs: now.clone() });
            self.outputs = Some(now);
        }
        self.writer.push(snapshot)
    }

    /// Adds events that came without a sample (the last ones of a run).
    pub fn events(&mut self, events: &[Event]) {
        for event in events {
            self.writer.event(event);
        }
    }

    /// Samples recorded so far.
    pub const fn samples(&self) -> u64 {
        self.writer.samples()
    }

    /// Writes the index and closes the recording.
    pub fn finish(self) -> io::Result<W> {
        self.writer.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aids() -> serde_json::Value {
        serde_json::json!({
            "roll_stiffness": 5000.0, "roll_damping": 500.0, "steer_kp": 2000.0, "steer_kd": 200.0, "steer_max_nm": 60.0
        })
    }

    fn flat_map() -> serde_json::Value {
        serde_json::to_value(euc_physics::map::GeneratedMapSpec::flat(17).generate(1).unwrap()).unwrap()
    }

    #[test]
    fn context_round_trips_every_rider_model_and_fills_in_the_schema() {
        for model in ["rigid_ankle_v1", "articulated_sagittal_v1", "articulated_contact_v1"] {
            let json = serde_json::json!({ "run_mode": "scenario", "rider_model": model, "model_aids": aids() });
            let context = RecordingContext::from_json(&json).unwrap();
            let mut expected = json.clone();
            expected["schema"] = 1.into();
            assert_eq!(context.to_json(), expected);
            assert_eq!(RecordingContext::from_json(&expected).unwrap(), context);
            let mut unknown = json;
            unknown["rider_model"] = "future_model".into();
            assert!(RecordingContext::from_json(&unknown).is_err());
        }
        let mut header = Header::new("test", "w", "f");
        assert_eq!(RecordingContext::of(&header).unwrap(), None);
        header.context = Some(serde_json::json!({
            "schema": 2, "run_mode": "scenario", "rider_model": "rigid_ankle_v1", "model_aids": aids()
        }));
        assert!(RecordingContext::of(&header).unwrap_err().contains("schema 2"));
    }

    #[test]
    fn contact_feet_record_the_upper_body_roll_aid_and_welded_feet_do_not() {
        let assist = euc_physics::Assist::default();
        for (model, upper) in [
            (euc_physics::RiderModel::RigidAnkleV1, 0.0),
            (euc_physics::RiderModel::ArticulatedSagittalV1, 0.0),
            (euc_physics::RiderModel::ArticulatedContactV1, assist.roll_stiffness),
        ] {
            let rider = euc_physics::Rider { model, ..euc_physics::Rider::default() };
            let aids = ModelAids::of(&rider, &assist);
            assert_eq!(aids.upper_body_roll_stiffness, upper);
            let json = serde_json::to_value(aids).unwrap();
            // Absent unless the model has the aid, so welded and rigid contexts stay as before.
            assert_eq!(json.get("upper_body_roll_stiffness").is_some(), upper > 0.0, "{model:?}: {json}");
            assert_eq!(serde_json::from_value::<ModelAids>(json).unwrap(), aids);
        }
        let mut bad = aids();
        bad["upper_body_roll_damping"] = (-1.0).into();
        let json =
            serde_json::json!({ "run_mode": "scenario", "rider_model": "articulated_contact_v1", "model_aids": bad });
        assert!(RecordingContext::from_json(&json).is_err());
    }

    #[test]
    fn free_ride_context_needs_a_well_formed_map() {
        let valid = serde_json::json!({
            "schema": 1, "run_mode": "free_ride", "rider_model": "rigid_ankle_v1",
            "height_map": flat_map(), "model_aids": aids(),
        });
        let context = RecordingContext::from_json(&valid).unwrap();
        assert_eq!(context.height_map.as_ref().unwrap().heights_mm.len(), 129 * 129);
        assert_eq!(context.to_json(), valid);

        let mut bad = Vec::new();
        let mut v = valid.clone();
        v.as_object_mut().unwrap().remove("height_map");
        bad.push(("no map", v));
        let mut v = valid.clone();
        v["height_map"]["nx"] = 128.into();
        bad.push(("wrong grid", v));
        let mut v = valid.clone();
        v["height_map"]["heights_mm"].as_array_mut().unwrap().pop();
        bad.push(("short map", v));
        let mut v = valid.clone();
        v["height_map"]["heights_mm"][7] = 12_001.into();
        bad.push(("too high", v));
        let mut v = valid.clone();
        v["height_map"]["spacing_m"] = 0.5.into();
        bad.push(("wrong spacing", v));
        let mut v = valid.clone();
        v["height_map"]["extra"] = true.into();
        bad.push(("unknown map field", v));
        let mut v = valid.clone();
        v["extra"] = true.into();
        bad.push(("unknown field", v));
        let mut v = valid.clone();
        v["model_aids"]["steer_kp"] = (-1.0).into();
        bad.push(("negative aid", v));
        let mut v = valid.clone();
        v["model_aids"]["roll_damping"] = "x".into();
        bad.push(("aid type", v));
        let mut v = valid;
        v["run_mode"] = "track".into();
        bad.push(("run mode", v));
        for (what, json) in bad {
            assert!(RecordingContext::from_json(&json).is_err(), "{what} must fail");
        }
    }

    #[test]
    fn the_profile_hash_covers_the_profile_files() {
        let builtin = profile_sha256(&ProfileSource::Builtin, "begode-extreme");
        assert_eq!(builtin, profile_sha256(&ProfileSource::Builtin, "begode-extreme"), "stable");
        assert_ne!(builtin, profile_sha256(&ProfileSource::Builtin, "no-such-wheel"));
        let dir = std::env::temp_dir().join(format!("euc-profile-hash-{}", std::process::id()));
        let wheel = dir.join("begode-extreme");
        std::fs::create_dir_all(&wheel).unwrap();
        for file in PROFILE_FILES {
            if let Ok(text) = ProfileSource::Builtin.read("begode-extreme", file) {
                std::fs::write(wheel.join(file), text).unwrap();
            }
        }
        let copy = ProfileSource::Override(euc_profile::ProfileDir::new(&dir));
        assert_eq!(profile_sha256(&copy, "begode-extreme"), builtin, "the same files hash the same");
        std::fs::write(wheel.join("plant.toml"), "changed").unwrap();
        assert_ne!(profile_sha256(&copy, "begode-extreme"), builtin, "a changed file changes the hash");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
