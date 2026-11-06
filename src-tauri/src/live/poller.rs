//! Polls `allgamedata` once a second while a game is in progress (SPEC §5.6). The API errors
//! during loading, so failures just mean "try again next second".

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use super::models::{AllGameData, Event};
use super::view::{self, WindowMode};
use crate::game::purchases::PurchaseFeed;
use crate::overlay;
use crate::state::AppState;
use crate::{profile, riot_tls};

pub const LIVE_EVENT: &str = "live";
const URL: &str = "https://127.0.0.1:2999/liveclientdata/allgamedata";
const TICK: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct LiveRuntime {
    task: Option<tauri::async_runtime::JoinHandle<()>>,
    pub view: Option<view::LiveView>,
    /// Kill events recorded this game, for the post-game review (M5). Memory only.
    pub kills: Vec<Event>,
    pub window_mode: Option<WindowMode>,
}

/// Starts polling for a new game. Safe to call twice.
pub fn start(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut rt = state.live.lock().unwrap();
    if rt.task.is_some() {
        return;
    }
    rt.view = None;
    rt.kills.clear();
    rt.window_mode = read_window_mode(app);
    let app = app.clone();
    rt.task = Some(tauri::async_runtime::spawn(async move { run(app).await }));
    tracing::info!("live game polling started");
}

/// Stops polling (game over or client gone). The last view stays for the Live screen.
pub fn stop(app: &AppHandle) {
    if let Some(task) = app.state::<AppState>().live.lock().unwrap().task.take() {
        task.abort();
        tracing::info!("live game polling stopped");
    }
    overlay::game_ended(app);
}

fn read_window_mode(app: &AppHandle) -> Option<WindowMode> {
    let install = app.state::<AppState>().install_dir.lock().unwrap().clone()?;
    let cfg = std::fs::read_to_string(install.join("Config").join("game.cfg")).ok()?;
    view::parse_window_mode(&cfg)
}

async fn fetch(client: &reqwest::Client) -> Option<AllGameData> {
    #[cfg(debug_assertions)]
    if let Ok(path) = std::env::var("PIX_LIVE_FIXTURE") {
        return serde_json::from_str(&tokio::fs::read_to_string(path).await.ok()?).ok();
    }
    client.get(URL).send().await.ok()?.error_for_status().ok()?.json().await.ok()
}

async fn run(app: AppHandle) {
    let client = riot_tls::local_http_client();
    let mut feed = PurchaseFeed::default();
    let mut first = true;
    loop {
        if let Some(game) = fetch(&client).await
            && game.me().is_some()
            && tick(&app, &game, &mut feed, first).await
        {
            first = false;
        }
        tokio::time::sleep(TICK).await;
    }
}

/// Processes one snapshot. Returns false if it couldn't yet (data still loading).
async fn tick(app: &AppHandle, game: &AllGameData, feed: &mut PurchaseFeed, first: bool) -> bool {
    let state = app.state::<AppState>();
    let Some(dd) = state.ddragon() else { return false };
    let Some(http) = state.lcu_http() else { return false };
    let Ok(catalog) = profile::catalog(app, &http).await else { return false };
    feed.update(game, &dd);
    if first {
        overlay::game_started(app);
    }

    let draft = state.draft.lock().unwrap().view.clone();
    let view = {
        let mut rt = state.live.lock().unwrap();
        for e in game.events.events.iter().filter(|e| e.name == "ChampionKill") {
            if !rt.kills.iter().any(|k| k.id == e.id) {
                rt.kills.push(e.clone());
            }
        }
        let view = view::build_view(game, draft.as_ref(), feed, &catalog, &dd, rt.window_mode);
        if rt.view.as_ref() == Some(&view) {
            return true;
        }
        rt.view = Some(view.clone());
        view
    };
    if let Err(e) = app.emit(LIVE_EVENT, &view) {
        tracing::warn!("failed to emit {LIVE_EVENT}: {e}");
    }
    true
}
