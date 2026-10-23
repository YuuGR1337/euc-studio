//! The Compare workspace: two firmware images ride one scenario on the same inputs, and both runs
//! stream to the web view.
//!
//! Both sessions live on one thread and advance sample by sample, so they stay side by side. The
//! comparison is `euc_sim::compare`, the code behind `euc sim compare`, on the same 100 Hz samples
//! and with the tolerance file CI uses: the studio and the CLI give the same numbers. While the
//! runs go on, the summary is recomputed over the samples so far; the last one also judges the
//! BLE streams and the scenario's checks.
//!
//! A run is stopped by flag, never joined: a new comparison starts at once on new channels, and
//! the web view ignores whatever the old run still sends.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use euc_profile::{ProfileSource, Sha256};
use euc_sim::compare::{self, Comparison, Tolerances, Trace};
use euc_sim::config::StockMap;
use euc_sim::scenario::{Outcome, Ride, Scenario};
use euc_sim::{Machine, MachineConfig, PlantState, Session, WheelFiles};
use euc_telemetry::BodyInfo;
use euc_telemetry::batch::BatchWriter;
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};

use super::{cpu, firmware, scenarios};

/// Samples per simulated second, as `euc sim compare` takes them by default.
const SAMPLE_HZ: f64 = 100.0;
/// Every this many samples goes to the web view (50 Hz).
const STREAM_EVERY: u32 = 2;
/// While the runs go on, the summary is recomputed every this many samples (0.5 s).
const SUMMARY_EVERY: u32 = 50;
/// Samples a side's stream holds for a web view that does not drain it (60 s at 50 Hz).
const STREAM_SAMPLES: usize = 3000;
/// How often the pump sends.
const PUMP_PERIOD: Duration = Duration::from_millis(16);
/// How often the status goes out while the runs go on.
const STATUS_PERIOD: Duration = Duration::from_millis(250);
/// The tolerance file CI judges compat against stock with (docs/compare.md).
const TOLERANCE_FILE: &str = "scenarios/compare/compat-vs-stock.toml";
const TOLERANCE: &str = include_str!("../../../../scenarios/compare/compat-vs-stock.toml");

/// A firmware for one side.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FirmwareSpec {
    /// The pinned official build (from the firmware cache, fetched if missing).
    Stock,
    /// Our compat build in `firmware/build/target-compat/`.
    OursCompat,
    /// Our clean build in `firmware/build/target-clean/`.
    OursClean,
    /// Our firmware as the host SIL library (`firmware/build/host-sil/`), with these quirks on.
    OursCustom { quirks: Vec<String> },
    /// A `.bin` or `.elf` the user picked (an ELF names our variables through `probes.toml`).
    File { name: String, bytes: Vec<u8> },
}

impl FirmwareSpec {
    fn id(&self) -> &str {
        match self {
            Self::Stock => "stock",
            Self::OursCompat => "ours-compat",
            Self::OursClean => "ours-clean",
            Self::OursCustom { .. } => "ours-custom",
            Self::File { .. } => "file",
        }
    }
}

/// What to compare.
#[derive(Debug, Clone, Deserialize)]
pub struct CompareSpec {
    pub a: FirmwareSpec,
    pub b: FirmwareSpec,
    pub scenario: String,
    /// As fast as the machine can (true) or in real time.
    pub fast: bool,
}

/// A firmware the workspace offers, and why it cannot run if it cannot.
#[derive(Debug, Clone, Serialize)]
pub struct FirmwareChoice {
    id: &'static str,
    label: &'static str,
    detail: String,
    disabled_reason: Option<String>,
}

/// What the Compare workspace needs to set up.
#[derive(Debug, Clone, Serialize)]
pub struct CompareInfo {
    firmwares: Vec<FirmwareChoice>,
    scenarios: Vec<scenarios::ScenarioInfo>,
    tolerance_file: &'static str,
}

