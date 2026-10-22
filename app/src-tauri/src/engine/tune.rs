//! Bounded, read-only access to the active session's numeric firmware map.
//! All Host calls occur on the simulation thread. IPC waits never hold EngineState's lock.

use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::Serializer;
use tauri::State;

use super::host::Host;
use super::{EngineState, with_runner};

pub const MAX_CATALOG: usize = 512;
pub const MAX_NAMES: usize = 64;
pub const MAX_NAME_BYTES: usize = 128;
pub const MAX_HANDLER_ROWS: usize = 128;
const REPLY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize)]
pub struct Run {
    pub session_id: u64,
    pub backend: &'static str,
    pub scenario: String,
    pub firmware: Option<String>,
    pub build: Option<String>,
    pub state: &'static str,
    /// This identifies the read API, not a guessed stock/probes file or firmware image.
    pub source: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Variable {
    pub name: String,
    pub unit: Option<&'static str>,
    pub unit_reference: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct Catalog {
    pub run: Run,
    pub available: bool,
    pub reason: Option<String>,
    pub variables: Vec<Variable>,
    pub total: usize,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct Reading {
    pub name: String,
    pub value: Option<f64>,
    pub reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub run: Run,
    pub t_s: f64,
    pub values: Vec<Reading>,
    pub handler_counters: Option<HandlerCounters>,
    pub handler_reason: Option<&'static str>,
}

/// Decimal strings preserve exact cumulative u64 values across the JavaScript boundary.
fn decimal<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.to_string())
}

#[derive(Debug, Serialize)]
pub struct HandlerCounter {
    pub exception: u16,
    pub name: String,
    #[serde(serialize_with = "decimal")]
    pub occupied_cycles: u64,
}

#[derive(Debug, Serialize)]
pub struct HandlerCounters {
    #[serde(serialize_with = "decimal")]
    pub cycle: u64,
    #[serde(serialize_with = "decimal")]
    pub isr_cycles: u64,
    pub rows: Vec<HandlerCounter>,
    pub total_handlers: usize,
    pub truncated: bool,
}

/// The parent calls this through Host::handler_counters, on the simulation thread only.
pub(super) fn handler_counters(machine: &euc_sim::Machine) -> HandlerCounters {
    let active =
        machine.handler_cycles().iter().enumerate().filter(|(exception, cycles)| *exception > 0 && **cycles > 0);
    let total_handlers = active.clone().count();
    let rows = active
        .take(MAX_HANDLER_ROWS)
        .filter_map(|(exception, &occupied_cycles)| {
            let exception = u16::try_from(exception).ok()?;
            Some(HandlerCounter { exception, name: machine.exception_name(exception), occupied_cycles })
        })
        .collect::<Vec<_>>();
    HandlerCounters {
        cycle: machine.cycles,
        isr_cycles: machine.isr_cycles(),
        truncated: rows.len() < total_handlers,
        total_handlers,
        rows,
    }
}

type HandlerReader = fn(&dyn Host) -> Option<HandlerCounters>;

pub(super) enum Query {
    Catalog,
    Snapshot { session_id: u64, names: Vec<String>, include_handlers: bool },
}

pub(super) enum Reply {
    Catalog(Catalog),
    Snapshot(Snapshot),
}

pub(super) struct Request {
    query: Query,
    deadline: Instant,
    reply: SyncSender<Result<Reply, String>>,
}

pub(super) struct Pending {
    reply: Receiver<Result<Reply, String>>,
}

impl Pending {
    fn wait(self) -> Result<Reply, String> {
        self.reply.recv_timeout(REPLY_TIMEOUT).map_err(|_| "The probe request timed out. Try again.".to_owned())?
    }
}

/// The one-slot queue bounds pending work even while a firmware image loads.
pub(super) fn enqueue(tx: &SyncSender<Request>, query: Query) -> Result<Pending, String> {
    let query = match query {
        Query::Snapshot { session_id, names, include_handlers } => {
            if names.len() > MAX_NAMES {
                return Err(format!("Request at most {MAX_NAMES} probe names."));
            }
            if names.iter().any(|n| n.is_empty() || n.len() > MAX_NAME_BYTES || n.chars().any(char::is_control)) {
                return Err(format!(
                    "Each probe name must contain 1 to {MAX_NAME_BYTES} bytes without control characters."
                ));
            }
            Query::Snapshot {
                session_id,
                names: names.into_iter().collect::<BTreeSet<_>>().into_iter().collect(),
                include_handlers,
            }
        }
        query => query,
    };
    let (reply, rx) = mpsc::sync_channel(1);
    let request = Request { query, deadline: Instant::now() + REPLY_TIMEOUT, reply };
    tx.try_send(request).map_err(|error| match error {
        TrySendError::Full(_) => "The probe queue is busy. Try again.".to_owned(),
        TrySendError::Disconnected(_) => "The simulator thread stopped.".to_owned(),
    })?;
    Ok(Pending { reply: rx })
}

/// Called only by Sim::run, between simulation steps. The Host hook can return None.
pub(super) fn respond_with_handlers(
    request: Request,
    host: Option<&dyn Host>,
    run: Run,
    error: Option<&str>,
    read_handlers: HandlerReader,
) {
    respond_inner(request, host, run, error, Some(read_handlers));
}

fn respond_inner(
    request: Request,
    host: Option<&dyn Host>,
    run: Run,
    error: Option<&str>,
    read_handlers: Option<HandlerReader>,
) {
    if Instant::now() >= request.deadline {
        return;
    }
    let result = match request.query {
        Query::Catalog => Ok(Reply::Catalog(catalog(host, run, error))),
        Query::Snapshot { session_id, names, include_handlers } => {
            if session_id != run.session_id {
                Err("The session changed. Reload the probe catalog.".to_owned())
            } else {
                snapshot(host, run, &names, include_handlers, read_handlers).map(Reply::Snapshot)
            }
        }
    };
    let _ = request.reply.try_send(result);
}

fn catalog(host: Option<&dyn Host>, run: Run, error: Option<&str>) -> Catalog {
    let names = host.and_then(Host::firmware_var_names);
    let mut names: Vec<_> = names.unwrap_or_default().into_iter().collect::<BTreeSet<_>>().into_iter().collect();
    let total = names.len();
    names.retain(|n| !n.is_empty() && n.len() <= MAX_NAME_BYTES && !n.chars().any(char::is_control));
    names.truncate(MAX_CATALOG);
    let available = !names.is_empty();
    let reason = if available {
        None
    } else {
        Some(error.unwrap_or("This host has no numeric Session firmware map.").to_owned())
    };
    let truncated = names.len() != total;
    let variables = names.into_iter().map(variable).collect();
    Catalog { run, available, reason, variables, total, truncated }
}

fn snapshot(
    host: Option<&dyn Host>,
    run: Run,
    names: &[String],
    include_handlers: bool,
    read_handlers: Option<HandlerReader>,
) -> Result<Snapshot, String> {
    let host = host.ok_or("The simulator has no active host.")?;
    let known: BTreeSet<_> = host.firmware_var_names().unwrap_or_default().into_iter().collect();
    let values = names
        .iter()
        .map(|name| {
            let (value, reason) = if !known.contains(name) {
                (None, Some("The active firmware map has no such name."))
            } else {
                match host.firmware_value(name) {
                    Some(value) if value.is_finite() => (Some(value), None),
                    Some(_) => (None, Some("The firmware value is not finite.")),
                    None => (None, Some("The firmware value is unavailable.")),
                }
            };
            Reading { name: name.clone(), value, reason }
        })
        .collect();
    let mut handler_counters = if include_handlers { read_handlers.and_then(|read| read(host)) } else { None };
    if let Some(counters) = handler_counters.as_mut()
        && counters.rows.len() > MAX_HANDLER_ROWS
    {
        counters.truncated = true;
        counters.rows.truncate(MAX_HANDLER_ROWS);
    }
    let handler_reason = if !include_handlers {
        Some("Handler counters were not requested.")
    } else if handler_counters.is_none() {
        Some("This host does not expose handler counters.")
    } else {
        None
    };
    Ok(Snapshot { run, t_s: host.time_s(), values, handler_counters, handler_reason })
}

fn variable(name: String) -> Variable {
    // These units describe the raw values, without inferred scaling or suffix guessing.
    // Both stockmap.toml and probes.toml document these semantic names for Begode Extreme.
    let unit = match name.as_str() {
        "speed_cms" => Some("cm/s"),
        "attitude_pitch_deg" | "tiltback_speed_deg" | "tiltback_battery_deg" | "tiltback_pwm_deg" => Some("deg"),
        "vq_active" => Some("Q15"),
        _ => None,
    };
    let unit_reference = unit.map(|_| "profiles/begode-extreme/{stockmap,probes}.toml");
    Variable { name, unit, unit_reference }
}

#[tauri::command]
pub async fn engine_tune_catalog(state: State<'_, EngineState>) -> Result<Catalog, String> {
    let pending = with_runner(&state, |runner| runner.tune(Query::Catalog))??;
    let reply = tauri::async_runtime::spawn_blocking(move || pending.wait()).await.map_err(|e| e.to_string())??;
    match reply {
        Reply::Catalog(catalog) => Ok(catalog),
        Reply::Snapshot(_) => Err("The probe reply has the wrong type.".to_owned()),
    }
}

#[tauri::command]
pub async fn engine_tune_snapshot(
    state: State<'_, EngineState>,
    session_id: u64,
    names: Vec<String>,
    include_handlers: Option<bool>,
) -> Result<Snapshot, String> {
    let pending = with_runner(&state, |runner| {
        runner.tune(Query::Snapshot { session_id, names, include_handlers: include_handlers.unwrap_or(false) })
    })??;
    let reply = tauri::async_runtime::spawn_blocking(move || pending.wait()).await.map_err(|e| e.to_string())??;
    match reply {
        Reply::Snapshot(snapshot) => Ok(snapshot),
        Reply::Catalog(_) => Err("The probe reply has the wrong type.".to_owned()),
    }
}

#[cfg(test)]
mod acceptance_tests {
    use std::cell::Cell;

