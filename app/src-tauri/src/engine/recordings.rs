//! Recordings in the desktop app: the live run captured into an `.eucrec` file
//! (`euc_telemetry::recording`) in the app cache, "Stop and save" copying it to Downloads, and a
//! bounded import of a file the user picked, decoded here and handed to the web view's playback
//! engine as typed JSON.
//!
//! Capture runs on the sim thread, before the lossy telemetry outbox: the runner binds the run
//! ([`RecordingState::bind_run`]) before its first sample, feeds every sample with its events, the
//! firmware's BLE bytes and the board's outputs ([`RecordingState::ingest`]) and ends the run on
//! restart, halt, completion and shutdown ([`RecordingState::end_run`]). With automatic capture
//! on, every run is captured from its first sample; an unsaved capture holds until it is saved.

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use euc_profile::ProfileSource;
use euc_proto_begode::Assembler;
use euc_proto_begode::describe::{describe, hex, segment_bytes};
use euc_sim::WheelFiles;
pub use euc_sim::recording::RecordingContext;
use euc_sim::scenario::Scenario;
use euc_telemetry::recording::{
    BleFrame, ChannelInfo, EXTENSION, Header, OutputsAt, ReadLimits, Reader, Writer, now_unix_ms, utc,
};
use euc_telemetry::{CHANNELS, Channel, Event, Outputs, Snapshot};
use serde::Serialize;
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager, State};

use super::display::DisplayLayout;
use super::firmware;
use super::lights::LightsLayout;
use super::phone::PhoneFrame;
use super::runner::SuspensionInfo;

/// What wrote a recording.
const PRODUCER: &str = concat!("EUC Studio ", env!("CARGO_PKG_VERSION"));

/// The most a recording file may hold, bytes (import and capture).
pub const MAX_FILE_BYTES: usize = 32 * 1024 * 1024;
/// Samples at most (600 s at 120 Hz).
pub const MAX_SAMPLES: usize = 72_000;
/// Scalar and pose values at most, so the import's JSON stays bounded.
pub const MAX_VALUES: usize = 2_000_000;
/// Simulated seconds at most.
pub const MAX_DURATION_S: f64 = 600.0;
const MAX_HEADER_BYTES: usize = 256 * 1024;
const MAX_EVENT_BYTES: usize = 1024 * 1024;
const MAX_EVENTS: usize = 10_000;
const MAX_EVENT_TEXT: usize = 2048;
const MAX_FRAMES: usize = 100_000;
const MAX_OUTPUTS: usize = 100_000;
const MAX_BODIES: usize = 128;
// Bound the total drawing work, independently of shape distribution across bodies.
// The Extreme's tire alone uses 48 capsules.
const MAX_GEOMS: usize = 256;
/// Bytes a sample takes before compression: its time, its columns and its poses.
const fn sample_bytes(width: usize) -> usize {
    8 + 4 * width
}

/// What the web view gets for an opened recording. Unmodelled scalar values travel as null;
/// poses are always finite.
#[derive(Serialize)]
pub struct RecordingData {
    /// The file's header as it is (its `context` as JSON).
    pub header: Header,
    /// The header's context, validated; absent on a run that recorded none.
    pub context: Option<RecordingContext>,
    /// The channels of this build the file holds, in schema order: the columns of `signals`.
    pub channels: Vec<ChannelInfo>,
    pub start_s: f64,
    pub end_s: f64,
    pub times: Vec<f64>,
    pub signals: Vec<Vec<Option<f32>>>,
    /// Sample order, then body order, seven floats per pose.
    pub poses: Vec<f32>,
    pub events: Vec<Event>,
    /// The firmware's BLE frames, as the virtual phone lists them.
    pub frames: Vec<PhoneFrame>,
    /// Board output changes (display, LEDs, buzzer), in time order.
    pub outputs: Vec<OutputsAt>,
    /// Why the file ends early (its writer stopped), if it does.
    pub truncated: Option<String>,
    /// Chunks and items of kinds this build does not know.
    pub skipped: usize,
    /// The display replica, the tail light, the shock and the terrain, rebuilt from the wheel's
    /// built-in profile and the scenario the file carries (none of them is in the file).
    pub display: Option<DisplayLayout>,
    pub lights: Option<LightsLayout>,
    pub terrain: Option<Vec<[f32; 3]>>,
    pub suspension: Option<SuspensionInfo>,
}

fn require(ok: bool, message: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(message.into()) }
}

fn label_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}

fn pose_ok(values: &[f32]) -> bool {
    values.len() == 7
        && values.iter().all(|v| v.is_finite())
        && values[..3].iter().all(|v| v.abs() <= 1_000_000.0)
        && (0.5..=1.5).contains(&values[3..].iter().map(|v| v * v).sum::<f32>())
}

/// The channels of this build `header` lists, in schema order, as `(column, spec)`: the file's
/// known columns must be this build's schema or an older prefix of it (ids are append-only), in
/// order; columns of channels this build does not know (a later build) are left out.
fn known_columns(header: &Header) -> Result<Vec<(usize, ChannelInfo)>, String> {
    let known: Vec<(usize, &ChannelInfo)> =
        header.channels.iter().enumerate().filter(|(_, c)| Channel::from_id(c.id).is_some()).collect();
    require(!known.is_empty() && known.len() <= CHANNELS.len(), "Unsupported channel schema")?;
    for ((column, actual), expected) in known.iter().zip(CHANNELS.iter()) {
        require(
            actual.id == expected.id
                && actual.key == expected.key
                && actual.unit == expected.unit
                && label_ok(&actual.label)
                && matches!(actual.rate.as_str(), "fast" | "slow")
                && header.channels[..*column].iter().all(|c| c.id != actual.id),
            "Unsupported channel schema",
        )?;
    }
    Ok(known.into_iter().map(|(i, c)| (i, c.clone())).collect())
}

fn validate_header(h: &Header) -> Result<Option<RecordingContext>, String> {
    let context = RecordingContext::of(h)?;
    require(label_ok(&h.wheel) && label_ok(&h.firmware) && label_ok(&h.producer), "Invalid recording labels")?;
    require(h.scenario.as_deref().is_none_or(label_ok), "Invalid scenario label")?;
    require(h.settings.as_deref().is_none_or(label_ok), "Invalid settings label")?;
    require(h.sample_hz.is_finite() && h.sample_hz > 0.0 && h.sample_hz <= 1000.0, "Invalid sample rate")?;
    require(h.bodies.len() <= MAX_BODIES, "Too many bodies")?;
    known_columns(h)?;
    let mut geom_count = 0;
    for body in &h.bodies {
        require(label_ok(&body.name), "Invalid body metadata")?;
        require(body.geoms.len() <= MAX_GEOMS - geom_count, "Too many body shapes")?;
        geom_count += body.geoms.len();
        for geom in &body.geoms {
            let p = [geom.pos[0], geom.pos[1], geom.pos[2], geom.quat[0], geom.quat[1], geom.quat[2], geom.quat[3]];
            require(
                pose_ok(&p) && geom.size.iter().all(|v| v.is_finite() && (0.0..=100.0).contains(v)),
                "Invalid body shape",
            )?;
        }
    }
    require(serde_json::to_vec(h).is_ok_and(|json| json.len() <= MAX_HEADER_BYTES), "Recording header is too large")?;
    Ok(context)
}

