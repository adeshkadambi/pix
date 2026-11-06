//! Enemy purchase feed (SPEC §5.9): compare each enemy's item multiset between ticks and keep
//! the last 10 notable purchases. Enemies are keyed by champion, never by player name.

use std::collections::{HashMap, VecDeque};

use crate::ddragon::DData;
use crate::live::models::{AllGameData, Item};

pub const FEED_LEN: usize = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct Purchase {
    pub game_time: f64,
    /// Data Dragon champion id of the buyer.
    pub champion: String,
    pub item_id: u32,
}

#[derive(Debug, Default)]
pub struct PurchaseFeed {
    last: HashMap<String, Vec<Item>>,
    pub recent: VecDeque<Purchase>,
}

fn counts(items: &[Item]) -> HashMap<u32, u32> {
    let mut m = HashMap::new();
    for it in items {
        *m.entry(it.item_id).or_insert(0) += it.count.max(1);
    }
    m
}

/// Items added between two inventories (one entry per added copy).
pub fn added(before: &[Item], after: &[Item]) -> Vec<u32> {
    let (b, a) = (counts(before), counts(after));
    let mut out: Vec<u32> = a
        .iter()
        .flat_map(|(id, n)| std::iter::repeat_n(*id, n.saturating_sub(*b.get(id).unwrap_or(&0)) as usize))
        .collect();
    out.sort_unstable();
    out
}

/// Worth a line in the feed: finished items and boots, not potions, wards, trinkets or components.
pub fn notable(item_id: u32, dd: &DData) -> bool {
    dd.items.get(&item_id).is_some_and(|it| {
        let tag = |t: &str| it.tags.iter().any(|x| x == t);
        !tag("Consumable") && !tag("Trinket") && (it.completed || tag("Boots")) && it.gold_total >= 300
    })
}

impl PurchaseFeed {
    /// Updates from a new snapshot. Returns true if new purchases were added.
    /// The first snapshot only records inventories (no "purchases" when Pix starts mid-game).
    pub fn update(&mut self, game: &AllGameData, dd: &DData) -> bool {
        let mut changed = false;
        for enemy in game.enemies() {
            let key = enemy.ddragon_id().to_owned();
            if let Some(before) = self.last.get(&key) {
                for item_id in added(before, &enemy.items).into_iter().filter(|id| notable(*id, dd)) {
                    self.recent.push_front(Purchase { game_time: game.game_data.game_time, champion: key.clone(), item_id });
                    changed = true;
                }
            }
            self.last.insert(key, enemy.items.clone());
        }
        self.recent.truncate(FEED_LEN);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddragon::ItemInfo;

    fn item(id: u32, count: u32) -> Item {
        Item { item_id: id, count, ..Default::default() }
    }

    fn info(tags: &[&str], completed: bool, gold: u32) -> ItemInfo {
        ItemInfo { name: String::new(), tags: tags.iter().map(|s| s.to_string()).collect(), description: String::new(), gold_total: gold, completed, on_rift: true }
    }

    fn dd() -> DData {
        let mut items = HashMap::new();
        items.insert(3075, info(&["Armor"], true, 2450)); // Thornmail
        items.insert(3047, info(&["Boots", "Armor"], true, 1200)); // Steelcaps
        items.insert(1029, info(&["Armor"], false, 300)); // Cloth Armor (component)
        items.insert(2003, info(&["Consumable"], true, 50)); // Health Potion
        items.insert(3340, info(&["Trinket"], true, 0)); // Ward trinket
        DData { items, ..Default::default() }
    }

    #[test]
    fn multiset_diff() {
        assert_eq!(added(&[item(1036, 1)], &[item(1036, 2), item(3078, 1)]), vec![1036, 3078]);
        assert!(added(&[item(3078, 1)], &[item(3078, 1)]).is_empty());
        assert!(added(&[item(1036, 2)], &[item(3071, 1)]) == vec![3071], "components turning into an item");
    }

    #[test]
    fn notable_items_only() {
        let d = dd();
        assert!(notable(3075, &d) && notable(3047, &d));
        assert!(!notable(1029, &d) && !notable(2003, &d) && !notable(3340, &d) && !notable(9999, &d));
    }

    fn snapshot(time: f64, malphite: Vec<Item>) -> AllGameData {
        serde_json::from_value(serde_json::json!({
            "activePlayer": {"riotId": "Me#1"},
            "allPlayers": [
                {"riotId": "Me#1", "team": "ORDER", "rawChampionName": "game_character_displayname_Briar"},
                {"riotId": "X#1", "team": "CHAOS", "rawChampionName": "game_character_displayname_Malphite",
                 "items": malphite.iter().map(|i| serde_json::json!({"itemID": i.item_id, "count": i.count})).collect::<Vec<_>>()}
            ],
            "gameData": {"gameTime": time}
        }))
        .unwrap()
    }

    #[test]
    fn feed_records_new_purchases_after_the_first_snapshot() {
        let d = dd();
        let mut feed = PurchaseFeed::default();
        assert!(!feed.update(&snapshot(600.0, vec![item(3047, 1)]), &d), "first snapshot is a baseline");
        assert!(!feed.update(&snapshot(601.0, vec![item(3047, 1), item(1029, 1), item(2003, 1)]), &d), "components and potions are skipped");
        assert!(feed.update(&snapshot(900.0, vec![item(3047, 1), item(3075, 1)]), &d));
        assert_eq!(feed.recent.front(), Some(&Purchase { game_time: 900.0, champion: "Malphite".into(), item_id: 3075 }));
    }

    #[test]
    fn feed_keeps_the_last_ten() {
        let mut d = dd();
        for id in 4000..4012 {
            d.items.insert(id, info(&[], true, 3000));
        }
        let mut feed = PurchaseFeed::default();
        feed.update(&snapshot(0.0, vec![]), &d);
        let mut inv = Vec::new();
        for (n, id) in (4000..4012).enumerate() {
            inv.push(item(id, 1));
            feed.update(&snapshot(n as f64, inv.clone()), &d);
        }
        assert_eq!(feed.recent.len(), FEED_LEN);
        assert_eq!(feed.recent.front().unwrap().item_id, 4011, "newest first");
    }
}
