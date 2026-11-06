//! Builds the post-game review when a game ends (SPEC §5.9) and saves it to the game log.

use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, Manager};

use crate::builds::Role;
use crate::builds::comp_profile::{self, EnemyChampion};
use crate::builds::draft::SituationalView;
use crate::ddragon::DData;
use crate::game::kills::Actor;
use crate::game::review::{self, CounterView, DeathGroup, DeathMark, ReviewView};
use crate::lcu::catalog::{Catalog, Icon};
use crate::profile::{self, MatchResult};
use crate::state::AppState;

pub const REVIEW_EVENT: &str = "review";
const COUNTERED_KILLERS: usize = 2;

fn actor_icon(a: &Actor, catalog: &Catalog, dd: &DData) -> Icon {
    match a {
        Actor::Champion(id) => match dd.champion_key(id) {
            Some(key) => catalog.champion(key),
            None => Icon { id: 0, name: id.clone(), icon: String::new() },
        },
        Actor::Other(label) => Icon { id: 0, name: label.clone(), icon: String::new() },
    }
}

pub(crate) fn role_from_position(position: &str) -> Option<Role> {
    Role::from_lcu(match position.to_ascii_uppercase().as_str() {
        "MIDDLE" => "middle",
        "BOTTOM" => "bottom",
        "UTILITY" => "utility",
        other => return Role::from_lcu(&other.to_ascii_lowercase()),
    })
}

/// Counter items for one killer: OP.GG data for your champion vs. theirs, else your overall
/// build, filtered to items that answer that champion.
async fn counter(app: &AppHandle, my_id: &str, role: Role, killer: &str, catalog: &Catalog, dd: &DData) -> Option<CounterView> {
    let state = app.state::<AppState>();
    let service = state.builds.clone();
    let patch = state.patch.lock().unwrap().clone().unwrap_or_default();
    let (mine, theirs) = (my_id.to_uppercase(), killer.to_uppercase());
    let (build, matchup) = match service.matchup(&patch, &mine, role, &theirs).await {
        Ok(b) if !b.core_sets.is_empty() => (b, true),
        _ => (service.base(&patch, &mine, role).await.ok()?, false),
    };
    let traits = service.traits(&patch, &theirs).await.unwrap_or_default();
    let info = dd.champion_key(killer).and_then(|k| dd.champions.get(&k));
    let enemy = EnemyChampion {
        name: info.map(|c| c.name.clone()).unwrap_or_else(|| killer.to_owned()),
        id: killer.to_owned(),
        tags: info.map(|c| c.tags.clone()).unwrap_or_default(),
        damage: traits.damage_type,
    };
    let items = comp_profile::threat_answers(&build, &enemy, dd)
        .iter()
        .map(|p| SituationalView {
            item: catalog.item_icon(p.item_id as i32),
            reason: p.reason.clone(),
            play: p.ranked.play,
            win: p.ranked.win,
            smoothed_wr: p.ranked.smoothed_wr,
        })
        .collect();
    Some(CounterView {
        killer: actor_icon(&Actor::Champion(killer.to_owned()), catalog, dd),
        games: if matchup { build.champion_games } else { 0 },
        win_rate: matchup.then_some(build.champion_wr),
        items,
    })
}

async fn build(app: &AppHandle) -> Result<ReviewView, String> {
    let state = app.state::<AppState>();
    let (game, kills, game_id) = {
        let rt = state.live.lock().unwrap();
        (rt.last_game.clone().ok_or("no game data was recorded")?, rt.kills.clone(), rt.game_id)
    };
    let me = game.me().ok_or("couldn't find you in the game data")?;
    let dd = state.ddragon().ok_or("champion data isn't loaded")?;
    let http = state.lcu_http().ok_or("League client isn't connected")?;
    let catalog = profile::catalog(app, &http).await.map_err(|e| e.to_string())?;

    let groups = review::death_groups(&kills);
    let mut counters = Vec::new();
    // Compliance rule 6: build data only for Summoner's Rift games.
    if game.is_summoners_rift() {
        let role = state
            .draft
            .lock()
            .unwrap()
            .view
            .as_ref()
            .and_then(|d| d.my_role)
            .or_else(|| role_from_position(&me.position))
            .unwrap_or(Role::Mid);
        for killer in review::top_killers(&kills, COUNTERED_KILLERS) {
            if let Some(c) = counter(app, me.ddragon_id(), role, &killer, &catalog, &dd).await {
                counters.push(c);
            }
        }
    }

    let result = game.events.events.iter().find(|e| e.name == "GameEnd").map(|e| {
        if e.result.eq_ignore_ascii_case("win") { MatchResult::Win } else { MatchResult::Loss }
    });
    Ok(ReviewView {
        game_id: game_id.unwrap_or(0),
        created_ms: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
        champion: actor_icon(&Actor::Champion(me.ddragon_id().to_owned()), &catalog, &dd),
        result,
        kills: me.scores.kills,
        deaths: me.scores.deaths,
        assists: me.scores.assists,
        cs: me.scores.creep_score,
        duration_secs: game.game_data.game_time as u32,
        deaths_by: groups
            .iter()
            .map(|(killer, count, assists)| DeathGroup {
                killer: actor_icon(killer, &catalog, &dd),
                count: *count,
                assists: assists.iter().map(|a| actor_icon(&Actor::Champion(a.clone()), &catalog, &dd)).collect(),
            })
            .collect(),
        timeline: kills
            .iter()
            .filter(|k| k.victim_is_me)
            .map(|k| DeathMark { time: k.time, killer: actor_icon(&k.killer, &catalog, &dd) })
            .collect(),
        counters,
        lp_change: None,
    })
}

/// Builds the review in the background, saves it (when the game id is known) and shows it.
pub fn review_soon(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let review = match build(&app).await {
            Ok(r) => r,
            Err(e) => return tracing::info!("no post-game review: {e}"),
        };
        let state = app.state::<AppState>();
        let puuid = state.puuid.lock().unwrap().clone();
        if review.game_id != 0
            && let (Some(db), Some(puuid)) = (state.db.as_ref(), puuid)
            && let Err(e) = db.save_review(&puuid, &review)
        {
            tracing::warn!("review not saved: {e}");
        }
        tracing::info!("post-game review ready ({} deaths, {} counters)", review.deaths, review.counters.len());
        *state.review.lock().unwrap() = Some(review.clone());
        if let Err(e) = app.emit(REVIEW_EVENT, &review) {
            tracing::warn!("failed to emit {REVIEW_EVENT}: {e}");
        }
    });
}

/// A saved review (`game_id`), or the latest one.
pub fn get(app: &AppHandle, game_id: Option<u64>) -> Option<ReviewView> {
    let state = app.state::<AppState>();
    let puuid = state.puuid.lock().unwrap().clone();
    if let (Some(db), Some(puuid)) = (state.db.as_ref(), puuid)
        && let Ok(Some(r)) = db.review(&puuid, game_id)
    {
        return Some(r);
    }
    state.review.lock().unwrap().clone().filter(|r| game_id.is_none_or(|g| g == r.game_id))
}
