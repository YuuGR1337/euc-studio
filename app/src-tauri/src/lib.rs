//! EUC Studio desktop shell: the web UI plus the live engine (`engine`), which runs the real
//! firmware in the simulator on its own thread and streams telemetry to the web view.

mod engine;
use tauri::Manager;

/// Web view errors and warnings, printed where the app was started (WKWebView and WebView2 keep
/// their consoles to themselves).
#[tauri::command]
fn frontend_log(level: String, message: String) {
    eprintln!("[webview {level}] {message}");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // "Open recording…" asks for the file with the system's open panel.
        .plugin(tauri_plugin_dialog::init())
        .manage(engine::EngineState::default())
        .manage(engine::recordings::RecordingState::default())
        .manage(engine::compare::CompareState::default())
        .manage(engine::free_ride::FreeRideState::default())
        .setup(|_app| {
            if let Err(error) = _app
                .path()
                .app_cache_dir()
                .map_err(|e| e.to_string())
                .and_then(|dir| _app.state::<engine::recordings::RecordingState>().enable_autocapture(dir))
            {
                eprintln!("[recordings] Automatic capture unavailable: {error}");
            }
            if let Ok(resources) = _app.path().resource_dir() {
                engine::images::set_resource_directory(resources);
            }
            // EUC_STUDIO_CONTROL: drive the run from a file (dev builds only, see engine::control).
            #[cfg(dev)]
            engine::control::spawn(_app.handle().clone());
            Ok(())
        })
        .on_page_load(|webview, payload| {
            eprintln!("[page] {:?} {}", payload.event(), webview.url().map(|u| u.to_string()).unwrap_or_default());
        })
        .invoke_handler(tauri::generate_handler![
            frontend_log,
            engine::engine_connect,
            engine::engine_play,
            engine::engine_step,
            engine::engine_set_rtf,
            engine::engine_restart,
            engine::engine_send,
            engine::engine_manual,
            engine::engine_step_off,
            engine::recordings::recordings_open,
            engine::recordings::recordings_start,
            engine::recordings::recordings_save,
            engine::recordings::recordings_status,
            engine::recordings::recordings_suggest_filename,
            engine::engine_sound,
            engine::engine_sound_mix,
            engine::engine_audio_status,
            engine::engine_set_firmware,
            engine::engine_import_scenario,
            engine::engine_button,
            engine::engine_button_state,
            engine::free_ride::free_ride_start,
            engine::free_ride::free_ride_restart,
            engine::free_ride::free_ride_step_off,
            engine::free_ride::free_ride_status,
            engine::free_ride::free_ride_map,
            engine::free_ride::free_ride_arm,
            engine::free_ride::free_ride_input,
            engine::free_ride::free_ride_pause,
            engine::free_ride::free_ride_world,
            engine::world::world_chunks,
            engine::world::world_origin_z,
            engine::tune::engine_tune_catalog,
            engine::tune::engine_tune_snapshot,
            engine::compare::compare_info,
            engine::compare::compare_start,
            engine::compare::compare_stop,
        ])
        .run(tauri::generate_context!())
        .expect("error while running EUC Studio");
}
