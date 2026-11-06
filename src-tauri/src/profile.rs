//! Native profile view (SPEC §5.10): your own summoner, ranked stats and recent match history,
//! all from the League client. Only your own account is ever read.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use ts_rs::TS;

use crate::events::PROFILE;
use crate::lcu::catalog::{Catalog, Icon};
use crate::lcu::http::{LcuError, LcuHttp};
use crate::state::AppState;

const RANKED_QUEUES: [i32; 2] = [420, 440];
const TOP_CHAMPIONS: usize = 5;

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
    /// Newest first.
    pub matches: Vec<MatchSummary>,
    /// Ranked champions in the recent history, most played first.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub enum MatchResult {
    Win,
    Loss,
    Remake,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct MatchSummary {
    #[ts(type = "number")]
    pub game_id: u64,
    pub champion: Icon,
    pub queue: String,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[derive(Default)]
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
        result,
        kills: s.kills,
        deaths: s.deaths,
        assists: s.assists,
        cs: s.total_minions_killed + s.neutral_minions_killed,
        duration_secs: game.game_duration,
        created_ms: game.game_creation,
        items: [s.item0, s.item1, s.item2, s.item3, s.item4, s.item5].map(|id| catalog.item(id)).to_vec(),
        spells: vec![catalog.spell(me.spell1_id), catalog.spell(me.spell2_id)],
    })
}