    use super::*;
    use euc_telemetry::{BodyInfo, Event, Snapshot as TelemetrySnapshot};

    // Synthetic hosts test request policy only. They do not stand in for firmware evidence.
    #[derive(Default)]
    struct PolicyHost {
        names: Vec<String>,
        calls: Cell<usize>,
        handler_calls: Cell<usize>,
    }

    impl Host for PolicyHost {
        fn step(&mut self, _: f64) {
            panic!("A read-only request must not step the host");
        }
        fn time_s(&self) -> f64 {
            1.25
        }
        fn bodies(&self) -> Vec<BodyInfo> {
            vec![]
        }
        fn snapshot(&self, _: &mut TelemetrySnapshot) {
            panic!("Not a telemetry request");
        }
        fn drain_events(&mut self, _: &mut Vec<Event>) {}
        fn drain_ble_tx(&mut self, _: &mut Vec<(f64, u8)>) {}
        fn inject_ble(&mut self, _: &[u8]) -> Result<(), String> {
            panic!("A probe must not send BLE");
        }
        fn halted(&self) -> Option<String> {
            None
        }
        fn firmware_var_names(&self) -> Option<Vec<String>> {
            self.calls.set(self.calls.get() + 1);
            Some(self.names.clone())
        }
        fn firmware_value(&self, name: &str) -> Option<f64> {
            self.calls.set(self.calls.get() + 1);
            match name {
                "finite" => Some(-12.5),
                "nan" => Some(f64::NAN),
                "infinite" => Some(f64::INFINITY),
                _ => None,
            }
        }
        fn handler_counters(&self) -> Option<HandlerCounters> {
            self.handler_calls.set(self.handler_calls.get() + 1);
            Some(HandlerCounters {
                cycle: u64::MAX,
                isr_cycles: (1 << 53) + 1,
                rows: (1..=MAX_HANDLER_ROWS + 2)
                    .map(|exception| HandlerCounter {
                        exception: exception as u16,
                        name: format!("fixture-{exception}"),
                        occupied_cycles: u64::MAX,
                    })
                    .collect(),
                total_handlers: MAX_HANDLER_ROWS + 2,
                truncated: false,
            })
        }
    }

