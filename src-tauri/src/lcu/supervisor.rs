//! Keeps a connection to the League client: find the lockfile, connect, follow the gameflow
//! phase, and tear down when the client closes or restarts (SPEC §5.1).

use std::path::Path;
use std::time::Duration;

use futures_util::StreamExt;
use tauri::{AppHandle, Manager};
use tokio_tungstenite::tungstenite::Message;

use super::gameflow::{is_summoners_rift, parse_phase};
use super::http::LcuHttp;
use super::lockfile::Lockfile;
use super::{discovery, ws};
use crate::events::{ClientStatus, Phase};
use crate::state::{AppState, update_client_status};
use crate::{ddragon, profile, riot_tls};

const POLL: Duration = Duration::from_secs(2);
const PHASE_URI: &str = "/lol-gameflow/v1/gameflow-phase";

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(supervise(app));
}

async fn supervise(app: AppHandle) {
    let client = riot_tls::local_http_client();
    loop {
        let override_path = app.state::<AppState>().config.lock().unwrap().install_path.clone();
        if let Some(lock_path) = discovery::locate(override_path.as_deref())
            && let Ok(text) = tokio::fs::read_to_string(&lock_path).await
        {
            match Lockfile::parse(&text) {
                Ok(lockfile) => {
                    let http = LcuHttp::new(client.clone(), &lockfile);
                    if run_session(&app, &http, &lockfile, &lock_path, &text).await {
                        disconnected(&app).await;
                    }
                }
                Err(e) => tracing::debug!("lockfile not usable yet: {e}"),
            }
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Runs one connection. Returns false if it never connected (client still starting).
async fn run_session(app: &AppHandle, http: &LcuHttp, lockfile: &Lockfile, lock_path: &Path, lock_text: &str) -> bool {
    let mut socket = match ws::connect(lockfile).await {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!("League client not ready: {e}");
            return false;
        }
    };
    tracing::info!("connected to League client on port {}", lockfile.port);
    *app.state::<AppState>().lcu.lock().unwrap() = Some(http.clone());

    let phase = http.get_json::<String>(PHASE_URI).await.map(|s| parse_phase(&s)).unwrap_or_default();
    update_client_status(app, |s| {
        s.connected = true;
        s.phase = phase;
    });
    on_phase_change(app, http, None, phase).await;
    profile::refresh_soon(app);
    load_ddragon_for_client(app, http);

    let mut tick = tokio::time::interval(POLL);
    loop {
        tokio::select! {
            frame = socket.next() => match frame {
                Some(Ok(Message::Text(text))) => {
                    if let Some(event) = ws::parse_frame(&text)
                        && event.uri == PHASE_URI
                    {
                        let new = event.data.as_str().map(parse_phase).unwrap_or_default();
                        let old = app.state::<AppState>().client.lock().unwrap().phase;
                        if new != old {
                            update_client_status(app, |s| s.phase = new);
                            on_phase_change(app, http, Some(old), new).await;
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Err(e)) => {
                    tracing::info!("League client socket closed: {e}");
                    break;
                }
                Some(Ok(_)) => {}
            },
            _ = tick.tick() => {
                // A changed or missing lockfile means the client closed or restarted.
                match tokio::fs::read_to_string(lock_path).await {
                    Ok(text) if text == lock_text => {}
                    _ => break,
                }
            }
        }
    }
    true
}

async fn on_phase_change(app: &AppHandle, http: &LcuHttp, old: Option<Phase>, new: Phase) {
    tracing::info!("gameflow phase: {new:?}");
    // Compliance rule 6: build data only on Summoner's Rift classic games.
    let rift = if matches!(new, Phase::ChampSelect | Phase::InProgress) {
        match http.get_json::<serde_json::Value>("/lol-gameflow/v1/session").await {
            Ok(session) => Some(is_summoners_rift(&session)),
            Err(e) => {
                tracing::warn!("couldn't read gameflow session: {e}");
                Some(false)
            }
        }
    } else {
        None
    };
    update_client_status(app, |s| s.summoners_rift = rift);

    // Game log (SPEC §5.12): a rank snapshot before the game, and the LP notification after it.
    if new == Phase::ChampSelect {
        profile::snapshot_soon(app);
    }
    if new == Phase::EndOfGame {
        profile::log_lp_notification(app);
    }
    // A finished game shows up in match history once you're back in the lobby or home screen.
    if old == Some(Phase::EndOfGame) && matches!(new, Phase::Lobby | Phase::None) {
        profile::log_lp_notification(app);
        profile::refresh_soon(app);
        // The client can update rank a bit after the lobby returns; a later refresh catches that
        // so the game still gets its LP change.
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            profile::refresh_soon(&app);
        });
    }
}

async fn disconnected(app: &AppHandle) {
    tracing::info!("League client disconnected");
    let state = app.state::<AppState>();
    *state.lcu.lock().unwrap() = None;
    *state.catalog.lock().await = None;
    update_client_status(app, |s| *s = ClientStatus::default());
}

/// Loads Data Dragon for the client's patch in the background.
fn load_ddragon_for_client(app: &AppHandle, http: &LcuHttp) {
    let (app, http) = (app.clone(), http.clone());
    tauri::async_runtime::spawn(async move {
        let patch = http.get_json::<String>("/lol-patch/v1/game-version").await.ok().and_then(|v| ddragon::patch_of(&v));
        let current = app.state::<AppState>().ddragon();
        if let (Some(p), Some(dd)) = (&patch, &current)
            && dd.version.starts_with(&format!("{p}."))
        {
            return;
        }
        load_ddragon(&app, patch.as_deref()).await;
    });
}

/// Loads Data Dragon into state; logs and keeps the old data on failure.
pub async fn load_ddragon(app: &AppHandle, patch: Option<&str>) {
    let state = app.state::<AppState>();
    let root = state.cache_dir.join("ddragon");
    match ddragon::load(&state.public_http, &root, patch).await {
        Ok(data) => {
            tracing::info!("Data Dragon {} ready", data.version);
            *state.ddragon.write().unwrap() = Some(std::sync::Arc::new(data));
        }
        Err(e) => tracing::warn!("Data Dragon unavailable: {e}"),
    }
}
