//! The simulation thread and the stream pump.
//!
//! The sim thread owns the [`Host`] (built inside it, so the CPU backend never crosses a
//! thread), paces it against wall time at the requested real-time factor and drops every
//! snapshot, event and phone frame into an [`Outbox`]. The pump thread empties the outbox about
//! 60 times a second and sends it to the web view. The two meet only in a mutex held for a copy:
//! a slow web view costs dropped samples (counted in the batch header), never a stalled sim.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use euc_sim::Manual;
use euc_telemetry::batch::BatchWriter;
use euc_telemetry::recording::Header;
use euc_telemetry::{BodyInfo, Event, EventKind, Outputs, Snapshot};
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};

use super::buttons;
use super::display::DisplayLayout;
use super::free_ride::{FreeRideSpec, FreeRideState, InputSource};
use super::host::{Host, Tone};
use super::lights::LightsLayout;
use super::native_audio::{AudioError, AudioStatus, NativeBuzzer};
use super::phone::{PhoneDecoder, PhoneFrame};
use super::recordings::{RecordingState, RunInfo};
use super::tune;
use super::world::WorldFrame;

/// Physics and firmware advance in steps of this many simulated seconds.
const STEP_S: f64 = 1.0 / 120.0;
/// A snapshot every this many steps (60 Hz).
const SNAPSHOT_EVERY: u32 = 2;
/// Samples the outbox keeps when nobody drains it (10 s).
const OUTBOX_SAMPLES: usize = 600;
/// The sim thread yields to commands at least this often.
const SLICE: Duration = Duration::from_millis(8);
/// How often the pump sends.
const PUMP_PERIOD: Duration = Duration::from_millis(16);
/// Behind by more than this, the sim gives up catching up (the measured factor drops instead).
const MAX_DEBT_S: f64 = 0.25;
/// Simulated seconds between two queued phone commands, as two taps would be.
const COMMAND_GAP_S: f64 = 0.3;
/// How long the queue holds after a command that expects a text answer (`V`, `N`). The stock
/// firmware answers about 0.7 to 1.7 s later and drops the answer when another command arrives
/// first (seen in the simulator), so the next command waits for the answer or this timeout.
const ANSWER_TIMEOUT_S: f64 = 3.0;

/// A byte waiting for its simulated send time.
struct Pending {
    at: f64,
    byte: u8,
    /// The command's label, on its first byte (the timeline shows it when that byte goes out).
    label: Option<String>,
    /// On the last byte of a command that expects a text answer.
    answer: bool,
}

/// Builds a host for a scenario, reporting progress (`"Fetching the stock image"`).
pub type HostFactory = Box<dyn Fn(&str, &str, &mut dyn FnMut(&str)) -> Result<Built, String> + Send>;

/// A host and what the status bar says about it.
pub struct Built {
    pub host: Box<dyn Host>,
    /// The wheel profile the run is built on (for a recording's header; see `recordings`).
    pub wheel: String,
    /// The header a recording of this run starts with; none when the run cannot be recorded.
    pub header: Option<Header>,
    /// What runs, for the status bar ("Stock firmware, no physics yet").
    pub label: String,
    /// Firmware build label.
    pub build: String,
    /// A note that stays in the status (for example what is not modelled yet).
    pub note: Option<String>,
    /// The wheel's display replica, if the profile describes it.
    pub display: Option<DisplayLayout>,
    /// The tail light's LED layout, if the profile describes it.
    pub lights: Option<LightsLayout>,
    /// The ground along the path, `[x, z, slope z]` metres, when it is not flat (`terrain_profile`).
    pub terrain: Option<Vec<[f32; 3]>>,
    /// The streaming world the run rides, when it rides one (a `[world]` scenario or a world free ride).
    pub world: Option<WorldFrame>,
    /// The rear suspension's shock, for drawing it compressed; none on a wheel without one.
    pub suspension: Option<SuspensionInfo>,
}

/// The wheel's rear shock as the 3D view draws it: where its eyes are and how axle travel maps to
/// shock travel. From the profile: `plant.toml` `[body.suspension]` and `geometry.toml` `[shock]`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SuspensionInfo {
    /// Axle travel per unit of shock travel.
    pub linkage_ratio: f64,
    /// The eye that stays with the frame, wheel frame (x forward, y left, z up), metres.
    pub eye_top_m: [f64; 3],
    /// The eye that the linkage moves.
    pub eye_bottom_m: [f64; 3],
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// `scenario` or `free_ride`.
    pub run_mode: &'static str,
    /// Counts the runs since the engine started; input that names an older run is dropped.
    pub run_epoch: u64,
    /// `starting`, `running`, `paused`, `halted`, `error`.
    pub state: &'static str,
    pub message: String,
    /// What runs ("Stock firmware, no physics yet").
    pub label: Option<String>,
    pub build: Option<String>,
    pub backend: &'static str,
    pub scenario: String,
    pub firmware: String,
    pub t_s: f64,
    /// Measured sim seconds per wall second over the last second.
    pub rtf: f64,
    pub target_rtf: f64,
    pub playing: bool,
    /// The same authoritative admission condition as the physical input gate.
    pub buttons_ready: bool,
    pub power_button_ready: bool,
}

/// What the pump sends on the JSON channel.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Status(Status),
    Audio {
        status: AudioStatus,
    },
    /// A new run started: forget the old history.
    Reset {
        scenario: String,
    },
    Bodies {
        bodies: Vec<BodyInfo>,
    },
    /// The display replica (sent with the bodies; `null` when the wheel has none).
    Display {
        layout: Option<DisplayLayout>,
    },
    /// The tail light's LED layout (sent with the bodies; `null` when the wheel has none).
    Lights {
        layout: Option<LightsLayout>,
    },
    /// The ground's height along the path (sent with the bodies; `null` on flat ground).
    Terrain {
        profile: Option<Vec<[f32; 3]>>,
    },
    /// The streaming world and its frame (sent with the bodies; `null` when the run has none).
    World {
        frame: Option<WorldFrame>,
    },
    /// The rear shock (sent with the bodies; `null` on a wheel without suspension).
    Suspension {
        info: Option<SuspensionInfo>,
    },
    Events {
        events: Vec<Event>,
    },
    Frames {
        frames: Vec<PhoneFrame>,
    },
    Outputs {
        outputs: Outputs,
    },
    /// Buzzer changes at their simulated times, for the web view to play.
    Buzzer {
        tones: Vec<Tone>,
    },
}

enum Cmd {
    /// Repeat the reset, bodies and status messages for a web view that just attached.
    Announce,
    Play(bool),
    Step,
    Rtf(f64),
    Restart(Option<String>),
    Firmware(String),
    FreeRide(Box<FreeRideSpec>, u64),
    ManualArm(u64, InputSource),
    ManualPause(u64),
    Button(buttons::Request),
    Send {
        bytes: Vec<(f64, u8)>,
        label: String,
        answer: bool,
    },
    /// Manual control of the rider, or `None` to give it back to the scenario.
    Manual(Option<Manual>),
    StepOff,
    Stop,
}

#[derive(Default)]
struct Outbox {
    writer: Option<BatchWriter>,
    messages: Vec<Message>,
}

pub struct Sinks {
    pub stream: Channel<InvokeResponseBody>,
    pub messages: Channel<Message>,
}

