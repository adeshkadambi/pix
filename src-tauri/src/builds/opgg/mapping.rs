//! OP.GG replies → `BuildSet` / `ChampionTraits`.
//!
//! - `lol_get_champion_analysis` (class notation): single top picks for core, boots, starter,
//!   spells, runes and skill order, plus ranked lists for the 4th/5th/6th items.
//! - `lol_get_lane_matchup_guide` (JSON): ranked lists for everything; the head-to-head win rate
//!   comes from `counters[]` for the opponent.
//! Missing fields are treated as empty, since low-sample champions omit some.

use std::collections::HashMap;

use serde_json::Value;

use crate::builds::ranking::rank;
use crate::builds::{BuildSet, ChampRecord, ChampionTraits, DamageType, MetaEntry, Ranked, Role, RunePage};

fn u32_of(v: &Value, key: &str) -> u32 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0) as u32
}

fn f32_of(v: Option<&Value>) -> Option<f32> {
    v.and_then(Value::as_f64).map(|f| f as f32)
}

fn ids(v: &Value) -> Vec<u32> {
    v.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u32)).collect()).unwrap_or_default()
}

fn strings(v: &Value) -> Vec<String> {
    v.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

/// An object or a list of objects, as a list.
fn entries(v: Option<&Value>) -> Vec<&Value> {
    match v {
        Some(Value::Array(a)) => a.iter().filter(|x| x.is_object()).collect(),
        Some(o @ Value::Object(_)) => vec![o],
        _ => Vec::new(),
    }
}

fn rows<T>(v: Option<&Value>, f: impl Fn(&Value) -> Option<T>) -> Vec<(T, u32, u32)> {
    entries(v).into_iter().filter_map(|e| Some((f(e)?, u32_of(e, "play"), u32_of(e, "win")))).collect()
}

fn rune_page(v: &Value) -> Option<RunePage> {
    let list = |k: &str| -> Vec<u32> {
        v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u32)).collect()).unwrap_or_default()
    };
    let mut perks = list("primary_rune_ids");
    perks.extend(list("secondary_rune_ids"));
    let page = RunePage {
        primary_style: v.get("primary_page_id")?.as_u64()? as u32,
        sub_style: v.get("secondary_page_id")?.as_u64()? as u32,
        perks,
        shards: list("stat_mod_ids"),
    };
    (page.perks.len() == 6).then_some(page)
}

fn role_rates(summary: Option<&Value>) -> Vec<(Role, f32)> {
    entries(summary.and_then(|s| s.get("positions")))
        .into_iter()
        .filter_map(|p| {
            let role = Role::from_opgg(p.get("name")?.as_str()?)?;
            Some((role, f32_of(p.pointer("/stats/role_rate"))?))
        })
        .collect()
}

fn single(ids: Vec<u32>) -> Option<u32> {
    (ids.len() == 1).then(|| ids[0])
}

/// Every item seen in the build rows, keeping each item's largest sample.
fn item_pool(sources: &[&Vec<(Vec<u32>, u32, u32)>], prior: f32) -> Vec<Ranked<u32>> {
    let mut best: HashMap<u32, (u32, u32)> = HashMap::new();
    for rows in sources {
        for (items, play, win) in rows.iter() {
            for id in items {
                let e = best.entry(*id).or_insert((0, 0));
                if *play > e.0 {
                    *e = (*play, *win);
                }
            }
        }
    }
    rank(best.into_iter().map(|(id, (p, w))| (id, p, w)), prior)
}

