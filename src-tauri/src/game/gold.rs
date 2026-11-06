//! Item-gold difference (SPEC §5.7, flag `overlay_gold`, on by default).
//!
//! The Live Client API doesn't give other players' gold, so this is **item gold**: the sum of
//! each player's items at Data Dragon's full price (`gold.total × count`). It's labeled that way.
//!
//! Rows pair by **scoreboard position**: the Live Client lists each team in scoreboard order, so
//! enemy row `i` is compared with your team's row `i`, the two players side by side on the
//! scoreboard. In ranked and draft games the scoreboard is sorted by role, so rows are lanes.

use std::collections::HashMap;

use crate::ddragon::DData;
use crate::live::models::{AllGameData, Item};

pub fn item_gold(items: &[Item], dd: &DData) -> u32 {
    items.iter().map(|it| dd.items.get(&it.item_id).map(|i| i.gold_total).unwrap_or(0) * it.count.max(1)).sum()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldSummary {
    pub ally_total: u32,
    pub enemy_total: u32,
    /// Enemy champion (Data Dragon id) → (item gold of your player in the same scoreboard row,
    /// the enemy's item gold). Missing when your team has no player in that row.
    pub rows: HashMap<String, (u32, u32)>,
}

pub fn summarize(game: &AllGameData, dd: &DData) -> GoldSummary {
    let allies: Vec<u32> = game.allies().map(|p| item_gold(&p.items, dd)).collect();
    let enemies: Vec<(&str, u32)> = game.enemies().map(|p| (p.ddragon_id(), item_gold(&p.items, dd))).collect();
    GoldSummary {
        ally_total: allies.iter().sum(),
        enemy_total: enemies.iter().map(|(_, g)| g).sum(),
        rows: enemies
            .iter()
            .enumerate()
            .filter_map(|(i, (champ, eg))| Some((champ.to_string(), (*allies.get(i)?, *eg))))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddragon::ItemInfo;

    fn dd() -> DData {
        let mut items = HashMap::new();
        for (id, gold) in [(3078, 3333), (3748, 3300), (3047, 1200), (1036, 350), (2055, 75), (3364, 0), (3075, 2450), (3153, 3200), (6692, 2800), (3118, 2900), (3031, 3450), (6655, 2900), (3124, 3000), (3190, 2200)] {
            items.insert(id, ItemInfo { name: String::new(), tags: vec![], description: String::new(), gold_total: gold, completed: true, on_rift: true });
        }
        DData { items, ..Default::default() }
    }

    fn game() -> AllGameData {
        let path = format!("{}/tests/fixtures/live/allgamedata.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn item_gold_sums_full_prices_times_count() {
        let d = dd();
        let briar = game().me().unwrap().items.clone();
        // Trinity 3333 + Hydra 3300 + Steelcaps 1200 + 2× Long Sword 700 + Control Ward 75 + lens 0.
        assert_eq!(item_gold(&briar, &d), 8608);
        assert_eq!(item_gold(&[], &d), 0);
    }

    #[test]
    fn rows_pair_by_scoreboard_order_and_totals_add_up() {
        let s = summarize(&game(), &dd());
        // Row 1: Aatrox (Eclipse 2800 + Steelcaps 1200) vs Malphite (Thornmail 2450 + Steelcaps 1200).
        assert_eq!(s.rows["Malphite"], (4000, 3650));
        // Row 2: Briar 8608 vs Viego 3200.
        assert_eq!(s.rows["Viego"], (8608, 3200));
        // Row 5: Thresh has no items vs Leona's Locket.
        assert_eq!(s.rows["Leona"], (0, 2200));
        assert_eq!(s.ally_total, 4000 + 8608 + 2900 + 3450);
        assert_eq!(s.enemy_total, 3650 + 3200 + 2900 + 3000 + 2200);
    }

    #[test]
    fn positions_dont_matter_only_row_order() {
        let mut g = game();
        for p in &mut g.all_players {
            p.position = "NONE".into();
        }
        assert_eq!(summarize(&g, &dd()).rows["Viego"], (8608, 3200));
    }

    #[test]
    fn uneven_teams_leave_extra_enemy_rows_without_a_diff() {
        let mut g = game();
        g.all_players.retain(|p| p.ddragon_id() != "Thresh");
        let s = summarize(&g, &dd());
        assert!(!s.rows.contains_key("Leona"), "your team has no fifth row");
        assert_eq!(s.rows.len(), 4);
    }
}
