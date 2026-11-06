//! User settings and feature flags, saved as JSON in the app config dir.
//!
//! Every flag defaults to off (SPEC §2): the gray overlay features stay off until Riot approves
//! them, and the live advisor is off-policy.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const FILE_NAME: &str = "config.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct Flags {
    /// Summoner spell timers in the overlay. Gray: not named in Riot policy.
    pub overlay_timers: bool,
    /// Item-gold diff in the overlay. Gray: not named in Riot policy.
    pub overlay_gold: bool,
    /// In-game item advice in the companion. Not Riot-compliant.
    pub live_advisor: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Flag {
    OverlayTimers,
    OverlayGold,
    LiveAdvisor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct Config {
    pub flags: Flags,
    /// Key that opens the in-game scoreboard, for people who rebind Tab.
    pub scoreboard_key: String,
    /// League install folder, used if Riot's install-settings file can't be read.
    pub install_path: Option<String>,
    /// Tab panel position from calibration, per monitor resolution ("1920x1080" → offset from
    /// the monitor's top-left, in physical pixels).
    pub tabpanel_offsets: std::collections::BTreeMap<String, (i32, i32)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            flags: Flags::default(),
            scoreboard_key: "Tab".into(),
            install_path: None,
            tabpanel_offsets: Default::default(),
        }
    }
}

impl Config {
    /// Loads the config. A missing or unreadable file gives the defaults, so the app always starts.
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("config at {} is invalid, using defaults: {e}", path.display());
                Self::default()
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                tracing::warn!("can't read config at {}, using defaults: {e}", path.display());
                Self::default()
            }
        }
    }

    /// Saves the config. Writes a temp file first so a crash never leaves a half-written file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)
    }

    pub fn set_flag(&mut self, flag: Flag, on: bool) {
        let slot = match flag {
            Flag::OverlayTimers => &mut self.flags.overlay_timers,
            Flag::OverlayGold => &mut self.flags.overlay_gold,
            Flag::LiveAdvisor => &mut self.flags.live_advisor,
        };
        *slot = on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_default_to_off() {
        let flags = Config::default().flags;
        assert!(!flags.overlay_timers);
        assert!(!flags.overlay_gold);
        assert!(!flags.live_advisor);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Config::load(&dir.path().join(FILE_NAME)), Config::default());
    }

    #[test]
    fn invalid_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(Config::load(&path), Config::default());
    }

    #[test]
    fn partial_file_fills_in_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        fs::write(&path, r#"{"flags":{"overlay_gold":true}}"#).unwrap();
        let cfg = Config::load(&path);
        assert!(cfg.flags.overlay_gold);
        assert!(!cfg.flags.overlay_timers);
        assert!(!cfg.flags.live_advisor);
        assert_eq!(cfg.scoreboard_key, "Tab");
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(FILE_NAME);
        let mut cfg = Config::default();
        cfg.set_flag(Flag::LiveAdvisor, true);
        cfg.scoreboard_key = "O".into();
        cfg.save(&path).unwrap();
        assert_eq!(Config::load(&path), cfg);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn set_flag_changes_only_that_flag() {
        let mut cfg = Config::default();
        cfg.set_flag(Flag::OverlayTimers, true);
        assert_eq!(
            cfg.flags,
            Flags { overlay_timers: true, overlay_gold: false, live_advisor: false }
        );
        cfg.set_flag(Flag::OverlayTimers, false);
        assert_eq!(cfg.flags, Flags::default());
    }
}
