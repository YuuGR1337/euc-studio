//! The live engine behind the studio: Tauri commands, the sim thread and the stream.
//!
//! The web view calls `engine_connect` with two channels: a binary one for telemetry batches
//! (`euc_telemetry::batch`) and a JSON one for status, bodies, the display layout, events, phone frames and board outputs. The
//! engine starts on the first connect and keeps running across web view reloads.

mod buttons;
pub mod compare;
#[cfg(dev)]
pub mod control;
mod cpu;
mod display;
mod firmware;
pub mod free_ride;
mod host;
pub(super) mod images;
mod lights;
mod native_audio;
mod phone;
pub mod recordings;
mod ride_sound;
mod runner;
mod scenarios;
#[cfg(test)]
mod showcase;
pub mod tune;
pub mod world;

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};

use runner::{Message, Runner, Sinks};

#[derive(Default)]
pub struct EngineState(Mutex<Option<Runner>>);

/// What the web view needs to set up its live client.
#[derive(Serialize)]
pub struct EngineInfo {
    backend: &'static str,
    audio: native_audio::AudioStatus,
    scenarios: Vec<scenarios::ScenarioInfo>,
    commands: Vec<phone::CommandSpec>,
    firmwares: Vec<images::FirmwareInfo>,
    /// `EUC_STUDIO_PERF=<seconds>`: the web view measures its frame times for that long and logs
    /// them (to the terminal through `frontend_log`).
    perf_seconds: Option<f64>,
}

fn with_runner<T>(state: &State<'_, EngineState>, f: impl FnOnce(&Runner) -> T) -> Result<T, String> {
    let guard = state.0.lock().map_err(|_| "the engine state is poisoned".to_owned())?;
    guard.as_ref().map(f).ok_or_else(|| "the engine is not running; connect first".to_owned())
}

/// Attaches the web view's channels and starts the engine if it is not running yet.
#[tauri::command]
pub fn engine_connect(
    state: State<'_, EngineState>,
    stream: Channel<InvokeResponseBody>,
    messages: Channel<Message>,
    recordings: State<'_, recordings::RecordingState>,
    free_ride: State<'_, free_ride::FreeRideState>,
    sound_enabled: bool,
) -> Result<EngineInfo, String> {
    let scenarios = scenarios::infos();
    let first = scenarios.first().map(|s| s.id.clone()).unwrap_or_default();
    let mut guard = state.0.lock().map_err(|_| "the engine state is poisoned".to_owned())?;
    let runner = guard.get_or_insert_with(|| {
        Runner::start(
            Box::new(firmware::build_host),
            first,
            cpu::backend().name(),
            recordings.inner().clone(),
            free_ride.inner().clone(),
            sound_enabled,
        )
    });
    runner.attach(Sinks { stream, messages });
    // A reloaded web view needs the bodies and a clean slate even though the run goes on.
    runner.announce();
    Ok(EngineInfo {
        backend: cpu::backend().name(),
        audio: runner.audio_status(),
        scenarios,
        commands: phone::catalogue(),
        firmwares: images::catalogue("begode-extreme"),
        perf_seconds: std::env::var("EUC_STUDIO_PERF").ok().and_then(|v| v.parse().ok()),
    })
}

/// Native output opens before firmware execution. The renderer only changes its preference.
#[tauri::command]
pub fn engine_sound(state: State<'_, EngineState>, enabled: bool) -> Result<native_audio::AudioStatus, String> {
    with_runner(&state, |runner| runner.set_sound(enabled))
}

/// The master volume (0 to 1) of the buzzer and the ride bed, and whether the bed sounds.
#[tauri::command]
pub fn engine_sound_mix(state: State<'_, EngineState>, volume: f64, bed: bool) -> Result<(), String> {
    with_runner(&state, |runner| runner.set_sound_mix(volume, bed))?
}

#[tauri::command]
pub fn engine_audio_status(state: State<'_, EngineState>) -> Result<native_audio::AudioStatus, String> {
    with_runner(&state, Runner::audio_status)
}

