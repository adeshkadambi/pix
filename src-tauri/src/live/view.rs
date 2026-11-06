//! The companion's Live screen and the tab panel's data (SPEC §5.9), built from one Live Client
//! snapshot plus the build chosen in champ select. Pure: no I/O.

use serde::Serialize;
use ts_rs::TS;

use crate::builds::draft::{DraftView, RunePageView, ThreatView};
use crate::ddragon::DData;
use crate::game::purchases::PurchaseFeed;
use crate::lcu::catalog::{Catalog, Icon};
use crate::live::models::{AllGameData, Player};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub enum WindowMode {
    FullScreen,
    Windowed,
    Borderless,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct LiveMe {
    pub champion: Icon,
    pub level: u32,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub cs: u32,
    pub gold: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub enum StepState {
    Owned,
    Next,
    Later,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct BuildStep {
    pub item: Icon,
    pub state: StepState,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct LiveBuild {
    /// Your pre-game build in order: core items, boots, then the top 4th/5th/6th options.
    /// "Next" is simply the first item you don't own yet; it doesn't react to the game.
    pub steps: Vec<BuildStep>,
    pub runes: Option<RunePageView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct PurchaseView {
    pub game_time: f64,
    pub champion: Icon,
    pub item: Icon,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct LiveEnemy {
    pub champion: Icon,
    pub level: u32,
    pub dead: bool,
    /// "TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY" or "".
    pub position: String,
    pub spells: Vec<Icon>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct LiveView {
    pub game_time: f64,
    pub summoners_rift: bool,
    pub me: Option<LiveMe>,
    pub build: Option<LiveBuild>,
    /// Why there's no build, when there isn't one.
    pub build_note: Option<String>,
    pub threats: Vec<ThreatView>,
    pub purchases: Vec<PurchaseView>,
    pub enemies: Vec<LiveEnemy>,
    pub window_mode: Option<WindowMode>,
}

fn champion_icon(p: &Player, catalog: &Catalog, dd: &DData) -> Icon {
    match dd.champion_key(p.ddragon_id()) {
        Some(key) => catalog.champion(key),
        None => Icon { id: 0, name: p.champion_name.clone(), icon: String::new() },
    }
}

fn spell_icons(p: &Player, catalog: &Catalog, dd: &DData) -> Vec<Icon> {
    [&p.summoner_spells.summoner_spell_one, &p.summoner_spells.summoner_spell_two]
        .iter()
        .map(|s| match s.key().and_then(|k| dd.summoners.get(k)) {
            Some(info) => catalog.spell(info.id),
            None => Icon { id: 0, name: s.display_name.clone(), icon: String::new() },
        })
        .collect()
}

/// Build steps from the draft's chosen options, marked against the items you own.
fn steps(draft_build: &crate::builds::draft::BuildView, chosen_core: usize, owned: &[u32]) -> Vec<BuildStep> {
    let mut items: Vec<Icon> = Vec::new();
    let core = draft_build.core.get(chosen_core).or(draft_build.core.first());
    let boots = draft_build.boots.first();
    let later = draft_build.later.iter().filter_map(|slot| slot.first());
    for icon in core.into_iter().chain(boots).chain(later).flat_map(|o| o.icons.iter()) {
        if !items.iter().any(|i| i.id == icon.id) {
            items.push(icon.clone());
        }
    }
    let mut next_given = false;
    items
        .into_iter()
        .map(|item| {
            let state = if owned.contains(&(item.id as u32)) {
                StepState::Owned
            } else if !next_given {
                next_given = true;
                StepState::Next
            } else {
                StepState::Later
            };
            BuildStep { item, state }
        })
        .collect()
}

pub fn build_view(
    game: &AllGameData,
    draft: Option<&DraftView>,
    feed: &PurchaseFeed,
    catalog: &Catalog,
    dd: &DData,
    window_mode: Option<WindowMode>,
) -> LiveView {
    let rift = game.is_summoners_rift();
    let me = game.me();
    let my_key = me.and_then(|m| dd.champion_key(m.ddragon_id()));

    // Compliance rule 6: build and threat data only on Summoner's Rift.
    let draft_build = draft.and_then(|d| d.build.as_ref().map(|b| (d, b)));
    let (build, build_note, threats) = match (rift, draft_build) {
        (false, _) => (None, Some("Build data is only shown for Summoner's Rift games.".to_owned()), Vec::new()),
        (true, None) => (None, Some("No build from champ select for this game.".to_owned()), Vec::new()),
        (true, Some((_, b))) if Some(b.champion.id) != my_key => {
            (None, Some(format!("The champ select build was for {}.", b.champion.name)), Vec::new())
        }
        (true, Some((d, b))) => {
            let owned: Vec<u32> = me.map(|m| m.items.iter().map(|i| i.item_id).collect()).unwrap_or_default();
            let runes = b.runes.get(d.chosen.runes as usize).or(b.runes.first()).cloned();
            (Some(LiveBuild { steps: steps(b, d.chosen.core as usize, &owned), runes }), None, b.threats.clone())
        }
    };

    let by_champ = |id: &str| game.all_players.iter().find(|p| p.ddragon_id() == id);
    LiveView {
        game_time: game.game_data.game_time,
        summoners_rift: rift,
        me: me.map(|m| LiveMe {
            champion: champion_icon(m, catalog, dd),
            level: m.level,
            kills: m.scores.kills,
            deaths: m.scores.deaths,
            assists: m.scores.assists,
            cs: m.scores.creep_score,
            gold: game.active_player.current_gold.max(0.0) as u32,
        }),
        build,
        build_note,
        threats,
        purchases: feed
            .recent
            .iter()
            .map(|p| PurchaseView {
                game_time: p.game_time,
                champion: by_champ(&p.champion).map(|pl| champion_icon(pl, catalog, dd)).unwrap_or(Icon {
                    id: 0,
                    name: p.champion.clone(),
                    icon: String::new(),
                }),
                item: catalog.item_icon(p.item_id as i32),
            })
            .collect(),
        enemies: game
            .enemies()
            .map(|p| LiveEnemy {
                champion: champion_icon(p, catalog, dd),
                level: p.level,
                dead: p.is_dead,
                position: p.position.clone(),
                spells: spell_icons(p, catalog, dd),
            })
            .collect(),
        window_mode,
    }
}

/// League's `Config/game.cfg` → `[General] WindowMode=` (0 full screen, 1 windowed, 2 borderless).
pub fn parse_window_mode(game_cfg: &str) -> Option<WindowMode> {
    let mut in_general = false;
    for line in game_cfg.lines().map(str::trim) {
        if line.starts_with('[') {
            in_general = line.eq_ignore_ascii_case("[General]");
        } else if in_general && let Some(v) = line.strip_prefix("WindowMode=") {
            return match v.trim() {
                "0" => Some(WindowMode::FullScreen),
                "1" => Some(WindowMode::Windowed),
                "2" => Some(WindowMode::Borderless),
                _ => None,
            };
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builds::Role;
    use crate::builds::draft::{BuildView, ChosenBuild, DraftStatus, IconOption};
    use crate::ddragon::{ChampInfo, SpellInfo};
    use std::collections::HashMap;

    fn game() -> AllGameData {
        let path = format!("{}/tests/fixtures/live/allgamedata.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn dd() -> DData {
        let mut champions = HashMap::new();
        for (key, id) in [(233, "Briar"), (54, "Malphite"), (234, "Viego"), (134, "Syndra"), (145, "Kaisa"), (89, "Leona")] {
            champions.insert(key, ChampInfo { id: id.into(), name: id.into(), tags: vec![] });
        }
        let mut summoners = HashMap::new();
        summoners.insert("SummonerFlash".to_string(), SpellInfo { id: 4, name: "Flash".into(), cooldown: 300.0 });
        summoners.insert("SummonerSmite".to_string(), SpellInfo { id: 11, name: "Smite".into(), cooldown: 90.0 });
        DData { champions, summoners, ..Default::default() }
    }

    fn catalog() -> Catalog {
        Catalog::from_json(
            r#"[{"id":233,"name":"Briar","squarePortraitPath":"/c/233.png"},{"id":234,"name":"Viego","squarePortraitPath":"/c/234.png"}]"#,
            r#"[{"id":3078,"name":"Trinity Force","iconPath":"/i/3078.png"},{"id":3748,"name":"Titanic Hydra","iconPath":"/i/3748.png"}]"#,
            r#"[{"id":4,"name":"Flash","iconPath":"/s/4.png"},{"id":11,"name":"Smite","iconPath":"/s/11.png"}]"#,
        )
    }

    fn icon(id: i32) -> Icon {
        Icon { id, name: format!("item {id}"), icon: String::new() }
    }

    fn option(ids: &[i32]) -> IconOption {
        IconOption { icons: ids.iter().map(|i| icon(*i)).collect(), play: 100, win: 55, smoothed_wr: 0.54 }
    }

    fn draft(champion: i32) -> DraftView {
        DraftView {
            chosen: ChosenBuild { runes: 0, core: 1 },
            status: DraftStatus::Ready,
            message: None,
            my_role: Some(Role::Jungle),
            role_from_client: true,
            allies: vec![],
            enemies: vec![],
            ally_bans: vec![],
            enemy_bans: vec![],
            suggestions: None,
            build: Some(BuildView {
                champion: Icon { id: champion, name: "Briar".into(), icon: String::new() },
                role: Role::Jungle,
                opponent: None,
                games: 1000,
                win_rate: 0.52,
                runes: vec![],
                // Option 0 is not chosen; option 1 is.
                core: vec![option(&[6672, 3124, 3302]), option(&[3078, 3748, 3036])],
                boots: vec![option(&[3047])],
                starters: vec![],
                spells: vec![],
                later: vec![vec![option(&[3026]), option(&[6333])], vec![option(&[3036])]],
                skills: vec![],
                situational: vec![],
                threats: vec![],
                opgg_url: String::new(),
            }),
        }
    }

    #[test]
    fn builds_the_live_view() {
        let v = build_view(&game(), Some(&draft(233)), &PurchaseFeed::default(), &catalog(), &dd(), Some(WindowMode::Borderless));
        let me = v.me.unwrap();
        assert_eq!((me.champion.name.as_str(), me.level, me.kills, me.deaths, me.assists, me.cs, me.gold), ("Briar", 11, 6, 2, 5, 142, 1130));
        assert!(v.summoners_rift && v.build_note.is_none());
        let steps: Vec<(i32, StepState)> = v.build.unwrap().steps.iter().map(|s| (s.item.id, s.state)).collect();
        // Chosen core (option 1) + boots + top 4th/5th; 3036 isn't repeated.
        assert_eq!(steps, vec![
            (3078, StepState::Owned),
            (3748, StepState::Owned),
            (3036, StepState::Next),
            (3047, StepState::Owned),
            (3026, StepState::Later),
        ]);
        assert_eq!(v.enemies.len(), 5);
        let viego = &v.enemies[1];
        assert_eq!(viego.champion.name, "Viego");
        assert_eq!(viego.spells.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["Flash", "Smite"]);
        assert!(v.enemies[2].dead, "Syndra is dead in the fixture");
    }

    #[test]
    fn no_build_for_another_champion_or_off_the_rift() {
        let v = build_view(&game(), Some(&draft(1)), &PurchaseFeed::default(), &catalog(), &dd(), None);
        assert!(v.build.is_none());
        assert_eq!(v.build_note.as_deref(), Some("The champ select build was for Briar."));
        let mut aram = game();
        aram.game_data.map_number = 12;
        aram.game_data.game_mode = "ARAM".into();
        let v = build_view(&aram, Some(&draft(233)), &PurchaseFeed::default(), &catalog(), &dd(), None);
        assert!(v.build.is_none() && v.threats.is_empty() && !v.summoners_rift);
        let v = build_view(&game(), None, &PurchaseFeed::default(), &catalog(), &dd(), None);
        assert_eq!(v.build_note.as_deref(), Some("No build from champ select for this game."));
    }

    #[test]
    fn window_mode_from_game_cfg() {
        let cfg = "[General]\nWindowMode=2\nHeight=1080\n[HUD]\nWindowMode=0\n";
        assert_eq!(parse_window_mode(cfg), Some(WindowMode::Borderless));
        assert_eq!(parse_window_mode("[General]\nWindowMode=0"), Some(WindowMode::FullScreen));
        assert_eq!(parse_window_mode("[General]\nWindowMode=1"), Some(WindowMode::Windowed));
        assert_eq!(parse_window_mode("[HUD]\nWindowMode=2"), None);
    }
}
