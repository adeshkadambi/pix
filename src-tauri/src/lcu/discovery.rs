//! Finding the League install folder and its lockfile (SPEC §5.1). Never inspects processes.

use std::fs;
use std::path::{Path, PathBuf};

pub const PRODUCT_SETTINGS: &str = r"C:\ProgramData\Riot Games\Metadata\league_of_legends.live\league_of_legends.live.product_settings.yaml";
pub const DEFAULT_INSTALL: &str = r"C:\Riot Games\League of Legends";

/// Reads `product_install_full_path` from Riot's install-settings YAML.
/// A line scan is enough for this one flat key; no YAML parser needed.
pub fn install_path_from_settings(yaml: &str) -> Option<PathBuf> {
    yaml.lines().find_map(|line| {
        let value = line.trim().strip_prefix("product_install_full_path:")?.trim();
        let value = value.trim_matches(|c| c == '"' || c == '\'');
        (!value.is_empty()).then(|| PathBuf::from(value))
    })
}

/// Install folders to try, in order: Riot's settings file, the default path, the config override.
pub fn candidates(settings_yaml: Option<&str>, config_override: Option<&str>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = settings_yaml.and_then(install_path_from_settings) {
        out.push(p);
    }
    out.push(PathBuf::from(DEFAULT_INSTALL));
    if let Some(p) = config_override.filter(|p| !p.trim().is_empty()) {
        out.push(PathBuf::from(p.trim()));
    }
    out.dedup();
    out
}

/// The first candidate folder that has a lockfile right now.
pub fn find_lockfile(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().map(|dir| dir.join("lockfile")).find(|p| p.is_file())
}

/// Reads the settings file and returns the current lockfile path, if the client is running.
pub fn locate(config_override: Option<&str>) -> Option<PathBuf> {
    let yaml = fs::read_to_string(Path::new(PRODUCT_SETTINGS)).ok();
    find_lockfile(&candidates(yaml.as_deref(), config_override))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
auto_patching_enabled_by_player: false
dependencies:
    Direct X 9:
        hash: ""
product_install_full_path: "C:/Riot Games/League of Legends"
product_install_root: "C:/Riot Games"
"#;

    #[test]
    fn reads_install_path_from_settings() {
        assert_eq!(
            install_path_from_settings(SAMPLE),
            Some(PathBuf::from("C:/Riot Games/League of Legends"))
        );
        assert_eq!(install_path_from_settings("product_install_full_path: 'D:/LoL'"), Some(PathBuf::from("D:/LoL")));
        assert_eq!(install_path_from_settings("product_install_full_path: \"\""), None);
        assert_eq!(install_path_from_settings("other: x"), None);
    }

    #[test]
    fn candidate_order() {
        let c = candidates(Some("product_install_full_path: \"D:/Games/LoL\""), Some(r"E:\LoL"));
        assert_eq!(c, vec![PathBuf::from("D:/Games/LoL"), PathBuf::from(DEFAULT_INSTALL), PathBuf::from(r"E:\LoL")]);
        assert_eq!(candidates(None, Some("  ")), vec![PathBuf::from(DEFAULT_INSTALL)]);
    }

    #[test]
    fn finds_first_folder_with_a_lockfile() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        fs::write(b.path().join("lockfile"), "LeagueClient:1:2:p:https").unwrap();
        let found = find_lockfile(&[a.path().to_path_buf(), b.path().to_path_buf()]);
        assert_eq!(found, Some(b.path().join("lockfile")));
        assert_eq!(find_lockfile(&[a.path().to_path_buf()]), None);
    }
}
