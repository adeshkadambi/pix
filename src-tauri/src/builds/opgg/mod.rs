//! OP.GG provider (SPEC §5.4).

pub mod client;
pub mod mapping;
pub mod notation;

use serde_json::{Value, json};

use super::{BuildProvider, BuildSet, ChampRecord, ChampionTraits, MetaEntry, ProviderError, Role};
use client::McpClient;

const BASE_FIELDS: &[&str] = &[
    "data.summary.average_stats.{win_rate,play}",
    "data.summary.positions[].name",
    "data.summary.positions[].stats.{role_rate}",
    "data.runes.{id,primary_page_id,primary_rune_ids[],secondary_page_id,secondary_rune_ids[],stat_mod_ids[],play,win}",
    "data.core_items.{ids[],play,win}",
    "data.boots.{ids[],play,win}",
    "data.starter_items.{ids[],play,win}",
    "data.summoner_spells.{ids[],play,win}",
    "data.fourth_items[].{ids[],play,win}",
    "data.fifth_items[].{ids[],play,win}",
    "data.sixth_items[].{ids[],play,win}",
    "data.last_items[].{ids[],play,win}",
    "data.skill_masteries.{ids[],play,win}",
    "data.damage_type",
];

const TRAIT_FIELDS: &[&str] = &["data.summary.positions[].name", "data.summary.positions[].stats.{role_rate}", "data.damage_type"];

pub struct OpggProvider {
    client: McpClient,
}

impl OpggProvider {
    pub fn new(http: reqwest::Client) -> Self {
        Self { client: McpClient::new(http) }
    }

    async fn analysis(&self, champion: &str, role: Role, fields: &[&str]) -> Result<Value, ProviderError> {
        let text = self
            .client
            .call_tool(
                "lol_get_champion_analysis",
                json!({"game_mode": "ranked", "champion": champion, "position": role.opgg(), "desired_output_fields": fields}),
            )
            .await?;
        notation::parse(&text).map_err(|e| ProviderError::Format(e.to_string()))
    }
}

#[async_trait::async_trait]
impl BuildProvider for OpggProvider {
    fn id(&self) -> &'static str {
        "opgg"
    }

    async fn base(&self, champion: &str, role: Role) -> Result<BuildSet, ProviderError> {
        Ok(mapping::analysis_to_buildset(&self.analysis(champion, role, BASE_FIELDS).await?))
    }

    async fn matchup(&self, champion: &str, role: Role, opponent: &str) -> Result<BuildSet, ProviderError> {
        let text = self
            .client
            .call_tool(
                "lol_get_lane_matchup_guide",
                json!({"my_champion": champion, "opponent_champion": opponent, "position": role.opgg()}),
            )
            .await?;
        let v: Value = serde_json::from_str(&text).map_err(|e| ProviderError::Format(e.to_string()))?;
        Ok(mapping::matchup_to_buildset(&v))
    }

    async fn traits(&self, champion: &str) -> Result<ChampionTraits, ProviderError> {
        // Role rates come back for every position, whichever one is asked for.
        Ok(mapping::analysis_to_traits(&self.analysis(champion, Role::Mid, TRAIT_FIELDS).await?))
    }

    async fn lane_meta(&self, role: Role) -> Result<Vec<MetaEntry>, ProviderError> {
        let field = format!("data.positions.{}[].{{champion,play,win,role_rate,tier,is_rip}}", role.opgg());
        let text = self
            .client
            .call_tool("lol_list_lane_meta_champions", json!({"position": role.opgg(), "desired_output_fields": [field]}))
            .await?;
        let v = notation::parse(&text).map_err(|e| ProviderError::Format(e.to_string()))?;
        Ok(mapping::meta_entries(&v, role))
    }

    async fn counters_to(&self, enemy: &str, role: Role) -> Result<Vec<ChampRecord>, ProviderError> {
        // Asked as the enemy (against itself), the reply lists its record vs every lane opponent.
        let text = self
            .client
            .call_tool(
                "lol_get_lane_matchup_guide",
                json!({"my_champion": enemy, "opponent_champion": enemy, "position": role.opgg()}),
            )
            .await?;
        let v: Value = serde_json::from_str(&text).map_err(|e| ProviderError::Format(e.to_string()))?;
        Ok(mapping::counters_to_enemy(&v))
    }

    async fn synergies(&self, ally: &str, ally_role: Role, partner_role: Role) -> Result<Vec<ChampRecord>, ProviderError> {
        if partner_role == Role::Top {
            return Ok(Vec::new()); // OP.GG has no top-lane synergy lists
        }
        let field = format!(
            "data.synergies.{}[].{{synergy_champion_id,synergy_champion_name,play,win}}",
            partner_role.opgg()
        );
        let v = self.analysis(ally, ally_role, &[field.as_str()]).await?;
        Ok(mapping::synergy_records(&v, partner_role))
    }
}
