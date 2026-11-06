//! Live item advisor (SPEC §5.9, flag `live_advisor`, off by default).
//!
//! 1. Threat score per enemy: 0.5·norm(item-gold lead over you) + 0.3·norm(kills + assists)
//!    + 0.2·norm(kills on you).
//! 2. The top one or two threats are classified from their class and **current** items: magic or
//!    physical damage, and healing.
//! 3. Candidates are items that answer those traits, from your champion's OP.GG item pool, minus
//!    items you own, ranked by smoothed win rate. Two or three options, each with its reason.
//! It only advises; it never acts.

use crate::builds::BuildSet;
use crate::builds::comp_profile::{HEALERS, Pick, Trait, answers, eligible};
use crate::ddragon::DData;

pub const MAX_PICKS: usize = 3;
const TOP_THREATS: usize = 2;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EnemyState {
    /// Data Dragon champion id.
    pub key: String,
    pub name: String,
    pub tags: Vec<String>,
    pub items: Vec<u32>,
    pub item_gold: u32,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub kills_on_me: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Threat {
    pub index: usize,
    pub score: f32,
    /// e.g. "Zed 6/0/2, killed you 3×".
    pub reason: String,
}

fn norm(values: &[f32]) -> Vec<f32> {
    let max = values.iter().cloned().fold(0.0, f32::max);
    values.iter().map(|v| if max > 0.0 { v / max } else { 0.0 }).collect()
}

/// Enemies ranked by threat, highest first. Enemies with no threat at all are left out.
pub fn threats(enemies: &[EnemyState], my_gold: u32) -> Vec<Threat> {
    let lead = norm(&enemies.iter().map(|e| e.item_gold.saturating_sub(my_gold) as f32).collect::<Vec<_>>());
    let ka = norm(&enemies.iter().map(|e| (e.kills + e.assists) as f32).collect::<Vec<_>>());
    let kom = norm(&enemies.iter().map(|e| e.kills_on_me as f32).collect::<Vec<_>>());
    let mut out: Vec<Threat> = enemies
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let mut reason = format!("{} {}/{}/{}", e.name, e.kills, e.deaths, e.assists);
            if e.kills_on_me > 0 {
                reason.push_str(&format!(", killed you {}×", e.kills_on_me));
            }
            Threat { index: i, score: 0.5 * lead[i] + 0.3 * ka[i] + 0.2 * kom[i], reason }
        })
        .filter(|t| t.score > 0.0)
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

/// What an enemy's damage and sustain look like right now: from their items, else their class.
pub fn traits_of(e: &EnemyState, dd: &DData) -> Vec<Trait> {
    let count = |tag: &str| e.items.iter().filter(|id| dd.items.get(id).is_some_and(|it| it.tags.iter().any(|t| t == tag))).count();
    let (ap, ad) = (count("SpellDamage"), count("Damage") + count("CriticalStrike"));
    let magic = if ap + ad > 0 { ap > ad } else { e.tags.first().is_some_and(|t| t == "Mage") };
    let mut out = vec![if magic { Trait::MagicDamage } else { Trait::PhysicalDamage }];
    if HEALERS.contains(&e.key.as_str()) || count("LifeSteal") + count("SpellVamp") > 0 {
        out.insert(0, Trait::Healing);
    }
    out
}