fn event_ok(event: &Event, end_s: f64) -> bool {
    event.t_s.is_finite() && event.t_s >= 0.0 && event.t_s <= end_s + 1.0 && event.text.len() <= MAX_EVENT_TEXT
}

/// The scene a recording plays in, rebuilt from the wheel's built-in profile and the scenario
/// the header carries.
struct Scene {
    display: Option<DisplayLayout>,
    lights: Option<LightsLayout>,
    terrain: Option<Vec<[f32; 3]>>,
    suspension: Option<SuspensionInfo>,
}

impl Scene {
    fn of(header: &Header) -> Self {
        let source = ProfileSource::Builtin;
        let read = |file: &str| source.read(&header.wheel, file).ok();
        let suspension = WheelFiles::load(&source, &header.wheel).ok().and_then(|files| {
            let s = files.body.suspension.as_ref()?;
            let (eye_top_m, eye_bottom_m) = firmware::shock_eyes(&read("geometry.toml")?)?;
            Some(SuspensionInfo { linkage_ratio: s.linkage_ratio, eye_top_m, eye_bottom_m })
        });
        let scenario = header.scenario_toml.as_deref().and_then(|text| Scenario::from_toml(text).ok());
        // Scenario text is file input too. Bound optional reconstruction independently of
        // telemetry chunks, and distinguish a recorded flat surface from missing ground.
        // A `[world]` scenario's ground comes from the recorded world (its context), never a strip.
        let terrain = scenario
            .as_ref()
            .filter(|s| {
                s.world.is_none()
                    && s.terrain.length_m <= 10_000.0
                    && s.terrain.features.len() <= 128
                    && s.terrain.validate().is_ok()
            })
            .map(|s| {
                firmware::terrain_profile(s)
                    .unwrap_or_else(|| vec![[-5.0, 0.0, 0.0], [s.terrain.length_m as f32, 0.0, 0.0]])
            });
        Self {
            display: read("display.toml").and_then(|t| DisplayLayout::parse(&t).ok()),
            lights: read("lights.toml").and_then(|t| LightsLayout::parse(&t).ok()),
            terrain,
            suspension,
        }
    }
}

/// The phone's frame kinds, as the live decoder names them.
fn frame_kind(kind: &str) -> &'static str {
    match kind {
        "live" => "live",
        "bms" => "bms",
        "cells" => "cells",
        "status" => "status",
        "power" => "power",
        "text" => "text",
        _ => "other",
    }
}

/// Decodes a recording the user selected, within the limits. A file whose writer stopped early
/// reads up to its last whole second and says so (`truncated`); every other defect is an error.
pub fn read_recording(bytes: &[u8]) -> Result<RecordingData, String> {
    require(bytes.len() <= MAX_FILE_BYTES, "Recording exceeds 32 MiB")?;
    require(bytes.len() >= 16, "Recording is empty or truncated")?;
    require(bytes.get(..8) == Some(&euc_telemetry::recording::MAGIC[..]), "Select an .eucrec recording")?;
    let limits = ReadLimits {
        max_file_bytes: MAX_FILE_BYTES as u64,
        max_header_bytes: MAX_HEADER_BYTES as u64,
        max_chunk_bytes: MAX_FILE_BYTES as u64,
        max_chunks: 10_000,
        max_samples: MAX_SAMPLES as u64,
        max_sample_values: MAX_VALUES as u64,
        max_sparse_bytes: 16 * 1024 * 1024,
        max_events: MAX_EVENTS as u64,
        max_frames: MAX_FRAMES as u64,
        max_outputs: MAX_OUTPUTS as u64,
    };
    let mut reader = Reader::new_with_limits(Cursor::new(bytes), limits).map_err(|e| e.to_string())?;
    let header = reader.header().clone();
    let context = validate_header(&header)?;
    let columns = known_columns(&header)?;
    let bodies = header.bodies.len();
    // The reader decodes stored columns before unknown columns are discarded.
    let width = header.channels.len() + bodies * euc_telemetry::batch::POSE_FLOATS;

    let n = usize::try_from(reader.sample_count()).map_err(|_| "Invalid sample count")?;
    require(n > 0, "Recording has no samples")?;
    require(
        n <= MAX_SAMPLES && n.checked_mul(width).is_some_and(|values| values <= MAX_VALUES),
        "Recording exceeds sample or value limits",
    )?;
    let samples = reader.read_all().map_err(|e| e.to_string())?;
    require(samples.len() == n && samples.bodies == bodies, "Recording samples do not match its index")?;
    let mut times = Vec::with_capacity(n);
    for &t in &samples.t {
        require(t.is_finite() && (0.0..=604_800.0).contains(&t), "Invalid sample time")?;
        if let Some(&previous) = times.last() {
            require(t > previous, "Sample times must increase")?;
        }
        times.push(t);
    }
    let start_s = times[0];
    let mut end_s = times[n - 1];
    require(end_s - start_s <= MAX_DURATION_S, "Recording exceeds 600 seconds")?;
    let mut signals = Vec::with_capacity(columns.len());
    for (column, _) in &columns {
        let mut out = Vec::with_capacity(n);
        for &v in &samples.columns[*column] {
            require(!v.is_infinite(), "Infinite signal value")?;
            out.push(if v.is_nan() { None } else { Some(v) });
        }
        signals.push(out);
    }
    require(
        samples.poses.len() == n * bodies * euc_telemetry::batch::POSE_FLOATS
            && samples.poses.as_chunks::<{ euc_telemetry::batch::POSE_FLOATS }>().0.iter().all(|p| pose_ok(p)),
        "Invalid body pose",
    )?;

    let mut events = reader.events().to_vec();
    require(events.len() <= MAX_EVENTS, "Too many events")?;
    require(serde_json::to_vec(&events).is_ok_and(|json| json.len() <= MAX_EVENT_BYTES), "Events exceed 1 MiB")?;
    require(events.iter().all(|e| event_ok(e, end_s)), "Invalid recording event")?;
    events.sort_by(|a, b| a.t_s.total_cmp(&b.t_s));
    // Final scenario checks can come just after the last sample.
    if let Some(event) = events.last() {
        end_s = end_s.max(event.t_s);
    }
    require(end_s - start_s <= MAX_DURATION_S, "Recording exceeds 600 seconds")?;

    require(reader.frames().len() <= MAX_FRAMES, "Too many BLE frames")?;
    require(reader.outputs().len() <= MAX_OUTPUTS, "Too many output changes")?;
    let mut frames: Vec<&BleFrame> = reader.frames().iter().collect();
    frames.sort_by(|a, b| a.t_s.total_cmp(&b.t_s));
    let frames = frames
        .into_iter()
        .enumerate()
        .map(|(i, f)| PhoneFrame {
            id: i as u64 + 1,
            t: f.t_s,
            kind: frame_kind(&f.kind),
            summary: f.summary.clone(),
            fields: f.fields.clone(),
            hex: hex(&f.bytes),
            reply: f.reply,
        })
        .collect();
    require(frames_ok(reader.frames(), end_s), "Invalid BLE frame")?;
    let mut outputs = reader.outputs().to_vec();
    require(outputs.iter().all(|o| o.t_s.is_finite() && o.t_s >= 0.0), "Invalid output change")?;
    outputs.sort_by(|a, b| a.t_s.total_cmp(&b.t_s));

    let scene = Scene::of(&header);
    Ok(RecordingData {
        channels: columns.into_iter().map(|(_, c)| c).collect(),
        context,
        header,
        start_s,
        end_s,
        times,
        signals,
        poses: samples.poses,
        events,
        frames,
        outputs,
        truncated: reader.truncated().map(str::to_owned),
        skipped: reader.skipped(),
        display: scene.display,
        lights: scene.lights,
        terrain: scene.terrain,
        suspension: scene.suspension,
    })
}

