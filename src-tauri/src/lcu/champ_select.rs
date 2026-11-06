//! Champion select session → `DraftState` (SPEC §5.2).
//!
//! Compliance rule 5: only champion ids, cell ids and assigned positions are read. Player names,
//! summoner ids and PUUIDs in the session are never read, stored or shown.

use serde_json::Value;

use crate::builds::Role;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftSlot {
    pub cell_id: i64,
    /// Locked champion, else the hovered one; 0 when nothing is picked.
    pub champion_id: i32,
    pub locked: bool,
    /// Allies only: the client's assigned position (empty in blind pick).
    pub role: Option<Role>,
    pub is_me: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DraftState {
    pub allies: Vec<DraftSlot>,
    pub enemies: Vec<DraftSlot>,
    pub ally_bans: Vec<i32>,
    pub enemy_bans: Vec<i32>,
}

impl DraftState {
    pub fn me(&self) -> Option<&DraftSlot> {
        self.allies.iter().find(|s| s.is_me)
    }
}

fn int(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}

/// Pick actions per cell: (completed, champion id of the latest pick action).
fn pick_actions(session: &Value) -> Vec<(i64, bool, bool, i32)> {
    let mut out = Vec::new();
    for group in session.get("actions").and_then(Value::as_array).into_iter().flatten() {
        for a in group.as_array().into_iter().flatten() {
            if a.get("type").and_then(Value::as_str) == Some("pick") {
                out.push((
                    int(a, "actorCellId"),
                    a.get("completed").and_then(Value::as_bool).unwrap_or(false),
                    a.get("isInProgress").and_then(Value::as_bool).unwrap_or(false),
                    int(a, "championId") as i32,
                ));
            }
        }
    }
    out
}

fn bans(session: &Value) -> (Vec<i32>, Vec<i32>) {
    let (mut ally, mut enemy) = (Vec::new(), Vec::new());
    for group in session.get("actions").and_then(Value::as_array).into_iter().flatten() {
        for a in group.as_array().into_iter().flatten() {
            let id = int(a, "championId") as i32;
            if a.get("type").and_then(Value::as_str) == Some("ban") && a.get("completed").and_then(Value::as_bool) == Some(true) && id > 0 {
                if a.get("isAllyAction").and_then(Value::as_bool).unwrap_or(false) { ally.push(id) } else { enemy.push(id) }
            }
        }
    }
    if ally.is_empty() && enemy.is_empty() {
        let list = |p: &str| -> Vec<i32> {
            session.pointer(p).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_i64()).map(|x| x as i32).filter(|x| *x > 0).collect()).unwrap_or_default()
        };
        return (list("/bans/myTeamBans"), list("/bans/theirTeamBans"));
    }
    (ally, enemy)
}

pub fn parse(session: &Value) -> Option<DraftState> {
    let me = session.get("localPlayerCellId")?.as_i64()?;
    let picks = pick_actions(session);
    let slot = |member: &Value, ally: bool| -> DraftSlot {
        let cell = int(member, "cellId");
        let mine: Vec<&(i64, bool, bool, i32)> = picks.iter().filter(|p| p.0 == cell).collect();
        let locked = mine.iter().any(|p| p.1);
        let mut champion = int(member, "championId") as i32;
        if champion == 0 {
            champion = int(member, "championPickIntent") as i32;
        }
        if champion == 0 {
            champion = mine.iter().rev().find(|p| p.2 || p.1).map(|p| p.3).unwrap_or(0);
        }
        DraftSlot {
            cell_id: cell,
            champion_id: champion,
            locked: locked || (!ally && champion > 0),
            role: if ally { member.get("assignedPosition").and_then(Value::as_str).and_then(Role::from_lcu) } else { None },
            is_me: ally && cell == me,
        }
    };
    let team = |key: &str, ally: bool| -> Vec<DraftSlot> {
        session.get(key).and_then(Value::as_array).map(|a| a.iter().map(|m| slot(m, ally)).collect()).unwrap_or_default()
    };
    let (ally_bans, enemy_bans) = bans(session);
    Some(DraftState { allies: team("myTeam", true), enemies: team("theirTeam", false), ally_bans, enemy_bans })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        let path = format!("{}/tests/fixtures/lcu/champ_select_session.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn parses_the_fixture_session() {
        let d = parse(&fixture()).unwrap();
        let me = d.me().unwrap();
        assert_eq!((me.champion_id, me.locked, me.role), (233, true, Some(Role::Jungle)));
        assert_eq!(d.allies.len(), 5);
        let thresh = d.allies.iter().find(|s| s.role == Some(Role::Support)).unwrap();
        assert_eq!((thresh.champion_id, thresh.locked), (412, false), "hover counts as the champion");
        assert_eq!(d.enemies.iter().map(|s| s.champion_id).collect::<Vec<_>>(), vec![54, 234, 134, 145, 89]);
        assert!(d.enemies.iter().all(|s| s.locked && s.role.is_none()));
        assert_eq!(d.ally_bans, vec![200, 76, 238, 901, 53]);
        assert_eq!(d.enemy_bans, vec![64, 157, 25, 119, 9]);
    }

    #[test]
    fn hover_from_in_progress_action_and_blind_pick() {
        let s = json!({
            "localPlayerCellId": 2,
            "myTeam": [{"cellId": 2, "championId": 0, "championPickIntent": 0, "assignedPosition": ""}],
            "theirTeam": [{"cellId": 7, "championId": 0}],
            "actions": [[{"actorCellId": 2, "championId": 103, "completed": false, "isInProgress": true, "type": "pick"}]]
        });
        let d = parse(&s).unwrap();
        let me = d.me().unwrap();
        assert_eq!((me.champion_id, me.locked, me.role), (103, false, None));
        assert_eq!((d.enemies[0].champion_id, d.enemies[0].locked), (0, false));
    }

    #[test]
    fn falls_back_to_ban_lists() {
        let s = json!({"localPlayerCellId": 0, "myTeam": [], "theirTeam": [], "actions": [],
                       "bans": {"myTeamBans": [1, 0, 2], "theirTeamBans": [3]}});
        let d = parse(&s).unwrap();
        assert_eq!((d.ally_bans, d.enemy_bans), (vec![1, 2], vec![3]));
    }

    #[test]
    fn not_a_session() {
        assert!(parse(&json!({"errorCode": "RPC_ERROR"})).is_none());
    }
}
