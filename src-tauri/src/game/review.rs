//! Post-game review (SPEC §5.9): who killed you, when, and what would have countered them.
//! Saved to the local game log per game (SPEC §5.12).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::kills::{Actor, KillRecord};
use crate::builds::draft::SituationalView;
use crate::lcu::catalog::Icon;
use crate::lp::LpChange;
use crate::profile::MatchResult;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DeathGroup {
    /// The killer: a champion, or something else (turret, minions…) with an empty icon.
    pub killer: Icon,
    pub count: u32,
    /// Champions that assisted on these deaths, most frequent first.
    pub assists: Vec<Icon>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DeathMark {
    pub time: f64,
    pub killer: Icon,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CounterView {
    pub killer: Icon,
    /// Your champion vs. this one: OP.GG games and win rate, when there's matchup data.
    pub games: u32,
    pub win_rate: Option<f32>,
    /// Items your champion builds that answer this killer, best first.
    pub items: Vec<SituationalView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReviewView {
    #[ts(type = "number")]
    pub game_id: u64,
    #[ts(type = "number")]
    pub created_ms: u64,
    pub champion: Icon,
    pub result: Option<MatchResult>,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub cs: u32,
    pub duration_secs: u32,
    pub deaths_by: Vec<DeathGroup>,
    pub timeline: Vec<DeathMark>,
    pub counters: Vec<CounterView>,
    /// From the game log, when it's known.
    #[serde(default)]
    pub lp_change: Option<LpChange>,
}

/// Your deaths grouped by killer (most first), with assisting champions (most first).
pub fn death_groups(kills: &[KillRecord]) -> Vec<(Actor, u32, Vec<String>)> {
    let mut groups: Vec<(Actor, u32, Vec<(String, u32)>)> = Vec::new();
    for k in kills.iter().filter(|k| k.victim_is_me) {
        let idx = match groups.iter().position(|g| g.0 == k.killer) {
            Some(i) => i,
            None => {
                groups.push((k.killer.clone(), 0, Vec::new()));
                groups.len() - 1
            }
        };
        let g = &mut groups[idx];
        g.1 += 1;
        for a in &k.assisters {
            match g.2.iter_mut().find(|(n, _)| n == a) {
                Some((_, c)) => *c += 1,
                None => g.2.push((a.clone(), 1)),
            }
        }
    }
    groups.sort_by(|a, b| b.1.cmp(&a.1));
    groups
        .into_iter()
        .map(|(killer, count, mut assists)| {
            assists.sort_by(|a, b| b.1.cmp(&a.1));
            (killer, count, assists.into_iter().map(|(n, _)| n).collect())
        })
        .collect()
}

/// The champions who killed you most (up to `n`), for counter items.
pub fn top_killers(kills: &[KillRecord], n: usize) -> Vec<String> {
    death_groups(kills)
        .into_iter()
        .filter_map(|(a, _, _)| match a {
            Actor::Champion(c) => Some(c),
            Actor::Other(_) => None,
        })
        .take(n)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn death(time: f64, killer: Actor, assisters: &[&str]) -> KillRecord {
        KillRecord {
            event_id: time as u64,
            time,
            killer,
            victim: "Briar".into(),
            assisters: assisters.iter().map(|s| s.to_string()).collect(),
            victim_is_me: true,
            killer_is_me: false,
        }
    }

    fn champ(c: &str) -> Actor {
        Actor::Champion(c.into())
    }

    #[test]
    fn groups_deaths_by_killer_with_assists() {
        let mut kills = vec![
            death(300.0, champ("Zed"), &["Leona"]),
            death(720.0, champ("Syndra"), &[]),
            death(800.0, champ("Zed"), &["Leona", "Syndra"]),
            death(900.0, Actor::Other("Turret".into()), &["Zed"]),
            death(1100.0, champ("Zed"), &["Syndra"]),
        ];
        // A kill you got isn't one of your deaths.
        kills.push(KillRecord { victim_is_me: false, killer_is_me: true, victim: "Zed".into(), ..death(1200.0, champ("Briar"), &[]) });
        let g = death_groups(&kills);
        assert_eq!(g[0], (champ("Zed"), 3, vec!["Leona".to_string(), "Syndra".to_string()]));
        assert_eq!(g.len(), 3);
        assert_eq!(top_killers(&kills, 2), vec!["Zed", "Syndra"], "turrets aren't champions to counter");
    }

    #[test]
    fn no_deaths() {
        assert!(death_groups(&[]).is_empty());
        assert!(top_killers(&[], 2).is_empty());
    }
}