fn frames_ok(frames: &[BleFrame], end_s: f64) -> bool {
    frames.iter().all(|f| {
        f.t_s.is_finite()
            && f.t_s >= 0.0
            && f.t_s <= end_s + 1.0
            && f.summary.len() <= 1024
            && f.bytes.len() <= 4096
            && f.fields.len() <= 256
    })
}

/// Only bytes the user selected cross the IPC boundary: no file path is accepted.
#[tauri::command]
pub fn recordings_open(request: Request<'_>) -> Result<RecordingData, String> {
    let InvokeBody::Raw(bytes) = request.body() else { return Err("Send recording bytes".into()) };
    read_recording(bytes)
}

/// The run the runner binds: the header its recording starts with (the wheel, the firmware, the
/// scenario, the bodies and the sample rate) and the optional schema-1 context (run mode, rider
/// model, aids, the free ride's map), validated before capture starts.
#[derive(Clone)]
pub struct RunInfo {
    pub header: Header,
    pub context: Option<serde_json::Value>,
}

impl RunInfo {
    /// A run described by its labels alone, for runs without a full header (and tests).
    pub fn basic(
        wheel: &str,
        firmware: &str,
        scenario: Option<&str>,
        sample_hz: f64,
        bodies: Vec<euc_telemetry::BodyInfo>,
    ) -> Self {
        let mut header = Header::new(PRODUCER, wheel, firmware);
        header.scenario = scenario.map(str::to_owned);
        header.sample_hz = sample_hz;
        header.bodies = bodies;
        Self { header, context: None }
    }

    fn header(&self) -> Header {
        let mut header = self.header.clone();
        header.producer = PRODUCER.to_owned();
        header.started_unix_ms = now_unix_ms();
        header.context = self.context.clone();
        header
    }
}

/// Where capture stands, for the recordings panel.
#[derive(Clone, Default, Serialize)]
pub struct CaptureStatus {
    /// `idle`, `capturing`, `ready` or `error`. A ready capture is a finished segment, not
    /// necessarily a whole run.
    pub state: String,
    pub can_start: bool,
    /// True only when the runner started capture before its first sample.
    pub from_start: bool,
    pub samples: usize,
    pub start_s: Option<f64>,
    pub end_s: Option<f64>,
    pub message: String,
}

struct Capture {
    writer: Option<Writer<BufWriter<File>>>,
    path: PathBuf,
    bodies: usize,
    assembler: Assembler,
    outputs: Option<Outputs>,
    events: usize,
    event_bytes: usize,
}

#[derive(Default)]
struct Store {
    run: Option<RunInfo>,
    capture: Option<Capture>,
    status: CaptureStatus,
    autocapture_dir: Option<PathBuf>,
}

/// Capture state shared by the runner (which feeds it) and the commands (which start, save and
/// report it). Clone it into the runner.
#[derive(Clone, Default)]
pub struct RecordingState(Arc<Mutex<Store>>);

impl RecordingState {
    /// Turns automatic capture on; call it before the runner starts. `cache_dir` is Tauri's app
    /// cache directory.
    pub fn enable_autocapture(&self, cache_dir: PathBuf) -> Result<(), String> {
        let dir = cache_dir.join("recordings");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut store = self.0.lock().map_err(|_| "Recording state is unavailable")?;
        require(store.run.is_none(), "Enable automatic capture before creating the run")?;
        store.autocapture_dir = Some(dir);
        Ok(())
    }

    /// A new run starts: ends the previous capture, remembers the run and, with automatic
    /// capture on and no unsaved capture, starts capturing it.
    pub fn bind_run(&self, run: RunInfo) -> Result<(), String> {
        validate_header(&run.header())?;
        let mut store = self.0.lock().map_err(|_| "Recording state is unavailable")?;
        Self::finish(&mut store, "The run changed. Save the captured segment.");
        store.run = Some(run);
        if let Some(dir) = store.autocapture_dir.clone() {
            if store.capture.is_none() {
                if let Err(e) = Self::start(&mut store, &dir, true) {
                    store.status.state = "error".into();
                    store.status.message = format!("Automatic capture failed: {e}");
                    return Err(e);
                }
            } else {
                store.status.message = "Save the previous capture. The new run has no capture.".into();
            }
        }
        Ok(())
    }

    /// A sample with the events that came with it, the BLE bytes the firmware sent since the last
    /// sample and the board's outputs now. Call it before the runner clears its events or puts
    /// the sample in its bounded outbox.
    pub fn ingest(&self, snapshot: &Snapshot, events: &[Event], ble: &[(f64, u8)], outputs: Option<&Outputs>) {
        let Ok(mut store) = self.0.lock() else { return };
        if store.status.state != "capturing" {
            return;
        }
        let Some(capture) = store.capture.as_ref() else { return };
        let width = CHANNELS.len() + capture.bodies * euc_telemetry::batch::POSE_FLOATS;
        // A terminal snapshot can repeat the last time when the CPU halts without advancing.
        // Keep the earlier pose and take the new events without writing a duplicate sample.
        let repeated = store.status.end_s == Some(snapshot.t_s);
        let count = store.status.samples + usize::from(!repeated);
        let start = store.status.start_s.unwrap_or(snapshot.t_s);
        let invalid = !snapshot.t_s.is_finite()
            || !(0.0..=604_800.0).contains(&snapshot.t_s)
            || store.status.end_s.is_some_and(|t| snapshot.t_s < t)
            || (!repeated
                && (snapshot.bodies.len() != capture.bodies
                    || snapshot.bodies.iter().any(|p| !pose_ok(&p.to_array()))
                    || CHANNELS.iter().any(|c| snapshot.signals.get(c.channel).is_infinite())));
        if invalid {
            Self::fail(&mut store, "Invalid telemetry stopped capture");
            return;
        }
        let new_event_bytes = events.iter().try_fold(0usize, |size, e| {
            if !event_ok(e, snapshot.t_s) {
                return None;
            }
            serde_json::to_vec(e).ok().and_then(|json| size.checked_add(json.len() + 1))
        });
        let Some(new_event_bytes) = new_event_bytes else {
            Self::fail(&mut store, "Invalid events stopped capture");
            return;
        };
        let written = capture.writer.as_ref().map_or(0, |w| {
            usize::try_from(w.bytes_written()).unwrap_or(usize::MAX) + w.pending_samples() * sample_bytes(width)
        });
        let new_sample_bytes = if repeated { 0 } else { sample_bytes(width) };
        let ble_bytes = ble.len() * 2;
        let limit = if count > MAX_SAMPLES {
            Some("72,000 samples")
        } else if count * width > MAX_VALUES {
            Some("2,000,000 signal and pose values")
        } else if snapshot.t_s - start > MAX_DURATION_S || events.iter().any(|event| event.t_s - start > MAX_DURATION_S)
        {
            Some("600 simulated seconds")
        } else if capture.events + events.len() > MAX_EVENTS {
            Some("10,000 events")
        } else if capture.event_bytes + new_event_bytes > MAX_EVENT_BYTES {
            Some("1 MiB of events")
        } else if written + new_sample_bytes + new_event_bytes + ble_bytes + 1024 > MAX_FILE_BYTES {
            Some("32 MiB")
        } else {
            None
        };
        if let Some(limit) = limit {
            Self::finish(&mut store, &format!("Capture stopped at {limit}. Save the captured segment."));
            return;
        }
        let capture = store.capture.as_mut().expect("capture exists");
        let Some(writer) = capture.writer.as_mut() else { return };
        for event in events {
            writer.event(event);
        }
        if let Some(&(t_s, _)) = ble.last() {
            let bytes: Vec<u8> = ble.iter().map(|&(_, b)| b).collect();
            capture.assembler.push(&bytes);
            while let Some(segment) = capture.assembler.next_segment() {
                let d = describe(&segment);
                writer.frame(BleFrame {
                    t_s,
                    kind: d.kind.to_owned(),
                    summary: d.summary,
                    fields: d.fields,
                    bytes: segment_bytes(&segment).to_vec(),
                    reply: d.reply,
                });
            }
        }
        if let Some(now) = outputs
            && capture.outputs.as_ref() != Some(now)
        {
            writer.outputs(OutputsAt { t_s: snapshot.t_s, outputs: now.clone() });
            capture.outputs = Some(now.clone());
        }
        if !repeated && let Err(e) = writer.push(snapshot) {
            Self::fail(&mut store, &format!("Capture write failed: {e}"));
            return;
        }
        capture.events += events.len();
        capture.event_bytes += new_event_bytes;
        store.status.samples = count;
        store.status.start_s = Some(start);
        store.status.end_s = Some(snapshot.t_s);
    }