    fn run(session_id: u64) -> Run {
        Run {
            session_id,
            backend: "policy-test-only",
            scenario: "policy-test-only".into(),
            firmware: None,
            build: None,
            state: "paused",
            source: "Synthetic policy host",
        }
    }

    fn query(session_id: u64, names: &[&str], include_handlers: bool) -> Query {
        Query::Snapshot { session_id, names: names.iter().map(|n| (*n).into()).collect(), include_handlers }
    }

    fn exchange(host: Option<&dyn Host>, run: Run, query: Query) -> Result<Reply, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        let pending = enqueue(&tx, query)?;
        respond_with_handlers(rx.recv().unwrap(), host, run, None, |host| host.handler_counters());
        pending.wait()
    }

    fn snapshot_reply(reply: Result<Reply, String>) -> Snapshot {
        match reply.unwrap() {
            Reply::Snapshot(value) => value,
            _ => panic!("Expected snapshot"),
        }
    }

    #[test]
    fn rejects_invalid_names_before_queue_admission() {
        let (tx, rx) = mpsc::sync_channel(1);
        for name in [String::new(), "x".repeat(MAX_NAME_BYTES + 1), "x\n".into(), "x\0".into(), "é".repeat(65)] {
            assert!(
                enqueue(&tx, Query::Snapshot { session_id: 1, names: vec![name], include_handlers: false }).is_err()
            );
            assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        }
        assert!(
            enqueue(
                &tx,
                Query::Snapshot { session_id: 1, names: vec!["x".into(); MAX_NAMES + 1], include_handlers: false }
            )
            .err()
            .unwrap()
            .contains("at most 64")
        );
        assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    }

    #[test]
    fn accepts_exact_limits_and_sorts_and_deduplicates_names() {
        let (tx, rx) = mpsc::sync_channel(1);
        let names = (0..MAX_NAMES).map(|i| format!("{i:03}{}", "x".repeat(MAX_NAME_BYTES - 3))).rev().collect();
        let _pending = enqueue(&tx, Query::Snapshot { session_id: 1, names, include_handlers: true }).unwrap();
        let Query::Snapshot { names, include_handlers, .. } = rx.recv().unwrap().query else { panic!("snapshot") };
        assert_eq!(names.len(), MAX_NAMES);
        assert!(include_handlers);
        assert!(names.iter().all(|name| name.len() == MAX_NAME_BYTES));
        assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
        let _pending = enqueue(&tx, query(1, &["z", "a", "z"], false)).unwrap();
        let Query::Snapshot { names, .. } = rx.recv().unwrap().query else { panic!("snapshot") };
        assert_eq!(names, ["a", "z"]);
    }