fn champion_stats(games: &[RawGame], matches: &[MatchSummary]) -> Vec<ChampionStats> {
    let mut by_champ: HashMap<i32, ChampionStats> = HashMap::new();
    for (game, m) in games.iter().zip(matches) {
        if !RANKED_QUEUES.contains(&game.queue_id) || m.result == MatchResult::Remake {
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

fn build(
    summoner: RawSummoner,
    region: RawRegion,
    ranked: RawRanked,
    history: RawHistory,
    catalog: &Catalog,
    now_ms: u64,
) -> ProfileView {
    let mut games = history.games.games;
    games.sort_by(|a, b| b.game_creation.cmp(&a.game_creation));
    // Keep games and summaries aligned: drop games with no participant.
    games.retain(|g| !g.participants.is_empty());
    let matches: Vec<MatchSummary> = games.iter().filter_map(|g| summarize(g, catalog)).collect();
    ProfileView {
        game_name: summoner.game_name,
        tag_line: summoner.tag_line,
        region: region.region,
        level: summoner.summoner_level,
        profile_icon: format!("/lol-game-data/assets/v1/profile-icons/{}.jpg", summoner.profile_icon_id),
        solo: ranked_queue(&ranked, "RANKED_SOLO_5x5"),
        flex: ranked_queue(&ranked, "RANKED_FLEX_SR"),
        champions: champion_stats(&games, &matches),
        matches,
        fetched_at_ms: now_ms,
    }
}

async fn fetch(http: &LcuHttp, catalog: &Catalog) -> Result<ProfileView, LcuError> {
    let (summoner, region, ranked, history) = tokio::try_join!(
        http.get_json::<RawSummoner>("/lol-summoner/v1/current-summoner"),
        http.get_json::<RawRegion>("/riotclient/region-locale"),
        http.get_json::<RawRanked>("/lol-ranked/v1/current-ranked-stats"),
        http.get_json::<RawHistory>(
            "/lol-match-history/v1/products/lol/current-summoner/matches?begIndex=0&endIndex=20"
        ),
    )?;
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    Ok(build(summoner, region, ranked, history, catalog, now_ms))
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
    let view = fetch(&http, &catalog).await.map_err(|e| e.to_string())?;
    *app.state::<AppState>().profile.lock().unwrap() = Some(view.clone());
    if let Err(e) = app.emit(PROFILE, &view) {
        tracing::warn!("failed to emit {PROFILE}: {e}");
    }
    Ok(view)
}

/// Refreshes in the background and logs failures.
pub fn refresh_soon(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = refresh(&app).await {
            tracing::warn!("profile refresh failed: {e}");
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

    fn view() -> ProfileView {
        let summoner = serde_json::from_value(serde_json::json!({
            "gameName": "Tester", "tagLine": "NA1", "summonerLevel": 120, "profileIconId": 29,
            "puuid": "ignored"
        }))
        .unwrap();
        let region = serde_json::from_value(serde_json::json!({"region": "NA", "locale": "en_US"})).unwrap();
        let ranked = serde_json::from_value(serde_json::json!({"queues": [
            {"queueType": "RANKED_SOLO_5x5", "tier": "SILVER", "division": "III", "leaguePoints": 20, "wins": 3, "losses": 5},
            {"queueType": "RANKED_FLEX_SR", "tier": "", "division": "NA", "leaguePoints": 0, "wins": 0, "losses": 0},
            {"queueType": "RANKED_TFT", "tier": "GOLD", "division": "I"}
        ]}))
        .unwrap();
        let history = serde_json::from_value(serde_json::json!({"games": {"games": [
            game(1, 100, 420, 233, true, false, 10, 2, 5),
            game(2, 300, 420, 233, false, false, 3, 7, 7),
            game(3, 200, 450, 1, true, false, 1, 1, 1),
            game(4, 400, 420, 1, false, true, 0, 0, 0),
            game(5, 50, 440, 1, true, false, 4, 0, 9)
        ]}}))
        .unwrap();
        build(summoner, region, ranked, history, &catalog(), 999)
    }

    #[test]
    fn builds_the_header_and_ranked_cards() {
        let v = view();
        assert_eq!((v.game_name.as_str(), v.tag_line.as_str(), v.region.as_str(), v.level), ("Tester", "NA1", "NA", 120));
        assert_eq!(v.profile_icon, "/lol-game-data/assets/v1/profile-icons/29.jpg");
        assert_eq!(
            v.solo,
            Some(RankedQueue { tier: "SILVER".into(), division: "III".into(), lp: 20, wins: 3, losses: 5 })
        );
        assert_eq!(v.flex, None, "unranked queue is left out");
        assert_eq!(v.fetched_at_ms, 999);
    }

    #[test]
    fn matches_are_newest_first_with_results_and_slots() {
        let v = view();
        let ids: Vec<u64> = v.matches.iter().map(|m| m.game_id).collect();
        assert_eq!(ids, vec![4, 2, 3, 1, 5]);
        assert_eq!(v.matches[0].result, MatchResult::Remake);
        assert_eq!(v.matches[1].result, MatchResult::Loss);
        assert_eq!(v.matches[2].queue, "ARAM");
        let m = &v.matches[3];
        assert_eq!((m.result, m.kills, m.deaths, m.assists, m.cs), (MatchResult::Win, 10, 2, 5, 180));
        assert_eq!(m.champion.name, "Briar");
        assert_eq!(m.items.len(), 6, "trinket is excluded");
        assert_eq!(m.items[0].as_ref().unwrap().name, "Black Cleaver");
        assert_eq!(m.items[2], None);
        assert_eq!(m.spells.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Smite", "Flash"]);
    }

    #[test]
    fn champion_stats_count_ranked_games_only_and_skip_remakes() {
        let v = view();
        let summary: Vec<(String, u32, u32)> =
            v.champions.iter().map(|c| (c.champion.name.clone(), c.games, c.wins)).collect();
        // Briar: 2 solo games (1 win). Annie: 1 flex win; her ARAM game and solo remake don't count.
        assert_eq!(summary, vec![("Briar".into(), 2, 1), ("Annie".into(), 1, 1)]);
        assert_eq!((v.champions[0].kills, v.champions[0].deaths, v.champions[0].assists), (13, 9, 12));
    }

    #[test]
    fn queue_names() {
        assert_eq!(queue_name(420), "Ranked Solo");
        assert_eq!(queue_name(1700), "Arena");
        assert_eq!(queue_name(12345), "Other");
    }
}
