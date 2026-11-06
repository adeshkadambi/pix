//! Pix backend: app setup, shared state and the commands the frontend calls.

mod config;
mod events;
mod overlay;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, Manager, State, WebviewWindowBuilder, WindowEvent};

use config::{Config, Flag};
use events::ClientStatus;

pub struct AppState {
    config: Mutex<Config>,
    config_path: PathBuf,
    client: Mutex<ClientStatus>,
}

#[tauri::command]
fn get_config(state: State<'_, AppState>) -> Config {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
fn set_flag(
    app: AppHandle,
    state: State<'_, AppState>,
    flag: Flag,
    on: bool,
) -> Result<Config, String> {
    let mut config = state.config.lock().unwrap();
    let mut next = config.clone();
    next.set_flag(flag, on);
    next.save(&state.config_path).map_err(|e| format!("couldn't save settings: {e}"))?;
    *config = next.clone();
    drop(config);
    events::emit_config_changed(&app, &next);
    Ok(next)
}

#[tauri::command]
fn get_client_status(state: State<'_, AppState>) -> ClientStatus {
    state.client.lock().unwrap().clone()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pix=info,pix_lib=info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config_path = app.path().app_config_dir()?.join(config::FILE_NAME);
            let config = Config::load(&config_path);
            tracing::info!("config loaded from {}", config_path.display());
            app.manage(AppState {
                config: Mutex::new(config),
                config_path,
                client: Mutex::new(ClientStatus::default()),
            });
            // Windows are marked `"create": false` in tauri.conf.json and built here, after the
            // state exists, so a page can't call a command before its state is managed.
            for window in app.config().app.windows.clone() {
                WebviewWindowBuilder::from_config(app.handle(), &window)?.build()?;
            }
            overlay::setup(app.handle())?;
            let status = app.state::<AppState>().client.lock().unwrap().clone();
            events::emit_client_status(app.handle(), &status);
            Ok(())
        })
        .on_window_event(|window, event| {
            // The overlays are hidden helpers; closing the companion quits the whole app.
            if window.label() == "companion" && matches!(event, WindowEvent::CloseRequested { .. }) {
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![get_config, set_flag, get_client_status])
        .run(tauri::generate_context!())
        .expect("error while running Pix");
}