/// Two or three item suggestions against the biggest threats, with reasons.
pub fn advise(build: &BuildSet, enemies: &[EnemyState], my_gold: u32, owned: &[u32], dd: &DData) -> Vec<Pick> {
    let mut picks: Vec<Pick> = Vec::new();
    for threat in threats(enemies, my_gold).into_iter().take(TOP_THREATS) {
        for t in traits_of(&enemies[threat.index], dd) {
            for r in build.item_pool.iter().filter(|r| {
                !owned.contains(&r.value)
                    && !picks.iter().any(|p| p.item_id == r.value)
                    && dd.items.get(&r.value).is_some_and(|it| eligible(it) && answers(it, t, build.damage_type))
            }) {
                picks.push(Pick { item_id: r.value, reason: threat.reason.clone(), ranked: r.clone() });
                if picks.len() >= MAX_PICKS {
                    return picks;
                }
                break; // best answer per trait, then the next trait or threat
            }
        }
    }
    picks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builds::DamageType;
    use crate::builds::ranking::rank;
    use crate::ddragon::ItemInfo;
    use std::collections::HashMap;

    fn item(tags: &[&str], description: &str, gold: u32) -> ItemInfo {
        ItemInfo { name: String::new(), tags: tags.iter().map(|s| s.to_string()).collect(), description: description.into(), gold_total: gold, completed: true, on_rift: true }
    }

    fn dd() -> DData {
        let mut items = HashMap::new();
        items.insert(3157, item(&["SpellDamage", "Armor"], "", 3250)); // Zhonya's
        items.insert(3026, item(&["Damage", "Armor"], "", 3200)); // Guardian Angel
        items.insert(3156, item(&["Damage", "SpellBlock"], "", 2800)); // Maw
        items.insert(3033, item(&["Damage", "ArmorPenetration"], "inflicts 40% Grievous Wounds", 3000)); // Mortal Reminder
        items.insert(3089, item(&["SpellDamage"], "", 3600)); // Rabadon's
        items.insert(3142, item(&["Damage"], "", 2800)); // Youmuu's (enemy item)
        items.insert(3153, item(&["Damage", "LifeSteal"], "", 3200)); // BotRK (enemy item)
        DData { items, ..Default::default() }
    }

    fn build() -> BuildSet {
        let pool = rank(vec![(3157, 300, 160), (3026, 900, 470), (3156, 400, 215), (3033, 200, 104), (3089, 800, 420)], 0.5);
        BuildSet { item_pool: pool, damage_type: Some(DamageType::AD), ..Default::default() }
    }

    fn enemy(key: &str, items: &[u32], gold: u32, k: u32, d: u32, a: u32, on_me: u32) -> EnemyState {
        EnemyState { key: key.into(), name: key.into(), tags: vec![], items: items.to_vec(), item_gold: gold, kills: k, deaths: d, assists: a, kills_on_me: on_me }
    }

    #[test]
    fn threat_scores_weigh_gold_lead_takedowns_and_kills_on_you() {
        let es = vec![
            enemy("Zed", &[3142], 9000, 6, 0, 2, 3),
            enemy("Leona", &[], 5000, 1, 4, 9, 0),
            enemy("Malphite", &[], 4000, 0, 3, 1, 0),
        ];
        let t = threats(&es, 6000);
        assert_eq!(t[0].index, 0);
        assert_eq!(t[0].reason, "Zed 6/0/2, killed you 3×");
        // Zed: lead 1, takedowns 8/10, kills on me 1 → 0.5 + 0.24 + 0.2.
        assert!((t[0].score - 0.94).abs() < 1e-4);
        assert_eq!(t[1].reason, "Leona 1/4/9", "no 'killed you' when it didn't happen");
        assert_eq!(t.len(), 3);
    }

    #[test]
    fn traits_come_from_current_items() {
        let d = dd();
        assert_eq!(traits_of(&enemy("Zed", &[3142], 0, 0, 0, 0, 0), &d), vec![Trait::PhysicalDamage]);
        assert_eq!(traits_of(&enemy("Syndra", &[3089], 0, 0, 0, 0, 0), &d), vec![Trait::MagicDamage]);
        assert_eq!(traits_of(&enemy("Viego", &[3153], 0, 0, 0, 0, 0), &d), vec![Trait::Healing, Trait::PhysicalDamage]);
        let mage = EnemyState { tags: vec!["Mage".into()], ..enemy("Lux", &[], 0, 0, 0, 0, 0) };
        assert_eq!(traits_of(&mage, &d), vec![Trait::MagicDamage], "no items yet: use the class");
    }

    #[test]
    fn advice_answers_the_top_threats_and_skips_owned_items() {
        let d = dd();
        let es = vec![enemy("Zed", &[3142], 9000, 6, 0, 2, 3), enemy("Syndra", &[3089], 7000, 4, 1, 3, 1)];
        let picks = advise(&build(), &es, 6000, &[3026], &d);
        let ids: Vec<u32> = picks.iter().map(|p| p.item_id).collect();
        // Zed (physical): best armor item not owned → Zhonya's; Syndra (magic) → Maw.
        assert_eq!(ids, vec![3157, 3156]);
        assert_eq!(picks[0].reason, "Zed 6/0/2, killed you 3×");
        assert_eq!(picks[1].reason, "Syndra 4/1/3, killed you 1×");
    }

    #[test]
    fn healing_threat_gets_anti_heal_first() {
        let d = dd();
        let es = vec![enemy("Viego", &[3153], 9000, 7, 1, 1, 2)];
        let ids: Vec<u32> = advise(&build(), &es, 5000, &[], &d).iter().map(|p| p.item_id).collect();
        assert_eq!(ids, vec![3033, 3157], "Mortal Reminder for the healing, then the best-ranked armor item (Zhonya's)");
    }

    #[test]
    fn nothing_when_no_one_is_a_threat() {
        assert!(advise(&build(), &[enemy("A", &[], 1000, 0, 0, 0, 0)], 5000, &[], &dd()).is_empty());
    }
}
