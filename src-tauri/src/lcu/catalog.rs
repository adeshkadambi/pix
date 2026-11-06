//! Champion, item and summoner spell names and icon paths, from the client's game data.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::http::{LcuError, LcuHttp};

/// A named thing with an icon. `icon` is a client asset path, served by the `lcu-asset` protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Icon {
    pub id: i32,
    pub name: String,
    pub icon: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEntry {
    /// i64 because some entries use 4294967295 as a "no id" placeholder.
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default, alias = "squarePortraitPath")]
    icon_path: Option<String>,
}

#[derive(Debug, Default)]
pub struct Catalog {
    champions: HashMap<i32, Icon>,
    items: HashMap<i32, Icon>,
    spells: HashMap<i32, Icon>,
}

fn index(entries: Vec<RawEntry>) -> HashMap<i32, Icon> {
    entries
        .into_iter()
        .filter_map(|e| {
            let id = i32::try_from(e.id).ok()?;
            Some((id, Icon { id, name: e.name, icon: e.icon_path.unwrap_or_default() }))
        })
        .collect()
}

impl Catalog {
    pub async fn fetch(http: &LcuHttp) -> Result<Self, LcuError> {
        let (champions, items, spells) = tokio::try_join!(
            http.get_json::<Vec<RawEntry>>("/lol-game-data/assets/v1/champion-summary.json"),
            http.get_json::<Vec<RawEntry>>("/lol-game-data/assets/v1/items.json"),
            http.get_json::<Vec<RawEntry>>("/lol-game-data/assets/v1/summoner-spells.json"),
        )?;
        Ok(Self { champions: index(champions), items: index(items), spells: index(spells) })
    }

    pub fn champion(&self, id: i32) -> Icon {
        self.champions.get(&id).cloned().unwrap_or_else(|| Icon {
            id,
            name: "Unknown".into(),
            icon: format!("/lol-game-data/assets/v1/champion-icons/{id}.png"),
        })
    }

    /// `None` for an empty slot (id 0).
    pub fn item(&self, id: i32) -> Option<Icon> {
        (id != 0).then(|| {
            self.items.get(&id).cloned().unwrap_or_else(|| Icon {
                id,
                name: format!("Item {id}"),
                icon: String::new(),
            })
        })
    }

    pub fn spell(&self, id: i32) -> Icon {
        self.spells.get(&id).cloned().unwrap_or_else(|| Icon { id, name: "Unknown".into(), icon: String::new() })
    }
}

#[cfg(test)]
impl Catalog {
    pub fn from_json(champions: &str, items: &str, spells: &str) -> Self {
        Self {
            champions: index(serde_json::from_str(champions).unwrap()),
            items: index(serde_json::from_str(items).unwrap()),
            spells: index(serde_json::from_str(spells).unwrap()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_entries_with_fallbacks() {
        let c = Catalog::from_json(
            r#"[{"id":-1,"name":"None"},{"id":1,"name":"Annie","squarePortraitPath":"/lol-game-data/assets/v1/champion-icons/1.png"}]"#,
            r#"[{"id":1001,"name":"Boots","iconPath":"/lol-game-data/assets/ASSETS/Items/Icons2D/1001_Class_T1_BootsofSpeed.png"}]"#,
            r#"[{"id":4,"name":"Flash","iconPath":"/lol-game-data/assets/DATA/Spells/Icons2D/Summoner_flash.png"},
                {"id":4294967295,"name":"Primal Smite","iconPath":"/x.png"}]"#,
        );
        assert_eq!(c.champion(1).name, "Annie");
        assert_eq!(c.champion(1).icon, "/lol-game-data/assets/v1/champion-icons/1.png");
        assert_eq!(c.champion(999).icon, "/lol-game-data/assets/v1/champion-icons/999.png");
        assert_eq!(c.item(1001).unwrap().name, "Boots");
        assert_eq!(c.item(0), None);
        assert_eq!(c.item(42).unwrap().name, "Item 42");
        assert_eq!(c.spell(4).name, "Flash");
    }
}
