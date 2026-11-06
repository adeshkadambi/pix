//! Polls `allgamedata` once a second while a game is in progress (SPEC §5.6). The API errors
//! during loading, so failures just mean "try again next second". Also owns this game's
//! summoner timers (SPEC §5.8), which the tab panel starts and clears.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use super::models::AllGameData;
use super::view::{self, WindowMode};
use crate::game::kills::{self, KillRecord};
use crate::game::purchases::PurchaseFeed;
use crate::game::spells::{self, TimerBook};
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
    /// Kills this game, by champion (never player names), for the review and the advisor.
    pub kills: Vec<KillRecord>,
    /// The client's game id (from the gameflow session), to link the review to match history.
    pub game_id: Option<u64>,
    /// How the advisor's top pick changed, newest first.
    advice_log: Vec<view::AdviceLog>,
    last_top_pick: Option<i32>,
    /// Checked once per game that there's build data for the champion being played.
    build_checked: bool,
    pub window_mode: Option<WindowMode>,
    feed: PurchaseFeed,
    timers: TimerBook,
    /// Latest snapshot, so a timer click can redraw at once.
    pub last_game: Option<AllGameData>,
}

/// Starts polling for a new game. Safe to call twice.
pub fn start(app: &AppHandle, game_id: Option<u64>) {
    let state = app.state::<AppState>();
    let mut rt = state.live.lock().unwrap();
    if rt.task.is_some() {
        return;
    }
    let window_mode = read_window_mode(app);
    *rt = LiveRuntime { window_mode, game_id, ..Default::default() };
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
    let mut first = true;
    loop {
        if let Some(game) = fetch(&client).await
            && game.me().is_some()
            && tick(&app, game, first).await
        {
            first = false;
        }
        tokio::time::sleep(TICK).await;
    }
}

/// Processes one snapshot. Returns false if it couldn't yet (data still loading).
async fn tick(app: &AppHandle, game: AllGameData, first: bool) -> bool {
    let state = app.state::<AppState>();
    let Some(dd) = state.ddragon() else { return false };
    {
        let mut rt = state.live.lock().unwrap();
        rt.feed.update(&game, &dd);
        rt.timers.prune(game.game_data.game_time);
        for e in &game.events.events {
            if !rt.kills.iter().any(|k| k.event_id == e.id)
                && let Some(k) = kills::record(e, &game)
            {
                rt.kills.push(k);
            }
        }
        rt.last_game = Some(game);
    }
    if first {
        overlay::game_started(app);
    }
    ensure_build(app);
    redraw(app).await
}

/// Once per game: if champ select left no build for the champion being played (Pix opened
/// mid-game or restarted), load the standard one so the Live screen and advisor still work.
fn ensure_build(app: &AppHandle) {
    let state = app.state::<AppState>();
    let (champion, position) = {
        let mut rt = state.live.lock().unwrap();
        if rt.build_checked {
            return;
        }
        let Some(game) = rt.last_game.as_ref().filter(|g| g.is_summoners_rift()) else { return };
        let Some(me) = game.me() else { return };
        let out = (me.ddragon_id().to_owned(), me.position.clone());
        rt.build_checked = true;
        out
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let (Some(dd), Some(http)) = (state.ddragon(), state.lcu_http()) else { return };
        let Ok(catalog) = profile::catalog(&app, &http).await else { return };
        let draft_role = state.draft.lock().unwrap().view.as_ref().and_then(|d| d.my_role);
        let role = match super::postgame::role_from_position(&position).or(draft_role) {
            Some(r) => r,
            None => {
                // Custom and practice games report no position: use the champion's main role.
                let patch = state.patch.lock().unwrap().clone().unwrap_or_default();
                let traits = state.builds.traits(&patch, &champion.to_uppercase()).await.unwrap_or_default();
                traits.role_rates.iter().max_by(|a, b| a.1.total_cmp(&b.1)).map(|r| r.0).unwrap_or(crate::builds::Role::Mid)
            }
        };
        crate::builds::draft::ensure_game_build(&app, &champion, role, &catalog, &dd).await;
        redraw(&app).await;
    });
}

/// Rebuilds the view from the latest snapshot and emits it if it changed.
async fn redraw(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let Some(dd) = state.ddragon() else { return false };
    let Some(http) = state.lcu_http() else { return false };
    let Ok(catalog) = profile::catalog(app, &http).await else { return false };
    let draft = state.draft.lock().unwrap().view.clone();
    let (features, advisor_on) = {
        let flags = &state.config.lock().unwrap().flags;
        (view::OverlayFeatures { timers: flags.overlay_timers, gold: flags.overlay_gold }, flags.live_advisor)
    };
    let build_set = if advisor_on { state.draft.lock().unwrap().build_set.clone() } else { None };
    let view = {
        let mut rt = state.live.lock().unwrap();
        let Some(game) = rt.last_game.clone() else { return false };
        let game = &game;
        let advisor = build_set.as_ref().map(|b| view::AdvisorInputs { build_set: b, kills: &rt.kills, log: &rt.advice_log });
        let mut view = view::build_view(game, draft.as_ref(), &rt.feed, &rt.timers, &catalog, &dd, rt.window_mode, features, advisor);
        // Log when the advisor's top pick changes.
        if let Some(advice) = view.advice.as_mut() {
            let top = advice.picks.first().map(|p| p.item.id);
            if top.is_some() && top != rt.last_top_pick {
                let p = &advice.picks[0];
                let t = game.game_data.game_time;
                let text = format!("{} → {} is now the top pick", p.reason, p.item.name);
                rt.advice_log.insert(0, view::AdviceLog { game_time: t, text });
                rt.advice_log.truncate(5);
                advice.log = rt.advice_log.clone();
            }
            rt.last_top_pick = top;
        }
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

fn timers_enabled(app: &AppHandle) -> Result<(), String> {
    if app.state::<AppState>().config.lock().unwrap().flags.overlay_timers {
        Ok(())
    } else {
        Err("Summoner timers are turned off in Settings.".into())
    }
}

/// Starts a spell's timer from the enemy's haste right now, or clears it if it's running.
pub async fn toggle_timer(app: &AppHandle, champion: &str, slot: u8) -> Result<(), String> {
    timers_enabled(app)?;
    let state = app.state::<AppState>();
    let dd = state.ddragon().ok_or("champion data isn't loaded yet")?;
    {
        let mut rt = state.live.lock().unwrap();
        let game = rt.last_game.clone().ok_or("no game running")?;
        let enemy = game.enemies().find(|p| p.ddragon_id() == champion).ok_or("unknown enemy")?;
        let spell = match slot {
            0 => &enemy.summoner_spells.summoner_spell_one,
            1 => &enemy.summoner_spells.summoner_spell_two,
            _ => return Err("bad spell slot".into()),
        };
        let key = spell.key().unwrap_or_default();
        let info = spells::spell_info(key, &dd).filter(|_| spells::trackable(key)).ok_or("this spell isn't timed")?;
        let haste = view::enemy_haste(enemy, &rt.timers, &dd);
        let duration = spells::cooldown(info.cooldown, haste);
        rt.timers.toggle(champion, slot, game.game_data.game_time, duration);
    }
    redraw(app).await;
    Ok(())
}

pub async fn set_cosmic(app: &AppHandle, champion: &str, on: bool) -> Result<(), String> {
    timers_enabled(app)?;
    app.state::<AppState>().live.lock().unwrap().timers.set_cosmic(champion, on);
    redraw(app).await;
    Ok(())
}
