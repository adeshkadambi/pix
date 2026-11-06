//! Champion-select builds (SPEC §5.3–5.5): provider abstraction, ranking, enemy role inference,
//! comp profile and the draft pipeline.

pub mod cache;
pub mod comp_profile;
pub mod draft;
pub mod opgg;
pub mod ranking;
pub mod role_inference;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Role {
    Top,
    Jungle,
    Mid,
    Adc,
    Support,
}

impl Role {
    pub const ALL: [Role; 5] = [Role::Top, Role::Jungle, Role::Mid, Role::Adc, Role::Support];

    /// OP.GG `position` argument.
    pub fn opgg(self) -> &'static str {
        match self {
            Role::Top => "top",
            Role::Jungle => "jungle",
            Role::Mid => "mid",
            Role::Adc => "adc",
            Role::Support => "support",
        }
    }

    /// League client `assignedPosition` (SPEC §5.2). Empty in blind pick.
    pub fn from_lcu(position: &str) -> Option<Role> {
        match position.to_ascii_lowercase().as_str() {
            "top" => Some(Role::Top),
            "jungle" => Some(Role::Jungle),
            "middle" | "mid" => Some(Role::Mid),
            "bottom" | "adc" => Some(Role::Adc),
            "utility" | "support" => Some(Role::Support),
            _ => None,
        }
    }

    /// OP.GG position names in replies ("MID", "ADC", …).
    pub fn from_opgg(name: &str) -> Option<Role> {
        Self::from_lcu(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum DamageType {
    AD,
    AP,
    BOTH,
}

impl DamageType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "AD" => Some(Self::AD),
            "AP" => Some(Self::AP),
            "BOTH" | "HYBRID" => Some(Self::BOTH),
            _ => None,
        }
    }
}

/// One option with its sample: games played, games won, and the smoothed win rate it's ranked by.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Ranked<T> {
    pub value: T,
    pub play: u32,
    pub win: u32,
    pub smoothed_wr: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RunePage {
    pub primary_style: u32,
    pub sub_style: u32,
    /// Keystone first, then the other primary runes, then the two secondary runes.
    pub perks: Vec<u32>,
    pub shards: Vec<u32>,
}

/// Everything a provider knows about one champion in one role (optionally vs a lane opponent).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BuildSet {
    /// The champion's win rate here: the prior for smoothing.
    pub champion_wr: f32,
    pub champion_games: u32,
    pub rune_pages: Vec<Ranked<RunePage>>,
    pub core_sets: Vec<Ranked<Vec<u32>>>,
    pub boots: Vec<Ranked<u32>>,
    pub starters: Vec<Ranked<Vec<u32>>>,
    pub summoners: Vec<Ranked<Vec<u32>>>,
    /// Options for the 4th, 5th and 6th items.
    pub later_items: Vec<Vec<Ranked<u32>>>,
    pub skill_orders: Vec<Ranked<Vec<String>>>,
    /// Every item seen in this champion's builds, for situational picks.
    pub item_pool: Vec<Ranked<u32>>,
    pub damage_type: Option<DamageType>,
    /// Role → share of this champion's games, for enemy role inference.
    pub role_rates: Vec<(Role, f32)>,
}

/// An enemy champion's traits from the provider: where it's played and how it deals damage.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChampionTraits {
    pub role_rates: Vec<(Role, f32)>,
    pub damage_type: Option<DamageType>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("OP.GG error: {0}")]
    Remote(String),
    #[error("unexpected OP.GG reply: {0}")]
    Format(String),
}

/// A source of build data. OP.GG today; a Riot-API provider can implement the same trait later.
#[async_trait::async_trait]
pub trait BuildProvider: Send + Sync {
    fn id(&self) -> &'static str;
    /// Builds for `champion` (OP.GG key, e.g. "MONKEYKING") in `role`.
    async fn base(&self, champion: &str, role: Role) -> Result<BuildSet, ProviderError>;
    /// Builds for `champion` in `role` against a lane `opponent` in the same role.
    async fn matchup(&self, champion: &str, role: Role, opponent: &str) -> Result<BuildSet, ProviderError>;
    async fn traits(&self, champion: &str) -> Result<ChampionTraits, ProviderError>;
}
