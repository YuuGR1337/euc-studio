//! In-process Python bindings. The CPU and physics live entirely inside detached calls
//! (`run_scenario_json`) or inside an unsendable [`Session`] that stays on the thread that made it.

use std::fs::File;
use std::io::{BufWriter, Read as _};
use std::path::{Path, PathBuf};
use std::time::Instant;

use euc_image::{Format, Image};
use euc_profile::{ProfileDir, ProfileError, ProfileSource, Sha256};
use euc_sim::config::{ROLE_CUTOFF, StockMap};
use euc_sim::cpu::Backend;
use euc_sim::recording::{CONTEXT_SCHEMA, FrameTap, ModelAids, RecordingContext, RiderModel, RunMode, verdict_events};
use euc_sim::scenario::{Expect, Outcome, Ride, Scenario as EngineScenario, Stat};
use euc_sim::{Machine, MachineConfig, Manual, Session as EngineSession, WheelFiles};
use euc_telemetry::recording::{BleFrame, Header, OutputsAt, ReadLimits, Reader, Writer};
use euc_telemetry::{BodyInfo, Event, Snapshot};
use pyo3::exceptions::{PyRuntimeError, PyTimeoutError, PyValueError};
use pyo3::prelude::*;
use serde::Serialize;

const MAX_SAMPLES: usize = 100_000;
const MAX_SECONDS: f64 = 600.0;
const MAX_WALL_SECONDS: f64 = 3600.0;
const MAX_TEXT_BYTES: u64 = 4 << 20;
const MAX_FIRMWARE_BYTES: u64 = 64 << 20;
const MAX_RECORDING_BYTES: u64 = 256 << 20;
const MAX_MEMORY_BYTES: u32 = 16 << 20;
/// What this package writes into a recording's header.
const PRODUCER: &str = concat!("euc-studio python ", env!("CARGO_PKG_VERSION"));

/// A parsed, immutable scenario. Loading does not construct a machine or run firmware.
#[pyclass(frozen, module = "euc_studio")]
struct Scenario {
    inner: EngineScenario,
    text: String,
}

#[pymethods]
impl Scenario {
    #[getter]
    fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    fn description(&self) -> &str {
        &self.inner.description
    }

    #[getter]
    fn wheel(&self) -> &str {
        &self.inner.wheel
    }

    #[getter]
    fn seconds(&self) -> f64 {
        self.inner.seconds
    }

    #[getter]
    fn release_s(&self) -> f64 {
        self.inner.release_s
    }

    fn __repr__(&self) -> String {
        format!("Scenario({:?}, wheel={:?}, seconds={})", self.inner.name, self.inner.wheel, self.inner.seconds)
    }
}

fn read_bounded(path: &Path, limit: u64) -> PyResult<Vec<u8>> {
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(PyValueError::new_err("The input path must name a regular file."));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(PyValueError::new_err(format!("The input file exceeds {limit} bytes.")));
    }
    Ok(bytes)
}

fn finite_toml(value: &toml::Value) -> bool {
    match value {
        toml::Value::Float(number) => number.is_finite(),
        toml::Value::Array(items) => items.iter().all(finite_toml),
        toml::Value::Table(items) => items.values().all(finite_toml),
        _ => true,
    }
}

fn parse_scenario(text: &str) -> PyResult<EngineScenario> {
    let inner = EngineScenario::from_toml(text).map_err(|e| PyValueError::new_err(e.to_string()))?;
    if !inner.seconds.is_finite() || inner.seconds > MAX_SECONDS {
        return Err(PyValueError::new_err("Scenario duration must not exceed 600 simulated seconds."));
    }
    // Validate every floating point input, including terrain and fault parameters, without
    // duplicating the scenario schema. Parsing alone accepts TOML's non-finite floats.
    let value: toml::Value = toml::from_str(text).map_err(|e| PyValueError::new_err(e.to_string()))?;
    if !finite_toml(&value) {
        return Err(PyValueError::new_err("Scenario numbers must be finite."));
    }
    Ok(inner)
}

