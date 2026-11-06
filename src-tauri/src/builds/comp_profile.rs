//! Comp profile and situational picks (SPEC §5.5).
//!
//! From the enemy champions only (pre-game): damage split (OP.GG damage type), class tags (Data
//! Dragon) and healing (curated list). Each trait maps to item tags. Only items the champion
//! actually builds (`BuildSet.item_pool`) are considered, ranked by smoothed win rate, and each
//! pick carries its reason.

use crate::ddragon::{DData, ItemInfo};

use super::{BuildSet, DamageType, Ranked};

/// Champions whose healing is a main threat (Data Dragon ids). Anti-heal is suggested against them.
pub const HEALERS: &[&str] = &[
    "Aatrox", "Briar", "DrMundo", "Fiddlesticks", "Illaoi", "Maokai", "Milio", "Nami", "Nidalee", "Olaf", "Senna",
    "Seraphine", "Sona", "Soraka", "Swain", "Sylas", "Vladimir", "Volibear", "Warwick", "Yuumi", "Zac",
];

const PICKS_PER_TRAIT: usize = 2;
const MIN_LEGENDARY_GOLD: u32 = 2000;

#[derive(Debug, Clone, PartialEq)]
pub struct EnemyChampion {
    pub name: String,
    /// Data Dragon id.
    pub id: String,
    pub tags: Vec<String>,
    pub damage: Option<DamageType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trait {
    MagicDamage,
    PhysicalDamage,
    Healing,
    Tanks,
    CrowdControl,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub item_id: u32,
    pub reason: String,
    pub ranked: Ranked<u32>,
}

/// Which traits the enemy team has, with the reason text for each.
pub fn traits(enemies: &[EnemyChampion]) -> Vec<(Trait, String)> {
    let n = enemies.len();
    if n == 0 {
        return Vec::new();
    }
    let count = |d: DamageType| enemies.iter().filter(|e| e.damage == Some(d)).count();
    let (ap, ad, both) = (count(DamageType::AP), count(DamageType::AD), count(DamageType::BOTH));
    let names = |f: &dyn Fn(&EnemyChampion) -> bool| -> Vec<String> {
        enemies.iter().filter(|e| f(e)).map(|e| e.name.clone()).collect()
    };
    let mut out = Vec::new();
    // Mixed-damage champions count half toward each side.
    if ap as f32 + both as f32 * 0.5 >= 2.5 {
        out.push((Trait::MagicDamage, damage_reason(ap, both, n, "AP")));
    }
    if ad as f32 + both as f32 * 0.5 >= 2.5 {
        out.push((Trait::PhysicalDamage, damage_reason(ad, both, n, "AD")));
    }
    let healers = names(&|e| HEALERS.contains(&e.id.as_str()));
    if !healers.is_empty() {
        out.push((Trait::Healing, format!("{} healing", healers.join(" + "))));
    }
    let tanks = names(&|e| e.tags.first().is_some_and(|t| t == "Tank"));
    if tanks.len() >= 2 {
        out.push((Trait::Tanks, format!("{} tanks: {}", tanks.len(), tanks.join(", "))));
    }
    let cc = names(&|e| e.tags.iter().any(|t| t == "Tank" || t == "Support"));
    if cc.len() >= 3 {
        out.push((Trait::CrowdControl, format!("lots of crowd control: {}", cc.join(", "))));
    }
    out
}

fn damage_reason(main: usize, both: usize, n: usize, kind: &str) -> String {
    if both == 0 { format!("{main}/{n} enemies {kind}") } else { format!("{main}/{n} enemies {kind}, {both} mixed") }
}

/// Does this item answer the trait? `mine` is the player's own damage type (for penetration).
pub fn answers(item: &ItemInfo, t: Trait, mine: Option<DamageType>) -> bool {
    let has = |tag: &str| item.tags.iter().any(|x| x == tag);
    match t {
        Trait::MagicDamage => has("SpellBlock"),
        Trait::PhysicalDamage => has("Armor"),
        Trait::Healing => item.description.contains("Grievous Wounds"),
        // Percent penetration only: lethality and flat magic pen are weak against tanks, but
        // Data Dragon tags them the same way.
        Trait::Tanks => {
            static SHRED: std::sync::OnceLock<(regex::Regex, regex::Regex)> = std::sync::OnceLock::new();
            let (armor_shred, mr_shred) = SHRED.get_or_init(|| {
                (
                    regex::Regex::new(r"reduces? (?:the target's |their )?Armor by \d").unwrap(),
                    regex::Regex::new(r"reduces? (?:the target's |their )?Magic Resist by \d").unwrap(),
                )
            });
            let d = &item.description;
            let armor = has("ArmorPenetration") && (d.contains("% Armor Penetration") || armor_shred.is_match(d));
            let magic = has("MagicPenetration") && (d.contains("% Magic Penetration") || mr_shred.is_match(d));
            match mine {
                Some(DamageType::AP) => magic,
                Some(DamageType::AD) => armor,
                _ => armor || magic,
            }
        }
        Trait::CrowdControl => has("Tenacity"),
    }
}

fn eligible(item: &ItemInfo) -> bool {
    item.completed && item.on_rift && (item.gold_total >= MIN_LEGENDARY_GOLD || item.tags.iter().any(|t| t == "Boots"))
}

/// Situational picks for the enemy comp: up to two per trait, from items this champion builds,
/// skipping items already in `exclude` (the main build).
pub fn situational(build: &BuildSet, enemies: &[EnemyChampion], dd: &DData, exclude: &[u32]) -> Vec<Pick> {
    let mut out: Vec<Pick> = Vec::new();
    for (t, reason) in traits(enemies) {
        let picks = build
            .item_pool
            .iter()
            .filter(|r| !exclude.contains(&r.value) && !out.iter().any(|p| p.item_id == r.value))
            .filter(|r| dd.items.get(&r.value).is_some_and(|it| eligible(it) && answers(it, t, build.damage_type)))
            .take(PICKS_PER_TRAIT)
            .map(|r| Pick { item_id: r.value, reason: reason.clone(), ranked: r.clone() })
            .collect::<Vec<_>>();
        out.extend(picks);
    }
    out
}

/// Threat card for one enemy: the top one or two answers to that champion's own traits.
pub fn threat_answers(build: &BuildSet, enemy: &EnemyChampion, dd: &DData) -> Vec<Pick> {
    let mut wanted: Vec<(Trait, String)> = Vec::new();
    match enemy.damage {
        Some(DamageType::AP) => wanted.push((Trait::MagicDamage, format!("{} deals magic damage", enemy.name))),
        Some(DamageType::AD) => wanted.push((Trait::PhysicalDamage, format!("{} deals physical damage", enemy.name))),
        _ => {}
    }
    if HEALERS.contains(&enemy.id.as_str()) {
        wanted.insert(0, (Trait::Healing, format!("{} heals", enemy.name)));
    }
    if enemy.tags.first().is_some_and(|t| t == "Tank") {
        wanted.push((Trait::Tanks, format!("{} is a tank", enemy.name)));
    }
    let mut out: Vec<Pick> = Vec::new();
    for (t, reason) in wanted {
        if let Some(r) = build
            .item_pool
            .iter()
            .filter(|r| !out.iter().any(|p| p.item_id == r.value))
            .find(|r| dd.items.get(&r.value).is_some_and(|it| eligible(it) && answers(it, t, build.damage_type)))
        {
            out.push(Pick { item_id: r.value, reason, ranked: r.clone() });
        }
        if out.len() == 2 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builds::ranking::rank;
    use std::collections::HashMap;

    fn enemy(name: &str, tags: &[&str], damage: DamageType) -> EnemyChampion {
        EnemyChampion { name: name.into(), id: name.replace(['\'', ' ', '.'], ""), tags: tags.iter().map(|s| s.to_string()).collect(), damage: Some(damage) }
    }

    fn item(name: &str, tags: &[&str], description: &str, gold: u32) -> ItemInfo {
        ItemInfo { name: name.into(), tags: tags.iter().map(|s| s.to_string()).collect(), description: description.into(), gold_total: gold, completed: true, on_rift: true }
    }

    fn dd() -> DData {
        let mut items = HashMap::new();
        items.insert(3111, item("Mercury's Treads", &["Boots", "SpellBlock", "Tenacity"], "", 1250));
        items.insert(3047, item("Plated Steelcaps", &["Boots", "Armor"], "", 1200));
        items.insert(3036, item("Lord Dominik's Regards", &["Damage", "ArmorPenetration"], "+35 Attack Damage +35% Armor Penetration", 3100));
        items.insert(6695, item("Serpent's Fang", &["Damage", "ArmorPenetration"], "+55 Attack Damage +15 Lethality", 2500));
        items.insert(6609, item("Chempunk Chainsword", &["Damage", "Health"], "inflicts 40% Grievous Wounds", 2800));
        items.insert(3156, item("Maw of Malmortius", &["Damage", "SpellBlock"], "", 2800));
        items.insert(3026, item("Guardian Angel", &["Damage", "Armor"], "", 3200));
        items.insert(3071, item("Black Cleaver", &["Damage", "Health", "ArmorPenetration"], "", 3000));
        items.insert(1001, item("Boots", &["Boots"], "", 300));
        DData { items, ..Default::default() }
    }

    fn build() -> BuildSet {
        // (item, play, win): ranked by smoothed WR with a 50% prior.
        let pool = rank(
            vec![
                (3111, 500, 270), (3047, 900, 460), (3036, 400, 220), (6609, 120, 64),
                (3156, 200, 101), (3026, 300, 160), (3071, 2000, 1000), (1001, 50, 20), (6695, 10, 7),
            ],
            0.5,
        );
        BuildSet { item_pool: pool, damage_type: Some(DamageType::AD), ..Default::default() }
    }

    #[test]
    fn traits_with_reasons() {
        let comp = vec![
            enemy("Malphite", &["Tank", "Mage"], DamageType::AP),
            enemy("Viego", &["Fighter", "Assassin"], DamageType::AD),
            enemy("Syndra", &["Mage", "Support"], DamageType::AP),
            enemy("Soraka", &["Support", "Mage"], DamageType::AP),
            enemy("Leona", &["Tank", "Support"], DamageType::AD),
        ];
        let t = traits(&comp);
        assert!(t.contains(&(Trait::MagicDamage, "3/5 enemies AP".into())));
        assert!(t.contains(&(Trait::Healing, "Soraka healing".into())));
        assert!(t.contains(&(Trait::Tanks, "2 tanks: Malphite, Leona".into())));
        assert!(t.iter().any(|(x, _)| *x == Trait::CrowdControl));
        assert!(!t.iter().any(|(x, _)| *x == Trait::PhysicalDamage));
    }

    #[test]
    fn mixed_damage_counts_half() {
        let comp = vec![
            enemy("A", &["Mage"], DamageType::AP),
            enemy("B", &["Mage"], DamageType::AP),
            enemy("Kaisa", &["Marksman"], DamageType::BOTH),
        ];
        assert_eq!(traits(&comp)[0], (Trait::MagicDamage, "2/3 enemies AP, 1 mixed".into()));
    }

    #[test]
    fn picks_only_from_the_pool_and_skip_the_main_build() {
        let comp = vec![
            enemy("Malphite", &["Tank"], DamageType::AP),
            enemy("Syndra", &["Mage"], DamageType::AP),
            enemy("Soraka", &["Support"], DamageType::AP),
            enemy("Leona", &["Tank", "Support"], DamageType::AD),
        ];
        let picks = situational(&build(), &comp, &dd(), &[3071]);
        let by_reason = |r: &str| picks.iter().filter(|p| p.reason.starts_with(r)).map(|p| p.item_id).collect::<Vec<_>>();
        assert_eq!(by_reason("3/4 enemies AP"), vec![3111, 3156], "MR options, best first");
        assert_eq!(by_reason("Soraka"), vec![6609]);
        assert_eq!(by_reason("2 tanks"), vec![3036], "Black Cleaver is in the main build; lethality doesn't count");
        // Plain Boots never qualify (not a finished answer to anything).
        assert!(!picks.iter().any(|p| p.item_id == 1001));
    }

    #[test]
    fn anti_tank_means_percent_pen_or_shred() {
        let tanks = |it: &ItemInfo, mine| answers(it, Trait::Tanks, Some(mine));
        let ldr = item("LDR", &["ArmorPenetration"], "+35 Attack Damage 35% Armor Penetration", 3100);
        let cleaver = item("BC", &["ArmorPenetration"], "Carve Dealing physical damage to champions reduces the target's Armor by 6% for 6 seconds.", 3000);
        let lethality = item("Fang", &["ArmorPenetration"], "+15 Lethality", 2500);
        let void = item("Void", &["MagicPenetration"], "95 Ability Power 40% Magic Penetration", 3000);
        let curse = item("Curse", &["MagicPenetration"], "reduces their Magic Resist by 7.5% for 6 seconds.", 2900);
        let flat = item("Storm", &["MagicPenetration"], "15 Magic Penetration", 2900);
        assert!(tanks(&ldr, DamageType::AD) && tanks(&cleaver, DamageType::AD) && !tanks(&lethality, DamageType::AD));
        assert!(tanks(&void, DamageType::AP) && tanks(&curse, DamageType::AP) && !tanks(&flat, DamageType::AP));
        assert!(!tanks(&void, DamageType::AD), "AD champions get armor answers");
    }

    #[test]
    fn threat_card_answers_one_champion() {
        let soraka = enemy("Soraka", &["Support", "Mage"], DamageType::AP);
        let answers = threat_answers(&build(), &soraka, &dd());
        assert_eq!(answers.iter().map(|p| p.item_id).collect::<Vec<_>>(), vec![6609, 3111]);
        assert_eq!(answers[0].reason, "Soraka heals");
    }
}
