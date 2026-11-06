//! Live Client Data API (`https://127.0.0.1:2999/liveclientdata/allgamedata`), SPEC §5.6.
//! Only the fields Pix uses; everything is lenient because the API omits fields during loading
//! and in some modes. Items have no descriptions here, and enemy rune pages aren't available.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AllGameData {
    #[serde(rename = "activePlayer")]
    pub active_player: ActivePlayer,
    #[serde(rename = "allPlayers")]
    pub all_players: Vec<Player>,
    pub events: Events,
    #[serde(rename = "gameData")]
    pub game_data: GameData,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ActivePlayer {
    /// "Name#TAG". Used only to find "me" in `allPlayers`; never stored.
    pub riot_id: String,
    pub summoner_name: String,
    pub level: u32,
    pub current_gold: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Player {
    pub riot_id: String,
    pub summoner_name: String,
    /// Localized display name, e.g. "Kai'Sa".
    pub champion_name: String,
    /// "game_character_displayname_<DataDragonId>".
    pub raw_champion_name: String,
    /// "ORDER" or "CHAOS".
    pub team: String,
    /// "TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY", or "" in some modes.
    pub position: String,
    pub level: u32,
    pub is_dead: bool,
    pub respawn_timer: f64,
    pub items: Vec<Item>,
    pub scores: Scores,
    pub summoner_spells: SummonerSpells,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Item {
    #[serde(rename = "itemID")]
    pub item_id: u32,
    pub count: u32,
    pub slot: u32,
    #[serde(rename = "displayName")]
    pub display_name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Scores {
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub creep_score: u32,
    pub ward_score: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SummonerSpells {
    pub summoner_spell_one: Spell,
    pub summoner_spell_two: Spell,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Spell {
    pub display_name: String,
    /// "GeneratedTip_SummonerSpell_SummonerFlash_DisplayName".
    pub raw_display_name: String,
}

impl Spell {
    /// Data Dragon spell key, e.g. "SummonerFlash".
    pub fn key(&self) -> Option<&str> {
        self.raw_display_name.strip_prefix("GeneratedTip_SummonerSpell_")?.strip_suffix("_DisplayName")
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Events {
    #[serde(rename = "Events")]
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Event {
    #[serde(rename = "EventID")]
    pub id: u64,
    #[serde(rename = "EventName")]
    pub name: String,
    #[serde(rename = "EventTime")]
    pub time: f64,
    #[serde(rename = "KillerName")]
    pub killer: String,
    #[serde(rename = "VictimName")]
    pub victim: String,
    #[serde(rename = "Assisters")]
    pub assisters: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GameData {
    pub game_mode: String,
    /// Seconds since the game started. Every timer uses this, so pauses don't break them.
    pub game_time: f64,
    pub map_number: i32,
}

impl Player {
    /// Data Dragon champion id, e.g. "Briar" or "MonkeyKing".
    pub fn ddragon_id(&self) -> &str {
        self.raw_champion_name.strip_prefix("game_character_displayname_").unwrap_or(&self.raw_champion_name)
    }

    fn id_matches(&self, riot_id: &str, summoner_name: &str) -> bool {
        (!riot_id.is_empty() && self.riot_id.eq_ignore_ascii_case(riot_id))
            || (!summoner_name.is_empty() && self.summoner_name.eq_ignore_ascii_case(summoner_name))
    }
}

impl AllGameData {
    /// The local player, matched by Riot ID (falling back to summoner name).
    pub fn me(&self) -> Option<&Player> {
        let a = &self.active_player;
        self.all_players.iter().find(|p| p.id_matches(&a.riot_id, &a.summoner_name))
    }

    pub fn allies(&self) -> impl Iterator<Item = &Player> {
        let team = self.me().map(|m| m.team.clone()).unwrap_or_default();
        self.all_players.iter().filter(move |p| p.team == team)
    }

    pub fn enemies(&self) -> impl Iterator<Item = &Player> {
        let team = self.me().map(|m| m.team.clone()).unwrap_or_default();
        self.all_players.iter().filter(move |p| !team.is_empty() && p.team != team)
    }

    /// Summoner's Rift classic only (compliance rule 6).
    pub fn is_summoners_rift(&self) -> bool {
        self.game_data.map_number == 11 && self.game_data.game_mode == "CLASSIC"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn fixture() -> AllGameData {
        let path = format!("{}/tests/fixtures/live/allgamedata.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn finds_me_allies_and_enemies() {
        let d = fixture();
        let me = d.me().unwrap();
        assert_eq!((me.ddragon_id(), me.team.as_str(), me.level), ("Briar", "ORDER", 11));
        assert_eq!(d.allies().count(), 5);
        assert_eq!(d.enemies().map(|p| p.ddragon_id()).collect::<Vec<_>>(), vec!["Malphite", "Viego", "Syndra", "Kaisa", "Leona"]);
        assert!(d.is_summoners_rift());
        assert!((d.game_data.game_time - 1122.5).abs() < 1e-9);
    }

    #[test]
    fn spell_keys_and_items() {
        let d = fixture();
        let viego = d.enemies().nth(1).unwrap();
        assert_eq!(viego.summoner_spells.summoner_spell_one.key(), Some("SummonerFlash"));
        assert_eq!(viego.summoner_spells.summoner_spell_two.key(), Some("SummonerSmite"));
        assert!(d.me().unwrap().items.iter().any(|i| i.item_id == 3078));
        assert_eq!(Spell { raw_display_name: "nope".into(), ..Default::default() }.key(), None);
    }

    #[test]
    fn falls_back_to_summoner_name_and_tolerates_loading_state() {
        let d: AllGameData = serde_json::from_str(r#"{
            "activePlayer": {"summonerName": "Old Name"},
            "allPlayers": [{"summonerName": "old name", "team": "CHAOS", "rawChampionName": "game_character_displayname_Ahri"}],
            "gameData": {}
        }"#).unwrap();
        assert_eq!(d.me().unwrap().ddragon_id(), "Ahri");
        assert_eq!(d.enemies().count(), 0);
        let empty: AllGameData = serde_json::from_str("{}").unwrap();
        assert!(empty.me().is_none() && !empty.is_summoners_rift());
    }

    #[test]
    fn kill_events() {
        let d = fixture();
        let kill = d.events.events.iter().find(|e| e.name == "ChampionKill").unwrap();
        assert_eq!((kill.killer.as_str(), kill.victim.as_str()), ("Viego Player#EUW", "Tester#NA1"));
        assert_eq!(kill.assisters, vec!["Syndra Player#NA1"]);
    }
}
