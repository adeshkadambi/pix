//! Pick suggestions before you lock in.
//!
//! No source has win rates for full five-champion comps, so each candidate gets:
//! - **Main score:** smoothed win rate against your lane opponent (when known and sampled),
//!   otherwise the champion's smoothed win rate in your role.
//! - **Synergy:** a half-weight bonus for OP.GG's top partners of your locked teammates.
//! - **Team gaps:** a small bonus for filling a missing damage type or frontline.
//! - **Your pool:** a small comfort bonus for champions you've played in recent ranked games.
//! Every boost comes with a reason, and several options are always shown (compliance rule 7).

use super::ranking::smoothed;

/// Matchup samples smaller than this fall back to the role's meta.
pub const MIN_MATCHUP_GAMES: u32 = 30;
const MIN_META_GAMES: u32 = 100;
const MIN_SYNERGY_GAMES: u32 = 30;
const SYNERGY_WEIGHT: f32 = 0.5;
const GAP_BONUS: f32 = 0.01;
const COMFORT_BONUS: f32 = 0.005;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Candidate {
    pub champion_id: i32,
    pub name: String,
    /// Data Dragon class tags, main class first.
    pub tags: Vec<String>,
    /// (games, wins) in your role.
    pub meta: Option<(u32, u32)>,
    /// (games, wins) against your lane opponent.
    pub vs_opponent: Option<(u32, u32)>,
    /// (teammate name, games together, wins together).
    pub synergies: Vec<(String, u32, u32)>,
    /// Your own recent ranked (games, wins) on this champion.
    pub mine: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TeamGaps {
    pub no_ap: bool,
    pub no_frontline: bool,
}

/// What your locked teammates lack. Judged only once two or more teammates have locked.
pub fn team_gaps(teammate_tags: &[Vec<String>]) -> TeamGaps {
    if teammate_tags.len() < 2 {
        return TeamGaps::default();
    }
    let main = |t: &Vec<String>| t.first().cloned().unwrap_or_default();
    TeamGaps {
        no_ap: !teammate_tags.iter().any(|t| main(t) == "Mage"),
        no_frontline: !teammate_tags.iter().any(|t| main(t) == "Tank" || (main(t) == "Fighter" && t.iter().any(|x| x == "Tank"))),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub champion_id: i32,
    pub name: String,
    pub score: f32,
    /// Raw win rate behind the main score.
    pub win_rate: f32,
    pub games: u32,
    /// True when the main score is the matchup against your lane opponent.
    pub vs_opponent: bool,
    pub reasons: Vec<String>,
    pub mine: Option<(u32, u32)>,
}

fn pct(win: u32, play: u32) -> String {
    format!("{:.0}%", win as f32 / play.max(1) as f32 * 100.0)
}

/// Ranks candidates best first. `opponent` is your lane opponent's name, `role_label` e.g. "jungle".
pub fn rank(candidates: Vec<Candidate>, opponent: Option<&str>, role_label: &str, gaps: TeamGaps) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = candidates
        .into_iter()
        .filter_map(|c| {
            let meta_wr = c.meta.filter(|(p, _)| *p > 0).map(|(p, w)| w as f32 / p as f32).unwrap_or(0.5);
            let mut reasons = Vec::new();
            let (main, win_rate, games, vs) = match (opponent, c.vs_opponent) {
                (Some(opp), Some((play, win))) if play >= MIN_MATCHUP_GAMES => {
                    reasons.push(format!("{} vs {opp} ({play} games)", pct(win, play)));
                    (smoothed(win, play, meta_wr), win as f32 / play as f32, play, true)
                }
                _ => {
                    let (play, win) = c.meta.filter(|(p, _)| *p >= MIN_META_GAMES)?;
                    reasons.push(format!("{} in {role_label} ({play} games)", pct(win, play)));
                    (smoothed(win, play, 0.5), win as f32 / play as f32, play, false)
                }
            };

            let mut score = main;
            for (ally, play, win) in c.synergies.iter().filter(|(_, p, _)| *p >= MIN_SYNERGY_GAMES) {
                let lift = smoothed(*win, *play, 0.5) - 0.5;
                score += lift * SYNERGY_WEIGHT;
                if lift > 0.0 {
                    reasons.push(format!("pairs well with {ally} ({})", pct(*win, *play)));
                }
            }
            let main_tag = c.tags.first().map(String::as_str).unwrap_or("");
            if gaps.no_ap && main_tag == "Mage" {
                score += GAP_BONUS;
                reasons.push("your team has no AP".into());
            }
            if gaps.no_frontline && (main_tag == "Tank" || c.tags.iter().any(|t| t == "Tank")) {
                score += GAP_BONUS;
                reasons.push("your team has no frontline".into());
            }
            if let Some((g, w)) = c.mine.filter(|(g, _)| *g > 0) {
                score += COMFORT_BONUS;
                reasons.push(format!("you: {w}W {}L recently", g - w));
            }
            Some(Suggestion { champion_id: c.champion_id, name: c.name, score, win_rate, games, vs_opponent: vs, reasons, mine: c.mine })
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then(b.games.cmp(&a.games)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: i32, name: &str, tags: &[&str]) -> Candidate {
        Candidate { champion_id: id, name: name.into(), tags: tags.iter().map(|s| s.to_string()).collect(), ..Default::default() }
    }

    #[test]
    fn matchup_data_beats_meta_and_reasons_explain_it() {
        let nunu = Candidate { vs_opponent: Some((116, 69)), meta: Some((8000, 4000)), ..cand(20, "Nunu & Willump", &["Tank"]) };
        let wukong = Candidate { vs_opponent: Some((500, 245)), meta: Some((18770, 9626)), ..cand(62, "Wukong", &["Fighter"]) };
        let yi = Candidate { vs_opponent: Some((12, 10)), meta: Some((17357, 8828)), ..cand(11, "Master Yi", &["Assassin"]) };
        let out = rank(vec![wukong, yi, nunu], Some("Nocturne"), "jungle", TeamGaps::default());
        assert_eq!(out.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["Nunu & Willump", "Master Yi", "Wukong"]);
        assert!(out[0].vs_opponent);
        assert_eq!(out[0].reasons[0], "59% vs Nocturne (116 games)");
        // Yi's 12-game matchup is too small; he's scored on his jungle meta instead.
        assert!(!out[1].vs_opponent);
        assert_eq!(out[1].reasons[0], "51% in jungle (17357 games)");
    }

    #[test]
    fn without_an_opponent_meta_decides() {
        let a = Candidate { meta: Some((10000, 5200)), ..cand(1, "A", &[]) };
        let b = Candidate { meta: Some((10000, 5000)), ..cand(2, "B", &[]) };
        let tiny = Candidate { meta: Some((50, 40)), ..cand(3, "Tiny", &[]) };
        let out = rank(vec![b, tiny, a], None, "mid", TeamGaps::default());
        assert_eq!(out.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["A", "B"], "small samples are dropped");
    }

    #[test]
    fn synergy_gaps_and_comfort_add_boosts_with_reasons() {
        let base = (10000, 5000);
        let plain = Candidate { meta: Some(base), ..cand(1, "Plain", &["Fighter"]) };
        let partner = Candidate { meta: Some(base), synergies: vec![("Ahri".into(), 400, 228)], ..cand(2, "Partner", &["Fighter"]) };
        let mage = Candidate { meta: Some(base), ..cand(3, "Mage", &["Mage"]) };
        let mine = Candidate { meta: Some(base), mine: Some((4, 3)), ..cand(4, "Mine", &["Fighter"]) };
        let out = rank(vec![plain, mine, mage, partner], None, "jungle", TeamGaps { no_ap: true, no_frontline: false });
        let names: Vec<&str> = out.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Partner", "Mage", "Mine", "Plain"]);
        assert!(out[0].reasons.contains(&"pairs well with Ahri (57%)".to_string()));
        assert!(out[1].reasons.contains(&"your team has no AP".to_string()));
        assert!(out[2].reasons.contains(&"you: 3W 1L recently".to_string()));
    }

    #[test]
    fn team_gaps_need_two_teammates() {
        let tags = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(team_gaps(&[tags(&["Marksman"])]), TeamGaps::default());
        assert_eq!(team_gaps(&[tags(&["Marksman"]), tags(&["Assassin"])]), TeamGaps { no_ap: true, no_frontline: true });
        assert_eq!(team_gaps(&[tags(&["Mage"]), tags(&["Tank", "Support"])]), TeamGaps::default());
        assert_eq!(
            team_gaps(&[tags(&["Fighter", "Tank"]), tags(&["Marksman"])]),
            TeamGaps { no_ap: true, no_frontline: false },
            "a Fighter/Tank counts as frontline"
        );
    }
}
