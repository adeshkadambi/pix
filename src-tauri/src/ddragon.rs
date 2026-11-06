//! Data Dragon: Riot's static game data CDN, cached on disk per version.
//!
//! M1 uses it for icon fallbacks when the client isn't running. Later milestones read champion
//! tags, item gold and descriptions, and summoner cooldowns from the same cached files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

const CDN: &str = "https://ddragon.leagueoflegends.com";
const FILES: [&str; 3] = ["champion.json", "item.json", "summoner.json"];

#[derive(Debug, thiserror::Error)]
pub enum DDragonError {
    #[error("Data Dragon request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Data Dragon cache error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Data Dragon data is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no Data Dragon version available")]
    NoVersion,
}

#[derive(Debug, Default)]
pub struct DData {
    pub version: String,
    /// Numeric champion key → Data Dragon id, e.g. 62 → "MonkeyKing".
    pub champion_ids: HashMap<i32, String>,
    /// Numeric champion key → name and class tags.
    pub champions: HashMap<i32, ChampInfo>,
    /// Item id → name, tags, plain description and cost.
    pub items: HashMap<u32, ItemInfo>,
    /// Summoner spell key ("SummonerFlash") → numeric id, name and base cooldown.
    pub summoners: HashMap<String, SpellInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpellInfo {
    pub id: i32,
    pub name: String,
    pub cooldown: f32,
}

fn summoners(summoner_json: &Value) -> HashMap<String, SpellInfo> {
    let Some(data) = summoner_json.get("data").and_then(Value::as_object) else {
        return HashMap::new();
    };
    data.iter()
        .filter_map(|(key, s)| {
            Some((
                key.clone(),
                SpellInfo {
                    id: s.get("key")?.as_str()?.parse().ok()?,
                    name: s.get("name")?.as_str()?.to_owned(),
                    cooldown: s.pointer("/cooldown/0").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                },
            ))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChampInfo {
    /// Data Dragon id, e.g. "MonkeyKing". Uppercased, it's the OP.GG champion key.
    pub id: String,
    pub name: String,
    /// Class tags: Assassin, Fighter, Mage, Marksman, Support, Tank.
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ItemInfo {
    pub name: String,
    pub tags: Vec<String>,
    /// Description with HTML tags removed.
    pub description: String,
    pub gold_total: u32,
    /// Finished item: builds into nothing else.
    pub completed: bool,
    /// Buyable on Summoner's Rift (map 11).
    pub on_rift: bool,
}

/// Strips HTML tags from Data Dragon descriptions.
pub fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn champions(champion_json: &Value) -> HashMap<i32, ChampInfo> {
    let Some(data) = champion_json.get("data").and_then(Value::as_object) else {
        return HashMap::new();
    };
    data.values()
        .filter_map(|c| {
            let key = c.get("key")?.as_str()?.parse().ok()?;
            let tags = c.get("tags").and_then(Value::as_array).map(|t| t.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default();
            Some((key, ChampInfo { id: c.get("id")?.as_str()?.to_owned(), name: c.get("name")?.as_str()?.to_owned(), tags }))
        })
        .collect()
}

fn items(item_json: &Value) -> HashMap<u32, ItemInfo> {
    let Some(data) = item_json.get("data").and_then(Value::as_object) else {
        return HashMap::new();
    };
    data.iter()
        .filter_map(|(id, it)| {
            let into_empty = it.get("into").and_then(Value::as_array).is_none_or(|a| a.is_empty());
            Some((
                id.parse().ok()?,
                ItemInfo {
                    name: it.get("name")?.as_str()?.to_owned(),
                    tags: it.get("tags").and_then(Value::as_array).map(|t| t.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default(),
                    description: strip_html(it.get("description").and_then(Value::as_str).unwrap_or_default()),
                    gold_total: it.pointer("/gold/total").and_then(Value::as_u64).unwrap_or(0) as u32,
                    completed: into_empty,
                    on_rift: it.pointer("/maps/11").and_then(Value::as_bool).unwrap_or(false),
                },
            ))
        })
        .collect()
}

impl DData {
    pub fn cdn_url(&self, rest: &str) -> String {
        format!("{CDN}/cdn/{}/{rest}", self.version)
    }

    /// Numeric champion key for a Data Dragon id ("MonkeyKing" → 62).
    pub fn champion_key(&self, ddragon_id: &str) -> Option<i32> {
        self.champions.iter().find(|(_, c)| c.id.eq_ignore_ascii_case(ddragon_id)).map(|(k, _)| *k)
    }
}

/// "16.22.8348910+branch…" → "16.22".
pub fn patch_of(game_version: &str) -> Option<String> {
    let mut parts = game_version.split('.');
    let (major, minor) = (parts.next()?, parts.next()?);
    (major.chars().all(|c| c.is_ascii_digit()) && minor.chars().all(|c| c.is_ascii_digit()) && !major.is_empty() && !minor.is_empty())
        .then(|| format!("{major}.{minor}"))
}

/// The newest Data Dragon version for this patch, else the newest overall. `versions` is newest first.
pub fn pick_version(versions: &[String], patch: Option<&str>) -> Option<String> {
    patch
        .and_then(|p| versions.iter().find(|v| v.starts_with(&format!("{p}."))))
        .or_else(|| versions.first())
        .cloned()
}

fn champion_ids(champion_json: &Value) -> HashMap<i32, String> {
    let Some(data) = champion_json.get("data").and_then(Value::as_object) else {
        return HashMap::new();
    };
    data.values()
        .filter_map(|c| {
            let key = c.get("key")?.as_str()?.parse().ok()?;
            Some((key, c.get("id")?.as_str()?.to_owned()))
        })
        .collect()
}

/// Newest version folder already in the cache, for offline starts.
fn newest_cached(root: &Path) -> Option<String> {
    let mut versions: Vec<String> = std::fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("champion.json").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    versions.sort_by_key(|v| v.split('.').map(|p| p.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>());
    versions.pop()
}

async fn fetch_versions(client: &reqwest::Client) -> Result<Vec<String>, DDragonError> {
    Ok(client.get(format!("{CDN}/api/versions.json")).send().await?.error_for_status()?.json().await?)
}

async fn cached_file(client: &reqwest::Client, dir: &Path, version: &str, file: &str) -> Result<Value, DDragonError> {
    let path = dir.join(file);
    if let Ok(text) = tokio::fs::read_to_string(&path).await
        && let Ok(v) = serde_json::from_str(&text)
    {
        return Ok(v);
    }
    let url = format!("{CDN}/cdn/{version}/data/en_US/{file}");
    let text = client.get(url).send().await?.error_for_status()?.text().await?;
    let value: Value = serde_json::from_str(&text)?;
    tokio::fs::create_dir_all(dir).await?;
    tokio::fs::write(&path, text).await?;
    Ok(value)
}

/// Loads Data Dragon for `patch` (or the newest version), using the disk cache under `root`.
pub async fn load(client: &reqwest::Client, root: &Path, patch: Option<&str>) -> Result<DData, DDragonError> {
    let version = match fetch_versions(client).await {
        Ok(versions) => pick_version(&versions, patch),
        Err(e) => {
            tracing::info!("Data Dragon versions unavailable ({e}); using the cache");
            newest_cached(root)
        }
    }
    .ok_or(DDragonError::NoVersion)?;
    let dir: PathBuf = root.join(&version);
    let (mut champion, mut item, mut summoner) = (Value::Null, Value::Null, Value::Null);
    for file in FILES {
        let v = cached_file(client, &dir, &version, file).await?;
        match file {
            "champion.json" => champion = v,
            "item.json" => item = v,
            _ => summoner = v,
        }
    }
    Ok(DData {
        champion_ids: champion_ids(&champion),
        champions: champions(&champion),
        items: items(&item),
        summoners: summoners(&summoner),
        version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_from_game_version() {
        assert_eq!(patch_of("16.22.8348910+branch.releases-16-22.code.public"), Some("16.22".into()));
        assert_eq!(patch_of("14.1.1"), Some("14.1".into()));
        assert_eq!(patch_of("garbage"), None);
        assert_eq!(patch_of("x.y.z"), None);
    }

    #[test]
    fn picks_the_version_for_the_patch() {
        let versions: Vec<String> = ["16.23.1", "16.22.2", "16.22.1", "16.21.1"].map(String::from).to_vec();
        assert_eq!(pick_version(&versions, Some("16.22")), Some("16.22.2".into()));
        assert_eq!(pick_version(&versions, Some("15.1")), Some("16.23.1".into()));
        assert_eq!(pick_version(&versions, None), Some("16.23.1".into()));
        assert_eq!(pick_version(&[], None), None);
    }

    #[test]
    fn maps_champion_keys_to_ids() {
        let json = serde_json::json!({"data": {
            "MonkeyKing": {"id": "MonkeyKing", "key": "62", "name": "Wukong"},
            "Kaisa": {"id": "Kaisa", "key": "145", "name": "Kai'Sa"}
        }});
        let ids = champion_ids(&json);
        assert_eq!(ids.get(&62).map(String::as_str), Some("MonkeyKing"));
        assert_eq!(ids.get(&145).map(String::as_str), Some("Kaisa"));
    }

    #[test]
    fn parses_champions_and_items() {
        let champs = champions(&serde_json::json!({"data": {
            "Malphite": {"id": "Malphite", "key": "54", "name": "Malphite", "tags": ["Tank", "Mage"]}
        }}));
        assert_eq!(champs[&54].tags, vec!["Tank", "Mage"]);
        let its = items(&serde_json::json!({"data": {
            "3165": {"name": "Morellonomicon", "tags": ["SpellDamage", "Health"],
                     "description": "<mainText><stats>+75 Ability Power</stats><br><passive>Affliction</passive>: inflicts <status>40% Grievous Wounds</status></mainText>",
                     "gold": {"total": 2950}, "maps": {"11": true, "12": true}},
            "3916": {"name": "Oblivion Orb", "into": ["3165"], "tags": [], "description": "", "gold": {"total": 800}, "maps": {"11": true}}
        }}));
        let morello = &its[&3165];
        assert!(morello.completed && morello.on_rift);
        assert_eq!(morello.gold_total, 2950);
        assert!(morello.description.contains("40% Grievous Wounds"));
        assert!(!morello.description.contains('<'));
        assert!(!its[&3916].completed);
    }

    #[test]
    fn parses_summoner_spells() {
        let s = summoners(&serde_json::json!({"data": {
            "SummonerFlash": {"id": "SummonerFlash", "key": "4", "name": "Flash", "cooldown": [300]},
            "SummonerTeleport": {"id": "SummonerTeleport", "key": "12", "name": "Teleport", "cooldown": [360]}
        }}));
        assert_eq!(s["SummonerFlash"], SpellInfo { id: 4, name: "Flash".into(), cooldown: 300.0 });
        assert_eq!(s["SummonerTeleport"].id, 12);
    }

    #[test]
    fn strips_html() {
        assert_eq!(strip_html("<b>12</b> Summoner Spell Haste<br>x"), "12 Summoner Spell Haste x");
    }

    #[test]
    fn finds_newest_cached_version() {
        let root = tempfile::tempdir().unwrap();
        for v in ["16.9.1", "16.22.1", "16.3.1"] {
            std::fs::create_dir_all(root.path().join(v)).unwrap();
            std::fs::write(root.path().join(v).join("champion.json"), "{}").unwrap();
        }
        std::fs::create_dir_all(root.path().join("16.99.1")).unwrap(); // incomplete: no champion.json
        assert_eq!(newest_cached(root.path()), Some("16.22.1".into()));
    }
}
