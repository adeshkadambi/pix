//! Local SQLite game log (SPEC §5.12): games, rank snapshots and credited LP changes.
//! Only the user's own data, keyed by account `puuid`. Nothing leaves the machine.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};

use crate::game::review::ReviewView;
use crate::lp::{self, GameRef, LpChange, Snapshot};
use crate::profile::{MatchSummary, RankedQueue};

const SCHEMA_VERSION: i32 = 2;

const SCHEMA_V1: &str = "
CREATE TABLE games (
    puuid TEXT NOT NULL,
    game_id INTEGER NOT NULL,
    created_ms INTEGER NOT NULL,
    queue_id INTEGER NOT NULL,
    duration_secs INTEGER NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY (puuid, game_id)
);
CREATE INDEX games_by_time ON games (puuid, created_ms DESC);

CREATE TABLE ranked_snapshots (
    puuid TEXT NOT NULL,
    queue_type TEXT NOT NULL,
    taken_ms INTEGER NOT NULL,
    tier TEXT NOT NULL,
    division TEXT NOT NULL,
    lp INTEGER NOT NULL,
    wins INTEGER NOT NULL,
    losses INTEGER NOT NULL,
    PRIMARY KEY (puuid, queue_type, taken_ms)
);

CREATE TABLE lp_changes (
    puuid TEXT NOT NULL,
    game_id INTEGER NOT NULL,
    queue_type TEXT NOT NULL,
    delta INTEGER NOT NULL,
    tier_before TEXT NOT NULL,
    division_before TEXT NOT NULL,
    tier_after TEXT NOT NULL,
    division_after TEXT NOT NULL,
    lp_after INTEGER NOT NULL,
    PRIMARY KEY (puuid, game_id)
);
";