    /// The run ends (restart, halt, completion, shutdown): the capture is finished with `reason`.
    pub fn end_run(&self, reason: &str) {
        if let Ok(mut store) = self.0.lock() {
            Self::finish(&mut store, reason);
            store.run = None;
        }
    }

    fn finish(store: &mut Store, reason: &str) {
        let Some(capture) = store.capture.as_mut() else { return };
        let Some(writer) = capture.writer.take() else { return };
        match writer.finish() {
            Ok(_) if store.status.samples > 0 => {
                store.status.state = "ready".into();
                store.status.message = reason.into();
            }
            Ok(_) => Self::fail(store, "Capture contains no samples"),
            Err(e) => Self::fail(store, &format!("Capture finish failed: {e}")),
        }
    }

    fn fail(store: &mut Store, reason: &str) {
        if let Some(capture) = store.capture.take() {
            drop(capture.writer);
            let _ = fs::remove_file(&capture.path);
            if let Some(dir) = capture.path.parent() {
                let _ = fs::remove_dir(dir);
            }
        }
        store.status.state = "error".into();
        store.status.message = reason.into();
    }

    fn start(store: &mut Store, dir: &Path, from_start: bool) -> Result<CaptureStatus, String> {
        require(store.capture.is_none(), "Save the current captured segment first")?;
        let run = store.run.as_ref().ok_or("Live capture is unavailable. Connect the runner first.")?;
        let header = run.header();
        validate_header(&header)?;
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
        // A private directory, made atomically: no two captures share a file.
        let dir = dir.join(format!("{}-{nonce}", std::process::id()));
        fs::create_dir(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("capture.{EXTENSION}"));
        let writer = Writer::create(&path, &header).map_err(|e| e.to_string())?;
        store.capture = Some(Capture {
            writer: Some(writer),
            path,
            bodies: header.bodies.len(),
            assembler: Assembler::new(),
            outputs: None,
            events: 0,
            event_bytes: 0,
        });
        store.status = CaptureStatus {
            state: "capturing".into(),
            from_start,
            message: if from_start {
                "Automatic capture started before the first sample"
            } else {
                "Capturing future samples only"
            }
            .into(),
            ..CaptureStatus::default()
        };
        Ok(store.status.clone())
    }

    /// A file name for a saved capture: `<scenario or run>-<UTC date>-<time>.eucrec`.
    pub fn suggested_filename(&self) -> String {
        let store = self.0.lock().ok();
        let name = store
            .as_ref()
            .and_then(|s| s.run.as_ref())
            .and_then(|r| r.header.scenario.clone())
            .unwrap_or_else(|| "run".into());
        let stamp: String = utc(now_unix_ms()).chars().filter(char::is_ascii_digit).collect();
        let (date, time) = stamp.split_at(8.min(stamp.len()));
        format!("{name}-{date}-{time}.{EXTENSION}")
    }
}

#[tauri::command]
pub fn recordings_status(state: State<'_, RecordingState>) -> Result<CaptureStatus, String> {
    let store = state.0.lock().map_err(|_| "Recording state is unavailable")?;
    let mut status = store.status.clone();
    status.can_start = store.run.is_some() && store.capture.is_none();
    if status.state.is_empty() {
        status.state = "idle".into();
    }
    if status.message.is_empty() && store.run.is_none() {
        status.message = "Live capture is unavailable until the runner supplies recording metadata".into();
    }
    Ok(status)
}

#[tauri::command]
pub fn recordings_start(app: AppHandle, state: State<'_, RecordingState>) -> Result<CaptureStatus, String> {
    let mut store = state.0.lock().map_err(|_| "Recording state is unavailable")?;
    let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("recordings");
    RecordingState::start(&mut store, &dir, false)
}

/// A saved capture: where it went.
#[derive(Serialize)]
pub struct SavedCapture {
    pub path: String,
    pub status: CaptureStatus,
}

/// A file name for a new capture, from the run's scenario and the time.
#[tauri::command]
pub fn recordings_suggest_filename(state: State<'_, RecordingState>) -> String {
    state.suggested_filename()
}