#[tauri::command]
pub fn engine_play(state: State<'_, EngineState>, playing: bool) -> Result<(), String> {
    with_runner(&state, |r| r.play(playing))
}

#[tauri::command]
pub fn engine_step(state: State<'_, EngineState>) -> Result<(), String> {
    with_runner(&state, Runner::step)
}

#[tauri::command]
pub fn engine_set_rtf(state: State<'_, EngineState>, rtf: f64) -> Result<(), String> {
    if !rtf.is_finite() || rtf <= 0.0 {
        return Err(format!("real-time factor must be positive, got {rtf}"));
    }
    with_runner(&state, |r| r.set_rtf(rtf))
}

#[tauri::command]
pub fn engine_restart(state: State<'_, EngineState>, scenario: Option<String>) -> Result<(), String> {
    if let Some(s) = &scenario
        && !scenarios::infos().iter().any(|i| i.id == *s)
    {
        return Err(format!("unknown scenario \"{s}\""));
    }
    with_runner(&state, |r| r.restart(scenario))
}

/// What the keyboard or a gamepad asks of the rider: lean and turn, each -1..1 of what the rider
/// can do (forward and left positive).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ManualInput {
    lean: f64,
    steer: f64,
}

/// Takes manual control of the rider, or gives it back to the scenario (`null`).
#[tauri::command]
pub fn engine_manual(state: State<'_, EngineState>, manual: Option<ManualInput>) -> Result<(), String> {
    if let Some(m) = manual
        && !(m.lean.is_finite() && m.steer.is_finite())
    {
        return Err(format!("manual input must be numbers, got lean {} and steer {}", m.lean, m.steer));
    }
    with_runner(&state, |r| r.manual(manual.map(|m| euc_sim::Manual { lean: m.lean, steer: m.steer })))
}

/// The rider steps off the pedals (the wheel goes on alone, as the firmware lets it).
#[tauri::command]
pub fn engine_step_off(state: State<'_, EngineState>) -> Result<(), String> {
    with_runner(&state, Runner::step_off)
}

/// Sends a phone command. Never-send commands do not exist here (see `phone`).
#[tauri::command]
pub fn engine_send(state: State<'_, EngineState>, id: String, value: Option<u8>) -> Result<String, String> {
    let command = phone::command(&id, value)?;
    let bytes = phone::encode(&command)?;
    let shown: String = bytes.iter().map(|&(_, b)| char::from(b)).collect();
    let label = format!("Phone sent {} ({shown})", command.info().name);
    let answer = command.safety() == euc_proto_begode::Safety::Read;
    with_runner(&state, |r| r.send(bytes, label.clone(), answer))?;
    Ok(shown)
}

/// Selects a simulator image. It never writes to a physical wheel.
#[tauri::command]
pub fn engine_set_firmware(state: State<'_, EngineState>, firmware: String) -> Result<(), String> {
    images::validate_choice(&firmware)?;
    with_runner(&state, |r| r.set_firmware(firmware))
}

/// Queues a bounded physical input pulse. It does not connect to real hardware.
#[tauri::command]
pub fn engine_button(state: State<'_, EngineState>, button: String, duration_ms: u32) -> Result<String, String> {
    let button = buttons::Button::parse(&button)?;
    with_runner(&state, |runner| runner.button(button, duration_ms))?
}

/// Replaces one live GPIO state. Renew a held state every 100 ms; its wall lease is 250 ms.
/// The epoch is required for release too, so a stale view cannot release a replacement run.
#[tauri::command]
pub fn engine_button_state(
    state: State<'_, EngineState>,
    button: String,
    pressed: bool,
    epoch: u64,
) -> Result<String, String> {
    let button = buttons::Button::parse(&button)?;
    with_runner(&state, |runner| runner.button_state(button, pressed, epoch))??;
    Ok(if pressed { "Pressed" } else { "Released" }.into())
}

/// Imports a user-selected TOML scenario into this app session.
#[tauri::command]
pub fn engine_import_scenario(text: String) -> Result<scenarios::ScenarioInfo, String> {
    scenarios::import(text)
}
