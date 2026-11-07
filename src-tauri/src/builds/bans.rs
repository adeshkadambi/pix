//! Suggested bans in champ select (from OP.GG data; nothing is sent to the client).
//!
//! OP.GG's tier list has no ban rates, so each candidate in your role gets:
//! - **Against you:** the champion's smoothed win rate against the champion you're hovering, when
//!   that matchup has enough games. A champion that beats your pick is the most useful ban.
//! - **Otherwise, role strength:** smoothed win rate in your role, plus a small bonus per tier
//!   (tier 1 = OP), so popular strong picks rank above niche ones with a similar win rate.
//!
//! Several options are always shown, each with its win rate and game count (compliance rule 7).

use super::ranking::smoothed;
use super::suggest::{MIN_MATCHUP_GAMES, pct};

const MIN_META_GAMES: u32 = 100;
/// Bonus per tier above tier 5 (tier 1 gets 4×).
const TIER_BONUS: f32 = 0.005;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BanCandidate {
    pub champion_id: i32,
    pub name: String,
    /// (games, wins, tier) in your role.
    pub meta: Option<(u32, u32, u8)>,
    /// (games, wins) for this champion against the one you're hovering.
    pub vs_you: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BanPick {
    pub champion_id: i32,
    pub name: String,
    pub score: f32,
    /// Win rate and games behind the score.
    pub win_rate: f32,
    pub games: u32,
    pub vs_you: bool,
    pub reason: String,
}

/// Ranks ban candidates best first. `mine` is the champion you're hovering, `role_label` e.g. "jungle".
pub fn rank(candidates: Vec<BanCandidate>, mine: Option<&str>, role_label: &str) -> Vec<BanPick> {
    let mut out: Vec<BanPick> = candidates
        .into_iter()
        .filter_map(|c| match (mine, c.vs_you) {
            (Some(me), Some((play, win))) if play >= MIN_MATCHUP_GAMES => Some(BanPick {
                score: smoothed(win, play, 0.5),
                win_rate: win as f32 / play as f32,
                games: play,
                vs_you: true,
                reason: format!("wins {} vs your {me} ({play} games)", pct(win, play)),
                champion_id: c.champion_id,
                name: c.name,
            }),
            _ => {
                let (play, win, tier) = c.meta.filter(|(p, _, _)| *p >= MIN_META_GAMES)?;
                let tier_bonus = TIER_BONUS * 5u8.saturating_sub(tier.clamp(1, 5)) as f32;
                Some(BanPick {
                    score: smoothed(win, play, 0.5) + tier_bonus,
                    win_rate: win as f32 / play as f32,
                    games: play,
                    vs_you: false,
                    reason: format!("tier {tier} {role_label}, {} ({play} games)", pct(win, play)),
                    champion_id: c.champion_id,
                    name: c.name,
                })
            }
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then(b.games.cmp(&a.games)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: i32, name: &str, meta: Option<(u32, u32, u8)>, vs_you: Option<(u32, u32)>) -> BanCandidate {
        BanCandidate { champion_id: id, name: name.into(), meta, vs_you }
    }

    #[test]
    fn champions_that_beat_your_pick_come_first() {
        let out = rank(
            vec![
                cand(1, "Strong", Some((20000, 10600, 1)), None),
                cand(2, "Counter", Some((5000, 2500, 3)), Some((400, 232))),
                cand(3, "Tiny matchup", Some((5000, 2500, 3)), Some((12, 11))),
            ],
            Some("Briar"),
            "jungle",
        );
        assert_eq!(out.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), vec!["Counter", "Strong", "Tiny matchup"]);
        assert!(out[0].vs_you);
        assert_eq!(out[0].reason, "wins 58% vs your Briar (400 games)");
        assert!(!out[2].vs_you, "a 12-game matchup falls back to role strength");
    }

    #[test]
    fn without_a_hover_tier_and_win_rate_decide() {
        let out = rank(
            vec![
                cand(1, "Tier 3", Some((10000, 5150, 3)), None),
                cand(2, "Tier 1", Some((10000, 5100, 1)), Some((500, 300))),
                cand(3, "Few games", Some((50, 45, 1)), None),
            ],
            None,
            "mid",
        );
        assert_eq!(out.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), vec!["Tier 1", "Tier 3"], "small samples are dropped");
        assert!(!out[0].vs_you, "matchups only count when you're hovering a champion");
        assert_eq!(out[0].reason, "tier 1 mid, 51% (10000 games)");
    }
}
