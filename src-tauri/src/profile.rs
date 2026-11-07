//! Native profile view (SPEC §5.10): your own summoner, ranked stats and match history from the
//! League client, merged with the local game log (§5.12) for older games and LP changes.
//! Only your own account is ever read.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use ts_rs::TS;

use crate::events::PROFILE;
use crate::lcu::catalog::{Catalog, Icon};
use crate::lcu::http::{LcuError, LcuHttp};
use crate::lp::{self, LpChange};
use crate::state::AppState;

const TOP_CHAMPIONS: usize = 5;
/// Games shown on the Profile screen.
const HISTORY_LIMIT: usize = 100;

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct ProfileView {
    pub game_name: String,
    pub tag_line: String,
    pub region: String,
    pub level: u32,
    pub profile_icon: String,
    pub solo: Option<RankedQueue>,
    pub flex: Option<RankedQueue>,
    /// Newest first: the client's recent games plus older ones from the local game log.
    pub matches: Vec<MatchSummary>,
    /// Ranked champions across the stored history, most played first.
    pub champions: Vec<ChampionStats>,
    #[ts(type = "number")]
    pub fetched_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct RankedQueue {
    /// e.g. "SILVER". Unranked queues are left out of the view entirely.
    pub tier: String,
    /// e.g. "III"; empty for Master and above.
    pub division: String,
    pub lp: i32,
    pub wins: u32,
    pub losses: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum MatchResult {
    Win,
    Loss,
    Remake,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MatchSummary {
    #[ts(type = "number")]
    pub game_id: u64,
    pub champion: Icon,
    pub queue: String,
    #[serde(default)]
    pub queue_id: i32,
    pub result: MatchResult,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub cs: u32,
    pub duration_secs: u32,
    #[ts(type = "number")]
    pub created_ms: u64,
    /// Six item slots (trinket excluded); `null` is an empty slot.
    pub items: Vec<Option<Icon>>,
    pub spells: Vec<Icon>,
    /// LP gained or lost, when the game log could prove it (§5.12).
    #[serde(default)]
    pub lp_change: Option<LpChange>,
    /// Pix saved a post-game review for this game.
    #[serde(default)]
    pub has_review: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ChampionStats {
    pub champion: Icon,
    pub games: u32,
    pub wins: u32,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
}

// ---- Raw client responses (only the fields we use) ----

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSummoner {
    puuid: String,
    game_name: String,
    tag_line: String,
    summoner_level: u32,
    profile_icon_id: i32,
}

#[derive(Debug, Deserialize)]
struct RawRegion {
    region: String,
}

#[derive(Debug, Default, Deserialize)]
struct RawRanked {
    #[serde(default)]
    queues: Vec<RawQueue>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawQueue {
    queue_type: String,
    tier: String,
    division: String,
    league_points: i32,
    wins: u32,
    losses: u32,
}

#[derive(Debug, Deserialize)]
struct RawHistory {
    games: RawGames,
}

#[derive(Debug, Deserialize)]
struct RawGames {
    #[serde(default)]
    games: Vec<RawGame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawGame {
    game_id: u64,
    game_creation: u64,
    game_duration: u32,
    queue_id: i32,
    participants: Vec<RawParticipant>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawParticipant {
    champion_id: i32,
    spell1_id: i32,
    spell2_id: i32,
    stats: RawStats,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawStats {
    win: bool,
    kills: u32,
    deaths: u32,
    assists: u32,
    #[serde(default)]
    total_minions_killed: u32,
    #[serde(default)]
    neutral_minions_killed: u32,
    #[serde(default)]
    game_ended_in_early_surrender: bool,
    #[serde(default)]
    item0: i32,
    #[serde(default)]
    item1: i32,
    #[serde(default)]
    item2: i32,
    #[serde(default)]
    item3: i32,
    #[serde(default)]
    item4: i32,
    #[serde(default)]
    item5: i32,
}

pub fn queue_name(queue_id: i32) -> &'static str {
    match queue_id {
        420 => "Ranked Solo",
        440 => "Ranked Flex",
        400 => "Normal Draft",
        430 => "Normal Blind",
        480 => "Swiftplay",
        490 => "Quickplay",
        450 => "ARAM",
        1700 | 1710 => "Arena",
        900 | 1010 | 1900 => "URF",
        830 | 840 | 850 | 870 | 880 | 890 => "Co-op vs AI",
        0 | 3100 | 3200 => "Custom",
        _ => "Other",
    }
}

fn ranked_queue(ranked: &RawRanked, queue_type: &str) -> Option<RankedQueue> {
    let q = ranked.queues.iter().find(|q| q.queue_type == queue_type)?;
    if q.tier.is_empty() || q.tier == "NONE" {
        return None;
    }
    let division = if q.division == "NA" { String::new() } else { q.division.clone() };
    Some(RankedQueue { tier: q.tier.clone(), division, lp: q.league_points, wins: q.wins, losses: q.losses })
}

fn summarize(game: &RawGame, catalog: &Catalog) -> Option<MatchSummary> {
    // The current-summoner history endpoint lists only you as a participant.
    let me = game.participants.first()?;
    let s = &me.stats;
    let result = if s.game_ended_in_early_surrender {
        MatchResult::Remake
    } else if s.win {
        MatchResult::Win
    } else {
        MatchResult::Loss
    };
    Some(MatchSummary {
        game_id: game.game_id,
        champion: catalog.champion(me.champion_id),
        queue: queue_name(game.queue_id).to_owned(),
        queue_id: game.queue_id,
        result,
        kills: s.kills,
        deaths: s.deaths,
        assists: s.assists,
        cs: s.total_minions_killed + s.neutral_minions_killed,
        duration_secs: game.game_duration,
        created_ms: game.game_creation,
        items: [s.item0, s.item1, s.item2, s.item3, s.item4, s.item5].map(|id| catalog.item(id)).to_vec(),
        spells: vec![catalog.spell(me.spell1_id), catalog.spell(me.spell2_id)],
        lp_change: None,
        has_review: false,
    })
}

/// The client's history as summaries, newest first.
fn summaries(history: RawHistory, catalog: &Catalog) -> Vec<MatchSummary> {
    let mut out: Vec<MatchSummary> = history.games.games.iter().filter_map(|g| summarize(g, catalog)).collect();
    out.sort_by(|a, b| b.created_ms.cmp(&a.created_ms));
    out
}

fn champion_stats(matches: &[MatchSummary]) -> Vec<ChampionStats> {
    let mut by_champ: HashMap<i32, ChampionStats> = HashMap::new();
    for m in matches {
        if lp::queue_type_for(m.queue_id).is_none() || m.result == MatchResult::Remake {
            continue;
        }
        let entry = by_champ.entry(m.champion.id).or_insert_with(|| ChampionStats {
            champion: m.champion.clone(),
            games: 0,
            wins: 0,
            kills: 0,
            deaths: 0,
            assists: 0,
        });
        entry.games += 1;
        entry.wins += u32::from(m.result == MatchResult::Win);
        entry.kills += m.kills;
        entry.deaths += m.deaths;
        entry.assists += m.assists;
    }
    let mut out: Vec<_> = by_champ.into_values().collect();
    out.sort_by(|a, b| b.games.cmp(&a.games).then(b.wins.cmp(&a.wins)).then(a.champion.name.cmp(&b.champion.name)));
    out.truncate(TOP_CHAMPIONS);
    out
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Saves rank snapshots and games, credits LP, and returns the stored history.
/// Falls back to the client's games alone if the database is unavailable.
fn merge_with_log(app: &AppHandle, puuid: &str, ranked: &[(&str, &Option<RankedQueue>)], fresh: Vec<MatchSummary>, now: u64) -> Vec<MatchSummary> {
    let state = app.state::<AppState>();
    let Some(db) = state.db.as_ref() else { return fresh };
    let result = (|| {
        for (queue_type, rank) in ranked {
            if let Some(rank) = rank {
                db.record_snapshot(puuid, queue_type, now, rank)?;
            }
        }
        db.upsert_games(puuid, &fresh)?;
        let credited = db.credit_lp(puuid)?;
        if credited > 0 {
            tracing::info!("credited LP to {credited} game(s)");
        }
        db.games(puuid, HISTORY_LIMIT)
    })();
    result.unwrap_or_else(|e| {
        tracing::warn!("game log unavailable, showing client history only: {e}");
        fresh
    })
}

async fn fetch(app: &AppHandle, http: &LcuHttp, catalog: &Catalog) -> Result<ProfileView, LcuError> {
    let (summoner, region, ranked, history) = tokio::try_join!(
        http.get_json::<RawSummoner>("/lol-summoner/v1/current-summoner"),
        http.get_json::<RawRegion>("/riotclient/region-locale"),
        http.get_json::<RawRanked>("/lol-ranked/v1/current-ranked-stats"),
        http.get_json::<RawHistory>(
            "/lol-match-history/v1/products/lol/current-summoner/matches?begIndex=0&endIndex=20"
        ),
    )?;
    let now = now_ms();
    *app.state::<AppState>().puuid.lock().unwrap() = Some(summoner.puuid.clone());
    let solo = ranked_queue(&ranked, lp::SOLO);
    let flex = ranked_queue(&ranked, lp::FLEX);
    let matches = merge_with_log(app, &summoner.puuid, &[(lp::SOLO, &solo), (lp::FLEX, &flex)], summaries(history, catalog), now);
    Ok(ProfileView {
        game_name: summoner.game_name,
        tag_line: summoner.tag_line,
        region: region.region,
        level: summoner.summoner_level,
        profile_icon: format!("/lol-game-data/assets/v1/profile-icons/{}.jpg", summoner.profile_icon_id),
        solo,
        flex,
        champions: champion_stats(&matches),
        matches,
        fetched_at_ms: now,
    })
}

/// The client's catalog, fetched on first use per connection.
pub async fn catalog(app: &AppHandle, http: &LcuHttp) -> Result<std::sync::Arc<Catalog>, LcuError> {
    let state = app.state::<AppState>();
    let mut slot = state.catalog.lock().await;
    if let Some(c) = slot.as_ref() {
        return Ok(c.clone());
    }
    let fresh = std::sync::Arc::new(Catalog::fetch(http).await?);
    *slot = Some(fresh.clone());
    Ok(fresh)
}

/// Fetches the profile from the client, stores it and tells the frontend.
pub async fn refresh(app: &AppHandle) -> Result<ProfileView, String> {
    let http = app.state::<AppState>().lcu_http().ok_or("League client isn't connected")?;
    let catalog = catalog(app, &http).await.map_err(|e| e.to_string())?;
    let view = fetch(app, &http, &catalog).await.map_err(|e| e.to_string())?;
    *app.state::<AppState>().profile.lock().unwrap() = Some(view.clone());
    if let Err(e) = app.emit(PROFILE, &view) {
        tracing::warn!("failed to emit {PROFILE}: {e}");
    }
    Ok(view)
}

/// Refreshes in the background. Right after the client starts, some endpoints (match history)
/// answer 400 for a while, so failures retry after 3, 6, 12 and 24 seconds.
pub fn refresh_soon(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut wait = std::time::Duration::from_secs(3);
        for attempt in 1..=5 {
            match refresh(&app).await {
                Ok(_) => return,
                Err(e) if attempt < 5 => {
                    tracing::warn!("profile refresh failed (attempt {attempt}), retrying in {wait:?}: {e}");
                    tokio::time::sleep(wait).await;
                    wait *= 2;
                }
                Err(e) => tracing::warn!("profile refresh failed, giving up: {e}"),
            }
        }
    });
}

/// Records a rank snapshot without a full refresh. Called when champ select starts, so the
/// game log has a "before" state for the coming game.
pub fn snapshot_soon(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(http) = app.state::<AppState>().lcu_http() else { return };
        let fetched = tokio::try_join!(
            http.get_json::<RawSummoner>("/lol-summoner/v1/current-summoner"),
            http.get_json::<RawRanked>("/lol-ranked/v1/current-ranked-stats"),
        );
        let (summoner, ranked) = match fetched {
            Ok(v) => v,
            Err(e) => return tracing::warn!("rank snapshot failed: {e}"),
        };
        let state = app.state::<AppState>();
        let Some(db) = state.db.as_ref() else { return };
        let now = now_ms();
        for queue_type in [lp::SOLO, lp::FLEX] {
            if let Some(rank) = ranked_queue(&ranked, queue_type)
                && let Err(e) = db.record_snapshot(&summoner.puuid, queue_type, now, &rank)
            {
                tracing::warn!("rank snapshot not saved: {e}");
            }
        }
    });
}

/// Logs the client's LP-change notification, to learn its shape (§5.12).
pub fn log_lp_notification(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(http) = app.state::<AppState>().lcu_http() else { return };
        match http.get_json::<serde_json::Value>("/lol-ranked/v1/current-lp-change-notification").await {
            Ok(v) => tracing::info!("LP change notification: {v}"),
            Err(e) => tracing::debug!("no LP change notification: {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog {
        Catalog::from_json(
            r#"[{"id":1,"name":"Annie","squarePortraitPath":"/a/1.png"},{"id":233,"name":"Briar","squarePortraitPath":"/a/233.png"}]"#,
            r#"[{"id":1001,"name":"Boots","iconPath":"/i/1001.png"},{"id":3071,"name":"Black Cleaver","iconPath":"/i/3071.png"}]"#,
            r#"[{"id":4,"name":"Flash","iconPath":"/s/4.png"},{"id":11,"name":"Smite","iconPath":"/s/11.png"}]"#,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn game(id: u64, created: u64, queue: i32, champ: i32, win: bool, remake: bool, k: u32, d: u32, a: u32) -> serde_json::Value {
        serde_json::json!({
            "gameId": id, "gameCreation": created, "gameDuration": 1800, "queueId": queue,
            "participants": [{
                "championId": champ, "spell1Id": 11, "spell2Id": 4,
                "stats": {
                    "win": win, "kills": k, "deaths": d, "assists": a,
                    "totalMinionsKilled": 30, "neutralMinionsKilled": 150,
                    "gameEndedInEarlySurrender": remake,
                    "item0": 3071, "item1": 1001, "item2": 0, "item3": 0, "item4": 0, "item5": 0, "item6": 3340
                }
            }]
        })
    }

    fn matches() -> Vec<MatchSummary> {
        let history = serde_json::from_value(serde_json::json!({"games": {"games": [
            game(1, 100, 420, 233, true, false, 10, 2, 5),
            game(2, 300, 420, 233, false, false, 3, 7, 7),
            game(3, 200, 450, 1, true, false, 1, 1, 1),
            game(4, 400, 420, 1, false, true, 0, 0, 0),
            game(5, 50, 440, 1, true, false, 4, 0, 9)
        ]}}))
        .unwrap();
        summaries(history, &catalog())
    }

    #[test]
    fn ranked_cards() {
        let ranked: RawRanked = serde_json::from_value(serde_json::json!({"queues": [
            {"queueType": "RANKED_SOLO_5x5", "tier": "SILVER", "division": "III", "leaguePoints": 20, "wins": 3, "losses": 5},
            {"queueType": "RANKED_FLEX_SR", "tier": "", "division": "NA", "leaguePoints": 0, "wins": 0, "losses": 0},
            {"queueType": "RANKED_TFT", "tier": "GOLD", "division": "I"}
        ]}))
        .unwrap();
        assert_eq!(
            ranked_queue(&ranked, lp::SOLO),
            Some(RankedQueue { tier: "SILVER".into(), division: "III".into(), lp: 20, wins: 3, losses: 5 })
        );
        assert_eq!(ranked_queue(&ranked, lp::FLEX), None, "unranked queue is left out");
        let apex: RawRanked = serde_json::from_value(serde_json::json!({"queues": [
            {"queueType": "RANKED_SOLO_5x5", "tier": "MASTER", "division": "NA", "leaguePoints": 120, "wins": 1, "losses": 0}
        ]}))
        .unwrap();
        assert_eq!(ranked_queue(&apex, lp::SOLO).unwrap().division, "");
    }

    #[test]
    fn summoner_includes_puuid() {
        let s: RawSummoner = serde_json::from_value(serde_json::json!({
            "gameName": "Tester", "tagLine": "NA1", "summonerLevel": 120, "profileIconId": 29, "puuid": "abc"
        }))
        .unwrap();
        assert_eq!(s.puuid, "abc");
    }

    #[test]
    fn matches_are_newest_first_with_results_and_slots() {
        let v = matches();
        let ids: Vec<u64> = v.iter().map(|m| m.game_id).collect();
        assert_eq!(ids, vec![4, 2, 3, 1, 5]);
        assert_eq!(v[0].result, MatchResult::Remake);
        assert_eq!(v[1].result, MatchResult::Loss);
        assert_eq!((v[2].queue.as_str(), v[2].queue_id), ("ARAM", 450));
        let m = &v[3];
        assert_eq!((m.result, m.kills, m.deaths, m.assists, m.cs), (MatchResult::Win, 10, 2, 5, 180));
        assert_eq!(m.champion.name, "Briar");
        assert_eq!(m.items.len(), 6, "trinket is excluded");
        assert_eq!(m.items[0].as_ref().unwrap().name, "Black Cleaver");
        assert_eq!(m.items[2], None);
        assert_eq!(m.spells.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Smite", "Flash"]);
        assert_eq!(m.lp_change, None);
    }

    #[test]
    fn champion_stats_count_ranked_games_only_and_skip_remakes() {
        let stats = champion_stats(&matches());
        let summary: Vec<(String, u32, u32)> = stats.iter().map(|c| (c.champion.name.clone(), c.games, c.wins)).collect();
        // Briar: 2 solo games (1 win). Annie: 1 flex win; her ARAM game and solo remake don't count.
        assert_eq!(summary, vec![("Briar".into(), 2, 1), ("Annie".into(), 1, 1)]);
        assert_eq!((stats[0].kills, stats[0].deaths, stats[0].assists), (13, 9, 12));
    }

    #[test]
    fn old_stored_games_without_new_fields_still_load() {
        // A row saved before queue_id / lp_change existed.
        let json = r#"{"game_id":1,"champion":{"id":1,"name":"Annie","icon":""},"queue":"ARAM","result":"Win",
            "kills":1,"deaths":1,"assists":1,"cs":1,"duration_secs":1,"created_ms":1,"items":[],"spells":[]}"#;
        let m: MatchSummary = serde_json::from_str(json).unwrap();
        assert_eq!((m.queue_id, m.lp_change, m.has_review), (0, None, false));
    }

    #[test]
    fn queue_names() {
        assert_eq!(queue_name(420), "Ranked Solo");
        assert_eq!(queue_name(1700), "Arena");
        assert_eq!(queue_name(12345), "Other");
    }
}
