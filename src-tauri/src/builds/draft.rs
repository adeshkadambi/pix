//! Draft pipeline (SPEC §5.2–5.5): champ select session → enemy roles → builds → `DraftView`.
//!
//! Runs on every session update. Cheap parts (teams, bans) are emitted at once; build data follows
//! when the provider answers. A generation counter drops stale results when the draft moves on.

use std::collections::HashMap;

use futures_util::future::join_all;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use ts_rs::TS;

use super::cache::{self, BuildCache};
use super::comp_profile::{self, EnemyChampion, Pick};
use super::opgg::OpggProvider;
use super::role_inference::{self, InferredRole};
use super::{BuildProvider, BuildSet, ChampionTraits, ProviderError, Ranked, Role, RunePage};
use crate::ddragon::DData;
use crate::lcu::catalog::{Catalog, Icon};
use crate::lcu::champ_select::{self, DraftState};
use crate::profile;
use crate::state::AppState;

pub const DRAFT_EVENT: &str = "draft";
const TOP_OPTIONS: usize = 3;

// ---------- View types (sent to the Draft screen) ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub enum DraftStatus {
    /// Waiting for you to hover or lock a champion.
    NeedChampion,
    /// Blind pick: choose your role.
    NeedRole,
    Loading,
    Ready,
    /// Build data is off (not Summoner's Rift).
    Off,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct AllyView {
    pub cell_id: i32,
    pub champion: Option<Icon>,
    pub role: Option<Role>,
    pub locked: bool,
    pub is_me: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct EnemyView {
    pub cell_id: i32,
    pub champion: Option<Icon>,
    /// e.g. "Tank · AP".
    pub tags: String,
    pub inferred: Option<InferredRole>,
    pub lane_opponent: bool,
}

/// One ranked option: icons plus its sample.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct IconOption {
    pub icons: Vec<Icon>,
    pub play: u32,
    pub win: u32,
    pub smoothed_wr: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct SkillOption {
    pub order: Vec<String>,
    pub play: u32,
    pub win: u32,
    pub smoothed_wr: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct RunePageView {
    pub primary_style: Icon,
    pub sub_style: Icon,
    /// Keystone first, three more primary runes, then two secondary runes.
    pub perks: Vec<Icon>,
    pub shards: Vec<Icon>,
    pub play: u32,
    pub win: u32,
    pub smoothed_wr: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct SituationalView {
    pub item: Icon,
    pub reason: String,
    pub play: u32,
    pub win: u32,
    pub smoothed_wr: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct ThreatView {
    pub champion: Icon,
    pub answers: Vec<SituationalView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct BuildView {
    pub champion: Icon,
    pub role: Role,
    /// Lane opponent when the build is matchup-specific.
    pub opponent: Option<Icon>,
    /// Games and win rate behind this build (matchup or overall).
    pub games: u32,
    pub win_rate: f32,
    pub runes: Vec<RunePageView>,
    pub core: Vec<IconOption>,
    pub boots: Vec<IconOption>,
    pub starters: Vec<IconOption>,
    pub spells: Vec<IconOption>,
    /// Options for the 4th, 5th (and 6th) items.
    pub later: Vec<Vec<IconOption>>,
    pub skills: Vec<SkillOption>,
    pub situational: Vec<SituationalView>,
    pub threats: Vec<ThreatView>,
    pub opgg_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct DraftView {
    pub status: DraftStatus,
    pub message: Option<String>,
    pub my_role: Option<Role>,
    /// False in blind pick, where the user chooses the role.
    pub role_from_client: bool,
    pub allies: Vec<AllyView>,
    pub enemies: Vec<EnemyView>,
    pub ally_bans: Vec<Icon>,
    pub enemy_bans: Vec<Icon>,
    pub build: Option<BuildView>,
}

// ---------- Runtime state ----------

#[derive(Default)]
pub struct DraftRuntime {
    pub state: Option<DraftState>,
    /// User role overrides for enemies, by cell id.
    pub overrides: HashMap<i64, Role>,
    /// User's role choice in blind pick.
    pub my_role: Option<Role>,
    /// Debug fixture mode: treat the draft as Summoner's Rift.
    pub force_rift: bool,
    generation: u64,
    last_key: String,
    pub view: Option<DraftView>,
}

/// OP.GG provider plus the 24 h cache.
pub struct BuildService {
    provider: OpggProvider,
    cache: BuildCache,
}

impl BuildService {
    pub fn new(http: reqwest::Client, cache_root: std::path::PathBuf) -> Self {
        Self { provider: OpggProvider::new(http), cache: BuildCache::new(cache_root) }
    }

    async fn base(&self, patch: &str, champ: &str, role: Role) -> Result<BuildSet, ProviderError> {
        let key = cache::key(&["base", champ, role.opgg()]);
        self.cache.get_or_fetch(self.provider.id(), patch, &key, || self.provider.base(champ, role)).await
    }

    async fn matchup(&self, patch: &str, champ: &str, role: Role, opp: &str) -> Result<BuildSet, ProviderError> {
        let key = cache::key(&["matchup", champ, role.opgg(), "vs", opp]);
        self.cache.get_or_fetch(self.provider.id(), patch, &key, || self.provider.matchup(champ, role, opp)).await
    }

    async fn traits(&self, patch: &str, champ: &str) -> Result<ChampionTraits, ProviderError> {
        let key = cache::key(&["traits", champ]);
        self.cache.get_or_fetch(self.provider.id(), patch, &key, || self.provider.traits(champ)).await
    }
}

// ---------- Entry points ----------

/// New champ select: forget the last draft.
pub fn reset(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut rt = state.draft.lock().unwrap();
    let force_rift = rt.force_rift;
    *rt = DraftRuntime { force_rift, ..Default::default() };
}

/// A champ select session update from the client.
pub fn on_session(app: &AppHandle, session: &Value) {
    let Some(parsed) = champ_select::parse(session) else { return };
    app.state::<AppState>().draft.lock().unwrap().state = Some(parsed);
    schedule(app);
}

/// Recompute the draft view in the background.
pub fn schedule(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { recompute(&app).await });
}

pub fn set_enemy_role(app: &AppHandle, cell_id: i64, role: Option<Role>) {
    {
        let state = app.state::<AppState>();
        let mut rt = state.draft.lock().unwrap();
        match role {
            Some(r) => rt.overrides.insert(cell_id, r),
            None => rt.overrides.remove(&cell_id),
        };
    }
    schedule(app);
}

pub fn set_my_role(app: &AppHandle, role: Role) {
    app.state::<AppState>().draft.lock().unwrap().my_role = Some(role);
    schedule(app);
}

// ---------- Pipeline ----------

struct Inputs {
    draft: DraftState,
    overrides: HashMap<i64, Role>,
    my_role_choice: Option<Role>,
    rift: Option<bool>,
    patch: String,
    generation: u64,
}

fn opgg_key(dd: &DData, champion_id: i32) -> Option<String> {
    dd.champions.get(&champion_id).map(|c| c.id.to_uppercase())
}

fn opt<T>(r: &Ranked<T>, icons: Vec<Icon>) -> IconOption {
    IconOption { icons, play: r.play, win: r.win, smoothed_wr: r.smoothed_wr }
}

fn situational_view(p: &Pick, catalog: &Catalog) -> SituationalView {
    SituationalView {
        item: catalog.item_icon(p.item_id as i32),
        reason: p.reason.clone(),
        play: p.ranked.play,
        win: p.ranked.win,
        smoothed_wr: p.ranked.smoothed_wr,
    }
}

fn rune_view(r: &Ranked<RunePage>, catalog: &Catalog) -> RunePageView {
    let p = &r.value;
    RunePageView {
        primary_style: catalog.perk(p.primary_style as i32),
        sub_style: catalog.perk(p.sub_style as i32),
        perks: p.perks.iter().map(|id| catalog.perk(*id as i32)).collect(),
        shards: p.shards.iter().map(|id| catalog.perk(*id as i32)).collect(),
        play: r.play,
        win: r.win,
        smoothed_wr: r.smoothed_wr,
    }
}

fn tags_label(dd: &DData, champion_id: i32, traits: Option<&ChampionTraits>) -> String {
    let mut parts: Vec<String> = dd.champions.get(&champion_id).map(|c| c.tags.iter().take(1).cloned().collect()).unwrap_or_default();
    if let Some(d) = traits.and_then(|t| t.damage_type) {
        parts.push(format!("{d:?}"));
    }
    parts.join(" · ")
}

/// Teams and bans: everything that doesn't need the provider.
fn teams_view(inputs: &Inputs, catalog: &Catalog) -> DraftView {
    let me = inputs.draft.me();
    let role_from_client = me.and_then(|m| m.role).is_some();
    DraftView {
        status: DraftStatus::Loading,
        message: None,
        my_role: me.and_then(|m| m.role).or(inputs.my_role_choice),
        role_from_client,
        allies: inputs
            .draft
            .allies
            .iter()
            .map(|s| AllyView {
                cell_id: s.cell_id as i32,
                champion: (s.champion_id > 0).then(|| catalog.champion(s.champion_id)),
                role: s.role,
                locked: s.locked,
                is_me: s.is_me,
            })
            .collect(),
        enemies: inputs
            .draft
            .enemies
            .iter()
            .map(|s| EnemyView {
                cell_id: s.cell_id as i32,
                champion: (s.champion_id > 0).then(|| catalog.champion(s.champion_id)),
                tags: String::new(),
                inferred: None,
                lane_opponent: false,
            })
            .collect(),
        ally_bans: inputs.draft.ally_bans.iter().map(|id| catalog.champion(*id)).collect(),
        enemy_bans: inputs.draft.enemy_bans.iter().map(|id| catalog.champion(*id)).collect(),
        build: None,
    }
}

fn with_status(mut v: DraftView, status: DraftStatus, message: Option<&str>) -> DraftView {
    v.status = status;
    v.message = message.map(str::to_owned);
    v
}

async fn compute(app: &AppHandle, inputs: &Inputs, catalog: &Catalog, dd: &DData, mut view: DraftView) -> DraftView {
    let service = app.state::<AppState>().builds.clone();
    let patch = inputs.patch.as_str();

    match inputs.rift {
        Some(true) => {}
        Some(false) => return with_status(view, DraftStatus::Off, Some("Build data is only shown for Summoner's Rift games.")),
        None => return with_status(view, DraftStatus::Loading, Some("Checking the game mode…")),
    }

    // Enemy traits (prefetched on every lock) and role inference.
    let locked: Vec<(usize, i32, String)> = inputs
        .draft
        .enemies
        .iter()
        .enumerate()
        .filter(|(_, s)| s.champion_id > 0)
        .filter_map(|(i, s)| Some((i, s.champion_id, opgg_key(dd, s.champion_id)?)))
        .collect();
    let traits: Vec<ChampionTraits> = join_all(locked.iter().map(|(_, _, key)| service.traits(patch, key)))
        .await
        .into_iter()
        .map(|r| r.unwrap_or_else(|e| {
            tracing::warn!("enemy traits unavailable: {e}");
            ChampionTraits::default()
        }))
        .collect();
    let fixed: Vec<Option<Role>> = locked.iter().map(|(i, _, _)| inputs.overrides.get(&inputs.draft.enemies[*i].cell_id).copied()).collect();
    let rates: Vec<Vec<(Role, f32)>> = traits.iter().map(|t| t.role_rates.clone()).collect();
    let inferred = role_inference::infer(&rates, &fixed);
    for (n, (i, champ, _)) in locked.iter().enumerate() {
        view.enemies[*i].tags = tags_label(dd, *champ, traits.get(n));
        view.enemies[*i].inferred = inferred.get(n).copied();
    }

    let Some(me) = inputs.draft.me() else { return with_status(view, DraftStatus::NeedChampion, None) };
    if me.champion_id <= 0 {
        return with_status(view, DraftStatus::NeedChampion, Some("Hover or lock a champion to see builds."));
    }
    let Some(role) = view.my_role else {
        return with_status(view, DraftStatus::NeedRole, Some("Blind pick: choose your role."));
    };
    let Some(my_key) = opgg_key(dd, me.champion_id) else {
        return with_status(view, DraftStatus::Error, Some("Unknown champion."));
    };

    // Lane opponent: the enemy inferred (or set) to your role.
    let opponent = locked.iter().enumerate().find(|(n, _)| inferred.get(*n).is_some_and(|r| r.role == role)).map(|(_, l)| l);
    if let Some((i, _, _)) = opponent {
        view.enemies[*i].lane_opponent = true;
    }

    let base = match service.base(patch, &my_key, role).await {
        Ok(b) => b,
        Err(e) => return with_status(view, DraftStatus::Error, Some(&format!("Couldn't load builds: {e}"))),
    };
    let (build, is_matchup) = match opponent {
        Some((_, _, opp_key)) => match service.matchup(patch, &my_key, role, opp_key).await {
            Ok(mut m) if !m.core_sets.is_empty() => {
                m.damage_type = base.damage_type;
                (m, true)
            }
            Ok(_) => (base, false),
            Err(e) => {
                tracing::warn!("matchup {my_key} vs {opp_key} ({}) unavailable, using overall build: {e}", role.opgg());
                (base, false)
            }
        },
        None => (base, false),
    };

    // Comp profile, situational picks and threat cards.
    let comp: Vec<EnemyChampion> = locked
        .iter()
        .zip(&traits)
        .filter_map(|((_, champ, _), t)| {
            let c = dd.champions.get(champ)?;
            Some(EnemyChampion { name: c.name.clone(), id: c.id.clone(), tags: c.tags.clone(), damage: t.damage_type })
        })
        .collect();
    let mut exclude: Vec<u32> = build.core_sets.first().map(|c| c.value.clone()).unwrap_or_default();
    exclude.extend(build.boots.first().map(|b| b.value));
    let situational = comp_profile::situational(&build, &comp, dd, &exclude);
    let threats: Vec<ThreatView> = locked
        .iter()
        .zip(&comp)
        .map(|((_, champ, _), e)| ThreatView {
            champion: catalog.champion(*champ),
            answers: comp_profile::threat_answers(&build, e, dd).iter().map(|p| situational_view(p, catalog)).collect(),
        })
        .filter(|t| !t.answers.is_empty())
        .collect();

    let my_dd_id = dd.champions.get(&me.champion_id).map(|c| c.id.to_lowercase()).unwrap_or_default();
    let mut opgg_url = format!("https://op.gg/lol/champions/{my_dd_id}/build/{}", role.opgg());
    if is_matchup && let Some((_, champ, _)) = opponent && let Some(c) = dd.champions.get(champ) {
        opgg_url.push_str(&format!("?target_champion={}", c.id.to_lowercase()));
    }
    let items = |ids: &[u32]| ids.iter().map(|id| catalog.item_icon(*id as i32)).collect::<Vec<_>>();

    view.build = Some(BuildView {
        champion: catalog.champion(me.champion_id),
        role,
        opponent: if is_matchup { opponent.map(|(_, c, _)| catalog.champion(*c)) } else { None },
        games: build.champion_games,
        win_rate: build.champion_wr,
        runes: build.rune_pages.iter().take(TOP_OPTIONS).map(|r| rune_view(r, catalog)).collect(),
        core: build.core_sets.iter().take(TOP_OPTIONS).map(|r| opt(r, items(&r.value))).collect(),
        boots: build.boots.iter().take(TOP_OPTIONS).map(|r| opt(r, items(&[r.value]))).collect(),
        starters: build.starters.iter().take(2).map(|r| opt(r, items(&r.value))).collect(),
        spells: build.summoners.iter().take(2).map(|r| opt(r, r.value.iter().map(|id| catalog.spell(*id as i32)).collect())).collect(),
        later: build.later_items.iter().filter(|s| !s.is_empty()).map(|s| s.iter().take(TOP_OPTIONS).map(|r| opt(r, items(&[r.value]))).collect()).collect(),
        skills: build.skill_orders.iter().take(2).map(|r| SkillOption { order: r.value.clone(), play: r.play, win: r.win, smoothed_wr: r.smoothed_wr }).collect(),
        situational: situational.iter().map(|p| situational_view(p, catalog)).collect(),
        threats,
        opgg_url,
    });
    with_status(view, DraftStatus::Ready, None)
}

fn emit(app: &AppHandle, generation: u64, view: DraftView) {
    let state = app.state::<AppState>();
    let mut rt = state.draft.lock().unwrap();
    if rt.generation != generation {
        return; // a newer draft state is being computed
    }
    rt.view = Some(view.clone());
    drop(rt);
    if let Err(e) = app.emit(DRAFT_EVENT, &view) {
        tracing::warn!("failed to emit {DRAFT_EVENT}: {e}");
    }
}

async fn recompute(app: &AppHandle) {
    let state = app.state::<AppState>();
    let inputs = {
        let rift = state.client.lock().unwrap().summoners_rift;
        let patch = state.patch.lock().unwrap().clone().unwrap_or_default();
        let mut rt = state.draft.lock().unwrap();
        let Some(draft) = rt.state.clone() else { return };
        let rift = if rt.force_rift { Some(true) } else { rift };
        let key = format!("{draft:?}|{:?}|{:?}|{rift:?}|{patch}", {
            let mut o: Vec<_> = rt.overrides.iter().collect();
            o.sort();
            o
        }, rt.my_role);
        if key == rt.last_key {
            return; // nothing changed
        }
        rt.last_key = key;
        rt.generation += 1;
        Inputs { draft, overrides: rt.overrides.clone(), my_role_choice: rt.my_role, rift, patch, generation: rt.generation }
    };

    let Some(http) = state.lcu_http() else { return };
    let catalog = match profile::catalog(app, &http).await {
        Ok(c) => c,
        Err(e) => return tracing::warn!("draft: client catalog unavailable: {e}"),
    };
    let teams = teams_view(&inputs, &catalog);
    let Some(dd) = state.ddragon() else {
        return emit(app, inputs.generation, with_status(teams, DraftStatus::Loading, Some("Loading champion data…")));
    };
    emit(app, inputs.generation, teams.clone());
    let view = compute(app, &inputs, &catalog, &dd, teams).await;
    emit(app, inputs.generation, view);
}