/// A running engine: the sim thread and the pump.
pub struct Runner {
    tx: Sender<Cmd>,
    button_gate: Arc<Mutex<buttons::Gate>>,
    tune_tx: mpsc::SyncSender<tune::Request>,
    sinks: Arc<Mutex<Option<Sinks>>>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    audio: Result<NativeBuzzer, AudioError>,
    audio_enabled: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Runner {
    pub fn start(
        factory: HostFactory,
        scenario: String,
        backend: &'static str,
        recordings: RecordingState,
        free_ride: FreeRideState,
        sound_enabled: bool,
    ) -> Self {
        // AudioQueue is ready before the simulation can produce its first startup tone.
        let audio = NativeBuzzer::start(sound_enabled);
        Self::start_with_audio(factory, scenario, backend, recordings, free_ride, sound_enabled, audio)
    }

    /// Runs the normal threads and command path without opening an OS audio device.
    /// This test seam does not validate startup audio; the native audio tests cover that path.
    #[cfg(test)]
    pub(super) fn start_without_audio_for_test(
        factory: HostFactory,
        scenario: String,
        backend: &'static str,
        recordings: RecordingState,
        free_ride: FreeRideState,
    ) -> Self {
        Self::start_with_audio(
            factory,
            scenario,
            backend,
            recordings,
            free_ride,
            false,
            Err(AudioError("No OS audio device in this runner test".into())),
        )
    }

    fn start_with_audio(
        factory: HostFactory,
        scenario: String,
        backend: &'static str,
        recordings: RecordingState,
        free_ride: FreeRideState,
        sound_enabled: bool,
        audio: Result<NativeBuzzer, AudioError>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let (tune_tx, tune_rx) = mpsc::sync_channel(1);
        let outbox = Arc::new(Mutex::new(Outbox::default()));
        let button_gate = Arc::new(Mutex::new(buttons::Gate::default()));
        let sinks: Arc<Mutex<Option<Sinks>>> = Arc::new(Mutex::new(None));
        let sim = {
            let outbox = Arc::clone(&outbox);
            let button_gate = Arc::clone(&button_gate);
            let buzzer = audio.as_ref().ok().cloned();
            thread::Builder::new()
                .name("euc-sim".into())
                .spawn(move || {
                    Sim::new(factory, scenario, backend, outbox, recordings, button_gate, free_ride)
                        .with_audio(buzzer)
                        .run(&rx, &tune_rx)
                })
                .expect("spawn the sim thread")
        };
        let stop = Arc::new(AtomicBool::new(false));
        let pump = {
            let (outbox, sinks, stop) = (Arc::clone(&outbox), Arc::clone(&sinks), Arc::clone(&stop));
            thread::Builder::new()
                .name("euc-pump".into())
                .spawn(move || pump(&outbox, &sinks, &stop))
                .expect("spawn the pump")
        };
        Self {
            tx,
            button_gate,
            tune_tx,
            sinks,
            stop,
            threads: vec![sim, pump],
            audio,
            audio_enabled: AtomicBool::new(sound_enabled),
        }
    }

    pub fn audio_status(&self) -> AudioStatus {
        match &self.audio {
            Ok(audio) => audio.status(),
            Err(error) => AudioStatus {
                backend: if cfg!(target_os = "macos") { "native" } else { "unsupported" },
                state: "error",
                enabled: self.audio_enabled.load(Ordering::Relaxed),
                error: Some(error.to_string()),
            },
        }
    }

    pub fn set_sound(&self, enabled: bool) -> AudioStatus {
        self.audio_enabled.store(enabled, Ordering::Relaxed);
        if let Ok(audio) = &self.audio
            && let Err(error) = audio.set_enabled(enabled)
        {
            return AudioStatus { error: Some(error.to_string()), state: "error", ..audio.status() };
        }
        self.audio_status()
    }

    /// Where the pump sends from now on (a reloaded web view attaches again).
    pub fn attach(&self, sinks: Sinks) {
        *lock(&self.sinks) = Some(sinks);
    }

    pub(super) fn tune(&self, query: tune::Query) -> Result<tune::Pending, String> {
        tune::enqueue(&self.tune_tx, query)
    }

    /// Asks the sim to describe the current run again (bodies, status) for a new web view.
    pub fn announce(&self) {
        let _ = self.tx.send(Cmd::Announce);
    }

    pub fn play(&self, on: bool) {
        if !on {
            let mut gate = lock(&self.button_gate);
            gate.accepting = false;
            gate.cancel_holds();
        }
        let _ = self.tx.send(Cmd::Play(on));
    }

    pub fn step(&self) {
        let _ = self.tx.send(Cmd::Step);
    }

    pub fn set_rtf(&self, rtf: f64) {
        let _ = self.tx.send(Cmd::Rtf(rtf.clamp(0.05, 8.0)));
    }

    pub fn set_firmware(&self, firmware: String) {
        let _ = self.tx.send(Cmd::Firmware(firmware));
    }

    /// Bounded admission only; GPIO application and epoch validation occur on the sim thread.
    pub(super) fn button(&self, button: buttons::Button, duration_ms: u32) -> Result<String, String> {
        let mut gate = lock(&self.button_gate);
        let request = gate.reserve(button, duration_ms)?;
        if self.tx.send(Cmd::Button(request)).is_err() {
            gate.pending[button.index()] = false;
            return Err("The simulation thread stopped".into());
        }
        Ok(button.label(duration_ms))
    }

    /// Fixed last-state slots, with synchronous admission and no queued renewal messages.
    pub(super) fn button_state(&self, button: buttons::Button, pressed: bool, epoch: u64) -> Result<(), String> {
        if self.stop.load(Ordering::Relaxed) {
            return Err("The simulation thread stopped".into());
        }
        lock(&self.button_gate).set_state(button, pressed, epoch)
    }

    pub fn restart(&self, scenario: Option<String>) {
        let _ = self.tx.send(Cmd::Restart(scenario));
    }

    pub(super) fn start_free_ride(&self, spec: FreeRideSpec, ticket: u64) -> Result<(), String> {
        self.tx.send(Cmd::FreeRide(Box::new(spec), ticket)).map_err(|_| "The simulation thread stopped".into())
    }

    pub(super) fn manual_arm(&self, epoch: u64, source: InputSource) -> Result<(), String> {
        self.tx.send(Cmd::ManualArm(epoch, source)).map_err(|_| "The simulation thread stopped".into())
    }

    pub(super) fn manual_pause(&self, epoch: u64) -> Result<(), String> {
        self.tx.send(Cmd::ManualPause(epoch)).map_err(|_| "The simulation thread stopped".into())
    }

    /// Queues bytes for the firmware's BLE input, each after its gap in simulated seconds;
    /// `answer` holds the queue after them until the firmware answers (see [`ANSWER_TIMEOUT_S`]).
    pub fn send(&self, bytes: Vec<(f64, u8)>, label: String, answer: bool) {
        let _ = self.tx.send(Cmd::Send { bytes, label, answer });
    }

    /// Takes manual control of the rider (kept across restarts), or gives it back (`None`).
    pub fn manual(&self, manual: Option<Manual>) {
        let _ = self.tx.send(Cmd::Manual(manual));
    }

    /// The rider steps off the pedals.
    pub fn step_off(&self) {
        let _ = self.tx.send(Cmd::StepOff);
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

fn pump(outbox: &Mutex<Outbox>, sinks: &Mutex<Option<Sinks>>, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(PUMP_PERIOD);
        let (batch, messages) = {
            let mut out = lock(outbox);
            (out.writer.as_mut().and_then(BatchWriter::take), std::mem::take(&mut out.messages))
        };
        let sinks = lock(sinks);
        let Some(s) = sinks.as_ref() else { continue };
        // Messages that reset or describe bodies go before the samples that depend on them.
        let (early, late): (Vec<_>, Vec<_>) = messages.into_iter().partition(|m| {
            matches!(
                m,
                Message::Reset { .. }
                    | Message::Bodies { .. }
                    | Message::Display { .. }
                    | Message::Lights { .. }
                    | Message::Terrain { .. }
                    | Message::Suspension { .. }
            )
        });
        for m in early {
            let _ = s.messages.send(m);
        }
        if let Some(bytes) = batch {
            let _ = s.stream.send(InvokeResponseBody::Raw(bytes));
        }
        for m in late {
            let _ = s.messages.send(m);
        }
    }
}

struct Sim {
    factory: HostFactory,
    recordings: RecordingState,
    session_id: u64,
    button_gate: Arc<Mutex<buttons::Gate>>,
    free_ride: FreeRideState,
    free_spec: Option<FreeRideSpec>,
    free_job: Option<u64>,
    /// Manual control of the rider (keys, gamepad), applied again to every new host.
    manual: Option<Manual>,
    scenario: String,
    firmware: String,
    backend: &'static str,
    outbox: Arc<Mutex<Outbox>>,
    host: Option<Box<dyn Host>>,
    build: Option<String>,
    label: Option<String>,
    note: Option<String>,
    display: Option<DisplayLayout>,
    lights: Option<LightsLayout>,
    terrain: Option<Vec<[f32; 3]>>,
    world: Option<WorldFrame>,
    suspension: Option<SuspensionInfo>,
    error: Option<String>,
    outputs: Option<Outputs>,
    outputs_at: Option<Instant>,
    playing: bool,
    rtf: f64,
    debt: f64,
    last: Instant,
    steps: u32,
    snapshot: Snapshot,
    events: Vec<Event>,
    ble: Vec<(f64, u8)>,
    tones: Vec<Tone>,
    frames: Vec<PhoneFrame>,
    phone: PhoneDecoder,
    /// Bytes waiting for their simulated send time; a command's first byte carries its label.
    pending: VecDeque<Pending>,
    /// Holding the queue for an answer until this simulated time.
    awaiting: Option<f64>,
    window: VecDeque<(Instant, f64)>,
    last_status: Instant,
    audio: Option<NativeBuzzer>,
    audio_playing: bool,
}

impl Sim {
    fn new(
        factory: HostFactory,
        scenario: String,
        backend: &'static str,
        outbox: Arc<Mutex<Outbox>>,
        recordings: RecordingState,
        button_gate: Arc<Mutex<buttons::Gate>>,
        free_ride: FreeRideState,
    ) -> Self {
        Self {
            factory,
            recordings,
            session_id: 0,
            button_gate,
            free_ride,
            free_spec: None,
            free_job: None,
            manual: None,
            scenario,
            firmware: "stock".into(),
            backend,
            outbox,
            host: None,
            build: None,
            label: None,
            note: None,
            display: None,
            lights: None,
            terrain: None,
            world: None,
            suspension: None,
            error: None,
            outputs: None,
            outputs_at: None,
            playing: true,
            rtf: 1.0,
            debt: 0.0,
            last: Instant::now(),
            steps: 0,
            snapshot: Snapshot::default(),
            events: Vec::new(),
            ble: Vec::new(),
            tones: Vec::new(),
            frames: Vec::new(),
            phone: PhoneDecoder::default(),
            pending: VecDeque::new(),
            awaiting: None,
            window: VecDeque::new(),
            last_status: Instant::now(),
            audio: None,
            audio_playing: false,
        }
    }

    fn with_audio(mut self, audio: Option<NativeBuzzer>) -> Self {
        self.audio = audio;
        self
    }

    fn post(&self, message: Message) {
        lock(&self.outbox).messages.push(message);
    }

    fn status(&self, state: &'static str, message: String) -> Status {
        let t_s = self.host.as_ref().map_or(0.0, |h| h.time_s());
        Status {
            run_mode: if self.free_spec.is_some() { "free_ride" } else { "scenario" },
            run_epoch: self.session_id,
            state,
            message,
            label: self.label.clone(),
            build: self.build.clone(),
            backend: self.backend,
            scenario: self.scenario.clone(),
            firmware: self.firmware.clone(),
            t_s,
            rtf: self.measured_rtf(),
            target_rtf: self.rtf,
            playing: self.playing,
            buttons_ready: self.buttons_ready(),
            power_button_ready: self.power_button_ready(),
        }
    }

    fn post_status(&mut self) {
        self.sync_audio();
        self.last_status = Instant::now();
        let status = match (&self.error, &self.host) {
            (Some(e), _) => self.status("error", e.clone()),
            (None, None) => self.status("starting", "Starting the simulator".into()),
            (None, Some(h)) => match h.halted() {
                Some(why) => self.status("halted", why),
                None if h.completed() => self.status("completed", "Scenario complete. Restart for another run".into()),
                None if self.playing => self.status("running", self.note.clone().unwrap_or_default()),
                None => self.status("paused", self.note.clone().unwrap_or_default()),
            },
        };
        self.post(Message::Status(status));
        if let Some(audio) = &self.audio {
            self.post(Message::Audio { status: audio.status() });
        }
    }

    fn sync_audio(&mut self) {
        let playing = self.playing
            && self.error.is_none()
            && self.host.as_ref().is_some_and(|host| host.halted().is_none() && !host.completed());
        if playing != self.audio_playing {
            if let Some(audio) = &self.audio
                && let Err(error) = audio.set_playing(self.session_id, playing)
            {
                eprintln!("[audio] {error}");
            }
            self.audio_playing = playing;
        }
    }

    fn measured_rtf(&self) -> f64 {
        match (self.window.front(), self.window.back()) {
            (Some(a), Some(b)) if b.0 > a.0 => {
                let wall = b.0.duration_since(a.0).as_secs_f64();
                if wall > 0.2 { (b.1 - a.1) / wall } else { 0.0 }
            }
            _ => 0.0,
        }
    }

    /// Builds the host for the current scenario, posting progress as status.
    fn build(&mut self) {
        self.recordings.end_run("The run changed. Save the previous capture.");
        self.session_id = self.session_id.saturating_add(1);
        self.audio_playing = false;
        if let Some(audio) = &self.audio
            && let Err(error) = audio.reset(self.session_id, 0.0)
        {
            eprintln!("[audio] {error}");
        }
        self.free_ride.begin(self.session_id, None, euc_physics::Rider::default());
        *lock(&self.button_gate) = buttons::Gate { epoch: self.session_id, ..buttons::Gate::default() };
        if let Some(host) = self.host.as_mut() {
            host.release_button_pulses();
        }
        self.host = None;
        self.error = None;
        self.build = None;
        self.label = None;
        self.note = None;
        self.display = None;
        self.lights = None;
        self.terrain = None;
        self.world = None;
        self.suspension = None;
        self.outputs = None;
        self.outputs_at = None;
        self.events.clear();
        self.frames.clear();
        self.tones.clear();
        self.ble.clear();
        self.pending.clear();
        self.awaiting = None;
        self.snapshot = Snapshot::default();
        {
            let mut out = lock(&self.outbox);
            out.writer = None;
            out.messages.clear();
            out.messages.push(Message::Reset { scenario: self.scenario.clone() });
            out.messages.push(Message::Bodies { bodies: Vec::new() });
            out.messages.push(Message::Display { layout: None });
            out.messages.push(Message::Lights { layout: None });
            out.messages.push(Message::Terrain { profile: None });
            out.messages.push(Message::Suspension { info: None });
        }
        let outbox = Arc::clone(&self.outbox);
        let (scenario, backend) = (self.scenario.clone(), self.backend);
        let firmware = self.firmware.clone();
        let mut progress = |step: &str| {
            lock(&outbox).messages.push(Message::Status(Status {
                run_mode: if self.free_spec.is_some() { "free_ride" } else { "scenario" },
                run_epoch: self.session_id,
                state: "starting",
                message: step.to_owned(),
                label: None,
                build: None,
                backend,
                scenario: scenario.clone(),
                firmware: firmware.clone(),
                t_s: 0.0,
                rtf: 0.0,
                target_rtf: 1.0,
                playing: false,
                buttons_ready: false,
                power_button_ready: false,
            }));
        };
        let built = if let Some(spec) = &self.free_spec {
            super::firmware::build_free_host(
                spec,
                &self.firmware,
                self.session_id,
                self.free_ride.clone(),
                &mut progress,
            )
        } else {
            (self.factory)(&self.scenario, &self.firmware, &mut progress)
        };
        if let Some(ticket) = self.free_job.take() {
            self.free_ride.finish_start(ticket);
        }
        match built {
            Ok(built) => {
                let bodies = built.host.bodies();
                let mut header = built.header.clone().unwrap_or_else(|| {
                    RunInfo::basic(&built.wheel, &built.build, Some(&self.scenario), 60.0, bodies.clone()).header
                });
                header.sample_hz = 1.0 / (STEP_S * f64::from(SNAPSHOT_EVERY));
                if let Err(error) = self.free_ride.recording_context(self.session_id).and_then(|context| {
                    let context = context.or_else(|| header.context.clone());
                    self.recordings.bind_run(RunInfo { header, context })
                }) {
                    eprintln!("[recordings] {error}");
                }
                {
                    let mut out = lock(&self.outbox);
                    out.writer = Some(BatchWriter::new(bodies.len(), OUTBOX_SAMPLES));
                    out.messages.push(Message::Reset { scenario: self.scenario.clone() });
                    out.messages.push(Message::Bodies { bodies });
                    out.messages.push(Message::Display { layout: built.display.clone() });
                    out.messages.push(Message::Lights { layout: built.lights.clone() });
                    out.messages.push(Message::Terrain { profile: built.terrain.clone() });
                    out.messages.push(Message::World { frame: built.world.clone() });
                    out.messages.push(Message::Suspension { info: built.suspension.clone() });
                }
                self.build = Some(built.build);
                self.label = Some(built.label);
                self.note = built.note;
                self.display = built.display;
                self.lights = built.lights;
                self.terrain = built.terrain;
                self.world = built.world;
                self.suspension = built.suspension;
                self.outputs = None;
                let mut host = built.host;
                if self.manual.is_some() {
                    host.set_manual(self.manual);
                }
                self.host = Some(host);
                self.sync_audio();
                self.phone = PhoneDecoder::default();
                self.pending.clear();
                self.awaiting = None;
                self.window.clear();
                self.debt = 0.0;
                self.steps = 0;
                self.last = Instant::now();
                self.sample();
            }
            Err(e) => {
                self.free_ride.failed(self.session_id, &e);
                self.error = Some(e);
            }
        }
        self.post_status();
        self.refresh_button_gate();
    }

    fn buttons_ready(&self) -> bool {
        self.playing
            && self.error.is_none()
            && self
                .host
                .as_ref()
                .is_some_and(|host| host.halted().is_none() && !host.completed() && host.buttons_ready())
    }

    fn power_button_ready(&self) -> bool {
        self.playing
            && self.error.is_none()
            && self
                .host
                .as_ref()
                .is_some_and(|host| host.halted().is_none() && !host.completed() && host.power_button_ready())
    }

    fn refresh_button_gate(&self) {
        let mut gate = lock(&self.button_gate);
        gate.set_readiness(self.buttons_ready(), self.power_button_ready());
        gate.active = self.host.as_ref().map_or([false; 2], |host| host.button_pulses());
    }

    /// Runs independently of telemetry/outbox delivery, including while the CPU is paused.
    fn sync_button_states(&mut self) {
        self.refresh_button_gate();
        let mut gate = lock(&self.button_gate);
        if gate.epoch != self.session_id {
            return;
        }
        gate.expire(Instant::now());
        for button in [buttons::Button::Power, buttons::Button::Handle] {
            let index = button.index();
            let pressed = gate.holds[index].pressed;
            if pressed == gate.holds[index].applied {
                continue;
            }
            let result = match self.host.as_mut() {
                Some(host) => host.set_button(button.id(), pressed),
                None if !pressed => Ok(()),
                None => Err("The run has no physical button inputs".into()),
            };
            if let Err(text) = result {
                gate.reject_hold(button);
                let t_s = self.host.as_ref().map_or(0.0, |host| host.time_s());
                self.post(Message::Events { events: vec![Event { t_s, kind: EventKind::Fault, text }] });
            } else {
                gate.holds[index].applied = pressed;
            }
        }
    }

    fn button(&mut self, request: buttons::Request) {
        // A request queued before a restart must never touch the replacement host.
        if request.epoch != self.session_id {
            return;
        }
        let result = match self.host.as_mut() {
            Some(host) if self.playing && self.error.is_none() && host.halted().is_none() && !host.completed() => {
                host.pulse_button(request.button.id(), request.duration_ms)
            }
            _ => Err("The run stopped or paused before the physical button pulse".into()),
        };
        if let Err(text) = result {
            let t_s = self.host.as_ref().map_or(0.0, |host| host.time_s());
            self.post(Message::Events { events: vec![Event { t_s, kind: EventKind::Fault, text }] });
        }
        self.refresh_button_gate();
        lock(&self.button_gate).pending[request.button.index()] = false;
    }

    fn run(mut self, rx: &Receiver<Cmd>, tune_rx: &Receiver<tune::Request>) {
        self.build();
        loop {
            // A free ride's input watchdog only makes the controls neutral (free_ride.rs): play mode
            // never pauses for want of input.
            self.sync_button_states();
            if let Ok(request) = tune_rx.try_recv() {
                let state = if self.error.is_some() {
                    "error"
                } else if self.host.as_ref().is_some_and(|h| h.halted().is_some()) {
                    "halted"
                } else if self.host.as_ref().is_some_and(|h| h.completed()) {
                    "completed"
                } else if self.playing {
                    "running"
                } else {
                    "paused"
                };
                let run = tune::Run {
                    session_id: self.session_id,
                    backend: self.backend,
                    scenario: self.scenario.clone(),
                    firmware: self.host.as_ref().and(self.label.clone()),
                    build: self.host.as_ref().and(self.build.clone()),
                    state,
                    source: "Session firmware map",
                };
                tune::respond_with_handlers(request, self.host.as_deref(), run, self.error.as_deref(), |host| {
                    host.handler_counters()
                });
            }
            let running = self.playing && self.host.as_ref().is_some_and(|h| h.halted().is_none() && !h.completed());
            let wait = if running { Duration::ZERO } else { Duration::from_millis(50) };
            loop {
                match rx.recv_timeout(wait) {
                    Ok(Cmd::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                    Ok(cmd) => {
                        self.handle(cmd);
                        self.sync_button_states();
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                }
                if !running {
                    break;
                }
            }
            self.sync_button_states();
            let running =
                self.playing && self.host.as_ref().is_some_and(|host| host.halted().is_none() && !host.completed());
            if running {
                self.advance();
            } else {
                self.last = Instant::now();
            }
            if self.last_status.elapsed() >= Duration::from_millis(250) {
                self.post_status();
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Announce => {
                lock(&self.button_gate).cancel_holds();
                if self.free_spec.is_some() {
                    // Play mode: the ride goes on with neutral controls until the view sends input.
                    self.free_ride.clear_input();
                }
                if let Some(host) = self.host.as_mut() {
                    host.manual_pause();
                    host.release_button_pulses();
                }
                if let Some(host) = &self.host {
                    let bodies = host.bodies();
                    let mut snapshot = Snapshot::default();
                    host.snapshot(&mut snapshot);
                    let mut writer = BatchWriter::new(bodies.len(), OUTBOX_SAMPLES);
                    writer.push(&snapshot);
                    let outputs = host.outputs();
                    let mut out = lock(&self.outbox);
                    out.writer = Some(writer);
                    out.messages.clear();
                    out.messages.push(Message::Reset { scenario: self.scenario.clone() });
                    out.messages.push(Message::Bodies { bodies });
                    out.messages.push(Message::Display { layout: self.display.clone() });
                    out.messages.push(Message::Lights { layout: self.lights.clone() });
                    out.messages.push(Message::Terrain { profile: self.terrain.clone() });
                    out.messages.push(Message::World { frame: self.world.clone() });
                    out.messages.push(Message::Suspension { info: self.suspension.clone() });
                    if let Some(outputs) = outputs {
                        out.messages.push(Message::Outputs { outputs });
                    }
                }
                self.post_status();
                self.refresh_button_gate();
            }
            Cmd::Play(on) => {
                if !on {
                    let mut gate = lock(&self.button_gate);
                    gate.accepting = false;
                    gate.cancel_holds();
                    if let Some(host) = self.host.as_mut() {
                        host.release_button_pulses();
                    }
                }
                if on && self.host.as_ref().is_some_and(|host| host.wants_pause()) {
                    self.post_status();
                    return;
                }
                if on && self.host.as_ref().is_some_and(|h| h.completed()) {
                    self.post_status();
                    return;
                }
                if on != self.playing {
                    if !on && let Some(host) = self.host.as_mut() {
                        host.manual_pause();
                        host.release_button_pulses();
                    }
                    if !on && self.free_spec.is_some() {
                        self.free_ride.clear_input();
                    }
                    self.playing = on;
                    self.window.clear();
                    self.post_status();
                    self.refresh_button_gate();
                }
            }
            Cmd::Step => {
                if !self.playing {
                    for _ in 0..SNAPSHOT_EVERY {
                        self.step_once();
                    }
                    self.post_status();
                }
            }
            Cmd::Rtf(rtf) => {
                self.rtf = if self.free_spec.is_some() { rtf.clamp(0.25, 1.0) } else { rtf };
                self.window.clear();
                self.post_status();
            }
            Cmd::Firmware(firmware) => {
                self.firmware = firmware;
                if self.free_spec.is_some() {
                    self.playing = true;
                }
                self.build();
            }
            Cmd::FreeRide(spec, ticket) => {
                self.free_spec = Some(*spec);
                self.free_job = Some(ticket);
                self.playing = true;
                self.rtf = self.rtf.clamp(0.25, 1.0);
                self.build();
            }
            Cmd::ManualArm(epoch, source) => {
                if epoch == self.session_id && self.free_spec.is_some() {
                    match self.free_ride.arm(epoch, source) {
                        Ok(()) => self.handle(Cmd::Play(true)),
                        Err(reason) => self.free_ride.reason(epoch, reason),
                    }
                }
            }
            Cmd::ManualPause(epoch) => {
                if epoch == self.session_id && self.free_spec.is_some() {
                    let _ = self.free_ride.pause_epoch(epoch);
                    self.handle(Cmd::Play(false));
                }
            }
            Cmd::Manual(manual) => {
                self.manual = manual;
                if let Some(h) = self.host.as_mut() {
                    h.set_manual(manual);
                }
            }
            Cmd::StepOff => {
                if let Some(h) = self.host.as_mut() {
                    h.step_off();
                }
            }
            Cmd::Button(request) => self.button(request),
            Cmd::Restart(scenario) => {
                self.playing = true;
                if let Some(s) = scenario {
                    self.free_spec = None;
                    self.scenario = s;
                }
                self.build();
            }
            Cmd::Send { bytes, label, answer } => {
                let now = self.host.as_ref().map_or(0.0, |h| h.time_s());
                let mut at = now.max(self.pending.back().map_or(now, |p| p.at + COMMAND_GAP_S));
                let mut label = Some(label);
                let last = bytes.len().saturating_sub(1);
                for (i, (gap, byte)) in bytes.into_iter().enumerate() {
                    at += gap;
                    self.pending.push_back(Pending { at, byte, label: label.take(), answer: answer && i == last });
                }
            }
            Cmd::Stop => {}
        }
    }

    fn advance(&mut self) {
        let now = Instant::now();
        self.debt += now.duration_since(self.last).as_secs_f64().min(0.1) * self.rtf;
        self.last = now;
        let mut stepped = false;
        loop {
            // A host still booting on its stand runs as fast as it can (a free ride's respawn).
            let booting = self.host.as_ref().is_some_and(|host| host.booting());
            if !booting && self.debt < STEP_S {
                break;
            }
            self.step_once();
            if !self.playing {
                break;
            }
            self.debt = if booting { 0.0 } else { self.debt - STEP_S };
            stepped = true;
            if now.elapsed() >= SLICE {
                break;
            }
        }
        if self.debt > MAX_DEBT_S {
            self.debt = 0.0;
        }
        if let Some(h) = &self.host {
            self.window.push_back((Instant::now(), h.time_s()));
            while self.window.len() > 2 && self.window.front().is_some_and(|f| f.0.elapsed() > Duration::from_secs(1)) {
                self.window.pop_front();
            }
        }
        if !stepped {
            // Ahead of the wall clock: sleep until the next step is due.
            let due = (STEP_S - self.debt) / self.rtf;
            thread::sleep(Duration::from_secs_f64(due.clamp(0.0005, 0.004)));
        }
    }

    fn step_once(&mut self) {
        self.sync_button_states();
        let Some(host) = self.host.as_mut() else { return };
        let t = host.time_s();
        if self.awaiting.is_some_and(|until| t >= until) {
            Self::release(&mut self.pending, &mut self.awaiting, t);
        }
        while self.awaiting.is_none()
            && let Some(p) = self.pending.pop_front_if(|p| p.at <= t)
        {
            if let Some(text) = p.label {
                self.events.push(Event { t_s: t, kind: EventKind::Command, text });
            }
            if let Err(e) = host.inject_ble(&[p.byte]) {
                self.events.push(Event { t_s: t, kind: EventKind::Fault, text: e });
                self.pending.clear();
                break;
            }
            if p.answer {
                self.awaiting = Some(t + ANSWER_TIMEOUT_S);
            }
        }
        if host.completed() {
            return;
        }
        host.step(STEP_S);
        let completed = host.completed();
        let halted = host.halted().is_some();
        let manual_pause = host.wants_pause();
        if manual_pause {
            host.manual_pause();
            host.release_button_pulses();
        }
        if completed || halted {
            host.release_button_pulses();
        }
        self.steps += 1;
        if self.steps.is_multiple_of(SNAPSHOT_EVERY) || completed || halted || manual_pause {
            self.sample();
        }
        if manual_pause {
            self.playing = false;
            self.debt = 0.0;
            self.pending.clear();
            self.awaiting = None;
            self.post_status();
        }
        if completed || halted {
            self.playing = false;
            self.recordings.end_run(if halted {
                "The simulator halted. Save the captured segment."
            } else {
                "The run completed. Save the captured run."
            });
            self.pending.clear();
            self.awaiting = None;
            self.debt = 0.0;
            self.post_status();
        }
        self.refresh_button_gate();
    }

    /// Ends a hold on the queue at `t`; the commands behind it keep their spacing from now on.
    fn release(pending: &mut VecDeque<Pending>, awaiting: &mut Option<f64>, t: f64) {
        *awaiting = None;
        let late = pending.front().map_or(0.0, |p| (t - p.at).max(0.0));
        for p in pending {
            p.at += late;
        }
    }

    /// Takes a snapshot, events and BLE output and puts them in the outbox.
    fn sample(&mut self) {
        let measured = self.measured_rtf();
        let audio_rate = if measured > 0.0 { measured } else { self.rtf };
        let Some(host) = self.host.as_mut() else { return };
        host.snapshot(&mut self.snapshot);
        host.drain_events(&mut self.events);
        host.drain_ble_tx(&mut self.ble);
        host.drain_tones(&mut self.tones);
        self.recordings.ingest(&self.snapshot, &self.events, &self.ble, host.outputs().as_ref());
        if self.audio_playing
            && let Some(audio) = &self.audio
            && audio.status().state != "error"
            && let Err(error) = audio.submit(self.session_id, self.snapshot.t_s, audio_rate, &self.tones)
        {
            eprintln!("[audio] {error}");
        }
        if !self.ble.is_empty() {
            let from = self.frames.len();
            self.phone.push(&self.ble, &mut self.frames);
            self.ble.clear();
            if self.awaiting.is_some() && self.frames[from..].iter().any(|f| f.reply) {
                Self::release(&mut self.pending, &mut self.awaiting, host.time_s());
            }
            for f in &self.frames[from..] {
                if f.reply {
                    self.events.push(Event {
                        t_s: f.t,
                        kind: EventKind::Command,
                        text: format!("Reply: {}", f.summary),
                    });
                }
            }
        }
        let mut out = lock(&self.outbox);
        if let Some(w) = out.writer.as_mut() {
            w.push(&self.snapshot);
        }
        if !self.events.is_empty() {
            out.messages.push(Message::Events { events: std::mem::take(&mut self.events) });
        }
        if !self.frames.is_empty() {
            out.messages.push(Message::Frames { frames: std::mem::take(&mut self.frames) });
        }
        if !self.tones.is_empty() {
            out.messages.push(Message::Buzzer { tones: std::mem::take(&mut self.tones) });
        }
        // Board outputs go out when they change, at most 30 times a second.
        if (host.completed()
            || host.halted().is_some()
            || self.outputs_at.is_none_or(|at| at.elapsed() >= Duration::from_millis(33)))
            && let Some(now) = host.outputs()
            && self.outputs.as_ref() != Some(&now)
        {
            self.outputs_at = Some(Instant::now());
            out.messages.push(Message::Outputs { outputs: now.clone() });
            self.outputs = Some(now);
        }
        // A web view that stopped draining must not grow the message list without bound.
        if out.messages.len() > 2000 {
            let excess = out.messages.len() - 2000;
            out.messages.drain(..excess);
        }
    }
}

impl Drop for Sim {
    fn drop(&mut self) {
        self.recordings.end_run("The simulator stopped. Save the captured segment.");
        if let Some(audio) = &self.audio {
            let _ = audio.set_playing(self.session_id, false);
        }
        {
            let mut gate = lock(&self.button_gate);
            gate.accepting = false;
            gate.cancel_holds();
        }
        if let Some(host) = self.host.as_mut() {
            host.release_button_pulses();
        }
    }
}

#[cfg(test)]
mod capture_integration_tests {
    use super::super::recordings::{read_recording, save_capture};
    use super::*;

    struct FixtureHost {
        ticks: u32,
        sent: bool,
    }

    impl Host for FixtureHost {
        fn step(&mut self, _seconds: f64) {
            self.ticks += 1;
        }
        fn time_s(&self) -> f64 {
            f64::from(self.ticks) / 120.0
        }
        fn completed(&self) -> bool {
            self.ticks >= 5
        }
        fn halted(&self) -> Option<String> {
            None
        }
        fn bodies(&self) -> Vec<BodyInfo> {
            Vec::new()
        }
        fn snapshot(&self, out: &mut Snapshot) {
            out.t_s = self.time_s();
        }
        fn drain_events(&mut self, out: &mut Vec<Event>) {
            if self.completed() {
                out.push(Event { t_s: self.time_s(), kind: EventKind::Scenario, text: "Fixture finished".into() });
            }
        }
        fn drain_ble_tx(&mut self, out: &mut Vec<(f64, u8)>) {
            if !self.sent {
                out.extend(b"NAME:FIXTURE\r\n".iter().map(|b| (self.time_s(), *b)));
                self.sent = true;
            }
        }
        fn inject_ble(&mut self, _bytes: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn outputs(&self) -> Option<Outputs> {
            Some(Outputs { buzzer_hz: self.ticks * 100, controller_powered: Some(true), ..Outputs::default() })
        }
    }

    #[test]
    fn capture_keeps_initial_and_terminal_samples_without_a_webview_and_survives_restart() {
        let dir = std::env::temp_dir().join(format!("euc-runner-capture-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let capture = RecordingState::default();
        capture.enable_autocapture(dir.clone()).unwrap();
        let mut sim = Sim::new(
            Box::new(|_, _, _| {
                Ok(Built {
                    host: Box::new(FixtureHost { ticks: 0, sent: false }),
                    wheel: "fixture-wheel".into(),
                    header: Some(Header::new("test", "fixture-wheel", "fixture-firmware")),
                    label: "Fixture".into(),
                    build: "fixture-firmware".into(),
                    note: None,
                    display: None,
                    lights: None,
                    terrain: None,
                    world: None,
                    suspension: None,
                })
            }),
            "fixture".into(),
            "fixture",
            Arc::new(Mutex::new(Outbox::default())),
            capture.clone(),
            Arc::new(Mutex::new(buttons::Gate::default())),
            FreeRideState::default(),
        );
        sim.build();
        for _ in 0..5 {
            sim.step_once();
        }
        // A replacement run must not overwrite the unsaved completed capture.
        sim.build();
        let path = dir.join("completed.eucrec");
        save_capture(&capture, &path).unwrap();
        let data = read_recording(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(data.times, vec![0.0, 2.0 / 120.0, 4.0 / 120.0, 5.0 / 120.0]);
        assert_eq!(data.events.last().unwrap().text, "Fixture finished");
        assert_eq!(data.header.firmware, "fixture-firmware");
        assert!(data.frames.iter().any(|f| f.summary.contains("FIXTURE")));
        assert_eq!(data.outputs.last().unwrap().outputs.buzzer_hz, 500);
        assert!(data.truncated.is_none());
        if let Some(export) = std::env::var_os("EUC_CAPTURE_TEST_EXPORT") {
            std::fs::copy(&path, export).unwrap();
        }
        drop(sim);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod native_startup_tests {
    use super::*;

    /// Explicit local check: opens the Mac audio device and plays the stock boot tones.
    #[test]
    #[ignore = "needs cached stock firmware and opens the local audio device"]
    fn stock_startup_uses_native_audio_without_a_webview() {
        let audio = NativeBuzzer::start(true).expect("open native audio before firmware boot");
        let outbox = Arc::new(Mutex::new(Outbox::default()));
        let (tx, rx) = mpsc::channel();
        let (_tune_tx, tune_rx) = mpsc::sync_channel(1);
        let sim_outbox = Arc::clone(&outbox);
        let sim_audio = audio.clone();
        let thread = thread::spawn(move || {
            Sim::new(
                Box::new(super::super::firmware::build_host),
                "balance-60s".into(),
                super::super::cpu::backend().name(),
                sim_outbox,
                RecordingState::default(),
                Arc::new(Mutex::new(buttons::Gate::default())),
                FreeRideState::default(),
            )
            .with_audio(Some(sim_audio))
            .run(&rx, &tune_rx);
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut reached = false;
        while Instant::now() < deadline {
            reached = lock(&outbox).messages.iter().any(|message| {
                matches!(message,
                Message::Status(status) if status.t_s >= 4.0)
            });
            if reached || audio.status().state == "error" {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = tx.send(Cmd::Stop);
        thread.join().unwrap();
        let status = audio.status();
        println!("native audio after stock boot: {status:?}");
        assert!(reached, "the stock host must advance through boot");
        assert_eq!(status.state, "ready", "native output must keep returning buffers: {status:?}");
        let out = lock(&outbox);
        let starts = out
            .messages
            .iter()
            .filter_map(|message| match message {
                Message::Buzzer { tones } => Some(tones.iter().filter(|tone| tone.hz > 0.0 && tone.duty > 0.0).count()),
                _ => None,
            })
            .sum::<usize>();
        assert!(starts > 0, "the stock firmware must produce a nonzero startup tone");
        let (samples, nonzero) = audio.rendered_samples();
        assert!(samples > 0 && nonzero > 0, "the native queue must receive nonzero firmware PCM: {samples}/{nonzero}");
        println!("{starts} firmware tone onsets reached the native runner before any webview attachment");
    }
}

#[cfg(test)]
mod button_state_tests {
    use super::*;

    #[derive(Default)]
    struct View {
        levels: [bool; 2],
        edges: Vec<(String, bool)>,
        now: f64,
        halted: bool,
        booting: bool,
    }

    struct ButtonHost(Arc<Mutex<View>>);

    impl Host for ButtonHost {
        fn step(&mut self, seconds: f64) {
            lock(&self.0).now += seconds;
        }
        fn time_s(&self) -> f64 {
            lock(&self.0).now
        }
        fn bodies(&self) -> Vec<BodyInfo> {
            Vec::new()
        }
        fn snapshot(&self, _out: &mut Snapshot) {}
        fn drain_events(&mut self, _out: &mut Vec<Event>) {}
        fn drain_ble_tx(&mut self, _out: &mut Vec<(f64, u8)>) {}
        fn inject_ble(&mut self, _bytes: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn set_button(&mut self, button: &str, pressed: bool) -> Result<(), String> {
            let index = buttons::Button::parse(button)?.index();
            let mut view = lock(&self.0);
            view.levels[index] = pressed;
            view.edges.push((button.into(), pressed));
            Ok(())
        }
        fn release_button_pulses(&mut self) {
            lock(&self.0).levels = [false; 2];
        }
        fn buttons_ready(&self) -> bool {
            !lock(&self.0).booting
        }
        fn halted(&self) -> Option<String> {
            lock(&self.0).halted.then(|| "Mock halt".into())
        }
    }

    fn sim() -> (Sim, Arc<Mutex<View>>) {
        let mut sim = Sim::new(
            Box::new(|_, _, _| Err("Button tests never load firmware".into())),
            "button-unit-test".into(),
            "mock",
            Arc::new(Mutex::new(Outbox::default())),
            RecordingState::default(),
            Arc::new(Mutex::new(buttons::Gate::default())),
            FreeRideState::default(),
        );
        let view = Arc::new(Mutex::new(View::default()));
        sim.host = Some(Box::new(ButtonHost(Arc::clone(&view))));
        sim.session_id = 7;
        lock(&sim.button_gate).epoch = 7;
        sim.sync_button_states();
        (sim, view)
    }

    fn press(sim: &Sim, button: buttons::Button, pressed: bool) {
        lock(&sim.button_gate).set_state(button, pressed, sim.session_id).unwrap();
    }

    #[test]
    fn status_exposes_the_same_button_readiness_as_the_input_gate() {
        let (mut sim, view) = sim();
        assert!(sim.status("running", String::new()).buttons_ready);
        lock(&view).booting = true;
        sim.sync_button_states();
        assert!(!sim.status("running", String::new()).buttons_ready);
        assert!(!lock(&sim.button_gate).accepting);
        lock(&view).booting = false;
        sim.playing = false;
        sim.sync_button_states();
        assert!(!sim.status("paused", String::new()).buttons_ready);
        sim.playing = true;
        lock(&view).halted = true;
        assert!(!sim.status("halted", String::new()).buttons_ready);
    }

    #[test]
    fn live_state_renews_without_cmds_or_extra_gpio_edges() {
        let (mut sim, view) = sim();
        let (tx, rx) = mpsc::channel();
        let (tune_tx, _) = mpsc::sync_channel(1);
        let runner = Runner {
            tx,
            button_gate: Arc::clone(&sim.button_gate),
            tune_tx,
            sinks: Arc::new(Mutex::new(None)),
            stop: Arc::new(AtomicBool::new(false)),
            threads: Vec::new(),
            audio: Err(AudioError("Mock runner has no audio device".into())),
            audio_enabled: AtomicBool::new(false),
        };
        for _ in 0..100 {
            runner.button_state(buttons::Button::Power, true, 7).unwrap();
            sim.sync_button_states();
        }
        assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        assert_eq!(lock(&view).edges, [("power".into(), true)]);
        assert_eq!(lock(&view).now, 0.0, "GPIO state never advances simulation time");
        runner.button_state(buttons::Button::Power, false, 7).unwrap();
        sim.sync_button_states();
        assert_eq!(lock(&view).levels, [false; 2]);
    }

    #[test]
    fn live_wall_expiry_releases_without_frames_or_cpu_steps() {
        let (mut sim, view) = sim();
        press(&sim, buttons::Button::Handle, true);
        sim.sync_button_states();
        assert!(lock(&view).levels[1]);
        // Move only the wall lease. No sleep, firmware, scenario, or simulated clock advance.
        lock(&sim.button_gate).expire(Instant::now() + buttons::HOLD_LEASE);
        let outbox = Arc::clone(&sim.outbox);
        let _blocked_frames = lock(&outbox);
        sim.sync_button_states();
        assert_eq!(lock(&view).levels, [false; 2]);
        assert_eq!(lock(&view).now, 0.0);
        assert!(lock(&sim.button_gate).set_state(buttons::Button::Handle, true, 7).is_err());
    }

    #[test]
    fn live_pause_releases_even_when_already_paused_and_prevents_replay() {
        let (mut sim, view) = sim();
        press(&sim, buttons::Button::Power, true);
        sim.sync_button_states();
        sim.handle(Cmd::Play(false));
        assert_eq!(lock(&view).levels, [false; 2]);
        sim.sync_button_states();
        sim.handle(Cmd::Play(false));
        sim.handle(Cmd::Play(true));
        sim.sync_button_states();
        assert_eq!(lock(&view).levels, [false; 2]);
        assert!(lock(&sim.button_gate).set_state(buttons::Button::Power, true, 7).is_err());
        press(&sim, buttons::Button::Power, false);
        press(&sim, buttons::Button::Power, true);
        sim.sync_button_states();
        assert!(lock(&view).levels[0]);
    }

    #[test]
    fn live_paused_loop_boundary_does_not_leave_a_gpio_held() {
        let (mut sim, view) = sim();
        press(&sim, buttons::Button::Power, true);
        sim.sync_button_states();
        sim.playing = false;
        sim.sync_button_states();
        assert_eq!(lock(&view).levels, [false; 2]);
        assert_eq!(lock(&view).now, 0.0);
    }

    #[test]
    fn live_source_reattach_and_drop_release_buttons() {
        let (mut sim, view) = sim();
        press(&sim, buttons::Button::Power, true);
        sim.sync_button_states();
        sim.handle(Cmd::Announce);
        sim.sync_button_states();
        assert_eq!(lock(&view).levels, [false; 2]);
        press(&sim, buttons::Button::Power, false);
        press(&sim, buttons::Button::Handle, true);
        sim.sync_button_states();
        assert!(lock(&view).levels[1]);
        drop(sim);
        assert_eq!(lock(&view).levels, [false; 2]);
    }

    #[test]
    fn live_halt_releases_and_does_not_allow_a_new_hold() {
        let (mut sim, view) = sim();
        press(&sim, buttons::Button::Handle, true);
        sim.sync_button_states();
        lock(&view).halted = true;
        sim.sync_button_states();
        assert_eq!(lock(&view).levels, [false; 2]);
        assert!(lock(&sim.button_gate).set_state(buttons::Button::Handle, true, 7).is_err());
    }

    #[test]
    fn live_reset_releases_old_host_and_rejects_stale_release() {
        let (mut sim, old) = sim();
        press(&sim, buttons::Button::Handle, true);
        sim.sync_button_states();
        sim.build(); // Factory returns a controlled error; no firmware or physics is loaded.
        assert_eq!(lock(&old).levels, [false; 2]);
        assert_eq!(sim.session_id, 8);
        let current = Arc::new(Mutex::new(View::default()));
        sim.host = Some(Box::new(ButtonHost(Arc::clone(&current))));
        sim.error = None;
        sim.sync_button_states();
        press(&sim, buttons::Button::Handle, true);
        sim.sync_button_states();
        assert!(lock(&sim.button_gate).set_state(buttons::Button::Handle, false, 7).is_err());
        sim.sync_button_states();
        assert!(lock(&current).levels[1]);
    }

    #[test]
    fn live_free_ride_host_keeps_a_hold_during_normal_refresh() {
        let (mut sim, view) = sim();
        sim.free_spec = Some(FreeRideSpec {
            wheel: "begode-extreme".into(),
            settings: "blank".into(),
            rider_model: "rigid_ankle_v1".into(),
            map: Some(euc_physics::GeneratedMapSpec::flat(0)),
            world: None,
            start: None,
        });
        press(&sim, buttons::Button::Power, true);
        sim.sync_button_states();
        press(&sim, buttons::Button::Power, true);
        sim.sync_button_states();
        assert!(sim.playing);
        assert!(lock(&view).levels[0]);
        assert_eq!(lock(&view).edges.len(), 1);
        sim.handle(Cmd::ManualPause(7));
        sim.sync_button_states();
        assert_eq!(lock(&view).levels, [false; 2]);
    }
}

#[cfg(test)]
mod play_mode_tests {
    use super::*;

    /// Boots on its stand until `boot_s` simulated seconds, then runs on.
    struct BootHost {
        now: f64,
        boot_s: f64,
    }

    impl Host for BootHost {
        fn step(&mut self, seconds: f64) {
            self.now += seconds;
        }
        fn time_s(&self) -> f64 {
            self.now
        }
        fn bodies(&self) -> Vec<BodyInfo> {
            Vec::new()
        }
        fn snapshot(&self, out: &mut Snapshot) {
            out.t_s = self.now;
        }
        fn drain_events(&mut self, _out: &mut Vec<Event>) {}
        fn drain_ble_tx(&mut self, _out: &mut Vec<(f64, u8)>) {}
        fn inject_ble(&mut self, _bytes: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn halted(&self) -> Option<String> {
            None
        }
        fn booting(&self) -> bool {
            self.now < self.boot_s
        }
    }

    #[test]
    fn a_booting_host_runs_unthrottled_then_at_the_wall_clock_rate() {
        let mut sim = Sim::new(
            Box::new(|_, _, _| Err("Play mode tests never load firmware".into())),
            "play-unit-test".into(),
            "mock",
            Arc::new(Mutex::new(Outbox::default())),
            RecordingState::default(),
            Arc::new(Mutex::new(buttons::Gate::default())),
            FreeRideState::default(),
        );
        sim.host = Some(Box::new(BootHost { now: 0.0, boot_s: 30.0 }));
        sim.playing = true;
        sim.rtf = 1.0;
        sim.last = Instant::now();
        let began = Instant::now();
        while sim.host.as_ref().is_some_and(|host| host.booting()) {
            sim.advance();
            assert!(began.elapsed() < Duration::from_secs(10), "the boot must not wait for the wall clock");
        }
        let booted = sim.host.as_ref().unwrap().time_s();
        let boot_wall = began.elapsed().as_secs_f64();
        assert!((30.0..30.0 + STEP_S + 1e-9).contains(&booted), "{booted}");
        assert!(boot_wall < 5.0, "30 simulated seconds of boot took {boot_wall} s of wall time");
        // Booted: the wall clock paces it again.
        let resumed = Instant::now();
        while resumed.elapsed() < Duration::from_millis(200) {
            sim.advance();
        }
        let ran = sim.host.as_ref().unwrap().time_s() - booted;
        let wall = resumed.elapsed().as_secs_f64();
        assert!(
            ran <= wall + SLICE.as_secs_f64() + 2.0 * STEP_S && ran > 0.5 * wall,
            "{ran} simulated s in {wall} s of wall time"
        );
    }
}
