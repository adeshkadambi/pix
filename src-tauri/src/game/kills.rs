//! Kill log for the post-game review and the live advisor (SPEC §5.9).
//!
//! The Live Client's kill events name players by Riot ID. They're turned into champions as they
//! arrive (compliance rule 5: Riot IDs are only used to match players and never stored).

use serde::{Deserialize, Serialize};

use crate::live::models::{AllGameData, Event, Player};

/// Who got the kill: a champion (Data Dragon id), or something else (turret, minions, a monster).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Actor {
    Champion(String),
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KillRecord {
    pub event_id: u64,
    pub time: f64,
    pub killer: Actor,
    /// Victim champion (Data Dragon id).
    pub victim: String,
    /// Assisting champions (Data Dragon ids).
    pub assisters: Vec<String>,
    pub victim_is_me: bool,
    pub killer_is_me: bool,
}

fn find<'a>(game: &'a AllGameData, name: &str) -> Option<&'a Player> {
    game.all_players
        .iter()
        .find(|p| (!p.riot_id.is_empty() && p.riot_id.eq_ignore_ascii_case(name)) || (!p.summoner_name.is_empty() && p.summoner_name.eq_ignore_ascii_case(name)))
}

/// A readable name for a non-champion killer ("Turret_T2_C_05_A" → "Turret").
pub fn other_label(raw: &str) -> String {
    let r = raw.to_ascii_lowercase();
    let label = if r.contains("turret") {
        "Turret"
    } else if r.contains("minion") {
        "Minions"
    } else if r.contains("baron") {
        "Baron Nashor"
    } else if r.contains("dragon") {
        "Dragon"
    } else if r.contains("herald") || r.contains("riftherald") {
        "Rift Herald"
    } else if r.contains("horde") || r.contains("voidgrub") {
        "Voidgrubs"
    } else if r.contains("atakhan") {
        "Atakhan"
    } else if r.starts_with("sru_") || r.contains("camp") {
        "Jungle monster"
    } else if r.is_empty() {
        "Unknown"
    } else {
        "Execution"
    };
    label.to_owned()
}

/// Turns a `ChampionKill` event into a record with champions instead of player names.
pub fn record(e: &Event, game: &AllGameData) -> Option<KillRecord> {
    if e.name != "ChampionKill" {
        return None;
    }
    let me = game.me()?;
    let victim = find(game, &e.victim)?;
    let killer = find(game, &e.killer);
    Some(KillRecord {
        event_id: e.id,
        time: e.time,
        killer: match killer {
            Some(p) => Actor::Champion(p.ddragon_id().to_owned()),
            None => Actor::Other(other_label(&e.killer)),
        },
        victim: victim.ddragon_id().to_owned(),
        assisters: e.assisters.iter().filter_map(|a| find(game, a)).map(|p| p.ddragon_id().to_owned()).collect(),
        victim_is_me: std::ptr::eq(victim, me),
        killer_is_me: killer.is_some_and(|k| std::ptr::eq(k, me)),
    })
}

/// How many times each enemy champion killed you.
pub fn kills_on_me(kills: &[KillRecord], champion: &str) -> u32 {
    kills.iter().filter(|k| k.victim_is_me && k.killer == Actor::Champion(champion.to_owned())).count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game() -> AllGameData {
        let path = format!("{}/tests/fixtures/live/allgamedata.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn kill_events_become_champions() {
        let g = game();
        let e = g.events.events.iter().find(|e| e.name == "ChampionKill").unwrap();
        let k = record(e, &g).unwrap();
        assert_eq!(k.killer, Actor::Champion("Viego".into()));
        assert_eq!(k.victim, "Briar");
        assert_eq!(k.assisters, vec!["Syndra"]);
        assert!(k.victim_is_me && !k.killer_is_me);
        assert_eq!(kills_on_me(&[k.clone()], "Viego"), 1);
        assert_eq!(kills_on_me(&[k], "Syndra"), 0);
        // Other event types are ignored.
        assert!(record(g.events.events.iter().find(|e| e.name == "GameStart").unwrap(), &g).is_none());
    }

    #[test]
    fn non_champion_killers() {
        let g = game();
        let e = Event { id: 9, name: "ChampionKill".into(), time: 500.0, killer: "Turret_T2_C_05_A".into(), victim: "Tester#NA1".into(), assisters: vec![], ..Default::default() };
        assert_eq!(record(&e, &g).unwrap().killer, Actor::Other("Turret".into()));
        assert_eq!(other_label("SRU_Baron12.1.1"), "Baron Nashor");
        assert_eq!(other_label("Minion_T200L0S16N0105"), "Minions");
    }
}
