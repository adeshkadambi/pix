//! Pix backend: app setup and the commands the frontend calls.

mod asset_protocol;
mod config;
mod db;
mod ddragon;
mod events;
mod lcu;
mod lp;
mod overlay;
mod profile;
mod riot_tls;
mod state;

use tauri::{AppHandle, Manager, State, WebviewWindowBuilder, WindowEvent};

use config::{Config, Flag};
use events::ClientStatus;
use profile::ProfileView;
use state::AppState;

#[tauri::command]
fn get_config(state: State<'_, AppState>) -> Config {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
fn set_flag(app: AppHandle, state: State<'_, AppState>, flag: Flag, on: bool) -> Result<Config, String> {
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

#[tauri::command]
fn get_profile(state: State<'_, AppState>) -> Option<ProfileView> {
    state.profile.lock().unwrap().clone()
}

#[tauri::command]
async fn refresh_profile(app: AppHandle) -> Result<ProfileView, String> {
    profile::refresh(&app).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pix=info,pix_lib=info".into()),
        )
        .init();
    riot_tls::install_crypto_provider();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .register_asynchronous_uri_scheme_protocol(asset_protocol::SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                responder.respond(asset_protocol::handle(&app, request).await);
            });
        })
        .setup(|app| {
            let config_path = app.path().app_config_dir()?.join(config::FILE_NAME);
            let config = Config::load(&config_path);
            tracing::info!("config loaded from {}", config_path.display());
            let cache_dir = app.path().app_cache_dir()?;
            let db_path = app.path().app_data_dir()?.join("pix.db");
            let db = match db::Db::open(&db_path) {
                Ok(db) => Some(db),
                Err(e) => {
                    tracing::error!("game log disabled, couldn't open {}: {e}", db_path.display());
                    None
                }
            };
            app.manage(AppState::new(config, config_path, cache_dir, db, riot_tls::public_http_client()));

            // Windows are marked `"create": false` in tauri.conf.json and built here, after the
            // state exists, so a page can't call a command before its state is managed.
            for window in app.config().app.windows.clone() {
                WebviewWindowBuilder::from_config(app.handle(), &window)?.build()?;
            }
            overlay::setup(app.handle())?;

            // Data Dragon for icon fallbacks, then keep a League client connection.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                lcu::supervisor::load_ddragon(&handle, None).await;
            });
            lcu::supervisor::spawn(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // The overlays are hidden helpers; closing the companion quits the whole app.
            if window.label() == "companion" && matches!(event, WindowEvent::CloseRequested { .. }) {
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_flag,
            get_client_status,
            get_profile,
            refresh_profile
        ])
        .run(tauri::generate_context!())
        .expect("error while running Pix");
}
