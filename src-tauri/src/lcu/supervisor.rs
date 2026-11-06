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
use crate::builds::draft;
use crate::events::{ClientStatus, Phase};
use crate::state::{AppState, update_client_status};
use crate::{ddragon, live, profile, riot_tls};

const POLL: Duration = Duration::from_secs(2);
const PHASE_URI: &str = "/lol-gameflow/v1/gameflow-phase";
const CHAMP_SELECT_URI: &str = "/lol-champ-select/v1/session";

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
    *app.state::<AppState>().install_dir.lock().unwrap() = lock_path.parent().map(Path::to_path_buf);

    let phase = http.get_json::<String>(PHASE_URI).await.map(|s| parse_phase(&s)).unwrap_or_default();
    update_client_status(app, |s| {
        s.connected = true;
        s.phase = phase;
    });
    on_phase_change(app, http, None, phase).await;
    profile::refresh_soon(app);
    load_ddragon_for_client(app, http);
    #[cfg(debug_assertions)]
    load_draft_fixture(app);

    let mut tick = tokio::time::interval(POLL);
    loop {
        tokio::select! {
            frame = socket.next() => match frame {
                Some(Ok(Message::Text(text))) => {
                    let Some(event) = ws::parse_frame(&text) else { continue };
                    if event.uri == PHASE_URI {
                        let new = event.data.as_str().map(parse_phase).unwrap_or_default();
                        let old = app.state::<AppState>().client.lock().unwrap().phase;
                        if new != old {
                            update_client_status(app, |s| s.phase = new);
                            on_phase_change(app, http, Some(old), new).await;
                        }
                    } else if event.uri == CHAMP_SELECT_URI && event.event_type != "Delete" {
                        draft::on_session(app, &event.data);
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
    let session = if matches!(new, Phase::ChampSelect | Phase::InProgress) {
        match http.get_json::<serde_json::Value>("/lol-gameflow/v1/session").await {
            Ok(session) => Some(session),
            Err(e) => {
                tracing::warn!("couldn't read gameflow session: {e}");
                None
            }
        }
    } else {
        None
    };
    let rift = matches!(new, Phase::ChampSelect | Phase::InProgress).then(|| session.as_ref().is_some_and(is_summoners_rift));
    update_client_status(app, |s| s.summoners_rift = rift);

    // Live game data while a game runs (SPEC §5.6).
    if new == Phase::InProgress {
        let game_id = session.as_ref().and_then(|s| s.pointer("/gameData/gameId")).and_then(serde_json::Value::as_u64);
        live::poller::start(app, game_id);
    } else if old == Some(Phase::InProgress) {
        live::poller::stop(app);
        // The game ended: build and save its review (SPEC §5.9).
        live::postgame::review_soon(app);
    }

    // Draft pipeline: a fresh draft per champ select, seeded from the current session.
    if new == Phase::ChampSelect {
        draft::reset(app);
        match http.get_json::<serde_json::Value>(CHAMP_SELECT_URI).await {
            Ok(session) => draft::on_session(app, &session),
            Err(e) => tracing::debug!("no champ select session yet: {e}"),
        }
    }

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
    live::poller::stop(app);
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
        *app.state::<AppState>().patch.lock().unwrap() = patch.clone();
        let current = app.state::<AppState>().ddragon();
        if let (Some(p), Some(dd)) = (&patch, &current)
            && dd.version.starts_with(&format!("{p}."))
        {
            return;
        }
        load_ddragon(&app, patch.as_deref()).await;
    });
}

/// Debug builds only: `PIX_DRAFT_FIXTURE=<session.json>` feeds a recorded champ select session into
/// the draft pipeline (treated as Summoner's Rift), to work on the Draft screen without a real draft.
#[cfg(debug_assertions)]
fn load_draft_fixture(app: &AppHandle) {
    // `PIX_LIVE_FIXTURE=<allgamedata.json>` replays one Live Client snapshot as a running game.
    if std::env::var("PIX_LIVE_FIXTURE").is_ok() {
        live::poller::start(app, Some(4242));
    }
    let Ok(path) = std::env::var("PIX_DRAFT_FIXTURE") else { return };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Give Data Dragon a moment to load first.
        tokio::time::sleep(Duration::from_secs(2)).await;
        match tokio::fs::read_to_string(&path).await.map(|t| serde_json::from_str::<serde_json::Value>(&t)) {
            Ok(Ok(session)) => {
                tracing::info!("draft fixture loaded from {path}");
                draft::reset(&app);
                app.state::<AppState>().draft.lock().unwrap().force_rift = true;
                draft::on_session(&app, &session);
            }
            Ok(Err(e)) => tracing::warn!("draft fixture isn't JSON: {e}"),
            Err(e) => tracing::warn!("draft fixture unreadable: {e}"),
        }
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
