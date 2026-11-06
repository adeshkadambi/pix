//! Gameflow phase mapping and the Summoner's Rift gate (SPEC §5.1, compliance rule 6).

use serde_json::Value;

use crate::events::Phase;

/// Maps the client's gameflow phase string onto the phases Pix cares about.
pub fn parse_phase(raw: &str) -> Phase {
    match raw {
        "Lobby" => Phase::Lobby,
        "Matchmaking" => Phase::Matchmaking,
        "ReadyCheck" => Phase::ReadyCheck,
        "ChampSelect" => Phase::ChampSelect,
        "GameStart" | "InProgress" | "Reconnect" => Phase::InProgress,
        "PreEndOfGame" | "WaitingForStats" | "EndOfGame" => Phase::EndOfGame,
        _ => Phase::None,
    }
}

/// True only for Summoner's Rift classic games: `map.id == 11` and `gameMode == "CLASSIC"`.
/// Build and win-rate data is turned off for everything else (Arena, ARAM, …).
pub fn is_summoners_rift(session: &Value) -> bool {
    let map_id = session.pointer("/map/id").and_then(Value::as_i64);
    let mode = session.pointer("/gameData/queue/gameMode").and_then(Value::as_str);
    map_id == Some(11) && mode == Some("CLASSIC")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_phases() {
        assert_eq!(parse_phase("Lobby"), Phase::Lobby);
        assert_eq!(parse_phase("ChampSelect"), Phase::ChampSelect);
        assert_eq!(parse_phase("GameStart"), Phase::InProgress);
        assert_eq!(parse_phase("Reconnect"), Phase::InProgress);
        assert_eq!(parse_phase("WaitingForStats"), Phase::EndOfGame);
        assert_eq!(parse_phase("None"), Phase::None);
        assert_eq!(parse_phase("CheckedIntoTournament"), Phase::None);
    }

    #[test]
    fn rift_gate() {
        let rift = json!({"map": {"id": 11}, "gameData": {"queue": {"gameMode": "CLASSIC", "id": 420}}});
        let aram = json!({"map": {"id": 12}, "gameData": {"queue": {"gameMode": "ARAM"}}});
        let arena = json!({"map": {"id": 30}, "gameData": {"queue": {"gameMode": "CHERRY"}}});
        let rift_urf = json!({"map": {"id": 11}, "gameData": {"queue": {"gameMode": "URF"}}});
        assert!(is_summoners_rift(&rift));
        assert!(!is_summoners_rift(&aram));
        assert!(!is_summoners_rift(&arena));
        assert!(!is_summoners_rift(&rift_urf));
        assert!(!is_summoners_rift(&json!({})));
    }
}