    #[test]
    fn queue_capacity_is_one_and_disconnection_is_explicit() {
        let (tx, rx) = mpsc::sync_channel(1);
        let _pending = enqueue(&tx, Query::Catalog).unwrap();
        assert_eq!(enqueue(&tx, Query::Catalog).err().unwrap(), "The probe queue is busy. Try again.");
        drop(rx);
        assert_eq!(enqueue(&tx, Query::Catalog).err().unwrap(), "The simulator thread stopped.");
    }

    #[test]
    fn expired_requests_do_not_read_the_host_or_send_a_reply() {
        let host = PolicyHost::default();
        let (tx, rx) = mpsc::sync_channel(1);
        let pending = enqueue(&tx, query(1, &["finite"], true)).unwrap();
        let mut request = rx.recv().unwrap();
        request.deadline = Instant::now() - Duration::from_millis(1);
        respond_with_handlers(request, Some(&host), run(1), None, |h| h.handler_counters());
        assert_eq!(host.calls.get(), 0);
        assert_eq!(host.handler_calls.get(), 0);
        assert!(matches!(pending.reply.try_recv(), Err(mpsc::TryRecvError::Disconnected)));
    }

    #[test]
    fn abandoned_reply_does_not_block_the_responder() {
        let (tx, rx) = mpsc::sync_channel(1);
        drop(enqueue(&tx, Query::Catalog).unwrap());
        respond_with_handlers(rx.recv().unwrap(), None, run(1), None, |h| h.handler_counters());
    }

    #[test]
    fn unanswered_requests_have_a_bounded_wait() {
        let (tx, rx) = mpsc::sync_channel(1);
        let pending = enqueue(&tx, Query::Catalog).unwrap();
        let _unanswered = rx.recv().unwrap();
        let start = Instant::now();
        assert_eq!(pending.wait().err().unwrap(), "The probe request timed out. Try again.");
        assert!(start.elapsed() >= REPLY_TIMEOUT);
    }

    #[test]
    fn reset_session_rejects_old_generation_before_host_reads() {
        let host = PolicyHost::default();
        assert_eq!(
            exchange(Some(&host), run(2), query(1, &["finite"], true)).err().unwrap(),
            "The session changed. Reload the probe catalog."
        );
        assert_eq!(host.calls.get(), 0);
        assert_eq!(host.handler_calls.get(), 0);
    }

    #[test]
    fn unavailable_host_has_no_catalog_or_snapshot_values() {
        let Reply::Catalog(catalog) = exchange(None, run(1), Query::Catalog).unwrap() else { panic!("catalog") };
        assert!(!catalog.available);
        assert!(catalog.variables.is_empty());
        assert_eq!(catalog.reason.as_deref(), Some("This host has no numeric Session firmware map."));
        assert_eq!(exchange(None, run(1), query(1, &[], true)).err().unwrap(), "The simulator has no active host.");
    }

    #[test]
    fn catalog_is_filtered_sorted_unique_and_bounded() {
        let mut names = (0..MAX_CATALOG + 3).map(|i| format!("probe_{i:04}")).collect::<Vec<_>>();
        names.extend(["probe_0000".into(), "".into(), "bad\tname".into(), "x".repeat(MAX_NAME_BYTES + 1)]);
        let total = names.iter().collect::<BTreeSet<_>>().len();
        let host = PolicyHost { names, ..Default::default() };
        let Reply::Catalog(catalog) = exchange(Some(&host), run(1), Query::Catalog).unwrap() else { panic!("catalog") };
        assert!(catalog.available && catalog.truncated);
        assert_eq!(catalog.total, total);
        assert_eq!(catalog.variables.len(), MAX_CATALOG);
        assert!(catalog.variables.windows(2).all(|pair| pair[0].name < pair[1].name));
        assert!(catalog.variables.iter().all(|v| v.name.starts_with("probe_")));
    }

    #[test]
    fn unavailable_and_nonfinite_values_are_null_with_reasons() {
        let host = PolicyHost {
            names: ["finite", "nan", "infinite", "missing"].map(str::to_owned).to_vec(),
            ..Default::default()
        };
        let value = snapshot_reply(exchange(
            Some(&host),
            run(1),
            query(1, &["finite", "nan", "infinite", "missing", "unknown"], false),
        ));
        assert_eq!(value.t_s, 1.25);
        assert_eq!(value.values[0].value, Some(-12.5));
        assert_eq!(value.values[0].reason, None);
        for reading in value.values.iter().filter(|v| v.name == "nan" || v.name == "infinite") {
            assert_eq!(reading.value, None);
            assert_eq!(reading.reason, Some("The firmware value is not finite."));
        }
        assert_eq!(
            value.values.iter().find(|v| v.name == "missing").unwrap().reason,
            Some("The firmware value is unavailable.")
        );
        assert_eq!(
            value.values.iter().find(|v| v.name == "unknown").unwrap().reason,
            Some("The active firmware map has no such name.")
        );
        assert_eq!(host.calls.get(), 5, "unknown names must not reach the value hook");
        let json = serde_json::to_value(value).unwrap();
        assert!(json["values"][1]["value"].is_null());
    }