/// One side's identity, as the summary shows it.
#[derive(Debug, Clone, Serialize)]
pub struct RunInfo {
    /// `stock`, `ours-compat`, `ours-clean` or `file`.
    spec: String,
    label: String,
    sha256: Sha256,
    /// Checks of the scenario this side cannot judge: they read a variable its map lacks.
    checks_skipped: Vec<String>,
}

/// The comparison so far (`done`: of the whole run, with the BLE streams and the checks).
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    done: bool,
    #[serde(flatten)]
    comparison: Comparison,
    /// Checks both runs judge with different verdicts (only once done).
    check_disagreements: Vec<String>,
    /// Within tolerance, and the checks agree where the tolerance asks for it (only once done).
    ok: Option<bool>,
    /// Each run's checks (only once done).
    outcome_a: Option<Outcome>,
    outcome_b: Option<Outcome>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// `starting`, `running`, `done`, `stopped`, `error`.
    state: &'static str,
    message: String,
    t_s: f64,
    duration_s: f64,
    /// Measured sim seconds per wall second.
    rtf: f64,
}

/// What the pump sends on the JSON channel.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CompareMessage {
    Status(Status),
    /// Both runs are built.
    Reset(Box<Reset>),
    Summary(Box<Summary>),
}

/// Who rides, the bodies each stream carries, the ground, and what judges the runs.
#[derive(Debug, Clone, Serialize)]
pub struct Reset {
    scenario: String,
    duration_s: f64,
    /// Samples per second on the streams.
    stream_hz: f64,
    a: RunInfo,
    b: RunInfo,
    bodies_a: Vec<BodyInfo>,
    bodies_b: Vec<BodyInfo>,
    terrain: Option<Vec<[f32; 3]>>,
    /// The streaming world both runs ride (a `[world]` scenario), for the view; both see the same.
    world: Option<super::world::WorldFrame>,
    tolerance_file: &'static str,
    /// Why the tolerance file treats this scenario specially, if it does.
    tolerance_note: Option<String>,
}