/// Checks a file name for `recordings_save`: a leaf ending in `.eucrec`, no path.
pub fn filename_ok(filename: &str) -> bool {
    !filename.is_empty()
        && filename.len() <= 128
        && filename.ends_with(&format!(".{EXTENSION}"))
        && filename.len() > EXTENSION.len() + 1
        && !filename.starts_with('.')
        && !filename
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

/// Finishes the capture and copies it to `<Downloads>/<filename>`. The name is a leaf; an
/// existing file is never replaced.
#[tauri::command]
pub fn recordings_save(
    app: AppHandle,
    state: State<'_, RecordingState>,
    filename: String,
) -> Result<SavedCapture, String> {
    require(filename_ok(&filename), "Use a file name ending in .eucrec, without a path")?;
    let target = app.path().download_dir().map_err(|e| e.to_string())?.join(&filename);
    save_capture(&state, &target)
}

/// `recordings_save` without Tauri: finishes the capture and copies it to `target`.
pub fn save_capture(state: &RecordingState, target: &Path) -> Result<SavedCapture, String> {
    let mut store = state.0.lock().map_err(|_| "Recording state is unavailable")?;
    RecordingState::finish(&mut store, "Capture stopped. This file contains the captured segment.");
    require(store.status.state == "ready", &store.status.message)?;
    let capture = store.capture.as_ref().ok_or("There is no captured segment")?;
    let mut source = File::open(&capture.path).map_err(|e| e.to_string())?;
    require(source.metadata().map_err(|e| e.to_string())?.len() <= MAX_FILE_BYTES as u64, "Capture exceeds 32 MiB")?;
    let mut dest = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|e| format!("Cannot create {}: {e}", target.display()))?;
    let copied =
        std::io::copy(&mut Read::by_ref(&mut source).take(MAX_FILE_BYTES as u64 + 1), &mut dest).and_then(|n| {
            if n > MAX_FILE_BYTES as u64 {
                return Err(std::io::Error::other("Capture exceeds 32 MiB"));
            }
            dest.flush()?;
            dest.sync_all()
        });
    if let Err(e) = copied {
        drop(dest);
        let _ = fs::remove_file(target);
        return Err(format!("Cannot save capture: {e}"));
    }
    let path = capture.path.clone();
    let status = store.status.clone();
    store.capture = None;
    store.status = CaptureStatus {
        state: "idle".into(),
        message: format!("Saved the captured segment to {}", target.display()),
        ..CaptureStatus::default()
    };
    drop(source);
    let _ = fs::remove_file(&path);
    if let Some(dir) = path.parent() {
        let _ = fs::remove_dir(dir);
    }
    Ok(SavedCapture { path: target.to_string_lossy().into_owned(), status })
}

#[cfg(test)]
mod tests {
    use euc_telemetry::{BodyInfo, BodyRole, EventKind, Geom, GeomKind, Pose, Signals};

    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("euc-capture-test-{}-{nonce}-{serial}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn shape() -> Geom {
        Geom { kind: GeomKind::Sphere, size: [0.1, 0.0, 0.0], pos: [0.0; 3], quat: Pose::IDENTITY.quat }
    }

    fn body(shapes: usize) -> BodyInfo {
        BodyInfo { name: "test_body".into(), role: BodyRole::Other, geoms: vec![shape(); shapes] }
    }

    fn run(bodies: Vec<BodyInfo>) -> RunInfo {
        RunInfo::basic("begode-extreme", "our-test-build", None, 120.0, bodies)
    }

    fn capturing(bodies: usize) -> (TestDir, RecordingState) {
        let dir = TestDir::new();
        let state = RecordingState::default();
        state.enable_autocapture(dir.0.clone()).unwrap();
        state.bind_run(run((0..bodies).map(|_| body(1)).collect())).unwrap();
        (dir, state)
    }

    fn snapshot(sample: u32, bodies: usize) -> Snapshot {
        Snapshot {
            t_s: f64::from(sample) / 120.0,
            signals: Signals { speed_kmh: sample as f32, battery_v: 130.0, ..Signals::default() },
            bodies: vec![Pose { pos: [sample as f32 / 100.0, 0.0, 0.3], ..Pose::IDENTITY }; bodies],
        }
    }

    fn event(t_s: f64, text: &str) -> Event {
        Event { t_s, kind: EventKind::Scenario, text: text.into() }
    }

    fn path(state: &RecordingState) -> PathBuf {
        state.0.lock().unwrap().capture.as_ref().unwrap().path.clone()
    }

    fn status(state: &RecordingState) -> CaptureStatus {
        state.0.lock().unwrap().status.clone()
    }

    fn finish_bytes(state: &RecordingState) -> Vec<u8> {
        state.end_run("Fixture ended. Captured segment only.");
        assert_eq!(status(state).state, "ready", "{}", status(state).message);
        fs::read(path(state)).unwrap()
    }

    fn fixture(samples: u32) -> Vec<u8> {
        let (_dir, state) = capturing(1);
        for sample in 0..samples {
            state.ingest(&snapshot(sample, 1), &[], &[], None);
        }
        finish_bytes(&state)
    }

    fn aids() -> serde_json::Value {
        serde_json::json!({
            "roll_stiffness": 5000.0, "roll_damping": 500.0, "steer_kp": 2000.0, "steer_kd": 200.0, "steer_max_nm": 60.0
        })
    }

    fn free_context(model: &str) -> serde_json::Value {
        serde_json::json!({
            "schema": 1, "run_mode": "free_ride", "rider_model": model,
            "height_map": euc_physics::map::GeneratedMapSpec::flat(17).generate(37).unwrap(),
            "model_aids": aids(),
        })
    }

    /// A scenario as recordings made before the city have it: no `[world]`, flat ground.
    const FLAT_SCENARIO: &str = "schema = 1\nname = \"balance-60s\"\ndescription = \"balance\"\n\
        wheel = \"begode-extreme\"\nseconds = 63.0\nrelease_s = 2.5\nspeed_kmh = [[0, 0]]\n";

    #[test]
    fn playback_restores_known_flat_ground_and_bounds_scenario_geometry() {
        let mut header = run(Vec::new()).header();
        header.scenario_toml = Some(FLAT_SCENARIO.into());
        let scene = Scene::of(&header);
        let terrain = scene.terrain.expect("saved flat scenario has known ground");
        assert!(terrain.iter().all(|point| point[1] == 0.0));
        // A city scenario draws its recorded world, not a strip.
        header.scenario_toml = Some(include_str!("../../../../scenarios/balance-60s.toml").into());
        assert!(Scene::of(&header).terrain.is_none(), "the world is the ground");
    }

    #[test]
    fn playback_bounds_optional_scenario_geometry() {
        let mut header = run(Vec::new()).header();
        header.scenario_toml = Some(format!("{FLAT_SCENARIO}\n[terrain]\nlength_m = 10001.0\nslope_pct = 2.0\n"));
        assert!(Scene::of(&header).terrain.is_none(), "unsupported geometry stays unavailable");
    }

    #[test]
    fn unknown_columns_still_count_toward_the_import_value_budget() {
        let mut header = Header::new(PRODUCER, "begode-extreme", "fixture");
        header.sample_hz = 120.0;
        for id in 1000..1100 {
            let mut channel = header.channels[0].clone();
            channel.id = id;
            channel.key = format!("future_{id}");
            header.channels.push(channel);
        }
        let values = vec![0.0; header.channels.len()];
        let mut writer = Writer::new(Vec::new(), &header).unwrap();
        for i in 0..20_000 {
            writer.push_values(f64::from(i) / 120.0, &values, &[]).unwrap();
        }
        let bytes = writer.finish().unwrap();
        assert!(bytes.len() < MAX_FILE_BYTES);
        let error = read_recording(&bytes).err().expect("all stored columns must count");
        assert!(error.contains("value"), "{error}");
    }