    #[test]
    fn handler_opt_in_row_limit_and_exact_u64_serialization() {
        let host = PolicyHost::default();
        let value = snapshot_reply(exchange(Some(&host), run(1), query(1, &[], false)));
        assert!(value.handler_counters.is_none());
        assert_eq!(value.handler_reason, Some("Handler counters were not requested."));
        assert_eq!(host.handler_calls.get(), 0);
        let value = snapshot_reply(exchange(Some(&host), run(1), query(1, &[], true)));
        let counters = value.handler_counters.as_ref().unwrap();
        assert_eq!(host.handler_calls.get(), 1);
        assert_eq!(counters.rows.len(), MAX_HANDLER_ROWS);
        assert_eq!(counters.total_handlers, MAX_HANDLER_ROWS + 2);
        assert!(counters.truncated);
        assert_eq!(value.handler_reason, None);
        let json = serde_json::to_value(value).unwrap();
        assert_eq!(json["handler_counters"]["cycle"], u64::MAX.to_string());
        assert_eq!(json["handler_counters"]["isr_cycles"], ((1u64 << 53) + 1).to_string());
        assert_eq!(json["handler_counters"]["rows"][0]["occupied_cycles"], u64::MAX.to_string());
    }

    #[test]
    fn absent_handler_hook_is_explicit_even_when_requested() {
        let host = PolicyHost::default();
        let (tx, rx) = mpsc::sync_channel(1);
        let pending = enqueue(&tx, query(1, &[], true)).unwrap();
        respond_with_handlers(rx.recv().unwrap(), Some(&host), run(1), None, |_| None);
        let value = snapshot_reply(pending.wait());
        assert!(value.handler_counters.is_none());
        assert_eq!(value.handler_reason, Some("This host does not expose handler counters."));
    }
}

#[cfg(test)]
mod cached_stock_acceptance_tests {
    use super::super::host::RideHost;
    use super::super::runner::Runner;
    use super::*;
    use euc_fwsource::FirmwareCache;
    use euc_profile::{Channel, FirmwareSource, ProfileSource, Sha256};
    use euc_sim::scenario::Scenario;
    use euc_sim::{Machine, MachineConfig, PlantState, Session, WheelFiles};

    // This fixture reads only an already cached, pinned image. Missing inputs fail the opt-in test.
    // No vendor bytes enter the source tree or test output. No fetch, audio device, or wheel is opened.
    fn cached_session() -> (Session, WheelFiles, String) {
        cached_session_with_timing(euc_sim::ExceptionTiming::default())
    }

    fn cached_session_with_timing(timing: euc_sim::ExceptionTiming) -> (Session, WheelFiles, String) {
        let files = WheelFiles::load(&ProfileSource::Builtin, "begode-extreme").unwrap();
        let current = files
            .profile
            .firmware
            .iter()
            .find_map(|source| match source {
                FirmwareSource::BegodeApi(build) if build.channel == Channel::Current => Some(build),
                _ => None,
            })
            .unwrap();
        assert_eq!(current.label, files.stockmap.build, "The map must belong to this pinned image");
        let root = std::env::var_os("EUC_CACHE_DIR")
            .map(Into::into)
            .or_else(FirmwareCache::default_root)
            .expect("Set EUC_CACHE_DIR to an existing firmware cache");
        let path = FirmwareCache::new(root)
            .verified(&current.sha256)
            .expect("The pinned stock image must already exist in the cache; this test never fetches");
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(Sha256::digest(&bytes), current.sha256);
        let image = euc_image::Image::from_bytes(&bytes).unwrap();
        let gate = euc_image::validate(&image, &files.profile);
        assert!(gate.passed, "The desktop image gates must pass");
        let build = format!("{} · {}", current.label, current.sha256.short());
        let board = files.board_with_settings("neutral").unwrap();
        let config = MachineConfig {
            board: &board,
            stockmap: Some(&files.stockmap),
            image: image.bytes(),
            load_addr: files.profile.board.app.base,
            hclk: files.hclk(),
            plant: PlantState::default(),
            injections: vec![],
            power_cycles: vec![],
            cpi: euc_sim::DEFAULT_CPI,
            exception_timing: timing,
            max_quantum: 4000,
            marker_window_s: 4.0,
        };
        let live = files.live(Default::default(), Default::default()).unwrap();
        let machine =
            Machine::new_live(&config, &euc_sim::registry(), euc_sim::cpu::Backend::Own.factory().unwrap(), live)
                .unwrap();
        (Session::new(machine, Some(&files.stockmap), 120.0), files, build)
    }

