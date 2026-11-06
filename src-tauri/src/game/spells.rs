//! Summoner spell timers (SPEC §5.8, gray flag `overlay_timers`).
//!
//! - Cooldown = base / (1 + haste / 100), worked out when you click, from the enemy's items at
//!   that moment.
//! - Haste comes from Data Dragon item descriptions ("N Summoner Spell Haste"), never hardcoded,
//!   plus +18 when you tick Cosmic Insight for that enemy.
//! - Timers run on game time, so pauses and lag don't break them.
//! - Smite isn't tracked (charges).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;

use crate::ddragon::{DData, SpellInfo};
use crate::live::models::Item;

pub const COSMIC_INSIGHT_HASTE: u32 = 18;
const UNTRACKED: &[&str] = &["SummonerSmite"];

/// Summoner spell haste from a set of items, read from their Data Dragon descriptions.
pub fn item_haste(items: &[Item], dd: &DData) -> u32 {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?i)(\d+)\s*Summoner Spell Haste").unwrap());
    items
        .iter()
        .filter_map(|it| {
            let desc = &dd.items.get(&it.item_id)?.description;
            let n: u32 = re.captures(desc)?.get(1)?.as_str().parse().ok()?;
            Some(n * it.count.max(1))
        })
        .sum()
}

pub fn cooldown(base: f32, haste: u32) -> f32 {
    base / (1.0 + haste as f32 / 100.0)
}

/// Data Dragon spell for a Live Client spell key, tolerating renamed variants
/// ("S12_SummonerTeleportUpgrade" → "SummonerTeleport").
pub fn spell_info<'a>(key: &str, dd: &'a DData) -> Option<&'a SpellInfo> {
    if let Some(s) = dd.summoners.get(key) {
        return Some(s);
    }
    let trimmed = key.rsplit_once('_').map(|(_, k)| k).unwrap_or(key);
    let base = trimmed.trim_end_matches("Upgrade");
    dd.summoners.get(trimmed).or_else(|| dd.summoners.get(base))
}

pub fn trackable(key: &str) -> bool {
    !UNTRACKED.iter().any(|u| key.contains(u))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timer {
    pub started_at: f64,
    pub duration: f32,
}

impl Timer {
    pub fn remaining(&self, game_time: f64) -> f32 {
        (self.started_at + self.duration as f64 - game_time).max(0.0) as f32
    }
}

/// Running timers and Cosmic Insight ticks for one game, keyed by enemy champion (never player).
#[derive(Debug, Default)]
pub struct TimerBook {
    timers: HashMap<(String, u8), Timer>,
    cosmic: HashSet<String>,
}

impl TimerBook {
    /// Starts the timer, or clears it if it's already running (undo a misclick).
    /// Returns true if a timer is now running.
    pub fn toggle(&mut self, champion: &str, slot: u8, game_time: f64, duration: f32) -> bool {
        let key = (champion.to_owned(), slot);
        if self.running(champion, slot, game_time).is_some() {
            self.timers.remove(&key);
            false
        } else {
            self.timers.insert(key, Timer { started_at: game_time, duration });
            true
        }
    }

    /// The timer if it's still counting down.
    pub fn running(&self, champion: &str, slot: u8, game_time: f64) -> Option<Timer> {
        self.timers.get(&(champion.to_owned(), slot)).copied().filter(|t| t.remaining(game_time) > 0.0)
    }

    pub fn set_cosmic(&mut self, champion: &str, on: bool) {
        if on {
            self.cosmic.insert(champion.to_owned());
        } else {
            self.cosmic.remove(champion);
        }
    }

    pub fn cosmic(&self, champion: &str) -> bool {
        self.cosmic.contains(champion)
    }

    /// Drops finished timers.
    pub fn prune(&mut self, game_time: f64) {
        self.timers.retain(|_, t| t.remaining(game_time) > 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddragon::ItemInfo;

    fn dd() -> DData {
        let mut items = HashMap::new();
        let info = |desc: &str| ItemInfo { name: String::new(), tags: vec![], description: desc.into(), gold_total: 0, completed: true, on_rift: true };
        items.insert(3158, info("Ionian Boots of Lucidity 10 Ability Haste 10 Summoner Spell Haste 45 Move Speed"));
        items.insert(3047, info("Plated Steelcaps 20 Armor"));
        let mut summoners = HashMap::new();
        summoners.insert("SummonerFlash".to_string(), SpellInfo { id: 4, name: "Flash".into(), cooldown: 300.0 });
        summoners.insert("SummonerTeleport".to_string(), SpellInfo { id: 12, name: "Teleport".into(), cooldown: 360.0 });
        DData { items, summoners, ..Default::default() }
    }

    fn item(id: u32) -> Item {
        Item { item_id: id, count: 1, ..Default::default() }
    }

    #[test]
    fn haste_from_item_descriptions() {
        let d = dd();
        assert_eq!(item_haste(&[item(3158), item(3047)], &d), 10);
        assert_eq!(item_haste(&[item(3047)], &d), 0);
        assert_eq!(item_haste(&[], &d), 0);
    }

    #[test]
    fn cooldown_formula() {
        assert!((cooldown(300.0, 0) - 300.0).abs() < 1e-3);
        // Lucidity boots + Cosmic Insight: 300 / 1.28.
        assert!((cooldown(300.0, 10 + COSMIC_INSIGHT_HASTE) - 234.375).abs() < 1e-3);
    }

    #[test]
    fn spell_lookup_tolerates_renamed_variants() {
        let d = dd();
        assert_eq!(spell_info("SummonerFlash", &d).unwrap().id, 4);
        assert_eq!(spell_info("S12_SummonerTeleportUpgrade", &d).unwrap().id, 12);
        assert!(spell_info("SummonerNope", &d).is_none());
        assert!(!trackable("SummonerSmite") && !trackable("S5_SummonerSmiteDuel") && trackable("SummonerFlash"));
    }

    #[test]
    fn click_starts_click_again_clears() {
        let mut book = TimerBook::default();
        assert!(book.toggle("Viego", 0, 600.0, 300.0));
        assert_eq!(book.running("Viego", 0, 700.0).unwrap().remaining(700.0), 200.0);
        assert!(!book.toggle("Viego", 0, 610.0, 300.0), "second click clears");
        assert!(book.running("Viego", 0, 611.0).is_none());
        // Other spell and other champion are independent.
        book.toggle("Viego", 1, 600.0, 90.0);
        book.toggle("Syndra", 0, 600.0, 300.0);
        assert!(book.running("Viego", 0, 601.0).is_none());
        assert!(book.running("Viego", 1, 601.0).is_some() && book.running("Syndra", 0, 601.0).is_some());
    }

    #[test]
    fn finished_timers_stop_and_can_restart() {
        let mut book = TimerBook::default();
        book.toggle("Leona", 1, 100.0, 180.0);
        assert!(book.running("Leona", 1, 280.5).is_none(), "finished");
        assert!(book.toggle("Leona", 1, 300.0, 180.0), "a click after it finished starts a new one");
        book.prune(1000.0);
        assert!(book.running("Leona", 1, 300.0).is_none());
    }

    #[test]
    fn cosmic_ticks() {
        let mut book = TimerBook::default();
        book.set_cosmic("Kaisa", true);
        assert!(book.cosmic("Kaisa") && !book.cosmic("Viego"));
        book.set_cosmic("Kaisa", false);
        assert!(!book.cosmic("Kaisa"));
    }
}