    #[test]
    fn capture_keeps_keyframes_null_signals_frames_outputs_and_repeated_terminal_events() {
        let (_dir, state) = capturing(1);
        // A live frame split across two samples, as BLE notifications split it.
        let frame: [u8; 24] = [
            0x55, 0xAA, 0x19, 0x55, 0, 0, 0, 0x75, 0, 0x06, 0, 0xBE, 0xF0, 0xFA, 0x14, 0xA9, 0, 0x09, 0, 0x18, 0x5A,
            0x5A, 0x5A, 0x5A,
        ];
        for sample in 0..201u32 {
            let s = snapshot(sample, 1);
            let ble: Vec<(f64, u8)> = match sample {
                10 => frame[..20].iter().map(|&b| (s.t_s, b)).collect(),
                11 => frame[20..].iter().map(|&b| (s.t_s, b)).collect(),
                _ => Vec::new(),
            };
            let outputs = Outputs { buzzer_hz: if sample < 150 { 0 } else { 2700 }, ..Outputs::default() };
            state.ingest(&s, &[], &ble, Some(&outputs));
        }
        let last = snapshot(200, 1);
        // A halt can add events without another sample. The previous sample stays.
        state.ingest(&last, &[event(last.t_s + 0.25, "fixture halt")], &[], None);
        let bytes = finish_bytes(&state);
        let decoded = read_recording(&bytes).unwrap();
        assert_eq!(decoded.times.len(), 201);
        assert_eq!(decoded.times[120], 1.0);
        assert_eq!(decoded.signals[0][200], Some(200.0));
        assert_eq!(decoded.channels.len(), CHANNELS.len());
        assert!(decoded.signals.iter().any(|column| column.iter().all(Option::is_none)), "unmodelled is null");
        assert_eq!(decoded.poses.len(), 201 * 7);
        assert_eq!(&decoded.poses[200 * 7..], &last.bodies[0].to_array());
        assert_eq!(decoded.events[0].text, "fixture halt");
        assert_eq!(decoded.end_s, last.t_s + 0.25);
        assert_eq!(decoded.frames.len(), 1);
        assert_eq!((decoded.frames[0].kind, decoded.frames[0].t), ("live", 11.0 / 120.0));
        assert_eq!(decoded.outputs.len(), 2, "only output changes are kept");
        assert_eq!(decoded.outputs[1].outputs.buzzer_hz, 2700);
        assert_eq!(decoded.truncated, None);
        assert!(decoded.context.is_none());
        assert_eq!(decoded.header.producer, PRODUCER);
        assert!(decoded.display.is_some() && decoded.lights.is_some(), "the built-in profile dresses the scene");
        let json = serde_json::to_value(&decoded).unwrap();
        assert!(
            json["signals"].as_array().unwrap().iter().any(|column| column
                .as_array()
                .unwrap()
                .iter()
                .all(serde_json::Value::is_null))
        );
    }

    #[test]
    fn reset_finishes_the_old_capture_and_the_replacement_run_cannot_append_to_it() {
        let (dir, state) = capturing(1);
        state.ingest(&snapshot(0, 1), &[event(0.0, "old run")], &[], None);
        let old_path = path(&state);
        state.end_run("Reset cancelled this run. Captured segment only.");
        let old_bytes = fs::read(&old_path).unwrap();
        let mut replacement = run(vec![]);
        replacement.header.firmware = "different-fixture-build".into();
        state.bind_run(replacement).unwrap();
        state.ingest(&snapshot(1, 0), &[event(0.0, "replacement run")], &[], None);
        {
            let mut store = state.0.lock().unwrap();
            assert_eq!(store.status.state, "ready");
            assert!(store.status.message.contains("previous capture"));
            assert!(RecordingState::start(&mut store, &dir.0, false).is_err());
            assert_eq!(store.run.as_ref().unwrap().header.firmware, "different-fixture-build");
        }
        assert_eq!(path(&state), old_path);
        assert_eq!(fs::read(&old_path).unwrap(), old_bytes);
        let old = read_recording(&old_bytes).unwrap();
        assert_eq!(old.times, [0.0]);
        assert_eq!(old.header.firmware, "our-test-build");
        assert_eq!(old.events.len(), 1);
        assert_eq!(old.events[0].text, "old run");
        state.end_run("Second reset");
        assert_eq!(fs::read(&old_path).unwrap(), old_bytes, "ready segments survive later resets");
    }

    #[test]
    fn manual_start_captures_only_future_samples_and_an_empty_cancel_leaves_no_file() {
        let dir = TestDir::new();
        let state = RecordingState::default();
        state.bind_run(run(vec![])).unwrap();
        state.ingest(&snapshot(0, 0), &[], &[], None);
        {
            let mut store = state.0.lock().unwrap();
            assert!(store.capture.is_none());
            assert!(!RecordingState::start(&mut store, &dir.0, false).unwrap().from_start);
        }
        let empty = path(&state);
        state.end_run("Cancelled before the first sample");
        assert!(!empty.exists());
        assert_eq!(status(&state).state, "error");
        state.bind_run(run(vec![])).unwrap();
        RecordingState::start(&mut state.0.lock().unwrap(), &dir.0, false).unwrap();
        state.ingest(&snapshot(120, 0), &[], &[], None);
        let decoded = read_recording(&finish_bytes(&state)).unwrap();
        assert_eq!(decoded.times, [1.0]);
        assert_eq!(decoded.start_s, 1.0);
    }

    #[test]
    fn invalid_samples_or_events_remove_the_unfinished_file_and_allow_a_fresh_capture() {
        let mut invalid = vec![snapshot(2, 0), snapshot(2, 2)];
        let mut bad_pose = snapshot(2, 1);
        bad_pose.bodies[0].quat = [0.0; 4];
        invalid.push(bad_pose);
        let mut infinite = snapshot(2, 1);
        infinite.signals.speed_kmh = f32::INFINITY;
        invalid.push(infinite);
        let mut reverse = snapshot(2, 1);
        reverse.t_s = -1.0;
        invalid.push(reverse);
        let mut nan = snapshot(2, 1);
        nan.t_s = f64::NAN;
        invalid.push(nan);
        for row in invalid {
            let (dir, state) = capturing(1);
            state.ingest(&snapshot(1, 1), &[], &[], None);
            let failed_path = path(&state);
            state.ingest(&row, &[], &[], None);
            assert!(!failed_path.exists());
            {
                let mut store = state.0.lock().unwrap();
                assert_eq!(store.status.state, "error");
                assert!(store.capture.is_none());
                RecordingState::start(&mut store, &dir.0, false).unwrap();
            }
            state.ingest(&snapshot(3, 1), &[], &[], None);
            let decoded = read_recording(&finish_bytes(&state)).unwrap();
            assert_eq!(decoded.times, [3.0 / 120.0]);
        }
        for bad in [event(-1.0, "past"), event(f64::NAN, "nan"), event(2.0, "future"), event(0.0, &"x".repeat(2049))] {
            let (_dir, state) = capturing(0);
            state.ingest(&snapshot(0, 0), &[], &[], None);
            let failed_path = path(&state);
            state.ingest(&snapshot(1, 0), &[bad], &[], None);
            assert_eq!(status(&state).state, "error");
            assert!(!failed_path.exists());
        }
    }

    #[test]
    fn cut_files_read_up_to_their_last_whole_second_and_say_so() {
        let bytes = fixture(250);
        let full = read_recording(&bytes).unwrap();
        assert_eq!((full.times.len(), full.truncated), (250, None));
        // Without its trailer: everything, flagged.
        let cut = read_recording(&bytes[..bytes.len() - 16]).unwrap();
        assert_eq!(cut.times.len(), 250);
        assert!(cut.truncated.is_some());
        // Inside the third keyframe: the first two whole seconds.
        let mut seen = 0;
        for length in (20..bytes.len()).step_by(97) {
            match read_recording(&bytes[..length]) {
                Ok(data) => {
                    assert!(data.truncated.is_some(), "a cut file says so ({length})");
                    assert!(data.times.len() % 120 == 0 || data.times.len() == 250, "{} samples", data.times.len());
                    seen += 1;
                }
                Err(e) => assert!(
                    e.contains("no samples") || e.contains("truncated") || e.contains("header"),
                    "{e} at {length}"
                ),
            }
        }
        assert!(seen > 0);
        let mut trailing = bytes.clone();
        trailing.extend_from_slice(b"junk after the trailer");
        assert!(read_recording(&trailing).is_err() || read_recording(&trailing).unwrap().truncated.is_some());
    }