#[pyfunction]
fn load_scenario(py: Python<'_>, path: PathBuf) -> PyResult<Scenario> {
    py.detach(move || {
        let bytes = read_bounded(&path, MAX_TEXT_BYTES)?;
        let text = std::str::from_utf8(&bytes).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Scenario { inner: parse_scenario(text)?, text: text.to_owned() })
    })
}

#[derive(Clone, Copy)]
struct Limits {
    sample_hz: f64,
    max_samples: usize,
    max_seconds: f64,
    max_wall_seconds: f64,
}

impl Limits {
    fn validate(self, scenario: &EngineScenario) -> PyResult<()> {
        if !self.sample_hz.is_finite() || !(1.0..=1000.0).contains(&self.sample_hz) {
            return Err(PyValueError::new_err("sample_hz must be between 1 and 1000."));
        }
        if !(1..=MAX_SAMPLES).contains(&self.max_samples) {
            return Err(PyValueError::new_err("max_samples must be between 1 and 100000."));
        }
        if !self.max_seconds.is_finite() || !(0.0..=MAX_SECONDS).contains(&self.max_seconds) || self.max_seconds == 0.0
        {
            return Err(PyValueError::new_err("max_seconds must be positive and at most 600."));
        }
        if scenario.seconds > self.max_seconds {
            return Err(PyValueError::new_err("The scenario exceeds max_seconds."));
        }
        if !self.max_wall_seconds.is_finite()
            || !(0.0..=MAX_WALL_SECONDS).contains(&self.max_wall_seconds)
            || self.max_wall_seconds == 0.0
        {
            return Err(PyValueError::new_err("max_wall_seconds must be positive and at most 3600."));
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct RunData {
    wheel: String,
    scenario: String,
    cpu: &'static str,
    firmware_sha256: Sha256,
    firmware_format: Format,
    firmware_probes: bool,
    requested_sample_hz: f64,
    sample_hz: f64,
    sim_seconds: f64,
    wall_seconds: f64,
    completed: bool,
    passed: bool,
    halted: Option<String>,
    outcome: Outcome,
    samples: Vec<Snapshot>,
    events: Vec<Event>,
    bodies: Vec<BodyInfo>,
}

fn load_error(error: euc_sim::LoadError) -> PyErr {
    match error {
        euc_sim::LoadError::Profile(ProfileError::Io { error, .. }) => error.into(),
        other => PyValueError::new_err(other.to_string()),
    }
}

fn profile_source(scenario: &EngineScenario, profiles_dir: Option<&Path>) -> PyResult<ProfileSource> {
    let Some(root) = profiles_dir else { return Ok(ProfileSource::Builtin) };
    if !std::fs::metadata(root)?.is_dir() {
        return Err(PyValueError::new_err("profiles_dir must name a folder."));
    }
    // WheelFiles owns the format parser. Bound the known text inputs before it reads them.
    // ProfileDir also checks the id before it uses the id in a path.
    if scenario.wheel.is_empty()
        || !scenario.wheel.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(PyValueError::new_err("The wheel id must use lowercase letters, digits, or hyphens."));
    }
    for name in ["wheel", "board", "stockmap", "plant", "geometry", "settings", "probes"] {
        let path = root.join(&scenario.wheel).join(format!("{name}.toml"));
        match std::fs::metadata(&path) {
            Ok(_) => {
                read_bounded(&path, MAX_TEXT_BYTES)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(ProfileSource::Override(ProfileDir::new(root.to_path_buf())))
}

fn load_files(scenario: &EngineScenario, source: &ProfileSource) -> PyResult<WheelFiles> {
    let files = WheelFiles::load(source, &scenario.wheel).map_err(load_error)?;
    let memory = &files.board.memory;
    for span in [memory.flash, memory.sram, memory.periph, memory.ppb] {
        if span.size == 0 || span.size > MAX_MEMORY_BYTES || span.base.checked_add(span.size).is_none() {
            return Err(PyValueError::new_err("Profile memory ranges must fit within 16 MiB each."));
        }
    }
    if memory.code_end <= memory.flash.base
        || memory.code_end > memory.flash.end()
        || files.hclk() == 0
        || files.hclk() > 1_000_000_000
    {
        return Err(PyValueError::new_err("The profile has an invalid code range or clock rate."));
    }
    Ok(files)
}

fn load_image(
    files: &WheelFiles,
    firmware_path: &Path,
    symbols_path: Option<&Path>,
) -> PyResult<(Image, Option<StockMap>)> {
    let bytes = read_bounded(firmware_path, MAX_FIRMWARE_BYTES)?;
    let image = Image::from_bytes(&bytes).map_err(|e| PyValueError::new_err(e.to_string()))?;
    if image.bytes().len() < 8 || image.bytes().len() > files.profile.board.app.max_size as usize {
        return Err(PyValueError::new_err("The firmware must contain vectors and fit the profile application slot."));
    }
    if image.load_addr().is_some_and(|addr| addr != files.profile.board.app.base) {
        return Err(PyValueError::new_err("The ELF load address differs from the profile application address."));
    }
    let digest = Sha256::digest(image.bytes());
    if files.profile.find_pin(&digest).is_some()
        || ProfileSource::Builtin.load(&files.profile.id).is_ok_and(|p| p.find_pin(&digest).is_some())
    {
        return Err(PyValueError::new_err("This API requires own or custom firmware. The image matches a stock pin."));
    }
    let symbols = match (image.format(), symbols_path) {
        (Format::Elf, Some(_)) => return Err(PyValueError::new_err("An ELF firmware already contains its symbols.")),
        (Format::Elf, None) => Some(bytes),
        (Format::Raw, Some(path)) => {
            let symbols = read_bounded(path, MAX_FIRMWARE_BYTES)?;
            let flat = Image::from_bytes(&symbols).map_err(|e| PyValueError::new_err(e.to_string()))?;
            if flat.format() != Format::Elf
                || flat.load_addr() != Some(files.profile.board.app.base)
                || flat.bytes() != image.bytes()
            {
                return Err(PyValueError::new_err("symbols_path must contain an ELF that matches the raw firmware."));
            }
            Some(symbols)
        }
        (Format::Raw, None) => None,
    };
    let map = match (&symbols, &files.probes) {
        (Some(elf), Some(probes)) => Some(probes.resolve(elf, "custom").map_err(PyValueError::new_err)?),
        _ => None,
    };
    Ok((image, map))
}

fn needs_cutoff_probes(expect: &Expect) -> bool {
    matches!(expect, Expect::NoCutoff { .. } | Expect::Cutoff { .. } | Expect::Record { stat: Stat::AtCutoff, .. })
}

// This free function supports attach's higher-ranked lifetime. A method pointer does not.
fn check_interrupts(py: Python<'_>) -> PyResult<()> {
    py.check_signals()
}

fn injections(scenario: &EngineScenario) -> PyResult<Vec<(f64, Vec<u8>)>> {
    scenario
        .inject
        .iter()
        .map(|inject| {
            if !(0.0..=scenario.seconds).contains(&inject.at_s) {
                return Err(PyValueError::new_err("Injection times must fall within the scenario."));
            }
            let bytes = if let Some(hex) = inject.bytes.strip_prefix("hex:") {
                (0..hex.len())
                    .step_by(2)
                    .map(|i| hex.get(i..i + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok()))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| PyValueError::new_err("Injection hex must contain complete byte pairs."))?
            } else {
                inject
                    .bytes
                    .chars()
                    .map(|c| u8::try_from(u32::from(c)))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| PyValueError::new_err("Injection text must contain only byte values."))?
            };
            Ok((inject.at_s, bytes))
        })
        .collect()
}

/// Everything a run needs, built from the inputs; `run_scenario` rides it to the end, a
/// [`Session`] steps it.
struct Built {
    scenario: EngineScenario,
    source: ProfileSource,
    files: WheelFiles,
    firmware: String,
    firmware_sha256: Sha256,
    firmware_format: Format,
    firmware_probes: bool,
    limits: Limits,
    session: EngineSession,
    ride: Ride,
}

fn build(
    scenario: EngineScenario,
    firmware_path: &Path,
    profiles_dir: Option<&Path>,
    symbols_path: Option<&Path>,
    limits: Limits,
) -> PyResult<Built> {
    limits.validate(&scenario)?;
    let source = profile_source(&scenario, profiles_dir)?;
    let files = load_files(&scenario, &source)?;
    let period_cycles = (files.hclk() as f64 / limits.sample_hz).round().max(1.0);
    let period_s = period_cycles / files.hclk() as f64;
    // Ride uses the same half-period completion rule. The loop also enforces the exact limit.
    let estimated = (scenario.seconds / period_s - 0.5).ceil();
    if estimated < 1.0 {
        return Err(PyValueError::new_err("sample_hz is too low for this scenario duration."));
    }
    if estimated > limits.max_samples as f64 {
        return Err(PyValueError::new_err("The scenario and sample rate exceed max_samples."));
    }
    let (image, map) = load_image(&files, firmware_path, symbols_path)?;
    let has_cutoff = map.as_ref().is_some_and(|m| m.ram.iter().any(|v| v.role.as_deref() == Some(ROLE_CUTOFF)));
    if !has_cutoff && scenario.expects.iter().any(needs_cutoff_probes) {
        return Err(PyValueError::new_err("Cutoff checks require matching firmware symbols and cutoff probes."));
    }
    let board = match scenario.settings.as_deref() {
        None | Some("blank") => files.board.clone(),
        Some(name) => files.board_with_settings(name).map_err(load_error)?,
    };
    let config = MachineConfig {
        board: &board,
        stockmap: None,
        image: image.bytes(),
        load_addr: files.profile.board.app.base,
        hclk: files.hclk(),
        plant: euc_sim::PlantState::default(),
        injections: injections(&scenario)?,
        power_cycles: Vec::new(),
        cpi: euc_sim::DEFAULT_CPI,
        exception_timing: euc_sim::ExceptionTiming::default(),
        max_quantum: 4000,
        marker_window_s: 4.0,
    };
    let live = files
        .live_on(scenario.rider, scenario.assist, scenario.terrain.clone())
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    let factory = Backend::Own.factory().map_err(PyRuntimeError::new_err)?;
    let machine = Machine::new_live(&config, &euc_sim::registry(), factory, live)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    let mut session = EngineSession::new(machine, map.as_ref(), limits.sample_hz);
    let ride = Ride::new(&scenario, &mut session).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let firmware =
        firmware_path.file_name().map_or_else(|| "firmware".to_owned(), |n| n.to_string_lossy().into_owned());
    Ok(Built {
        scenario,
        source,
        files,
        firmware,
        firmware_sha256: Sha256::digest(image.bytes()),
        firmware_format: image.format(),
        firmware_probes: map.is_some(),
        limits,
        session,
        ride,
    })
}

impl Built {
    /// The run so far as the result `run_scenario` returns.
    fn result(&self, started: Instant, samples: Vec<Snapshot>, events: Vec<Event>) -> RunData {
        let outcome = self.ride.outcome(&self.scenario);
        let halted = self.session.machine().halted().map(str::to_owned);
        let completed = halted.is_none() && self.ride.finished(&self.scenario, &self.session);
        RunData {
            wheel: self.files.profile.id.clone(),
            scenario: self.scenario.name.clone(),
            cpu: "own",
            firmware_sha256: self.firmware_sha256,
            firmware_format: self.firmware_format,
            firmware_probes: self.firmware_probes,
            requested_sample_hz: self.limits.sample_hz,
            sample_hz: 1.0 / self.session.sample_period_s(),
            sim_seconds: self.session.time_s(),
            wall_seconds: started.elapsed().as_secs_f64(),
            completed,
            passed: completed && halted.is_none() && outcome.passed(),
            halted,
            outcome,
            samples,
            events,
            bodies: self.session.bodies().to_vec(),
        }
    }

    /// The header a recording of this run starts with.
    fn header(&self) -> Header {
        let mut header = euc_sim::recording::header(
            PRODUCER,
            &self.source,
            &self.files,
            &self.session,
            &self.firmware,
            &self.firmware_sha256,
        );
        header.cpu = Some("own".into());
        header.cpi = Some(euc_sim::DEFAULT_CPI);
        header.scenario = Some(self.scenario.name.clone());
        header.settings = self.scenario.settings.clone().filter(|p| p != "blank");
        let rider = self.scenario.rider;
        let assist = self.scenario.assist;
        header.context = Some(
            RecordingContext {
                schema: CONTEXT_SCHEMA,
                run_mode: RunMode::Scenario,
                rider_model: match rider.model {
                    euc_physics::RiderModel::RigidAnkleV1 => RiderModel::RigidAnkleV1,
                    euc_physics::RiderModel::ArticulatedSagittalV1 => RiderModel::ArticulatedSagittalV1,
                },
                height_map: None,
                model_aids: ModelAids {
                    roll_stiffness: assist.roll_stiffness,
                    roll_damping: assist.roll_damping,
                    steer_kp: rider.control.steer_kp,
                    steer_kd: rider.control.steer_kd,
                    steer_max_nm: rider.control.steer_max_nm,
                },
            }
            .to_json(),
        );
        header
    }
}

fn run(
    scenario: EngineScenario,
    firmware_path: &Path,
    profiles_dir: Option<&Path>,
    symbols_path: Option<&Path>,
    limits: Limits,
) -> PyResult<RunData> {
    let started = Instant::now();
    let mut built = build(scenario, firmware_path, profiles_dir, symbols_path, limits)?;
    let mut samples = Vec::new();
    let mut events = Vec::new();
    while !built.ride.finished(&built.scenario, &built.session) && built.session.machine().halted().is_none() {
        Python::attach(check_interrupts)?;
        if started.elapsed().as_secs_f64() >= limits.max_wall_seconds {
            return Err(PyTimeoutError::new_err("The simulation exceeded max_wall_seconds."));
        }
        if samples.len() >= limits.max_samples {
            return Err(PyRuntimeError::new_err("The simulation reached max_samples before completion."));
        }
        let previous = built.session.time_s();
        samples.push(built.ride.sample(&built.scenario, &mut built.session));
        events.extend(built.session.take_events());
        if built.session.time_s() <= previous && built.session.machine().halted().is_none() {
            return Err(PyRuntimeError::new_err("The simulator clock did not advance."));
        }
    }
    Python::attach(check_interrupts)?;
    if started.elapsed().as_secs_f64() >= limits.max_wall_seconds {
        return Err(PyTimeoutError::new_err("The simulation exceeded max_wall_seconds."));
    }
    events.extend(built.session.take_events());
    Ok(built.result(started, samples, events))
}

#[pyfunction]
#[pyo3(signature = (scenario, firmware_path, *, profiles_dir=None, symbols_path=None, sample_hz=100.0, max_samples=100_000, max_seconds=600.0, max_wall_seconds=300.0))]
#[allow(clippy::too_many_arguments)] // Mirrors the explicit keyword-only Python API.
fn run_scenario_json(
    py: Python<'_>,
    scenario: &Scenario,
    firmware_path: PathBuf,
    profiles_dir: Option<PathBuf>,
    symbols_path: Option<PathBuf>,
    sample_hz: f64,
    max_samples: usize,
    max_seconds: f64,
    max_wall_seconds: f64,
) -> PyResult<String> {
    let scenario = scenario.inner.clone();
    let limits = Limits { sample_hz, max_samples, max_seconds, max_wall_seconds };
    // No Machine, CPU, Python reference, or physics object enters or leaves this closure.
    py.detach(move || {
        let result = run(scenario, &firmware_path, profiles_dir.as_deref(), symbols_path.as_deref(), limits)?;
        serde_json::to_string(&result).map_err(|e| PyRuntimeError::new_err(e.to_string()))
    })
}

/// A scenario run stepped from Python one sample at a time: the rider can be taken over,
/// pushed and stepped off between samples, the run reset, and everything so far saved as an
/// `.eucrec` recording. The machine lives on the thread that made the session (`unsendable`).
#[pyclass(unsendable, module = "euc_studio")]
struct Session {
    inputs: Inputs,
    built: Built,
    started: Instant,
    started_unix_ms: u64,
    samples: Vec<Snapshot>,
    events: Vec<Event>,
    frames: Vec<BleFrame>,
    outputs: Vec<OutputsAt>,
    tap: FrameTap,
}

/// What a session was built from, kept for `reset`.
struct Inputs {
    scenario: EngineScenario,
    scenario_toml: String,
    firmware_path: PathBuf,
    profiles_dir: Option<PathBuf>,
    symbols_path: Option<PathBuf>,
    limits: Limits,
}

impl Inputs {
    fn build(&self) -> PyResult<Built> {
        build(
            self.scenario.clone(),
            &self.firmware_path,
            self.profiles_dir.as_deref(),
            self.symbols_path.as_deref(),
            self.limits,
        )
    }
}

fn json<T: Serialize>(value: &T) -> PyResult<String> {
    serde_json::to_string(value).map_err(|e| PyRuntimeError::new_err(e.to_string()))
}

#[pymethods]
impl Session {
    #[new]
    #[pyo3(signature = (scenario, firmware_path, *, profiles_dir=None, symbols_path=None, sample_hz=100.0, max_samples=100_000, max_seconds=600.0, max_wall_seconds=300.0))]
    #[allow(clippy::too_many_arguments)] // Mirrors the explicit keyword-only Python API.
    fn new(
        _py: Python<'_>,
        scenario: &Scenario,
        firmware_path: PathBuf,
        profiles_dir: Option<PathBuf>,
        symbols_path: Option<PathBuf>,
        sample_hz: f64,
        max_samples: usize,
        max_seconds: f64,
        max_wall_seconds: f64,
    ) -> PyResult<Self> {
        let inputs = Inputs {
            scenario: scenario.inner.clone(),
            scenario_toml: scenario.text.clone(),
            firmware_path,
            profiles_dir,
            symbols_path,
            limits: Limits { sample_hz, max_samples, max_seconds, max_wall_seconds },
        };
        // Stable PyO3 requires Send across detach. The engine owns non-Send CPU and
        // peripheral objects, so the interactive session keeps them on its Python thread.
        let built = inputs.build()?;
        Ok(Self {
            inputs,
            built,
            started: Instant::now(),
            started_unix_ms: euc_telemetry::recording::now_unix_ms(),
            samples: Vec::new(),
            events: Vec::new(),
            frames: Vec::new(),
            outputs: Vec::new(),
            tap: FrameTap::default(),
        })
    }

    /// Simulated seconds so far.
    #[getter]
    fn time_s(&self) -> f64 {
        self.built.session.time_s()
    }

    /// Samples per simulated second, after cycle rounding.
    #[getter]
    fn sample_hz(&self) -> f64 {
        1.0 / self.built.session.sample_period_s()
    }

    /// The scenario ran to its end (or the machine halted).
    #[getter]
    fn finished(&self) -> bool {
        self.built.ride.finished(&self.built.scenario, &self.built.session)
    }

    /// Why the firmware halted, if it did.
    #[getter]
    fn halted(&self) -> Option<String> {
        self.built.session.machine().halted().map(str::to_owned)
    }

    /// Samples taken so far.
    #[getter]
    fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Runs `n` sample periods (the scenario drives the stand, its targets, pushes and faults as
    /// in `run_scenario`) and returns the last snapshot as JSON. Stops early when the run
    /// finishes or the firmware halts.
    #[pyo3(signature = (n=1))]
    fn step_json(&mut self, py: Python<'_>, n: usize) -> PyResult<String> {
        if n == 0 {
            return Err(PyValueError::new_err("Step at least one sample."));
        }
        if self.finished() {
            return Err(PyRuntimeError::new_err("The run has finished. Call reset() to start again."));
        }
        let mut last = None;
        for _ in 0..n {
            if self.finished() {
                break;
            }
            py.check_signals()?;
            if self.started.elapsed().as_secs_f64() >= self.inputs.limits.max_wall_seconds {
                return Err(PyTimeoutError::new_err("The session exceeded max_wall_seconds."));
            }
            if self.samples.len() >= self.inputs.limits.max_samples {
                return Err(PyRuntimeError::new_err("The session reached max_samples."));
            }
            let previous = self.built.session.time_s();
            let built = &mut self.built;
            let snapshot = built.ride.sample(&built.scenario, &mut built.session);
            self.events.extend(self.built.session.take_events());
            self.tap.drain(self.built.session.machine(), &mut self.frames);
            let outputs = euc_sim::recording::outputs(self.built.session.machine());
            if self.outputs.last().is_none_or(|at| at.outputs != outputs) {
                self.outputs.push(OutputsAt { t_s: snapshot.t_s, outputs });
            }
            if self.built.session.time_s() <= previous && self.built.session.machine().halted().is_none() {
                return Err(PyRuntimeError::new_err("The simulator clock did not advance."));
            }
            self.samples.push(snapshot.clone());
            last = Some(snapshot);
        }
        json(&last.expect("at least one step"))
    }

    /// Starts the run again from power-on with the same inputs; samples and events are cleared.
    fn reset(&mut self) -> PyResult<()> {
        let inputs = &self.inputs;
        self.built = inputs.build()?;
        self.started = Instant::now();
        self.started_unix_ms = euc_telemetry::recording::now_unix_ms();
        self.samples.clear();
        self.events.clear();
        self.frames.clear();
        self.outputs.clear();
        self.tap = FrameTap::default();
        Ok(())
    }

    /// Takes manual control of the rider: lean and steer as shares (-1 to 1, forward and left
    /// positive) of what the rider can do. The scenario's targets are ignored until
    /// `release_manual`.
    fn set_manual(&mut self, lean: f64, steer: f64) -> PyResult<()> {
        if !lean.is_finite() || !steer.is_finite() {
            return Err(PyValueError::new_err("lean and steer must be finite."));
        }
        self.built.session.set_manual(Some(Manual { lean, steer }));
        Ok(())
    }

    /// Gives the rider back to the scenario.
    fn release_manual(&mut self) {
        self.built.session.set_manual(None);
    }

    /// Pushes the rider along the heading, newtons, until changed.
    fn set_push(&mut self, newtons: f64) -> PyResult<()> {
        if !newtons.is_finite() || newtons.abs() > 10_000.0 {
            return Err(PyValueError::new_err("newtons must be finite and within ±10000."));
        }
        self.built.session.set_push(newtons);
        Ok(())
    }

    /// Lets the wheel off its stand (`False`) or puts it back (`True`).
    fn set_held(&mut self, held: bool) {
        self.built.session.set_held(held);
    }

    /// The rider steps off the pedals.
    fn step_off(&mut self) {
        self.built.session.step_off();
    }

    /// Every sample so far, as JSON.
    fn samples_json(&self) -> PyResult<String> {
        json(&self.samples)
    }

    /// Every event so far, as JSON.
    fn events_json(&self) -> PyResult<String> {
        json(&self.events)
    }

    /// The run so far as `run_scenario` would report it (`completed` is false until the end).
    fn result_json(&self) -> PyResult<String> {
        json(&self.built.result(self.started, self.samples.clone(), self.events.clone()))
    }

    /// Writes the run so far to a new `.eucrec` file at `path` (an existing file is never
    /// replaced). The recording holds the samples, events and BLE frames so far, never firmware.
    fn save(&self, py: Python<'_>, path: PathBuf) -> PyResult<()> {
        if self.samples.is_empty() {
            return Err(PyValueError::new_err("Step the session before saving a recording."));
        }
        let mut header = self.built.header();
        header.started_unix_ms = self.started_unix_ms;
        header.sample_hz = self.sample_hz();
        header.scenario_toml = Some(self.inputs.scenario_toml.clone());
        RecordingContext::of(&header).map_err(PyValueError::new_err)?;
        let mut events = self.events.clone();
        if self.finished() && self.halted().is_none() {
            events.extend(verdict_events(
                &self.built.scenario.name,
                &self.built.ride.outcome(&self.built.scenario),
                self.built.session.time_s(),
            ));
        }
        let (samples, frames, outputs) = (&self.samples, &self.frames, &self.outputs);
        py.detach(move || {
            let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
            let mut writer = Writer::new(BufWriter::new(file), &header)?;
            let mut events = events.into_iter().peekable();
            let mut frames = frames.iter().peekable();
            let mut outputs = outputs.iter().peekable();
            for snapshot in samples {
                while let Some(event) = events.next_if(|e| e.t_s <= snapshot.t_s) {
                    writer.event(&event);
                }
                while let Some(frame) = frames.next_if(|f| f.t_s <= snapshot.t_s) {
                    writer.frame(frame.clone());
                }
                while let Some(at) = outputs.next_if(|at| at.t_s <= snapshot.t_s) {
                    writer.outputs(at.clone());
                }
                writer.push(snapshot)?;
            }
            for event in events {
                writer.event(&event);
            }
            for frame in frames {
                writer.frame(frame.clone());
            }
            writer.finish()?;
            Ok(())
        })
    }
}

/// What `load_recording` hands to Python.
#[derive(Serialize)]
struct RecordingData {
    header: Header,
    context: Option<RecordingContext>,
    start_s: f64,
    end_s: f64,
    samples: Vec<Snapshot>,
    events: Vec<Event>,
    frames: Vec<BleFrame>,
    outputs: Vec<OutputsAt>,
    truncated: Option<String>,
    skipped: usize,
}

#[pyfunction]
fn load_recording_json(py: Python<'_>, path: PathBuf) -> PyResult<String> {
    py.detach(move || {
        let bytes = read_bounded(&path, MAX_RECORDING_BYTES)?;
        let limits = ReadLimits {
            max_file_bytes: MAX_RECORDING_BYTES,
            max_header_bytes: 8 << 20,
            max_chunk_bytes: 32 << 20,
            max_chunks: 10_000,
            max_samples: MAX_SAMPLES as u64,
            max_sample_values: 8_000_000,
            max_sparse_bytes: 32 << 20,
            max_events: 100_000,
            max_frames: 100_000,
            max_outputs: 100_000,
        };
        let mut reader = Reader::new_with_limits(std::io::Cursor::new(bytes), limits)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        let header = reader.header().clone();
        let context = RecordingContext::of(&header).map_err(PyValueError::new_err)?;
        let all = reader.read_all().map_err(|e| PyValueError::new_err(e.to_string()))?;
        let samples = (0..all.len()).map(|i| reader.snapshot(&all, i)).collect();
        let data = RecordingData {
            start_s: reader.start_s(),
            end_s: reader.end_s(),
            samples,
            events: reader.events().to_vec(),
            frames: reader.frames().to_vec(),
            outputs: reader.outputs().to_vec(),
            truncated: reader.truncated().map(str::to_owned),
            skipped: reader.skipped(),
            header,
            context,
        };
        json(&data)
    })
}

#[pyfunction]
fn channels_json() -> PyResult<String> {
    json(&euc_telemetry::CHANNELS)
}

/// The private extension behind the typed public Python package.
#[pymodule(gil_used = true)]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Scenario>()?;
    module.add_class::<Session>()?;
    module.add_function(wrap_pyfunction!(load_scenario, module)?)?;
    module.add_function(wrap_pyfunction!(run_scenario_json, module)?)?;
    module.add_function(wrap_pyfunction!(load_recording_json, module)?)?;
    module.add_function(wrap_pyfunction!(channels_json, module)?)?;
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