const SCHEMA_V2: &str = "
CREATE TABLE reviews (
    puuid TEXT NOT NULL,
    game_id INTEGER NOT NULL,
    created_ms INTEGER NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY (puuid, game_id)
);
";

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("stored game is unreadable: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, DbError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, DbError> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(SCHEMA_V1)?;
        }
        if version < 2 {
            conn.execute_batch(SCHEMA_V2)?;
        }
        if version < SCHEMA_VERSION {
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Saves games from the client's history. Existing rows are refreshed.
    pub fn upsert_games(&self, puuid: &str, games: &[MatchSummary]) -> Result<(), DbError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO games (puuid, game_id, created_ms, queue_id, duration_secs, data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (puuid, game_id) DO UPDATE SET data = excluded.data",
            )?;
            for g in games {
                let mut stored = g.clone();
                stored.lp_change = None; // joined from lp_changes on read
                stmt.execute(params![
                    puuid,
                    g.game_id as i64,
                    g.created_ms as i64,
                    g.queue_id,
                    g.duration_secs,
                    serde_json::to_string(&stored)?
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Records a rank snapshot if it differs from the latest one for that queue.
    /// Returns true if a row was written.
    pub fn record_snapshot(&self, puuid: &str, queue_type: &str, taken_ms: u64, rank: &RankedQueue) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let latest: Option<RankedQueue> = conn
            .query_row(
                "SELECT tier, division, lp, wins, losses FROM ranked_snapshots
                 WHERE puuid = ?1 AND queue_type = ?2 ORDER BY taken_ms DESC LIMIT 1",
                params![puuid, queue_type],
                |r| Ok(RankedQueue { tier: r.get(0)?, division: r.get(1)?, lp: r.get(2)?, wins: r.get(3)?, losses: r.get(4)? }),
            )
            .optional()?;
        if latest.as_ref() == Some(rank) {
            return Ok(false);
        }
        conn.execute(
            "INSERT OR REPLACE INTO ranked_snapshots (puuid, queue_type, taken_ms, tier, division, lp, wins, losses)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![puuid, queue_type, taken_ms as i64, rank.tier, rank.division, rank.lp, rank.wins, rank.losses],
        )?;
        Ok(true)
    }

    /// Credits LP changes for stored ranked games that don't have one yet. Returns how many.
    pub fn credit_lp(&self, puuid: &str) -> Result<usize, DbError> {
        let conn = self.conn.lock().unwrap();
        let games: Vec<(GameRef, bool)> = {
            let mut stmt = conn.prepare(
                "SELECT g.game_id, g.created_ms, g.duration_secs, g.queue_id, g.data, l.game_id IS NOT NULL
                 FROM games g LEFT JOIN lp_changes l ON l.puuid = g.puuid AND l.game_id = g.game_id
                 WHERE g.puuid = ?1 AND g.queue_id IN (420, 440)",
            )?;
            let rows = stmt.query_map(params![puuid], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, u32>(2)?, r.get::<_, i32>(3)?, r.get::<_, String>(4)?, r.get::<_, bool>(5)?))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (id, created, duration, queue, data, credited) = row?;
                let summary: MatchSummary = serde_json::from_str(&data)?;
                out.push((
                    GameRef { game_id: id as u64, created_ms: created as u64, duration_secs: duration, queue_id: queue, result: summary.result },
                    credited,
                ));
            }
            out
        };
        let pending: Vec<GameRef> = games.iter().filter(|(_, c)| !c).map(|(g, _)| g.clone()).collect();
        if pending.is_empty() {
            return Ok(0);
        }
        let all: Vec<GameRef> = games.into_iter().map(|(g, _)| g).collect();
        let snapshots: Vec<Snapshot> = {
            let mut stmt = conn.prepare(
                "SELECT queue_type, taken_ms, tier, division, lp, wins, losses FROM ranked_snapshots
                 WHERE puuid = ?1 ORDER BY taken_ms",
            )?;
            stmt.query_map(params![puuid], |r| {
                Ok(Snapshot {
                    queue_type: r.get(0)?,
                    taken_ms: r.get::<_, i64>(1)? as u64,
                    rank: RankedQueue { tier: r.get(2)?, division: r.get(3)?, lp: r.get(4)?, wins: r.get(5)?, losses: r.get(6)? },
                })
            })?
            .collect::<Result<_, _>>()?
        };
        let credits = lp::credit(&pending, &all, &snapshots);
        for c in &credits {
            let ch = &c.change;
            conn.execute(
                "INSERT OR IGNORE INTO lp_changes
                 (puuid, game_id, queue_type, delta, tier_before, division_before, tier_after, division_after, lp_after)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![puuid, c.game_id as i64, c.queue_type, ch.delta, ch.tier_before, ch.division_before, ch.tier_after, ch.division_after, ch.lp_after],
            )?;
        }
        Ok(credits.len())
    }

    /// Stored games, newest first, with their LP change when known.
    pub fn games(&self, puuid: &str, limit: usize) -> Result<Vec<MatchSummary>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT g.data, l.delta, l.tier_before, l.division_before, l.tier_after, l.division_after, l.lp_after,
                    r.game_id IS NOT NULL
             FROM games g
             LEFT JOIN lp_changes l ON l.puuid = g.puuid AND l.game_id = g.game_id
             LEFT JOIN reviews r ON r.puuid = g.puuid AND r.game_id = g.game_id
             WHERE g.puuid = ?1 ORDER BY g.created_ms DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![puuid, limit as i64], |r| Ok((r.get::<_, String>(0)?, lp_from(r, 1)?, r.get::<_, bool>(7)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (data, lp_change, has_review) = row?;
            let mut m: MatchSummary = serde_json::from_str(&data)?;
            m.lp_change = lp_change;
            m.has_review = has_review;
            out.push(m);
        }
        Ok(out)
    }

    /// Saves (or replaces) the post-game review for a game.
    pub fn save_review(&self, puuid: &str, review: &ReviewView) -> Result<(), DbError> {
        let mut stored = review.clone();
        stored.lp_change = None; // joined from lp_changes on read
        self.conn.lock().unwrap().execute(
            "INSERT OR REPLACE INTO reviews (puuid, game_id, created_ms, data) VALUES (?1, ?2, ?3, ?4)",
            params![puuid, review.game_id as i64, review.created_ms as i64, serde_json::to_string(&stored)?],
        )?;
        Ok(())
    }

    /// A game's review (`game_id`), or the newest one, with its LP change when known.
    pub fn review(&self, puuid: &str, game_id: Option<u64>) -> Result<Option<ReviewView>, DbError> {
        let conn = self.conn.lock().unwrap();
        let sql = "SELECT r.data, l.delta, l.tier_before, l.division_before, l.tier_after, l.division_after, l.lp_after
                   FROM reviews r LEFT JOIN lp_changes l ON l.puuid = r.puuid AND l.game_id = r.game_id
                   WHERE r.puuid = ?1 AND (?2 IS NULL OR r.game_id = ?2)
                   ORDER BY r.created_ms DESC LIMIT 1";
        let row = conn
            .query_row(sql, params![puuid, game_id.map(|g| g as i64)], |r| Ok((r.get::<_, String>(0)?, lp_from(r, 1)?)))
            .optional()?;
        let Some((data, lp)) = row else { return Ok(None) };
        let mut review: ReviewView = serde_json::from_str(&data)?;
        review.lp_change = lp;
        Ok(Some(review))
    }
}

