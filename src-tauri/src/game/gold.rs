//! Item-gold difference (SPEC §5.7, gray flag `overlay_gold`).
//!
//! The Live Client API doesn't give other players' gold, so this is **item gold**: the sum of
//! each player's items at Data Dragon's full price (`gold.total × count`). It's labeled that way.
//! Lanes pair players by position: the Live Client's `position` first, else the roles from champ
//! select.

use std::collections::HashMap;

use crate::builds::Role;
use crate::ddragon::DData;
use crate::live::models::{AllGameData, Item, Player};

pub fn item_gold(items: &[Item], dd: &DData) -> u32 {
    items.iter().map(|it| dd.items.get(&it.item_id).map(|i| i.gold_total).unwrap_or(0) * it.count.max(1)).sum()
}

/// Live Client position → role. Empty or "NONE" in some modes.
pub fn role_of(position: &str) -> Option<Role> {
    match position.to_ascii_uppercase().as_str() {
        "TOP" => Some(Role::Top),
        "JUNGLE" => Some(Role::Jungle),
        "MIDDLE" | "MID" => Some(Role::Mid),
        "BOTTOM" | "ADC" => Some(Role::Adc),
        "UTILITY" | "SUPPORT" => Some(Role::Support),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldSummary {
    pub ally_total: u32,
    pub enemy_total: u32,
    /// Enemy champion (Data Dragon id) → your laner's item gold minus theirs.
    pub lanes: HashMap<String, i32>,
    /// Enemy champion → their item gold.
    pub enemy_gold: HashMap<String, u32>,
}

/// `fallback_roles`: Data Dragon champion id → role from champ select, used when the Live
/// Client gives no position.
pub fn summarize(game: &AllGameData, dd: &DData, fallback_roles: &HashMap<String, Role>) -> GoldSummary {
    let role = |p: &Player| role_of(&p.position).or_else(|| fallback_roles.get(p.ddragon_id()).copied());
    let allies: Vec<(&Player, u32)> = game.allies().map(|p| (p, item_gold(&p.items, dd))).collect();
    let enemies: Vec<(&Player, u32)> = game.enemies().map(|p| (p, item_gold(&p.items, dd))).collect();
    let lanes = enemies
        .iter()
        .filter_map(|(enemy, eg)| {
            let r = role(enemy)?;
            let (_, ag) = allies.iter().find(|(ally, _)| role(ally) == Some(r))?;
            Some((enemy.ddragon_id().to_owned(), *ag as i32 - *eg as i32))
        })
        .collect();
    GoldSummary {
        ally_total: allies.iter().map(|(_, g)| g).sum(),
        enemy_total: enemies.iter().map(|(_, g)| g).sum(),
        lanes,
        enemy_gold: enemies.iter().map(|(p, g)| (p.ddragon_id().to_owned(), *g)).collect(),
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
    fn lanes_pair_by_position_and_totals_add_up() {
        let s = summarize(&game(), &dd(), &HashMap::new());
        // Briar (jungle) 8608 vs Viego 3200.
        assert_eq!(s.lanes["Viego"], 8608 - 3200);
        // Aatrox (Eclipse 2800 + Steelcaps 1200) vs Malphite (Thornmail 2450 + Steelcaps 1200).
        assert_eq!(s.lanes["Malphite"], 4000 - 3650);
        // Thresh has no items vs Leona's Locket.
        assert_eq!(s.lanes["Leona"], -2200);
        assert_eq!(s.ally_total, 4000 + 8608 + 2900 + 3450);
        assert_eq!(s.enemy_total, 3650 + 3200 + 2900 + 3000 + 2200);
        assert_eq!(s.enemy_gold["Kaisa"], 3000);
    }

    #[test]
    fn falls_back_to_champ_select_roles_without_positions() {
        let mut g = game();
        for p in &mut g.all_players {
            p.position = "NONE".into();
        }
        assert!(summarize(&g, &dd(), &HashMap::new()).lanes.is_empty());
        let roles: HashMap<String, Role> = [("Briar", Role::Jungle), ("Viego", Role::Jungle), ("Ahri", Role::Mid)]
            .into_iter()
            .map(|(c, r)| (c.to_string(), r))
            .collect();
        let s = summarize(&g, &dd(), &roles);
        assert_eq!(s.lanes.len(), 1, "only Viego has a known lane partner");
        assert_eq!(s.lanes["Viego"], 8608 - 3200);
    }

    #[test]
    fn positions() {
        assert_eq!(role_of("MIDDLE"), Some(Role::Mid));
        assert_eq!(role_of("utility"), Some(Role::Support));
        assert_eq!(role_of("NONE"), None);
        assert_eq!(role_of(""), None);
    }
}