    fn scenario() -> Scenario {
        Scenario::from_toml(
            r#"
schema = 1
name = "tune-cached-stock-acceptance"
description = "Keep the simulated wheel on its stand for read-only native probe checks."
wheel = "begode-extreme"
seconds = 30.0
release_s = 29.0
speed_kmh = [[0, 0]]
"#,
        )
        .unwrap()
    }

    fn run(session_id: u64, build: &str) -> Run {
        Run {
            session_id,
            backend: euc_sim::cpu::Backend::Own.name(),
            scenario: scenario().name,
            firmware: Some("stock firmware".into()),
            build: Some(build.into()),
            state: "paused",
            source: "Session firmware map",
        }
    }

    fn exchange(host: &dyn Host, run: Run, query: Query) -> Result<Reply, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        let pending = enqueue(&tx, query)?;
        respond_with_handlers(rx.recv().unwrap(), Some(host), run, None, |h| h.handler_counters());
        pending.wait()
    }

    fn read(host: &dyn Host, run: Run, names: Vec<String>) -> Snapshot {
        let query = Query::Snapshot { session_id: run.session_id, names, include_handlers: true };
        let Reply::Snapshot(value) = exchange(host, run, query).unwrap() else { panic!("snapshot") };
        value
    }

    fn bounded(counters: &HandlerCounters) {
        assert!(counters.rows.len() <= MAX_HANDLER_ROWS);
        assert!(counters.total_handlers >= counters.rows.len());
        assert_eq!(counters.truncated, counters.total_handlers > counters.rows.len());
        assert!(counters.isr_cycles <= counters.cycle);
        assert!(counters.rows.iter().all(|row| row.exception > 0 && row.occupied_cycles > 0 && !row.name.is_empty()));
        let sum: u64 = counters.rows.iter().map(|row| row.occupied_cycles).sum();
        if counters.truncated {
            assert!(sum <= counters.isr_cycles);
        } else {
            assert_eq!(sum, counters.isr_cycles);
        }
    }

    #[test]
    #[ignore = "requires the pinned local stock cache; runs real Cortex-M3 firmware and MuJoCo without audio"]
    fn cached_stock_catalog_snapshot_pause_reset_and_isr_intervals() {
        let (mut session, files, build) = cached_session();
        for _ in 0..480 {
            session.step();
        }
        assert_eq!(session.machine().halted(), None);
        assert!(session.time_s() >= 4.0);

        // Decode the declared RAM fields independently, before giving the unchanged Session to RideHost.
        let expected = files
            .stockmap
            .ram
            .iter()
            .filter(|var| var.kind != "ascii")
            .map(|var| {
                let bytes = session.machine().memory(var.addr, var.size as usize).unwrap();
                let value = match var.kind.as_str() {
                    "u8" => f64::from(bytes[0]),
                    "s16" => f64::from(i16::from_le_bytes(bytes.try_into().unwrap())),
                    "f32" => f64::from(f32::from_le_bytes(bytes.try_into().unwrap())),
                    other => panic!("Add an independent decoder for {other}"),
                };
                assert!(value.is_finite(), "{} must be readable after boot", var.name);
                (var.name.clone(), value)
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let oracle = handler_counters(session.machine());
        bounded(&oracle);
        assert!(oracle.isr_cycles > 0 && !oracle.rows.is_empty());
        let mut host = RideHost::new(session, scenario()).unwrap();
        let Reply::Catalog(catalog) = exchange(&host, run(7, &build), Query::Catalog).unwrap() else {
            panic!("catalog")
        };
        assert!(catalog.available && !catalog.truncated);
        assert_eq!(catalog.total, expected.len());
        assert_eq!(catalog.variables.iter().map(|v| &v.name).collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>());
        assert!(!catalog.variables.iter().any(|v| v.name == "version_string"));
        for variable in &catalog.variables {
            let unit = match variable.name.as_str() {
                "speed_cms" => Some("cm/s"),
                "vq_active" => Some("Q15"),
                "attitude_pitch_deg" | "tiltback_speed_deg" | "tiltback_battery_deg" | "tiltback_pwm_deg" => {
                    Some("deg")
                }
                _ => None,
            };
            assert_eq!(variable.unit, unit);
            assert_eq!(variable.unit_reference.is_some(), unit.is_some());
        }
        let mut names = expected.keys().cloned().collect::<Vec<_>>();
        names.push("unknown_probe".into());
        let before = read(&host, run(7, &build), names.clone());
        for reading in &before.values {
            assert_eq!(reading.value, expected.get(&reading.name).copied());
        }
        assert!(before.values.iter().find(|v| v.name == "unknown_probe").unwrap().reason.is_some());
        assert_eq!(
            serde_json::to_value(before.handler_counters.as_ref().unwrap()).unwrap(),
            serde_json::to_value(oracle).unwrap()
        );
        let unchanged = read(&host, run(7, &build), names.clone());
        assert_eq!(
            serde_json::to_value(&before).unwrap(),
            serde_json::to_value(unchanged).unwrap(),
            "Paused reads must not advance time, RAM, or counters"
        );

        host.step(0.25);
        assert_eq!(host.halted(), None);
        let after = read(&host, run(7, &build), names);
        assert!(after.t_s > before.t_s);
        let first = before.handler_counters.unwrap();
        let second = after.handler_counters.unwrap();
        bounded(&second);
        let elapsed = second.cycle - first.cycle;
        let occupied = second.isr_cycles - first.isr_cycles;
        assert!(elapsed > 0 && occupied > 0 && occupied <= elapsed);
        for row in &first.rows {
            let later = second.rows.iter().find(|later| later.exception == row.exception).unwrap();
            assert!(later.occupied_cycles >= row.occupied_cycles);
        }
        println!(
            "cached stock: {} numeric names; ISR interval {occupied}/{elapsed} simulated cycles; no firmware bytes logged",
            expected.len()
        );

        let (fresh_session, _, _) = cached_session();
        let fresh_host = RideHost::new(fresh_session, scenario()).unwrap();
        let stale = Query::Snapshot { session_id: 7, names: vec!["speed_cms".into()], include_handlers: true };
        assert_eq!(
            exchange(&fresh_host, run(8, &build), stale).err().unwrap(),
            "The session changed. Reload the probe catalog."
        );
        let fresh = read(&fresh_host, run(8, &build), vec!["speed_cms".into()]);
        assert_eq!(fresh.run.session_id, 8);
        assert_eq!(fresh.t_s, 0.0);
        assert_eq!(fresh.handler_counters.unwrap().cycle, 0);
    }

    #[test]
    #[ignore = "requires the pinned local stock cache; checks unsupported reference-timing ISR metrics"]
    fn cached_stock_reference_timing_reports_isr_metrics_unavailable() {
        let (mut session, _, build) = cached_session_with_timing(euc_sim::ExceptionTiming::REFERENCE);
        session.step();
        assert!(!session.machine().isr_metrics_available());
        assert!(session.snapshot().signals.isr_load_pct.is_nan());
        let host = RideHost::new(session, scenario()).unwrap();
        let snapshot = read(&host, run(1, &build), vec!["speed_cms".into()]);
        assert!(snapshot.handler_counters.is_none());
        assert_eq!(snapshot.handler_reason, Some("This host does not expose handler counters."));
    }

    fn runner_catalog(runner: &Runner, accept: impl Fn(&Catalog) -> bool) -> Catalog {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match runner.tune(Query::Catalog) {
                Ok(pending) => {
                    let Reply::Catalog(catalog) =
                        pending.wait().expect("The real runner must answer before the probe timeout")
                    else {
                        panic!("Expected a catalog from the real runner");
                    };
                    assert_ne!(catalog.run.state, "error", "{}", catalog.reason.as_deref().unwrap_or_default());
                    assert_ne!(catalog.run.state, "halted");
                    if accept(&catalog) {
                        return catalog;
                    }
                }
                Err(error) => assert_eq!(error, "The probe queue is busy. Try again."),
            }
            assert!(Instant::now() < deadline, "The real runner did not reach the expected catalog state");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn runner_snapshot(runner: &Runner, session_id: u64, names: Vec<String>) -> Result<Snapshot, String> {
        let pending = runner.tune(Query::Snapshot { session_id, names, include_handlers: true })?;
        let Reply::Snapshot(snapshot) = pending.wait()? else { panic!("Expected a snapshot from the real runner") };
        Ok(snapshot)
    }

    #[test]
    #[ignore = "requires the pinned stock cache; exercises the actual runner lifecycle without OS audio"]
    fn cached_stock_real_runner_pause_step_resume_restart_and_epoch_rejection() {
        // Fail before starting the desktop factory if the pinned local input is absent or invalid.
        let (_, files, build) = cached_session();
        let backend = super::super::cpu::backend().name();
        assert_eq!(
            backend,
            euc_sim::cpu::Backend::Own.name(),
            "This acceptance case requires the shipped Cortex-M3 interpreter"
        );
        let runner = Runner::start_without_audio_for_test(
            Box::new(super::super::firmware::build_host),
            "balance-60s".into(),
            backend,
            Default::default(),
            Default::default(),
        );
        runner.play(false);
        let initial = runner_catalog(&runner, |catalog| catalog.available && catalog.run.state == "paused");
        assert!(initial.run.session_id > 0);
        assert_eq!(initial.run.backend, backend);
        assert_eq!(initial.run.scenario, "balance-60s");
        assert_eq!(initial.run.firmware.as_deref(), Some("stock firmware"));
        assert_eq!(initial.run.build.as_deref(), Some(build.as_str()));
        assert_eq!(initial.run.source, "Session firmware map");
        let expected =
            files.stockmap.ram.iter().filter(|v| v.kind != "ascii").map(|v| v.name.clone()).collect::<BTreeSet<_>>();
        assert_eq!(initial.variables.iter().map(|v| v.name.clone()).collect::<BTreeSet<_>>(), expected);
        let names = initial.variables.iter().map(|v| v.name.clone()).collect::<Vec<_>>();
        let epoch = initial.run.session_id;
        runner.set_rtf(8.0);
        runner.play(true);
        runner_catalog(&runner, |catalog| catalog.run.state == "running");
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let snapshot = runner_snapshot(&runner, epoch, names.clone()).unwrap();
            assert_eq!(snapshot.run.state, "running");
            assert!(Instant::now() < deadline, "The real firmware did not finish boot within the bounded wait");
            if snapshot.t_s >= 4.0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        runner.play(false);
        runner_catalog(&runner, |catalog| catalog.run.state == "paused");
        let before = runner_snapshot(&runner, epoch, names.clone()).unwrap();
        assert!(before.t_s >= 4.0);
        assert!(before.values.iter().all(|value| value.value.is_some_and(f64::is_finite) && value.reason.is_none()));
        bounded(before.handler_counters.as_ref().unwrap());
        std::thread::sleep(Duration::from_millis(75));
        let paused = runner_snapshot(&runner, epoch, names.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(&before).unwrap(),
            serde_json::to_value(paused).unwrap(),
            "Runner::play(false) must freeze simulation time, firmware probes, and counters"
        );

        runner.step();
        let deadline = Instant::now() + Duration::from_secs(5);
        let after = loop {
            let value = runner_snapshot(&runner, epoch, names.clone()).unwrap();
            if value.t_s > before.t_s {
                break value;
            }
            assert!(Instant::now() < deadline, "The paused runner did not process Step");
        };
        assert_eq!(after.run.state, "paused");
        assert!((after.t_s - before.t_s - 1.0 / 60.0).abs() < 1e-6);
        let first = before.handler_counters.unwrap();
        let next = after.handler_counters.unwrap();
        bounded(&next);
        let elapsed = next.cycle - first.cycle;
        let occupied = next.isr_cycles - first.isr_cycles;
        assert!(occupied > 0 && occupied <= elapsed);

        runner.restart(None);
        runner.play(false);
        let reset = runner_catalog(&runner, |catalog| catalog.run.session_id > epoch && catalog.run.state == "paused");
        assert_eq!(reset.run.session_id, epoch + 1);
        assert_eq!(
            runner_snapshot(&runner, epoch, names.clone()).err().unwrap(),
            "The session changed. Reload the probe catalog."
        );
        let fresh = runner_snapshot(&runner, reset.run.session_id, names.clone()).unwrap();
        assert!(fresh.t_s < before.t_s, "Restart must replace the previous firmware clock");
        assert!(fresh.handler_counters.as_ref().unwrap().cycle < next.cycle);
        runner.play(true);
        runner_catalog(&runner, |catalog| catalog.run.state == "running");
        runner.play(false);
        runner_catalog(&runner, |catalog| catalog.run.state == "paused");
        println!(
            "real runner: {} mapped names; actual epochs {epoch} -> {}; paused Step ISR interval {occupied}/{elapsed} simulated cycles",
            names.len(),
            reset.run.session_id
        );
    }

    #[test]
    #[ignore = "requires the pinned stock cache; blocks only host construction to check actual runner queue expiry"]
    fn cached_stock_real_runner_bounds_pending_work_and_recovers_after_timeout() {
        let _verified_local_input = cached_session();
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let runner = Runner::start_without_audio_for_test(
            Box::new(move |scenario, firmware, progress| {
                entered_tx.send(()).map_err(|e| e.to_string())?;
                release_rx.recv_timeout(Duration::from_secs(8)).map_err(|e| e.to_string())?;
                super::super::firmware::build_host(scenario, firmware, progress)
            }),
            "balance-60s".into(),
            super::super::cpu::backend().name(),
            Default::default(),
            Default::default(),
        );
        entered_rx.recv_timeout(Duration::from_secs(5)).expect("The real sim thread must enter its host factory");
        let pending = runner.tune(Query::Catalog).unwrap();
        assert_eq!(runner.tune(Query::Catalog).err().unwrap(), "The probe queue is busy. Try again.");
        assert_eq!(
            runner
                .tune(Query::Snapshot {
                    session_id: 1,
                    names: vec!["speed_cms".into(); MAX_NAMES + 1],
                    include_handlers: true,
                })
                .err()
                .unwrap(),
            "Request at most 64 probe names."
        );
        assert_eq!(pending.wait().err().unwrap(), "The probe request timed out. Try again.");
        // The request still occupies the one-slot queue until Sim::run discards it after construction.
        assert_eq!(runner.tune(Query::Catalog).err().unwrap(), "The probe queue is busy. Try again.");
        release_tx.send(()).unwrap();
        runner.play(false);
        let catalog = runner_catalog(&runner, |catalog| catalog.available && catalog.run.state == "paused");
        let value = runner_snapshot(&runner, catalog.run.session_id, vec!["speed_cms".into()]).unwrap();
        assert_eq!(value.run.session_id, catalog.run.session_id);
        assert_eq!(value.values.len(), 1);
        assert!(value.values[0].value.is_some_and(f64::is_finite));
    }
}