    #[test]
    fn other_files_oversized_files_and_bad_headers_are_rejected() {
        assert_eq!(read_recording(&vec![0; MAX_FILE_BYTES + 1]).err().unwrap(), "Recording exceeds 32 MiB");
        assert_eq!(read_recording(b"EUCR\x01\0\0\0 the old format ..").err().unwrap(), "Select an .eucrec recording");
        assert!(read_recording(b"").is_err());
        let bytes = fixture(1);
        let mut newer = bytes.clone();
        newer[8] = 2;
        assert!(read_recording(&newer).err().unwrap().contains("newer"));
        let header_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let mut header: serde_json::Value = serde_json::from_slice(&bytes[16..16 + header_len]).unwrap();
        let replace = |header: &serde_json::Value| {
            let json = serde_json::to_vec(header).unwrap();
            let mut out = bytes[..12].to_vec();
            out.extend((json.len() as u32).to_le_bytes());
            out.extend(json);
            out.extend_from_slice(&bytes[16 + header_len..]);
            out
        };
        let good = header.clone();
        header["channels"][0]["key"] = "not_speed".into();
        assert_eq!(read_recording(&replace(&header)).err().unwrap(), "Unsupported channel schema");
        header = good.clone();
        header["wheel"] = "".into();
        assert_eq!(read_recording(&replace(&header)).err().unwrap(), "Invalid recording labels");
        header = good.clone();
        header["sample_hz"] = 0.0.into();
        assert_eq!(read_recording(&replace(&header)).err().unwrap(), "Invalid sample rate");
        // Use a structurally valid payload, so this checks the desktop body limit rather
        // than the canonical reader's earlier header/payload dimension check.
        let oversized = run(vec![body(0); MAX_BODIES + 1]).header();
        let mut writer = Writer::new(Vec::new(), &oversized).unwrap();
        writer.push(&snapshot(0, MAX_BODIES + 1)).unwrap();
        assert_eq!(read_recording(&writer.finish().unwrap()).err().unwrap(), "Too many bodies");
    }

    #[test]
    fn shape_budget_is_global_and_applies_to_capture_and_import() {
        assert!(validate_header(&run(vec![body(256)]).header()).is_ok());
        assert!(validate_header(&run(vec![body(128), body(128)]).header()).is_ok());
        assert_eq!(validate_header(&run(vec![body(257)]).header()).unwrap_err(), "Too many body shapes");
        assert_eq!(validate_header(&run(vec![body(128), body(129)]).header()).unwrap_err(), "Too many body shapes");
        assert_eq!(RecordingState::default().bind_run(run(vec![body(257)])).unwrap_err(), "Too many body shapes");
        assert!(validate_header(&run(vec![body(0); MAX_BODIES]).header()).is_ok());
        assert_eq!(validate_header(&run(vec![body(0); MAX_BODIES + 1]).header()).unwrap_err(), "Too many bodies");
        let mut h = run(vec![body(1)]).header();
        h.bodies[0].geoms[0].pos[0] = f32::NAN;
        assert!(validate_header(&h).is_err());
        h.bodies[0].geoms[0] = shape();
        h.bodies[0].geoms[0].quat = [0.0; 4];
        assert!(validate_header(&h).is_err());
        h.bodies[0].geoms[0] = shape();
        h.bodies[0].geoms[0].size[0] = -1.0;
        assert!(validate_header(&h).is_err());
    }

    #[test]
    fn capture_and_import_share_the_value_budget_at_maximum_body_count() {
        let dir = TestDir::new();
        let state = RecordingState::default();
        state.enable_autocapture(dir.0.clone()).unwrap();
        state.bind_run(run(vec![body(0); MAX_BODIES])).unwrap();
        let width = CHANNELS.len() + MAX_BODIES * euc_telemetry::batch::POSE_FLOATS;
        let samples = MAX_VALUES / width;
        let poses = vec![Pose::IDENTITY; MAX_BODIES];
        for sample in 0..=samples {
            state.ingest(
                &Snapshot { t_s: sample as f64 / 60.0, bodies: poses.clone(), ..Snapshot::default() },
                &[],
                &[],
                None,
            );
        }
        let s = status(&state);
        assert_eq!(s.state, "ready");
        assert!(s.message.contains("2,000,000"));
        assert_eq!(s.samples, samples);
        let bytes = fs::read(path(&state)).unwrap();
        assert!(bytes.len() < MAX_FILE_BYTES);
        let decoded = read_recording(&bytes).unwrap();
        assert_eq!(decoded.times.len(), samples);
        assert!(samples * width <= MAX_VALUES && (samples + 1) * width > MAX_VALUES);
    }

    #[test]
    fn duration_sample_and_event_limits_finish_only_the_accepted_prefix() {
        let (_dir, state) = capturing(0);
        state.ingest(&snapshot(0, 0), &[], &[], None);
        state.ingest(
            &Snapshot { t_s: MAX_DURATION_S, ..Snapshot::default() },
            &[event(MAX_DURATION_S + 0.5, "late check")],
            &[],
            None,
        );
        assert!(status(&state).message.contains("600"));
        let decoded = read_recording(&fs::read(path(&state)).unwrap()).unwrap();
        assert!(decoded.end_s - decoded.start_s <= MAX_DURATION_S);

        let (_dir, state) = capturing(0);
        for sample in 0..=MAX_SAMPLES as u32 {
            state.ingest(&snapshot(sample, 0), &[], &[], None);
        }
        assert_eq!(status(&state).samples, MAX_SAMPLES);
        assert!(status(&state).message.contains("72,000"));
        assert_eq!(read_recording(&finish_bytes(&state)).unwrap().times.len(), MAX_SAMPLES);

        let (_dir, state) = capturing(0);
        state.ingest(&snapshot(0, 0), &[], &[], None);
        state.ingest(&snapshot(1, 0), &vec![event(0.0, "fixture"); MAX_EVENTS], &[], None);
        state.ingest(&snapshot(2, 0), &[event(0.0, "over budget")], &[], None);
        assert!(status(&state).message.contains("10,000"));
        let decoded = read_recording(&finish_bytes(&state)).unwrap();
        assert_eq!(decoded.events.len(), MAX_EVENTS);
        assert_eq!(decoded.times.len(), 2);

        let (_dir, state) = capturing(0);
        state.ingest(&snapshot(0, 0), &[], &[], None);
        state.ingest(&snapshot(1, 0), &vec![event(0.0, &"\\".repeat(2048)); 512], &[], None);
        assert!(status(&state).message.contains("1 MiB"));
        let decoded = read_recording(&finish_bytes(&state)).unwrap();
        assert_eq!(decoded.times, [0.0]);
        assert!(decoded.events.is_empty());
    }

