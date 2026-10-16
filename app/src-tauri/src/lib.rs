//! EUC Studio desktop shell. M0 hosts the web UI only; the simulation engine (`euc-sim`) is wired
//! in at M1 through an `EngineClient` behind Tauri commands and binary IPC channels.

use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: &'static str,
    engine: &'static str,
}

/// Lets the UI show which engine it is talking to.
#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo { version: env!("CARGO_PKG_VERSION"), engine: "demo" }
}

/// Web view errors and warnings, printed where the app was started (WKWebView and WebView2 keep
/// their consoles to themselves).
#[tauri::command]
fn frontend_log(level: String, message: String) {
    eprintln!("[webview {level}] {message}");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .on_page_load(|webview, payload| {
            eprintln!("[page] {:?} {}", payload.event(), webview.url().map(|u| u.to_string()).unwrap_or_default());
        })
        .invoke_handler(tauri::generate_handler![app_info, frontend_log])
        .run(tauri::generate_context!())
        .expect("error while running EUC Studio");
}