#[derive(Default)]
struct Outbox {
    a: Option<BatchWriter>,
    b: Option<BatchWriter>,
    messages: Vec<CompareMessage>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A comparison in progress. Dropping it stops it (without waiting for the threads).
pub struct CompareRun {
    stop: Arc<AtomicBool>,
}

impl Drop for CompareRun {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
pub struct CompareState(Mutex<Option<CompareRun>>);

/// The web view's channels for one comparison.
pub struct CompareSinks {
    pub stream_a: Channel<InvokeResponseBody>,
    pub stream_b: Channel<InvokeResponseBody>,
    pub messages: Channel<CompareMessage>,
}

/// Firmwares and scenarios the workspace offers.
#[tauri::command]
pub fn compare_info() -> CompareInfo {
    // Every scenario rides the Extreme today; our builds are per wheel.
    let ours = |profile: &str| our_build(profile, "begode-extreme").err();
    CompareInfo {
        firmwares: vec![
            FirmwareChoice {
                id: "stock",
                label: "Stock",
                detail: "Begode Extreme, official image".into(),
                disabled_reason: None,
            },
            FirmwareChoice {
                id: "ours-compat",
                label: "Ours-compat",
                detail: "Our C rewrite, stock behaviour".into(),
                disabled_reason: ours("compat"),
            },
            FirmwareChoice {
                id: "ours-clean",
                label: "Ours-clean",
                detail: "Our C rewrite, quirks fixed".into(),
                disabled_reason: ours("clean"),
            },
            FirmwareChoice {
                id: "ours-custom",
                label: "Ours-custom",
                detail: "Ours (custom quirks): our C rewrite on the host, quirks switched below".into(),
                disabled_reason: super::images::sil_library().err(),
            },
        ],
        scenarios: scenarios::infos(),
        tolerance_file: TOLERANCE_FILE,
    }
}

/// Starts a comparison (stopping the one before).
#[tauri::command]
pub fn compare_start(
    state: State<'_, CompareState>,
    spec: CompareSpec,
    stream_a: Channel<InvokeResponseBody>,
    stream_b: Channel<InvokeResponseBody>,
    messages: Channel<CompareMessage>,
) -> Result<(), String> {
    if !scenarios::infos().iter().any(|i| i.id == spec.scenario) {
        return Err(format!("unknown scenario \"{}\"", spec.scenario));
    }
    let mut guard = lock(&state.0);
    *guard = None;
    *guard = Some(start(spec, CompareSinks { stream_a, stream_b, messages }));
    Ok(())
}

/// Stops the comparison; what it compared so far stays on screen.
#[tauri::command]
pub fn compare_stop(state: State<'_, CompareState>) {
    *lock(&state.0) = None;
}

fn start(spec: CompareSpec, sinks: CompareSinks) -> CompareRun {
    let stop = Arc::new(AtomicBool::new(false));
    let outbox = Arc::new(Mutex::new(Outbox::default()));
    {
        let (outbox, stop) = (Arc::clone(&outbox), Arc::clone(&stop));
        thread::Builder::new()
            .name("euc-compare".into())
            .spawn(move || run(&spec, &outbox, &stop))
            .expect("spawn the compare thread");
    }
    {
        let stop = Arc::clone(&stop);
        thread::Builder::new()
            .name("euc-compare-pump".into())
            .spawn(move || pump(&outbox, &sinks, &stop))
            .expect("spawn the compare pump");
    }
    CompareRun { stop }
}

/// Sends what the run produced, about 60 times a second, until the run is stopped and drained.
fn pump(outbox: &Mutex<Outbox>, sinks: &CompareSinks, stop: &AtomicBool) {
    loop {
        let stopping = stop.load(Ordering::Relaxed);
        thread::sleep(PUMP_PERIOD);
        let (a, b, messages) = {
            let mut out = lock(outbox);
            let a = out.a.as_mut().and_then(BatchWriter::take);
            let b = out.b.as_mut().and_then(BatchWriter::take);
            (a, b, std::mem::take(&mut out.messages))
        };
        // A reset goes before the samples that depend on it.
        let (early, late): (Vec<_>, Vec<_>) = messages.into_iter().partition(|m| matches!(m, CompareMessage::Reset(_)));
        for m in early {
            let _ = sinks.messages.send(m);
        }
        if let Some(bytes) = a {
            let _ = sinks.stream_a.send(InvokeResponseBody::Raw(bytes));
        }
        if let Some(bytes) = b {
            let _ = sinks.stream_b.send(InvokeResponseBody::Raw(bytes));
        }
        for m in late {
            let _ = sinks.messages.send(m);
        }
        if stopping {
            return;
        }
    }
}

/// One firmware image and the names it gives its variables.
struct Firmware {
    image: Vec<u8>,
    label: String,
    sha256: Sha256,
    stock: bool,
    map: Option<StockMap>,
    /// Our firmware as the SIL library: the library and its quirk mask.
    sil: Option<super::images::SilRun>,
}

/// One side while it runs.
struct Side {
    info: RunInfo,
    /// The scenario, without the checks this side's map cannot serve.
    scenario: Scenario,
    session: Session,
    ride: Ride,
    trace: Trace,
}

impl Side {
    fn new(
        files: &WheelFiles,
        scenario: &Scenario,
        spec: &FirmwareSpec,
        progress: &mut dyn FnMut(&str),
    ) -> Result<Self, String> {
        let fw = load_firmware(files, spec, progress)?;
        progress(&format!("Booting {}", fw.label));
        let mut session = ride_session(files, scenario, &fw)?;
        // As `euc sim compare`: checks that read a variable this firmware does not name are skipped.
        let mut scenario = scenario.clone();
        let names = session.firmware_var_names();
        let mut checks_skipped = Vec::new();
        scenario.expects.retain(|check| match check.firmware_var() {
            Some(var) if !names.iter().any(|n| n == var) => {
                checks_skipped.push(format!("{check:?}"));
                false
            }
            _ => true,
        });
        let ride = Ride::new(&scenario, &mut session).map_err(|e| format!("{}: {e}", fw.label))?;
        Ok(Self {
            info: RunInfo { spec: spec.id().to_owned(), label: fw.label, sha256: fw.sha256, checks_skipped },
            scenario,
            trace: Trace::of_session(&session),
            session,
            ride,
        })
    }

