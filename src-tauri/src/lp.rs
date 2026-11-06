//! Crediting LP changes to games from before/after rank snapshots (SPEC §5.12).
//!
//! Riot gives no LP per game. Pix snapshots the rank state around games and credits the change to
//! a game only when the snapshots prove it: exactly one more game played in that queue, the
//! result matches, and no other game of that queue falls in between. Anything else gets no LP.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::profile::{MatchResult, RankedQueue};

pub const SOLO: &str = "RANKED_SOLO_5x5";
pub const FLEX: &str = "RANKED_FLEX_SR";

const DIVIDED_TIERS: [&str; 7] = ["IRON", "BRONZE", "SILVER", "GOLD", "PLATINUM", "EMERALD", "DIAMOND"];
const APEX_BASE: i32 = 2800;

/// The ranked queue type a queue id earns LP in.
pub fn queue_type_for(queue_id: i32) -> Option<&'static str> {
    match queue_id {
        420 => Some(SOLO),
        440 => Some(FLEX),
        _ => None,
    }
}

/// Position on one continuous LP ladder: Iron IV 0 LP = 0, each division 100 LP,
/// Master and above share one pool starting at 2800.
pub fn ladder(rank: &RankedQueue) -> Option<i32> {
    if let Some(tier) = DIVIDED_TIERS.iter().position(|t| *t == rank.tier) {
        let division = match rank.division.as_str() {
            "IV" => 0,
            "III" => 1,
            "II" => 2,
            "I" => 3,
            _ => return None,
        };
        return Some(tier as i32 * 400 + division * 100 + rank.lp);
    }
    matches!(rank.tier.as_str(), "MASTER" | "GRANDMASTER" | "CHALLENGER").then_some(APEX_BASE + rank.lp)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub queue_type: String,
    pub taken_ms: u64,
    pub rank: RankedQueue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameRef {
    pub game_id: u64,
    pub created_ms: u64,
    pub duration_secs: u32,
    pub queue_id: i32,
    pub result: MatchResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LpChange {
    pub delta: i32,
    pub tier_before: String,
    pub division_before: String,
    pub tier_after: String,
    pub division_after: String,
    pub lp_after: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credit {
    pub game_id: u64,
    pub queue_type: &'static str,
    pub change: LpChange,
}

fn games_played(r: &RankedQueue) -> u32 {
    r.wins + r.losses
}

/// Credits LP changes for `pending` games (ones without an LP change yet).
/// `all_games` is every stored game, used to rule out other games between the snapshots.
/// `snapshots` must be sorted by `taken_ms`.
pub fn credit(pending: &[GameRef], all_games: &[GameRef], snapshots: &[Snapshot]) -> Vec<Credit> {
    let mut out = Vec::new();
    for game in pending {
        if game.result == MatchResult::Remake {
            continue;
        }
        let Some(queue_type) = queue_type_for(game.queue_id) else { continue };
        let snaps: Vec<&Snapshot> = snapshots.iter().filter(|s| s.queue_type == queue_type).collect();

        let Some(before) = snaps.iter().rev().find(|s| s.taken_ms <= game.created_ms) else { continue };
        let ended_ms = game.created_ms + u64::from(game.duration_secs) * 1000;
        let Some(after) = snaps
            .iter()
            .find(|s| s.taken_ms >= ended_ms && games_played(&s.rank) == games_played(&before.rank) + 1)
        else {
            continue;
        };

        let result_matches = match game.result {
            MatchResult::Win => after.rank.wins == before.rank.wins + 1,
            MatchResult::Loss => after.rank.losses == before.rank.losses + 1,
            MatchResult::Remake => false,
        };
        let others_between = all_games.iter().any(|g| {
            g.game_id != game.game_id
                && queue_type_for(g.queue_id) == Some(queue_type)
                && g.result != MatchResult::Remake
                && g.created_ms > before.taken_ms
                && g.created_ms < after.taken_ms
        });
        if !result_matches || others_between {
            continue;
        }

        let (Some(from), Some(to)) = (ladder(&before.rank), ladder(&after.rank)) else { continue };
        out.push(Credit {
            game_id: game.game_id,
            queue_type,
            change: LpChange {
                delta: to - from,
                tier_before: before.rank.tier.clone(),
                division_before: before.rank.division.clone(),
                tier_after: after.rank.tier.clone(),
                division_after: after.rank.division.clone(),
                lp_after: after.rank.lp,
            },
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rank(tier: &str, division: &str, lp: i32, wins: u32, losses: u32) -> RankedQueue {
        RankedQueue { tier: tier.into(), division: division.into(), lp, wins, losses }
    }

    fn snap(taken_ms: u64, r: RankedQueue) -> Snapshot {
        Snapshot { queue_type: SOLO.into(), taken_ms, rank: r }
    }

    fn game(id: u64, created_ms: u64, result: MatchResult) -> GameRef {
        // 30-minute solo queue game.
        GameRef { game_id: id, created_ms, duration_secs: 1800, queue_id: 420, result }
    }

    const MIN: u64 = 60_000;

    #[test]
    fn ladder_positions() {
        assert_eq!(ladder(&rank("IRON", "IV", 0, 0, 0)), Some(0));
        assert_eq!(ladder(&rank("SILVER", "III", 20, 0, 0)), Some(920));
        assert_eq!(ladder(&rank("DIAMOND", "I", 99, 0, 0)), Some(2799));
        assert_eq!(ladder(&rank("MASTER", "", 0, 0, 0)), Some(2800));
        assert_eq!(ladder(&rank("CHALLENGER", "", 1200, 0, 0)), Some(4000));
        assert_eq!(ladder(&rank("", "NA", 0, 0, 0)), None);
        assert_eq!(ladder(&rank("SILVER", "V", 0, 0, 0)), None);
    }

    #[test]
    fn credits_a_win_and_a_loss() {
        let g1 = game(1, 10 * MIN, MatchResult::Win);
        let g2 = game(2, 60 * MIN, MatchResult::Loss);
        let snaps = vec![
            snap(5 * MIN, rank("SILVER", "III", 20, 3, 5)),
            snap(45 * MIN, rank("SILVER", "III", 41, 4, 5)),
            snap(95 * MIN, rank("SILVER", "III", 24, 4, 6)),
        ];
        let games = vec![g1.clone(), g2.clone()];
        let credits = credit(&games, &games, &snaps);
        assert_eq!(credits.iter().map(|c| (c.game_id, c.change.delta)).collect::<Vec<_>>(), vec![(1, 21), (2, -17)]);
        assert_eq!(credits[0].queue_type, SOLO);
    }

    #[test]
    fn promotion_and_demotion_cross_divisions() {
        let up = game(1, 10 * MIN, MatchResult::Win);
        let down = game(2, 60 * MIN, MatchResult::Loss);
        let snaps = vec![
            snap(0, rank("SILVER", "I", 90, 10, 10)),
            snap(45 * MIN, rank("GOLD", "IV", 5, 11, 10)),
            snap(95 * MIN, rank("SILVER", "I", 75, 11, 11)),
        ];
        let games = vec![up.clone(), down.clone()];
        let credits = credit(&games, &games, &snaps);
        assert_eq!(credits[0].change.delta, 15);
        assert_eq!((credits[0].change.tier_after.as_str(), credits[0].change.division_after.as_str()), ("GOLD", "IV"));
        assert_eq!(credits[1].change.delta, -30);
    }

    #[test]
    fn two_games_between_snapshots_are_ambiguous() {
        let g1 = game(1, 10 * MIN, MatchResult::Win);
        let g2 = game(2, 45 * MIN, MatchResult::Win);
        let snaps = vec![snap(0, rank("GOLD", "II", 10, 5, 5)), snap(90 * MIN, rank("GOLD", "II", 50, 7, 5))];
        let games = vec![g1, g2];
        assert!(credit(&games, &games, &snaps).is_empty());
    }

    #[test]
    fn stale_after_snapshot_is_skipped_until_a_fresh_one_arrives() {
        let g = game(1, 10 * MIN, MatchResult::Win);
        let before = snap(5 * MIN, rank("GOLD", "II", 10, 5, 5));
        // Taken after the game, but the client hadn't updated yet: same count.
        let stale = snap(41 * MIN, rank("GOLD", "II", 10, 5, 5));
        let games = vec![g.clone()];
        assert!(credit(&games, &games, &[before.clone(), stale.clone()]).is_empty());
        let fresh = snap(50 * MIN, rank("GOLD", "II", 32, 6, 5));
        let credits = credit(&games, &games, &[before, stale, fresh]);
        assert_eq!(credits[0].change.delta, 22);
    }

    #[test]
    fn result_must_match_and_remakes_are_ignored() {
        let snaps = vec![snap(0, rank("GOLD", "II", 10, 5, 5)), snap(60 * MIN, rank("GOLD", "II", 30, 6, 5))];
        let says_loss = vec![game(1, 10 * MIN, MatchResult::Loss)];
        assert!(credit(&says_loss, &says_loss, &snaps).is_empty());
        let remake = vec![game(2, 10 * MIN, MatchResult::Remake)];
        assert!(credit(&remake, &remake, &snaps).is_empty());
    }

    #[test]
    fn needs_a_snapshot_from_before_the_game() {
        let g = vec![game(1, 10 * MIN, MatchResult::Win)];
        let snaps = vec![snap(60 * MIN, rank("GOLD", "II", 30, 6, 5))];
        assert!(credit(&g, &g, &snaps).is_empty());
    }

    #[test]
    fn flex_and_solo_are_tracked_separately() {
        let flex_game = GameRef { queue_id: 440, ..game(1, 10 * MIN, MatchResult::Win) };
        let normal = GameRef { queue_id: 400, ..game(2, 12 * MIN, MatchResult::Win) };
        let snaps = vec![
            Snapshot { queue_type: FLEX.into(), taken_ms: 0, rank: rank("GOLD", "IV", 50, 1, 1) },
            snap(0, rank("SILVER", "III", 20, 3, 5)),
            Snapshot { queue_type: FLEX.into(), taken_ms: 60 * MIN, rank: rank("GOLD", "IV", 70, 2, 1) },
        ];
        let games = vec![flex_game, normal];
        let credits = credit(&games, &games, &snaps);
        assert_eq!(credits.len(), 1);
        assert_eq!((credits[0].game_id, credits[0].queue_type, credits[0].change.delta), (1, FLEX, 20));
    }
}