/// An LP change from the 6 lp_changes columns starting at `at` (all NULL when there's none).
fn lp_from(r: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<Option<LpChange>> {
    Ok(match r.get::<_, Option<i32>>(at)? {
        Some(delta) => Some(LpChange {
            delta,
            tier_before: r.get(at + 1)?,
            division_before: r.get(at + 2)?,
            tier_after: r.get(at + 3)?,
            division_after: r.get(at + 4)?,
            lp_after: r.get(at + 5)?,
        }),
        None => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lcu::catalog::Icon;
    use crate::profile::MatchResult;

    const MIN: u64 = 60_000;

    fn summary(id: u64, created_ms: u64, queue_id: i32, result: MatchResult) -> MatchSummary {
        MatchSummary {
            game_id: id,
            champion: Icon { id: 233, name: "Briar".into(), icon: "/a/233.png".into() },
            queue: "Ranked Solo".into(),
            queue_id,
            result,
            kills: 1,
            deaths: 2,
            assists: 3,
            cs: 100,
            duration_secs: 1800,
            created_ms,
            items: vec![None; 6],
            spells: vec![],
            lp_change: None,
            has_review: false,
        }
    }

    fn rank(lp: i32, wins: u32, losses: u32) -> RankedQueue {
        RankedQueue { tier: "SILVER".into(), division: "III".into(), lp, wins, losses }
    }

    #[test]
    fn stores_games_newest_first_and_dedupes() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_games("me", &[summary(1, 100, 420, MatchResult::Win), summary(2, 300, 450, MatchResult::Loss)]).unwrap();
        db.upsert_games("me", &[summary(2, 300, 450, MatchResult::Loss), summary(3, 200, 420, MatchResult::Loss)]).unwrap();
        db.upsert_games("other", &[summary(9, 999, 420, MatchResult::Win)]).unwrap();
        let ids: Vec<u64> = db.games("me", 50).unwrap().iter().map(|m| m.game_id).collect();
        assert_eq!(ids, vec![2, 3, 1]);
        assert_eq!(db.games("me", 2).unwrap().len(), 2);
        assert_eq!(db.games("other", 50).unwrap().len(), 1, "accounts stay separate");
    }

    #[test]
    fn snapshots_are_written_only_when_rank_changes() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.record_snapshot("me", lp::SOLO, 1, &rank(20, 3, 5)).unwrap());
        assert!(!db.record_snapshot("me", lp::SOLO, 2, &rank(20, 3, 5)).unwrap());
        assert!(db.record_snapshot("me", lp::FLEX, 3, &rank(20, 3, 5)).unwrap(), "queues are separate");
        assert!(db.record_snapshot("me", lp::SOLO, 4, &rank(41, 4, 5)).unwrap());
    }

    #[test]
    fn end_to_end_lp_credit() {
        let db = Db::open_in_memory().unwrap();
        // Before the game (champ select), then the game, then the post-game fetch.
        db.record_snapshot("me", lp::SOLO, 5 * MIN, &rank(20, 3, 5)).unwrap();
        db.upsert_games("me", &[summary(1, 10 * MIN, 420, MatchResult::Win)]).unwrap();
        db.record_snapshot("me", lp::SOLO, 45 * MIN, &rank(41, 4, 5)).unwrap();
        assert_eq!(db.credit_lp("me").unwrap(), 1);
        assert_eq!(db.credit_lp("me").unwrap(), 0, "already credited");
        let games = db.games("me", 10).unwrap();
        let change = games[0].lp_change.as_ref().unwrap();
        assert_eq!((change.delta, change.lp_after), (21, 41));
        // A refetch of the same game keeps its LP change.
        db.upsert_games("me", &[summary(1, 10 * MIN, 420, MatchResult::Win)]).unwrap();
        assert_eq!(db.games("me", 10).unwrap()[0].lp_change.as_ref().unwrap().delta, 21);
    }

    fn review(game_id: u64, created_ms: u64) -> ReviewView {
        ReviewView {
            game_id,
            created_ms,
            champion: Icon { id: 233, name: "Briar".into(), icon: String::new() },
            result: Some(MatchResult::Win),
            kills: 6,
            deaths: 2,
            assists: 5,
            cs: 180,
            duration_secs: 1800,
            deaths_by: vec![],
            timeline: vec![],
            counters: vec![],
            lp_change: None,
        }
    }

    #[test]
    fn reviews_are_saved_linked_and_carry_lp() {
        let db = Db::open_in_memory().unwrap();
        db.record_snapshot("me", lp::SOLO, 5 * MIN, &rank(20, 3, 5)).unwrap();
        db.upsert_games("me", &[summary(1, 10 * MIN, 420, MatchResult::Win), summary(2, 100 * MIN, 400, MatchResult::Loss)]).unwrap();
        db.record_snapshot("me", lp::SOLO, 45 * MIN, &rank(41, 4, 5)).unwrap();
        db.credit_lp("me").unwrap();
        assert!(db.review("me", None).unwrap().is_none());
        db.save_review("me", &review(1, 41 * MIN)).unwrap();
        let r = db.review("me", Some(1)).unwrap().unwrap();
        assert_eq!((r.kills, r.lp_change.as_ref().unwrap().delta), (6, 21));
        assert!(db.review("me", Some(2)).unwrap().is_none());
        assert!(db.review("other", None).unwrap().is_none(), "accounts stay separate");
        let games = db.games("me", 10).unwrap();
        assert_eq!(games.iter().map(|g| (g.game_id, g.has_review)).collect::<Vec<_>>(), vec![(2, false), (1, true)]);
        db.save_review("me", &review(2, 120 * MIN)).unwrap();
        assert_eq!(db.review("me", None).unwrap().unwrap().game_id, 2, "newest review");
    }

    #[test]
    fn upgrades_a_version_1_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pix.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(SCHEMA_V1).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
        }
        let db = Db::open(&path).unwrap();
        db.save_review("me", &review(1, 1)).unwrap();
        assert!(db.review("me", Some(1)).unwrap().is_some());
    }

    #[test]
    fn reopening_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pix.db");
        Db::open(&path).unwrap().upsert_games("me", &[summary(1, 100, 420, MatchResult::Win)]).unwrap();
        assert_eq!(Db::open(&path).unwrap().games("me", 10).unwrap().len(), 1);
    }
}