    /// One sample: the ride's step, then the trace.
    fn sample(&mut self) -> euc_telemetry::Snapshot {
        let snapshot = self.ride.sample(&self.scenario, &mut self.session);
        let names = self.session.firmware_var_names();
        let vars: Vec<(String, f64)> =
            names.into_iter().filter_map(|name| Some((name.clone(), self.session.firmware_value(&name)?))).collect();
        self.trace.push(snapshot.t_s, snapshot.signals, vars);
        self.trace.events.extend(self.session.take_events());
        snapshot
    }

    fn halted(&self) -> Option<String> {
        self.session.machine().halted().map(|why| format!("{} halted: {why}", self.info.label))
    }

    /// The trace with the BLE bytes sent so far.
    fn trace(&mut self) -> &Trace {
        self.trace.ble.clone_from(&self.session.machine().board().ble_tx);
        &self.trace
    }
}

/// The comparison thread: builds both sides, rides them sample by sample, posts what it sees.
fn run(spec: &CompareSpec, outbox: &Mutex<Outbox>, stop: &AtomicBool) {
    let post = |m: CompareMessage| lock(outbox).messages.push(m);
    let status = |state: &'static str, message: String, t_s: f64, duration_s: f64, rtf: f64| {
        post(CompareMessage::Status(Status { state, message, t_s, duration_s, rtf }));
    };
    let built = (|| -> Result<_, String> {
        let scenario = scenarios::load(&spec.scenario)?;
        let mut progress = |step: &str| status("starting", step.to_owned(), 0.0, scenario.seconds, 0.0);
        progress("Loading the wheel profile");
        let files = WheelFiles::load(&ProfileSource::Builtin, &scenario.wheel).map_err(|e| e.to_string())?;
        let a = Side::new(&files, &scenario, &spec.a, &mut progress)?;
        let b = Side::new(&files, &scenario, &spec.b, &mut progress)?;
        Ok((scenario, a, b))
    })();
    let (scenario, mut a, mut b) = match built {
        Ok(built) => built,
        Err(e) => return status("error", e, 0.0, 0.0, 0.0),
    };
    let file = Tolerances::from_toml(TOLERANCE).unwrap_or_default();
    let tolerances = file.for_scenario(&scenario.name);
    {
        let mut out = lock(outbox);
        out.a = Some(BatchWriter::new(a.session.bodies().len(), STREAM_SAMPLES));
        out.b = Some(BatchWriter::new(b.session.bodies().len(), STREAM_SAMPLES));
        out.messages.push(CompareMessage::Reset(Box::new(Reset {
            scenario: scenario.name.clone(),
            duration_s: scenario.seconds,
            stream_hz: SAMPLE_HZ / f64::from(STREAM_EVERY),
            a: a.info.clone(),
            b: b.info.clone(),
            bodies_a: a.session.bodies().to_vec(),
            bodies_b: b.session.bodies().to_vec(),
            terrain: firmware::terrain_profile(&scenario),
            world: a
                .session
                .machine()
                .plant()
                .live_part()
                .and_then(|live| live.physics().setup().world.as_ref().map(super::world::WorldFrame::of)),
            tolerance_file: TOLERANCE_FILE,
            tolerance_note: file.scenario.get(&scenario.name).and_then(|adj| adj.note.clone()),
        })));
    }