pub fn analysis_to_buildset(v: &Value) -> BuildSet {
    let data = v.get("data").unwrap_or(&Value::Null);
    let avg = data.pointer("/summary/average_stats");
    let prior = f32_of(avg.and_then(|a| a.get("win_rate"))).unwrap_or(0.5);
    let games = avg.map(|a| u32_of(a, "play")).unwrap_or(0);

    let core = rows(data.get("core_items"), |e| Some(ids(e)).filter(|i| !i.is_empty()));
    let boots = rows(data.get("boots"), |e| single(ids(e)));
    let starters = rows(data.get("starter_items"), |e| Some(ids(e)).filter(|i| !i.is_empty()));
    let spells = rows(data.get("summoner_spells"), |e| Some(ids(e)).filter(|i| i.len() == 2));
    let later: Vec<Vec<(Vec<u32>, u32, u32)>> = ["fourth_items", "fifth_items", "sixth_items"]
        .iter()
        .map(|k| rows(data.get(*k), |e| Some(ids(e)).filter(|i| i.len() == 1)))
        .collect();
    let last = rows(data.get("last_items"), |e| Some(ids(e)).filter(|i| !i.is_empty()));
    let boots_rows: Vec<(Vec<u32>, u32, u32)> = boots.iter().map(|(b, p, w)| (vec![*b], *p, *w)).collect();

    let mut pool_sources: Vec<&Vec<(Vec<u32>, u32, u32)>> = vec![&core, &last, &boots_rows];
    pool_sources.extend(later.iter());
    let item_pool = item_pool(&pool_sources, prior);

    BuildSet {
        champion_wr: prior,
        champion_games: games,
        rune_pages: rank(rows(data.get("runes"), rune_page), prior),
        core_sets: rank(core, prior),
        boots: rank(boots, prior),
        starters: rank(starters, prior),
        summoners: rank(spells, prior),
        later_items: later.into_iter().map(|r| rank(r.into_iter().map(|(i, p, w)| (i[0], p, w)), prior)).collect(),
        skill_orders: rank(rows(data.get("skill_masteries"), |e| Some(strings(e)).filter(|s| !s.is_empty())), prior),
        item_pool,
        damage_type: data.get("damage_type").and_then(Value::as_str).and_then(DamageType::parse),
        role_rates: role_rates(data.get("summary")),
    }
}

pub fn analysis_to_traits(v: &Value) -> ChampionTraits {
    let data = v.get("data").unwrap_or(&Value::Null);
    ChampionTraits {
        role_rates: role_rates(data.get("summary")),
        damage_type: data.get("damage_type").and_then(Value::as_str).and_then(DamageType::parse),
    }
}

/// `opponent_name` is the champion's display name, as in `counters[].champion_name`.
pub fn matchup_to_buildset(v: &Value) -> BuildSet {
    let data = v.get("data").unwrap_or(&Value::Null);
    let opponent = v.get("opponent_champion").and_then(Value::as_str).unwrap_or_default();
    let avg = data.pointer("/summary/average_stats");
    let overall = f32_of(avg.and_then(|a| a.get("win_rate"))).unwrap_or(0.5);
    // Head-to-head win rate vs this opponent, when the sample is big enough.
    let head_to_head = entries(data.get("counters")).into_iter().find_map(|c| {
        let name = c.get("champion_name")?.as_str()?;
        let (play, win) = (u32_of(c, "play"), u32_of(c, "win"));
        (name.eq_ignore_ascii_case(opponent) && play >= 30).then(|| (win as f32 / play as f32, play))
    });
    let (prior, games) = head_to_head.unwrap_or((overall, avg.map(|a| u32_of(a, "play")).unwrap_or(0)));

    let core = rows(data.get("core_items"), |e| Some(ids(e)).filter(|i| !i.is_empty()));
    let boots = rows(data.get("boots"), |e| single(ids(e)));
    let starters = rows(data.get("starter_items"), |e| Some(ids(e)).filter(|i| !i.is_empty()));
    let spells = rows(data.get("summoner_spells"), |e| Some(ids(e)).filter(|i| i.len() == 2));
    let last = rows(data.get("last_items"), |e| Some(ids(e)).filter(|i| !i.is_empty()));
    let by_depth = |depth: u64| -> Vec<(Vec<u32>, u32, u32)> {
        entries(data.get("single_items"))
            .into_iter()
            .find(|s| s.get("depth").and_then(Value::as_u64) == Some(depth))
            .map(|s| rows(s.get("items"), |e| Some(ids(e)).filter(|i| i.len() == 1)))
            .unwrap_or_default()
    };
    let later: Vec<Vec<(Vec<u32>, u32, u32)>> = vec![by_depth(4), by_depth(5)];
    let singles: Vec<(Vec<u32>, u32, u32)> = (1..=5).flat_map(by_depth).collect();
    let boots_rows: Vec<(Vec<u32>, u32, u32)> = boots.iter().map(|(b, p, w)| (vec![*b], *p, *w)).collect();
    let item_pool = item_pool(&[&core, &last, &singles, &boots_rows], prior);

    BuildSet {
        champion_wr: prior,
        champion_games: games,
        rune_pages: rank(rows(data.get("runes"), rune_page), prior),
        core_sets: rank(core, prior),
        boots: rank(boots, prior),
        starters: rank(starters, prior),
        summoners: rank(spells, prior),
        later_items: later.into_iter().map(|r| rank(r.into_iter().map(|(i, p, w)| (i[0], p, w)), prior)).collect(),
        skill_orders: rank(rows(data.get("skill_masteries"), |e| Some(strings(e)).filter(|s| !s.is_empty())), prior),
        item_pool,
        damage_type: None,
        role_rates: role_rates(data.get("summary")),
    }
}