    #[test]
    fn free_ride_context_survives_capture_and_import_and_bad_context_is_refused_first() {
        for model in ["rigid_ankle_v1", "articulated_sagittal_v1", "articulated_contact_v1"] {
            let (_dir, state) = capturing(0);
            let context = free_context(model);
            let mut info = run(vec![body(1)]);
            info.context = Some(context.clone());
            state.end_run("x");
            state.bind_run(info).unwrap();
            state.ingest(&snapshot(0, 1), &[], &[], None);
            let decoded = read_recording(&finish_bytes(&state)).unwrap();
            let typed = decoded.context.clone().expect("context");
            assert_eq!(typed.rider_model.id(), model);
            assert_eq!(typed.height_map.as_ref().unwrap().nx, 129);
            assert_eq!(serde_json::to_value(&typed).unwrap(), context);
            assert_eq!(decoded.header.context, Some(context));
        }

        let valid = free_context("rigid_ankle_v1");
        let mut invalid = Vec::new();
        let mut v = valid.clone();
        v["schema"] = 2.into();
        invalid.push(v);
        let mut v = valid.clone();
        v["rider_model"] = "unknown_model".into();
        invalid.push(v);
        let mut v = valid.clone();
        v["extra"] = true.into();
        invalid.push(v);
        let mut v = valid.clone();
        v.as_object_mut().unwrap().remove("height_map");
        invalid.push(v);
        let mut v = valid.clone();
        v["height_map"]["nx"] = 128.into();
        invalid.push(v);
        let mut v = valid.clone();
        v["height_map"]["heights_mm"].as_array_mut().unwrap().pop();
        invalid.push(v);
        let mut v = valid;
        v["model_aids"]["steer_kp"] = (-1).into();
        invalid.push(v);
        for context in invalid {
            let (_dir, state) = capturing(1);
            state.ingest(&snapshot(0, 1), &[], &[], None);
            let old_path = path(&state);
            let mut replacement = run(vec![]);
            replacement.header.firmware = "invalid-replacement".into();
            replacement.context = Some(context.clone());
            assert!(state.bind_run(replacement).is_err());
            assert_eq!(path(&state), old_path);
            assert_eq!(status(&state).state, "capturing");
            assert_eq!(state.0.lock().unwrap().run.as_ref().unwrap().header.firmware, "our-test-build");
            state.ingest(&snapshot(1, 1), &[], &[], None);
            let bytes = finish_bytes(&state);
            assert_eq!(read_recording(&bytes).unwrap().times.len(), 2);
            // The same context planted in a file is refused by the reader.
            let header_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
            let mut header: serde_json::Value = serde_json::from_slice(&bytes[16..16 + header_len]).unwrap();
            header["context"] = context;
            let json = serde_json::to_vec(&header).unwrap();
            let mut planted = bytes[..12].to_vec();
            planted.extend((json.len() as u32).to_le_bytes());
            planted.extend(json);
            planted.extend_from_slice(&bytes[16 + header_len..]);
            assert!(read_recording(&planted).is_err());
        }
    }

    #[test]
    fn automatic_capture_reports_a_filesystem_failure_then_recovers() {
        let dir = TestDir::new();
        let state = RecordingState::default();
        state.enable_autocapture(dir.0.clone()).unwrap();
        let blocked = dir.0.join("recordings");
        fs::remove_dir(&blocked).unwrap();
        fs::write(&blocked, b"fixture blocks the cache directory").unwrap();
        assert!(state.bind_run(run(vec![])).is_err());
        assert_eq!(status(&state).state, "error");
        assert!(state.0.lock().unwrap().capture.is_none());
        fs::remove_file(&blocked).unwrap();
        fs::create_dir(&blocked).unwrap();
        state.bind_run(run(vec![])).unwrap();
        state.ingest(&snapshot(0, 0), &[], &[], None);
        assert_eq!(read_recording(&finish_bytes(&state)).unwrap().times, [0.0]);
    }

    #[test]
    fn an_older_channel_prefix_reads_and_a_newer_channel_is_dropped() {
        let (_dir, state) = capturing(1);
        state.ingest(&snapshot(0, 1), &[], &[], None);
        let bytes = finish_bytes(&state);
        let header_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let header: Header = serde_json::from_slice(&bytes[16..16 + header_len]).unwrap();
        // An older producer: one channel. Its samples chunk has one column.
        let mut old = header.clone();
        old.channels.truncate(1);
        let mut w = Writer::new(Vec::new(), &old).unwrap();
        w.push_values(0.0, &[7.0], &[Pose::IDENTITY]).unwrap();
        let decoded = read_recording(&w.finish().unwrap()).unwrap();
        assert_eq!(decoded.signals, [vec![Some(7.0)]]);
        assert_eq!(decoded.channels.len(), 1);
        assert!(decoded.context.is_none(), "a file without context invents none");
        // A newer producer: a channel this build does not know, after ours.
        let mut newer = header;
        newer.channels.push(ChannelInfo {
            id: 999,
            key: "gizmo".into(),
            label: "G".into(),
            unit: "".into(),
            rate: "fast".into(),
        });
        let mut w = Writer::new(Vec::new(), &newer).unwrap();
        let mut values = vec![f32::NAN; newer.channels.len()];
        values[0] = 3.0;
        values[newer.channels.len() - 1] = 9.0;
        w.push_values(0.0, &values, &[Pose::IDENTITY]).unwrap();
        let decoded = read_recording(&w.finish().unwrap()).unwrap();
        assert_eq!(decoded.channels.len(), CHANNELS.len());
        assert_eq!(decoded.signals[0], [Some(3.0)]);
    }

    #[test]
    fn save_copies_the_finished_capture_once_and_checks_the_name() {
        assert!(filename_ok("ride.eucrec"));
        for bad in ["", ".eucrec", "ride.eucr", "../ride.eucrec", "a/b.eucrec", ".hidden.eucrec", "x:y.eucrec"] {
            assert!(!filename_ok(bad), "{bad}");
        }
        let (dir, state) = capturing(1);
        state.ingest(&snapshot(0, 1), &[], &[], None);
        state.ingest(&snapshot(1, 1), &[], &[], None);
        let source = path(&state);
        let target = dir.0.join("saved.eucrec");
        let saved = save_capture(&state, &target).unwrap();
        assert_eq!(saved.path, target.to_string_lossy());
        assert_eq!(saved.status.samples, 2);
        assert!(!source.exists(), "the temporary file is gone");
        assert_eq!(read_recording(&fs::read(&target).unwrap()).unwrap().times.len(), 2);
        let s = status(&state);
        assert_eq!(s.state, "idle");
        assert!(s.message.contains("Saved"));
        assert!(save_capture(&state, &dir.0.join("again.eucrec")).is_err(), "nothing left to save");
        // A new capture refuses to replace an existing file.
        RecordingState::start(&mut state.0.lock().unwrap(), &dir.0, false).unwrap();
        state.ingest(&snapshot(2, 1), &[], &[], None);
        assert!(save_capture(&state, &target).err().unwrap().contains("Cannot create"));
        assert_eq!(status(&state).state, "ready", "the capture is still there to save elsewhere");
        assert!(state.suggested_filename().ends_with(".eucrec"));
    }
}