    let started = Instant::now();
    let mut last_status = Instant::now();
    let mut samples = 0_u32;
    let mut halted = None;
    while !stop.load(Ordering::Relaxed) && !a.ride.finished(&a.scenario, &a.session) {
        // In real time, wait for the wall clock to catch up with the runs.
        let ahead = a.session.time_s() - started.elapsed().as_secs_f64();
        if !spec.fast && ahead > 0.0 {
            thread::sleep(Duration::from_secs_f64(ahead.min(0.01)));
            continue;
        }
        let (snap_a, snap_b) = (a.sample(), b.sample());
        samples += 1;
        if samples.is_multiple_of(STREAM_EVERY) {
            let mut out = lock(outbox);
            if let Some(w) = out.a.as_mut() {
                w.push(&snap_a);
            }
            if let Some(w) = out.b.as_mut() {
                w.push(&snap_b);
            }
        }
        if samples.is_multiple_of(SUMMARY_EVERY)
            && let Ok(comparison) = compare::compare(a.trace(), b.trace(), &tolerances)
        {
            post(CompareMessage::Summary(Box::new(partial(comparison))));
        }
        if last_status.elapsed() >= STATUS_PERIOD {
            last_status = Instant::now();
            let t = a.session.time_s();
            status("running", String::new(), t, scenario.seconds, t / started.elapsed().as_secs_f64().max(1e-3));
        }
        halted = a.halted().or_else(|| b.halted());
        if halted.is_some() {
            break;
        }
    }

    let stopped = stop.load(Ordering::Relaxed);
    let t = a.session.time_s();
    let rtf = t / started.elapsed().as_secs_f64().max(1e-3);
    let finished = a.ride.finished(&a.scenario, &a.session);
    match compare::compare(a.trace(), b.trace(), &tolerances) {
        Ok(comparison) if finished => {
            let (outcome_a, outcome_b) = (a.ride.outcome(&a.scenario), b.ride.outcome(&b.scenario));
            let check_disagreements = compare::check_disagreements(&outcome_a, &outcome_b);
            let ok = comparison.within_tolerance && (!tolerances.same_checks || check_disagreements.is_empty());
            post(CompareMessage::Summary(Box::new(Summary {
                done: true,
                comparison,
                check_disagreements,
                ok: Some(ok),
                outcome_a: Some(outcome_a),
                outcome_b: Some(outcome_b),
            })));
        }
        Ok(comparison) => post(CompareMessage::Summary(Box::new(partial(comparison)))),
        Err(e) => return status("error", e, t, scenario.seconds, rtf),
    }
    match (halted, stopped) {
        (Some(why), _) => status("error", why, t, scenario.seconds, rtf),
        (None, true) => status("stopped", "Stopped".into(), t, scenario.seconds, rtf),
        (None, false) => status("done", String::new(), t, scenario.seconds, rtf),
    }
    // The pump sends the last messages, then ends with this flag.
    stop.store(true, Ordering::Relaxed);
}

fn partial(comparison: Comparison) -> Summary {
    Summary { done: false, comparison, check_disagreements: Vec::new(), ok: None, outcome_a: None, outcome_b: None }
}

/// Our build of `profile` (`compat` or `clean`) for `wheel`, in this checkout's `firmware/build/`.
fn our_build(profile: &str, wheel: &str) -> Result<PathBuf, String> {
    let relative = format!("firmware/build/target-{profile}/euc-fw-{wheel}.bin");
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(&relative);
    if path.is_file() {
        Ok(path.canonicalize().unwrap_or(path))
    } else {
        Err(format!("Not built here: run `cmake --workflow --preset target-{profile}` in firmware/ ({relative})"))
    }
}

/// The image of `spec` and its variable names: the stock map for the pinned stock build, the
/// probe map resolved against the ELF for ours.
fn load_firmware(files: &WheelFiles, spec: &FirmwareSpec, progress: &mut dyn FnMut(&str)) -> Result<Firmware, String> {
    let wheel = &files.profile;
    let read = |path: &Path| std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()));
    let (bytes, label, elf) = match spec {
        FirmwareSpec::Stock => {
            let (bytes, label) = firmware::stock_image(wheel, progress)?;
            (bytes, label, None)
        }
        FirmwareSpec::OursCompat | FirmwareSpec::OursClean => {
            let profile = if matches!(spec, FirmwareSpec::OursCompat) { "compat" } else { "clean" };
            progress(&format!("Loading our {profile} build"));
            let path = our_build(profile, &wheel.id)?;
            let elf = path.with_extension("elf");
            let elf = if elf.is_file() { Some(read(&elf)?) } else { None };
            (read(&path)?, format!("ours-{profile}"), elf)
        }
        FirmwareSpec::OursCustom { quirks } => {
            let choice = format!("{}:{}", super::images::OURS_CUSTOM, quirks.join(","));
            super::images::validate_choice(&choice)?;
            let loaded = super::images::load(files, &choice, progress)?;
            let sha256 = Sha256::digest(&loaded.image);
            return Ok(Firmware {
                image: loaded.image,
                label: loaded.label,
                sha256,
                stock: false,
                map: loaded.map,
                sil: loaded.sil,
            });
        }
        FirmwareSpec::File { name, bytes } => (bytes.clone(), name.clone(), None),
    };
    let (image, elf) = if bytes.starts_with(b"\x7fELF") {
        let flat = euc_image::Image::from_bytes(&bytes).map_err(|e| format!("{label}: {e}"))?;
        if flat.load_addr() != Some(wheel.board.app.base) {
            return Err(format!(
                "{label} loads at 0x{:08X}, but {} runs apps from 0x{:08X}",
                flat.load_addr().unwrap_or(0),
                wheel.id,
                wheel.board.app.base
            ));
        }
        (flat.bytes().to_vec(), Some(bytes))
    } else {
        (bytes, elf)
    };
    let sha256 = Sha256::digest(&image);
    // Stock-map addresses only mean something in the build they were taken from.
    let stock = wheel.find_pin(&sha256).is_some_and(|pin| pin.label == files.stockmap.build);
    let map = if stock {
        Some(files.stockmap.clone())
    } else {
        match (&elf, &files.probes) {
            (Some(elf), Some(probes)) => Some(probes.resolve(elf, &label).map_err(|why| format!("{label}: {why}"))?),
            _ => None,
        }
    };
    let label = if stock && !matches!(spec, FirmwareSpec::Stock) { format!("{label} (stock build)") } else { label };
    Ok(Firmware { image, label, sha256, stock, map, sil: None })
}