/// `lol_list_lane_meta_champions` → the role's tier list.
pub fn meta_entries(v: &Value, role: Role) -> Vec<MetaEntry> {
    entries(v.pointer(&format!("/data/positions/{}", role.opgg())))
        .into_iter()
        .filter_map(|e| {
            Some(MetaEntry {
                name: e.get("champion")?.as_str()?.to_owned(),
                play: u32_of(e, "play"),
                win: u32_of(e, "win"),
                role_rate: f32_of(e.get("role_rate")).unwrap_or(0.0),
                tier: u32_of(e, "tier") as u8,
                rip: e.get("is_rip").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect()
}

/// A matchup reply asked *as the enemy* lists the enemy's record against every lane opponent
/// (`counters[]`, wins on the enemy's side). Flipped, it's each challenger's record vs the enemy.
pub fn counters_to_enemy(v: &Value) -> Vec<ChampRecord> {
    entries(v.pointer("/data/counters"))
        .into_iter()
        .filter_map(|c| {
            let (play, enemy_wins) = (u32_of(c, "play"), u32_of(c, "win"));
            Some(ChampRecord {
                champion_id: c.get("champion_id")?.as_i64()? as i32,
                name: c.get("champion_name")?.as_str()?.to_owned(),
                play,
                win: play.saturating_sub(enemy_wins),
            })
        })
        .collect()
}

/// `data.synergies.<role>[]` → partners and their record together.
pub fn synergy_records(v: &Value, partner_role: Role) -> Vec<ChampRecord> {
    entries(v.pointer(&format!("/data/synergies/{}", partner_role.opgg())))
        .into_iter()
        .filter_map(|s| {
            Some(ChampRecord {
                champion_id: s.get("synergy_champion_id")?.as_i64()? as i32,
                name: s.get("synergy_champion_name")?.as_str()?.to_owned(),
                play: u32_of(s, "play"),
                win: u32_of(s, "win"),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builds::opgg::client::tool_text;
    use crate::builds::opgg::notation;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/opgg/{name}", env!("CARGO_MANIFEST_DIR"));
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        tool_text(&raw).unwrap()
    }

    fn analysis(name: &str) -> BuildSet {
        analysis_to_buildset(&notation::parse(&fixture(name)).unwrap())
    }

    #[test]
    fn ahri_mid_analysis() {
        let b = analysis("analysis_ahri_mid.raw");
        assert_eq!(b.champion_games, 34473);
        assert!((b.champion_wr - 0.51).abs() < 1e-6);
        assert_eq!(b.damage_type, Some(DamageType::AP));
        assert_eq!(b.role_rates, vec![(Role::Mid, 0.95)]);
        assert_eq!(b.core_sets[0].value, vec![3118, 4645, 3157]);
        assert_eq!((b.core_sets[0].play, b.core_sets[0].win), (2298, 1266));
        assert_eq!(b.boots[0].value, 3020);
        assert_eq!(b.summoners[0].value, vec![4, 14]);
        let page = &b.rune_pages[0].value;
        assert_eq!((page.primary_style, page.sub_style), (8100, 8200));
        assert_eq!(page.perks, vec![8112, 8139, 8140, 8106, 8210, 8226]);
        assert_eq!(page.shards, vec![5005, 5008, 5001]);
        assert_eq!(b.skill_orders[0].value, vec!["Q", "W", "E"]);
        // 4th and 5th item options; the 6th item rows are under 10 games and hidden.
        assert_eq!(b.later_items.len(), 3);
        // Ranked by smoothed win rate: Zhonya's 1119/1896 beats Rabadon's 1505/2605.
        assert_eq!(b.later_items[0].iter().map(|r| r.value).collect::<Vec<_>>(), vec![3157, 3089, 3135]);
        assert!(b.later_items[2].is_empty());
        let pool: Vec<u32> = b.item_pool.iter().map(|r| r.value).collect();
        for id in [3118, 4645, 3157, 3089, 3135, 3020] {
            assert!(pool.contains(&id), "item pool has {id}");
        }
    }

    #[test]
    fn wukong_and_kaisa_parse() {
        let w = analysis("analysis_monkeyking_jungle.raw");
        assert_eq!(w.damage_type, Some(DamageType::AD));
        assert_eq!(w.role_rates.iter().map(|r| r.0).collect::<Vec<_>>(), vec![Role::Jungle, Role::Top]);
        let k = analysis("analysis_kaisa_adc.raw");
        assert_eq!(k.damage_type, Some(DamageType::BOTH));
        assert_eq!(k.role_rates[0].0, Role::Adc);
        assert!(!k.core_sets.is_empty() && !k.rune_pages.is_empty());
    }

    #[test]
    fn traits_from_a_minimal_reply() {
        let text = "class LolGetChampionAnalysis: data\nclass Data: summary,damage_type,counters_meta\nclass Summary: positions\n\
                    class Position: name,stats\nclass Stats: role_rate\nclass CountersMeta: message\n\n\
                    LolGetChampionAnalysis(Data(Summary([Position(\"ADC\",Stats(1))]),\"AD\",CountersMeta(\"Insufficient\")))";
        let t = analysis_to_traits(&notation::parse(text).unwrap());
        assert_eq!(t.role_rates, vec![(Role::Adc, 1.0)]);
        assert_eq!(t.damage_type, Some(DamageType::AD));
    }

    #[test]
    fn jungle_meta_list() {
        let meta = meta_entries(&notation::parse(&fixture("meta_jungle.raw")).unwrap(), Role::Jungle);
        assert!(meta.len() > 20);
        let wukong = &meta[0];
        assert_eq!((wukong.name.as_str(), wukong.play, wukong.win, wukong.tier, wukong.rip), ("Wukong", 18770, 9626, 1, false));
        assert!((wukong.role_rate - 0.85).abs() < 1e-6);
        assert!(meta.iter().any(|m| m.name == "Cho'Gath"));
    }

    #[test]
    fn counters_to_nocturne_are_flipped_to_the_challenger() {
        let v: Value = serde_json::from_str(&fixture("matchup_nocturne_self_jungle.raw")).unwrap();
        let counters = counters_to_enemy(&v);
        assert!(counters.len() > 50);
        let nunu = counters.iter().find(|c| c.name == "Nunu & Willump").unwrap();
        // Nocturne won 47 of 116, so Nunu won 69.
        assert_eq!((nunu.champion_id, nunu.play, nunu.win), (20, 116, 69));
    }

    #[test]
    fn ahri_jungle_synergies() {
        let s = synergy_records(&notation::parse(&fixture("synergy_ahri_mid_jungle.raw")).unwrap(), Role::Jungle);
        assert_eq!(s[0], ChampRecord { champion_id: 62, name: "Wukong".into(), play: 174, win: 94 });
        assert_eq!(s.len(), 3);
        assert!(synergy_records(&Value::Null, Role::Top).is_empty());
    }

    #[test]
    fn ahri_vs_zed_matchup() {
        let v: Value = serde_json::from_str(&fixture("matchup_ahri_zed_mid.raw")).unwrap();
        let b = matchup_to_buildset(&v);
        assert!(b.core_sets.len() >= 3, "several core options");
        assert!(b.rune_pages.len() >= 2);
        assert!(b.boots.len() >= 2);
        assert!(b.later_items.iter().all(|slot| !slot.is_empty()));
        // Ranked best first by smoothed win rate.
        assert!(b.core_sets.windows(2).all(|w| w[0].smoothed_wr >= w[1].smoothed_wr));
        assert!(b.core_sets.iter().all(|r| r.play >= 10));
        // Prior is the head-to-head rate, not Ahri's overall 51%.
        assert!(b.champion_games < 34473);
        assert!(b.item_pool.len() > 10);
    }
}
