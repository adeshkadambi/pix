//! Smoothed win rate (SPEC §5.5): `(win + K·p0) / (play + K)`, K = 50. Rows with fewer than
//! 10 games are hidden; the game count is always shown.

use super::Ranked;

pub const K: f32 = 50.0;
pub const MIN_GAMES: u32 = 10;

pub fn smoothed(win: u32, play: u32, prior: f32) -> f32 {
    (win as f32 + K * prior) / (play as f32 + K)
}

/// Builds ranked options from `(value, play, win)` rows: drops small samples, sorts best first.
pub fn rank<T>(rows: impl IntoIterator<Item = (T, u32, u32)>, prior: f32) -> Vec<Ranked<T>> {
    let mut out: Vec<Ranked<T>> = rows
        .into_iter()
        .filter(|(_, play, _)| *play >= MIN_GAMES)
        .map(|(value, play, win)| Ranked { smoothed_wr: smoothed(win, play, prior), value, play, win })
        .collect();
    out.sort_by(|a, b| b.smoothed_wr.total_cmp(&a.smoothed_wr).then(b.play.cmp(&a.play)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_pulls_small_samples_toward_the_prior() {
        assert!((smoothed(0, 0, 0.5) - 0.5).abs() < 1e-6);
        // 10/10 wins is not 100%: with a 50% prior it's 60/60 + … = (10 + 25) / 60.
        assert!((smoothed(10, 10, 0.5) - 35.0 / 60.0).abs() < 1e-6);
        // Big samples barely move.
        assert!((smoothed(5500, 10000, 0.5) - 0.5498).abs() < 1e-3);
    }

    #[test]
    fn ranks_by_smoothed_rate_and_hides_small_samples() {
        let rows = vec![("lucky", 12, 11), ("solid", 2000, 1100), ("tiny", 9, 9), ("meh", 500, 240)];
        let ranked = rank(rows, 0.5);
        let order: Vec<&str> = ranked.iter().map(|r| r.value).collect();
        // lucky: (11+25)/62 = 0.58; solid: (1100+25)/2050 = 0.549; meh: 0.48. tiny is hidden.
        assert_eq!(order, vec!["lucky", "solid", "meh"]);
        assert_eq!((ranked[1].play, ranked[1].win), (2000, 1100));
    }
}