/// A session at power-on that rides `scenario` with `fw`, sampled as `euc sim compare` samples.
fn ride_session(files: &WheelFiles, scenario: &Scenario, fw: &Firmware) -> Result<Session, String> {
    let board = match scenario.settings.as_deref() {
        None | Some("blank") => files.board.clone(),
        Some(preset) => files.board_with_settings(preset).map_err(|e| e.to_string())?,
    };
    let config = MachineConfig {
        board: &board,
        stockmap: fw.stock.then_some(&files.stockmap),
        image: &fw.image,
        load_addr: files.profile.board.app.base,
        hclk: files.hclk(),
        plant: PlantState::default(),
        injections: scenario.inject.iter().map(|i| (i.at_s, i.bytes.bytes().collect())).collect(),
        power_cycles: Vec::new(),
        cpi: euc_sim::DEFAULT_CPI,
        exception_timing: if fw.sil.is_some() {
            euc_sim::sil::EXCEPTION_TIMING
        } else {
            euc_sim::ExceptionTiming::default()
        },
        max_quantum: 4000,
        marker_window_s: 4.0,
    };
    let live = files.live_for(scenario).map_err(|e| e.to_string())?;
    let factory = match &fw.sil {
        Some(sil) => sil.library.factory(sil.mask),
        None => cpu::factory()?,
    };
    let machine = Machine::new_live(&config, &euc_sim::registry(), factory, live).map_err(|e| e.to_string())?;
    Ok(Session::new(machine, fw.map.as_ref(), SAMPLE_HZ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tolerance_file_parses() {
        let file = Tolerances::from_toml(TOLERANCE).expect("compat-vs-stock.toml");
        assert!(file.same_checks);
        assert!(file.for_scenario("ride-20kmh-stop").channels.by_name.contains_key("speed_kmh"));
    }

    #[test]
    fn firmware_specs_read_like_the_web_view_sends_them() {
        let spec: CompareSpec = serde_json::from_str(
            r#"{"a":{"kind":"stock"},"b":{"kind":"file","name":"x.bin","bytes":[1,2]},"scenario":"balance-60s","fast":true}"#,
        )
        .unwrap();
        assert_eq!((spec.a.id(), spec.b.id()), ("stock", "file"));
        let compat: FirmwareSpec = serde_json::from_str(r#"{"kind":"ours-compat"}"#).unwrap();
        assert_eq!(compat.id(), "ours-compat");
        let custom: FirmwareSpec =
            serde_json::from_str(r#"{"kind":"ours-custom","quirks":["ASIN_ACCEL","KEIL_FTZ"]}"#).unwrap();
        assert_eq!(custom.id(), "ours-custom");
        assert!(matches!(custom, FirmwareSpec::OursCustom { quirks } if quirks == ["ASIN_ACCEL", "KEIL_FTZ"]));
    }

    /// The web view reads these shapes (src/engine/compareLive.ts, compareTypes.ts).
    #[test]
    fn messages_have_the_shape_the_web_view_reads() {
        let mut trace = Trace::new(1000);
        trace.push(0.0, euc_telemetry::Signals::default(), []);
        let comparison = compare::compare(&trace, &trace.clone(), &Tolerances::default()).unwrap();
        let json = serde_json::to_value(CompareMessage::Summary(Box::new(partial(comparison)))).unwrap();
        assert_eq!(json["type"], "summary");
        assert_eq!(json["done"], false);
        assert!(json["first_divergence"].is_null() && json["channels"].is_array() && json["ble"].is_object());
        assert!(json["ok"].is_null() && json["outcome_a"].is_null());
        let status = Status { state: "running", message: String::new(), t_s: 1.5, duration_s: 40.0, rtf: 1.0 };
        let json = serde_json::to_value(CompareMessage::Status(status)).unwrap();
        assert_eq!((json["type"].as_str(), json["t_s"].as_f64()), (Some("status"), Some(1.5)));
    }

    #[test]
    fn a_file_that_is_not_an_image_is_refused_with_its_name() {
        let files = WheelFiles::load(&ProfileSource::Builtin, "begode-extreme").unwrap();
        let elf = FirmwareSpec::File { name: "broken.elf".into(), bytes: b"\x7fELF garbage".to_vec() };
        let err = load_firmware(&files, &elf, &mut |_| {}).err().expect("refused");
        assert!(err.starts_with("broken.elf"), "{err}");
        // A raw .bin is taken as it is; without an ELF it names no variables.
        let bin = FirmwareSpec::File { name: "x.bin".into(), bytes: vec![0; 64] };
        let fw = load_firmware(&files, &bin, &mut |_| {}).unwrap();
        assert!(!fw.stock && fw.map.is_none());
    }

    /// Needs the stock image in the local firmware cache and our compat build:
    /// `cargo test -- --ignored --nocapture compare`.
    #[test]
    #[ignore = "needs the stock image in the local firmware cache and firmware/build/target-compat"]
    fn stock_against_compat_streams_both_and_judges_the_ride() {
        let spec = CompareSpec {
            a: FirmwareSpec::Stock,
            b: FirmwareSpec::OursCompat,
            scenario: "ride-20kmh-stop".into(),
            fast: true,
        };
        let outbox = Mutex::new(Outbox::default());
        let stop = AtomicBool::new(false);
        let started = Instant::now();
        run(&spec, &outbox, &stop);
        let out = lock(&outbox);
        println!("compared in {:.1} s", started.elapsed().as_secs_f64());
        assert!(out.a.as_ref().is_some_and(|w| !w.is_empty()) && out.b.as_ref().is_some_and(|w| !w.is_empty()));
        let last = out.messages.iter().rev().find_map(|m| match m {
            CompareMessage::Summary(s) => Some(s),
            _ => None,
        });
        let last = last.expect("a summary");
        assert!(last.done, "the last summary covers the whole run");
        println!(
            "first divergence {:?}; ok {:?}; within {}",
            last.comparison.first_divergence, last.ok, last.comparison.within_tolerance
        );
        assert!(last.ok == Some(true), "compat stays within the CI tolerance on ride-20kmh-stop");
        assert!(matches!(out.messages.last(), Some(CompareMessage::Status(s)) if s.state == "done"));
    }
}
