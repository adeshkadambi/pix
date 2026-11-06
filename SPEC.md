# Pix — Specification v2 (supersedes the original pasted spec)

> This is the only valid spec for the project. Ignore the earlier draft completely; the end of this document lists the assumptions from it that turned out wrong. In M0 this file is committed as `SPEC.md`.

## 1. Context and goals
Pix is a **personal, Windows-only** League of Legends companion and overlay built with **Tauri v2 + Rust**. It gives one player (the author) what u.gg doesn't:
- **Default (idle) view:** the logged-in account's own **profile, ranked stats and match history**, shown natively from League client data.
- **Champion select:** builds ranked by win rate **against the enemy team comp**.
- **In-game overlay:** **gold diff** and **summoner spell timers** (clickable while TAB is held).
- **In-game companion window (second monitor):** the pre-game build, a threat card, an enemy purchase feed, and an optional **live item advisor**.
- **Post-game:** a review of who killed you and what would have countered them.

Non-goals: public distribution, macOS/Linux builds, automating anything.

Environment: development, running and testing happen on the author's Windows PC (Rust 1.99, Node 26) in `C:\Users\adesh\Documents\pix`. The code is pushed to the GitHub repo `adeshkadambi/pix`. Windows CI builds an `.exe`.

Performance target: idle RAM under 80 MB, CPU under 1% in-game. One Rust process (tokio) plus WebView2 windows. No Electron, no Overwolf, no headless browsers.

## 2. Compliance rules (enforced in code review and CI)
Sources: developer.riotgames.com/docs/lol (Game Integrity, Game Policy), developer.riotgames.com/policies/general, support.riotgames.com "Third Party Applications".

**Technical rules (always on, no exceptions):**
1. **No game-process access.** No memory reads, no injection, no input simulation, no `OpenProcess` on League processes, and no reading of process command lines (on Windows that reads the process's memory).
2. **League client API is read-only.** GET requests and WebSocket subscriptions only. No auto-accept, auto-pick, rune/item-set import, or any other write.
3. **Keys are read passively.** TAB is read with `GetAsyncKeyState` polling, only while the League game window is in front. No `RegisterHotKey`, no keyboard or mouse hooks.
4. **Overlay windows are separate windows.** Topmost, transparent, non-activating (`WS_EX_NOACTIVATE`). No DirectX or graphics hooks. Clicks on the overlay never take focus from the game.
5. **No de-anonymization.** Don't fetch, store or show names, PUUIDs or ranks of players in champ select. In-game Riot IDs are used only to find "me" in `playerList`, and are never stored.
6. **Summoner's Rift only.** Build and win-rate data is turned off when the map isn't SR (map 11, mode CLASSIC). Arena and augment win rates are explicitly banned.
7. **Show several options.** Always show 2–3 ranked options with game counts, never a single "do this".
8. **Legal notice.** Riot's legal notice is shown on the About screen.

**Feature classes:**

| Feature | Class | Default |
|---|---|---|
| Own profile / match history view ("Showing (self) player stats" is explicitly approved) | Compliant | On |
| Comp-aware champ-select builds, threat card, pre-game build in-game, enemy purchase feed (companion), post-game review | Compliant | On |
| Summoner timers (overlay), gold diff (overlay) | Gray (not named in policy) | **Off** until Riot approves |
| Live item advisor (companion) | **Off-policy** ("drawing conclusions for you during gameplay"). The author accepted the risk | **Off**, labeled "Not Riot-compliant, use at own risk" |

Registration: the author registers the product on the Riot Developer Portal, discloses that it uses the League client API and the Live Client API, and describes the gray features. Gray flags flip only on approval.

CI guard: the build fails if the source contains `OpenProcess|ReadProcessMemory|WriteProcessMemory|SetWindowsHookEx|SendInput|keybd_event|mouse_event|RegisterHotKey|CreateRemoteThread`, or if `lcu/http.rs` issues anything other than GET.

## 3. Tech stack
- **Backend:** Rust, tokio, `reqwest` (rustls), `tokio-tungstenite` (rustls), `serde`/`serde_json`, `regex`, `rmcp` (MCP client; fall back to a small hand-rolled JSON-RPC client if it causes friction), `async-trait`, `thiserror`, `tracing`, `windows` crate (Win32: window styles, `GetAsyncKeyState`, `GetForegroundWindow`, `GetClassNameW`), `ts-rs` (generates TS types).
- **Frontend:** Vite + TypeScript + SolidJS (small, reactive), plain CSS.
- **Tauri v2 plugins:** only what's needed (`window-state` optional). Custom URI protocol `lcu-asset`.
- **TLS:** Riot's root `riotgames.pem` (from static.developer.riotgames.com/docs/lol/riotgames.pem) is bundled and is the **only** trusted root for 127.0.0.1 clients. Hostname checks are skipped only for those clients. No global `danger_accept_invalid_certs`.

## 4. Windows and layout
| Window | Where | Behavior | Contents |
|---|---|---|---|
| `companion` | Normal window, usually on the second monitor | Decorated, resizable, one window used across all phases | Idle (no game) → **own profile and match history** (default). Draft → builds. In-game → pre-game build, threat card, purchase feed, live advisor (flag). Post-game → review. Settings, About |
| `hud` | Over the game, full monitor | Transparent, undecorated, topmost, skip taskbar, **always click-through** (`set_ignore_cursor_events(true)`), non-activating | Gold diff strip; running summoner timer countdowns (display only) |
| `tabpanel` | Over the game, small, next to the scoreboard's enemy rows | Transparent, undecorated, topmost, non-activating, hidden by default | Enemy summoner spell buttons; shown and clickable **only while TAB is held** and League is in front |

- Non-activating is enforced in `overlay/win32.rs` with `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` (plus Tauri's `focus: false`).
- Calibration mode lets the user drag `tabpanel` into place once; the position is saved per resolution. The scoreboard key can be changed (default TAB) for people who rebind it.
- The game must be in **Borderless or Windowed** mode. The app detects exclusive fullscreen (game window rect equals monitor rect and the overlay isn't visible) and shows a warning in `companion`.

## 5. Backend architecture
```
src-tauri/src/
  lib.rs            builder, state, protocol registration, window setup, task supervisor
  config.rs         settings + flags { overlay_timers:false, overlay_gold:false, live_advisor:false }, saved to app_config_dir
  riot_tls.rs       reqwest + tungstenite connectors pinned to riotgames.pem
  events.rs         typed events to the frontend (ts-rs exported)
  lcu/discovery.rs  install path → lockfile
  lcu/lockfile.rs   parse name:pid:port:password:protocol
  lcu/http.rs       GET-only client (basic auth riot:<password>)
  lcu/ws.rs         WAMP subscribe, reconnect with backoff
  lcu/gameflow.rs   phase state machine
  lcu/champ_select.rs  session → DraftState
  live/poller.rs    1 s poll /liveclientdata/allgamedata (+ eventdata), gameTime clock
  live/models.rs
  ddragon/          versions, champion.json, item.json, summoner.json; disk cache per version
  builds/mod.rs     BuildProvider trait, DraftContext, BuildSet
  builds/ranking.rs smoothed win rate
  builds/role_inference.rs
  builds/comp_profile.rs
  builds/cache.rs   memory + disk, key (provider, champ, role, opponent?, patch)
  builds/opgg/{client.rs, notation.rs, mapping.rs}
  game/gold.rs, haste.rs, spells.rs, purchases.rs, threats.rs, advisor.rs, postgame.rs
  overlay/win32.rs, tab_watch.rs, foreground.rs
  asset_protocol.rs
  profile.rs        own summoner, ranked stats, match history → ProfileView
```

### 5.1 Finding the League client and connecting
- **Install path:** read `C:\ProgramData\Riot Games\Metadata\league_of_legends.live\league_of_legends.live.product_settings.yaml`, key `product_install_full_path`. If that fails, try `C:\Riot Games\League of Legends`, then a path set in config. Never inspect processes.
- **Lockfile:** poll `<install>\lockfile` every 2 s. Its contents are `LeagueClient:<pid>:<port>:<password>:https`. When the file changes or disappears, tear down and reconnect.
- **WebSocket:** `wss://127.0.0.1:<port>/` with header `Authorization: Basic base64("riot:<password>")`.
  - Subscribe by sending `[5,"OnJsonApiEvent_lol-gameflow_v1_gameflow-phase"]` and `[5,"OnJsonApiEvent_lol-champ-select_v1_session"]`.
  - Incoming frames look like `[8,"<event>",{"data":…,"eventType":"Create|Update|Delete","uri":"…"}]`.
  - On connect, GET `/lol-gameflow/v1/gameflow-phase` and `/lol-champ-select/v1/session` once to seed the state.
- **Phases:**
  - `ChampSelect`: start the draft pipeline.
  - `InProgress`: start `live/poller`; the Live Client API returns errors during loading, so retry every second until it answers.
  - `EndOfGame`: build the post-game review from GET `/lol-end-of-game/v1/eog-stats-block` plus the kill events recorded during the game.
  - `None` / `Lobby`: idle.
- **Map and queue gate:** GET `/lol-gameflow/v1/session` → `map.id == 11` and `gameData.queue.gameMode == "CLASSIC"`, otherwise build data is turned off (compliance rule 6).

### 5.2 Champion select → DraftState
- **Me:** `localPlayerCellId` identifies my cell; `myTeam[]` gives `{cellId, championId, championPickIntent, assignedPosition}`.
- **Enemies:** `theirTeam[]` gives `{cellId, championId}`. Their position is always empty.
- **Hover vs lock:** a champion is locked when its `actions[][]` entry with `type=="pick"` and `actorCellId` set is `completed:true`. Before the lock, `championPickIntent` (or the in-progress action's `championId`) counts as a hover.
- **Role mapping:** `top→top, jungle→jungle, middle→mid, bottom→adc, utility→support`. If `assignedPosition` is empty (blind pick), the user chooses the role in `companion`.
- **Enemy roles:** for each enemy champion, get role rates from OP.GG analysis (`data.summary.positions[].name` and `.stats.role_rate`, cached per patch). Score all 120 role assignments of the 5 enemies by the product of their role rates and pick the best. Show a confidence value; the user can override by clicking.
- **Prefetch:** start fetching on hover and on every enemy lock.

### 5.3 Build provider abstraction
```rust
// builds/mod.rs
#[async_trait::async_trait]
pub trait BuildProvider: Send + Sync {
    fn id(&self) -> &'static str;                      // "opgg", later "ugg", "riot"
    async fn base(&self, ctx: &DraftContext) -> Result<BuildSet, ProviderError>;
    async fn matchup(&self, ctx: &DraftContext, opponent: ChampionKey) -> Result<BuildSet, ProviderError>;
    async fn role_rates(&self, champ: ChampionKey, patch: &str) -> Result<Vec<(Role, f32)>, ProviderError>;
}

pub struct DraftContext {
    pub patch: String,                 // from LCU /lol-patch/v1/game-version, trimmed to "16.22"
    pub me: Pick,                      // champion + role
    pub allies: Vec<Pick>,
    pub enemies: Vec<EnemyPick>,       // champion + inferred role + confidence
}

pub struct BuildSet {
    pub champion_wr: f32,              // overall win rate, the prior for smoothing
    pub rune_pages: Vec<Ranked<RunePage>>,
    pub core_sets: Vec<Ranked<Vec<u32>>>,
    pub boots: Vec<Ranked<u32>>,
    pub starters: Vec<Ranked<Vec<u32>>>,
    pub summoners: Vec<Ranked<[u32; 2]>>,
    pub item_pool: Vec<Ranked<u32>>,   // every item seen in single_items/last_items, for situational picks + advisor
}
pub struct Ranked<T> { pub value: T, pub play: u32, pub win: u32, pub smoothed_wr: f32 }
pub struct RunePage { pub primary_style: u32, pub sub_style: u32, pub perks: Vec<u32>, pub shards: [u32; 3] }
```
The draft pipeline takes the matchup `BuildSet` if one exists (lane opponent locked), otherwise the base set. The Draft view shows the top build plus 2 alternatives, each with smoothed win rate and game count (rule 7). Its only actions are read-only: copy the build to the clipboard, or open it on op.gg in the system browser. Nothing is sent to the client (rule 2). Situational picks come from the comp profile (5.5). A later Riot API provider implements the same trait and can use all of `DraftContext.enemies`.

### 5.4 OP.GG provider (current data source)
- **Endpoint:** `https://mcp-api.op.gg/mcp` (MCP over Streamable HTTP, JSON-RPC 2.0). The session is `initialize` → `notifications/initialized` → `tools/call`. Send `Accept: application/json, text/event-stream`. The reply may come as SSE.
- **Champion key:** the Data Dragon `champion.json` entry's `id`, uppercased (Wukong → `MONKEYKING`, Kai'Sa → `KAISA`). It's mapped from the numeric `key`.
- **Base build:** `tools/call` `lol_get_champion_analysis` with arguments
  ```json
  {"game_mode":"ranked","champion":"AHRI","position":"mid",
   "desired_output_fields":[
     "data.summary.average_stats.{win_rate,play}",
     "data.summary.positions[].name","data.summary.positions[].stats.{role_rate}",
     "data.runes.{id,primary_page_id,primary_rune_ids[],secondary_page_id,secondary_rune_ids[],stat_mod_ids[],play,win}",
     "data.core_items.{ids[],play,win}","data.boots.{ids[],play,win}",
     "data.starter_items.{ids[],play,win}","data.summoner_spells.{ids[],play,win}",
     "data.last_items[].{ids[],play,win}","data.damage_type"]}
  ```
  The reply `result.content[0].text` is **class notation, not JSON**:
  ```
  class LolGetChampionAnalysis: data
  class Data: core_items,boots,summoner_spells,runes
  class CoreItems: ids,ids_names
  class Runes: id,primary_page_id,primary_rune_ids,secondary_page_id,secondary_rune_ids,stat_mod_ids

  LolGetChampionAnalysis(Data(CoreItems([3118,4645,3157],["Malignance",...]),CoreItems([3020],[...]),...))
  ```
  `opgg/notation.rs` reads the `class Name: f1,f2,…` header lines into a field-order table, then runs a recursive-descent parser over `Name(args)`, `[list]`, numbers, `"strings"` and `null|true|false`, mapping each argument to its field by position. The output is a `serde_json::Value`, which serde then converts to typed structs. It's tested against the real samples captured while planning (Ahri mid, MONKEYKING jungle, KAISA adc).
- **Matchup:** `tools/call` `lol_get_lane_matchup_guide` with `{"my_champion":"AHRI","opponent_champion":"ZED","position":"mid"}`. The reply text is **JSON**. Used fields: `data.summary.average_stats.win_rate`, `core_items[]`, `boots[]`, `starter_items[]`, `summoner_spells[]`, `rune_pages[].builds[]`, `single_items[].{depth,items[]}`, `last_items[]`, `counters[]`, all with `play` and `win`. This only fits the **lane** opponent (same position).
- **Cache:** results are stored in memory and on disk (`app_cache_dir/builds/opgg/<patch>/<key>.json`) for 24 h. Expected cold latency is roughly 300–1000 ms, which prefetch hides; warm latency is under 5 ms.

### 5.5 Ranking and comp profile
- **Smoothed win rate:** `smoothed = (win + K·p0) / (play + K)`, with `K = 50` and `p0` the champion's win rate for that matchup or overall. Rows with `play < 10` are hidden. The game count is always shown.
- **Comp profile** (from enemy champions only, pre-game):
  - Damage split: OP.GG `data.damage_type` (AD/AP/BOTH) for each enemy.
  - Class tags: Data Dragon `tags` (Assassin, Tank, Marksman, …).
  - Healing: a small curated list of champion keys, kept in `comp_profile.rs`.
- **Situational picks:** map comp traits to Data Dragon item tags (`SpellBlock`, `Armor`, `GrievousWounds`/anti-heal set, `Tenacity`, `MagicPenetration`, `ArmorPenetration`). Keep only items in `BuildSet.item_pool` (items the champion actually builds) and rank by `smoothed_wr`. Each pick carries a reason such as "3/5 enemies AP" or "Soraka + Aatrox healing".
- **Threat card:** for each enemy, the top 1–2 situational answers against that champion's traits, made pre-game, shown in-game.

### 5.6 In-game data (Live Client API)
- **Base:** `https://127.0.0.1:2999/liveclientdata/`. Poll `allgamedata` every 1 s and `eventdata?eventID=<last>` every 1 s.
- **Me:** `activePlayer.riotId` matched against `playerList[].riotId`. Enemies are the players whose `team` differs from mine.
- **What's available per player:** `items[] {itemID, count, slot}`, `level`, `scores {kills,deaths,assists,creepScore}`, `summonerSpells.summonerSpellOne/Two.rawDisplayName`, `runes {keystone, primaryRuneTree, secondaryRuneTree}`, `position`. **The full rune page isn't available for other players, and items have no descriptions.**
- **Clock:** `gameData.gameTime` (seconds). All timers use game time, so pauses and lag don't break them.

### 5.7 Gold diff (overlay `hud`, gray flag `overlay_gold`)
- **Item gold** = Σ Data Dragon `item[id].gold.total × count` per player.
- Shown per lane matchup (positions from Live Client `position`, else the champ-select inferred roles) as `+1,250`, plus a team total. Labeled "item gold" because it's not total gold.

### 5.8 Summoner timers (overlay `tabpanel` + `hud`, gray flag `overlay_timers`)
- **Spell key:** `rawDisplayName` `GeneratedTip_SummonerSpell_<Key>_DisplayName` → `<Key>` (e.g. `SummonerFlash`) → Data Dragon `summoner.json` → `cooldown[0]` and icon.
- **Haste:**
  - Items: strip HTML tags from the Data Dragon item `description`, then match `/(\d+)\s*Summoner Spell Haste/i`, summing over the enemy's current items. Never hardcode values.
  - Cosmic Insight: it can't be detected for enemies, so each enemy gets a **manual toggle** (right-click portrait) that adds `+18`.
- **Cooldown:** `cd = base / (1 + haste / 100)`. It's computed **at the moment of the click**, using the enemy's items at that time.
- **Controls (in `tabpanel` while TAB is held):** left-click starts, right-click clears, mouse wheel adjusts ±5 s.
- **Display:** `hud` shows running countdowns (display only, click-through).
- **Special cases in `spells.rs`:** Teleport (cooldown changes with level and the Unleashed upgrade), Smite (charges; not tracked by default).

### 5.9 Companion in-game view and live advisor
- **Compliant parts (always on):** pre-game build (chosen option), threat card, enemy purchase feed (`purchases.rs`: compares item multisets between tick T-1 and tick T, last 10 changes).
- **Live advisor (`live_advisor` flag, off by default, labeled not compliant):**
  1. Read `ChampionKill` events (`KillerName`, `VictimName`, `Assisters`).
  2. **Threat score** per enemy: `0.5·norm(item_gold_lead) + 0.3·norm(kills+assists) + 0.2·norm(kills_on_me)`.
  3. Classify each of the top 1–2 threats using the comp profile plus the Data Dragon tags of their **current** items (AP / AD / crit / on-hit / healing).
  4. Candidates are the matching defensive tags ∩ `BuildSet.item_pool`, minus items I already own, ranked by `smoothed_wr`.
  5. Show 2–3 options with a reason (e.g. "Zed 6/0, killed you 3× → Zhonya's (52%, 290 games)"). It only advises, never acts.
  - All of it is pure functions in `advisor.rs`/`threats.rs`, tested with recorded `eventdata` fixtures.
- **Post-game review (compliant):** deaths grouped by killer, a timeline, and for the top killers the best counter items from OP.GG matchup data (`my champ vs killer`).

### 5.10 Profile view (default companion screen)
A native screen (`design/Main.dc.html`), built only from your own account's League client data.
- **Who:** GET `/lol-summoner/v1/current-summoner` → `gameName`, `tagLine`, `summonerLevel`, `profileIconId`. GET `/riotclient/region-locale` → `region`.
- **Ranked:** GET `/lol-ranked/v1/current-ranked-stats` → Solo/Duo and Flex tier, division, LP, wins, losses.
- **Match history:** GET `/lol-match-history/v1/products/lol/current-summoner/matches` → per game: champion, queue, result, K/D/A, CS, duration, items, summoner spells. Ranked champion stats (games, win rate, KDA) are computed from the same history.
- **Labels:** the source is shown as "from League client".
- **Phase behavior:**
  - The companion shows Profile when the phase is `None`, `Lobby`, `Matchmaking` or `ReadyCheck`, and switches tabs as the phase changes (Draft, Live, Review). A tab the user picks by hand stays until the phase changes again.
  - Before the client connects (no account yet), the profile shows a "Waiting for League client" placeholder.
- **Later option:** embed the OP.GG profile page (child webview with no IPC capabilities, plus a navigation guard that keeps it on `op.gg`), or use OP.GG `lol_list_summoner_matches` for extra stats.

### 5.11 TAB watch and foreground
- `tab_watch.rs` runs at about 60 Hz on a dedicated thread.
- `fg = GetForegroundWindow()`, and the window class is `RiotWindowClass` (from `GetClassNameW`; no process handle is opened).
- `held = GetAsyncKeyState(VK_TAB) & 0x8000 != 0`.
- On a rising edge of (`fg && held`), show `tabpanel` and enable cursor events; on the falling edge, hide it.
- Debounced. Nothing is written to the input stream.

## 6. Asset pipeline (`lcu-asset` protocol)
- Registered with `register_asynchronous_uri_scheme_protocol("lcu-asset", …)`. On Windows the frontend URL is `http://lcu-asset.localhost/<path>`; build it with `convertFileSrc(path, 'lcu-asset')`.
- The handler accepts **GET only**, with an **allowlist of paths under `/lol-game-data/assets/`** so the webview can't call other client endpoints with the user's credentials. It adds basic auth from the current lockfile, fetches through the pinned TLS client, and returns the bytes with the upstream content type.
- Memory LRU (about 32 MB) plus a disk cache keyed by asset path (`app_cache_dir/assets/…`). When the client isn't running, it serves from the disk cache, or falls back to the Data Dragon CDN image (`ddragon.leagueoflegends.com/cdn/<ver>/img/...`), cached the same way.
- Icon paths come from LCU `/lol-game-data/assets/v1/{champion-summary,items,summoner-spells,perks}.json` → `iconPath`.

## 7. Frontend
```
src/windows/companion/  Companion.tsx (header, tabs Profile · Draft · Live · Review), Profile.tsx, Draft.tsx, InGame.tsx, PostGame.tsx, Settings.tsx (flags + risk labels), About.tsx (legal notice)
src/windows/hud/        GoldStrip.tsx, TimerBadges.tsx
src/windows/tabpanel/   EnemySpells.tsx, Calibrate.tsx
src/lib/                ipc.ts, events.ts, assets.ts, types/ (generated by ts-rs on `cargo test`, committed)
src/styles/             tokens.css (design tokens), base.css
```
Every window subscribes to backend events. There's no polling in JavaScript. All three windows load one page and pick their view by window label. Fonts are bundled locally (no Google Fonts at runtime).

**Design:** the screens and tokens come from the Claude Design export in `design/` (see `design/README.md`). Where a design conflicts with section 2, section 2 wins:
- The Live screen leads with the threat card and purchase feed. The "Next buy" card shows only with the `live_advisor` flag on.
- Overlay gold diff is per lane matchup (5.7).
- Timer controls follow 5.8.

## 8. Milestones (each ends working and pushed)
- **M0 Scaffold:** `git init`, Tauri v2 + Vite + SolidJS, three windows, config and flags, About page, `SPEC.md` (this file), Windows CI (`cargo test`, compliance grep, `tauri build` artifact). Create a private GitHub repo with `gh` (confirm with the author before pushing).
- **M1 League client connection and assets:** discovery, lockfile, pinned TLS, GET client, WebSocket, gameflow, map gate, `lcu-asset`, Data Dragon cache. Companion shows connection status and phase. **Profile view** (native: summoner, ranked stats, match history).
- **M2 Comp-aware builds:** champ-select parsing, OP.GG MCP client and notation parser, role inference, ranking, comp profile, cache, Draft view.
- **M3 In-game shell:** live poller, companion in-game view (pre-game build, threat card, purchase feed), `hud`/`tabpanel` window styles, TAB watch, calibration, fullscreen warning.
- **M4 Overlay features (gray, off by default):** gold diff, summoner timers with haste and Cosmic toggle.
- **M5 Live advisor (off-policy, off by default) and post-game review.**
- **M6 Compliance close-out:** Riot Developer Portal registration and disclosures. Turn on only approved gray features.

## 9. Verification
- **`cargo test`** (on Mac and in CI): notation parser (real captured samples), matchup JSON parsing (recorded fixture), smoothing, role inference, comp profile, lockfile parsing, discovery YAML, haste regex against a Data Dragon `item.json` fixture, cooldown formula, gold sum, purchase diff, threat scoring and advisor filtering, flags default to off, asset allowlist rejects non-asset paths.
- **Mac UI development:** replay mode `PIX_REPLAY=fixtures/<session>` feeds recorded LCU frames and `allgamedata`/`eventdata`.
- **Windows manual checklist:**
  1. Client open → connected and phase shown. Profile shows your own ranked stats and match history; entering champ select switches to Draft.
  2. Draft lobby → builds on hover and lock, matchup update when the enemy laner locks, sensible enemy roles.
  3. Practice Tool in Borderless with flags on locally → TAB shows the panel, clicks work and the game keeps focus, releasing TAB hides it; timers follow game time through a pause; gold strip updates.
  4. Advisor flag on → suggestions change after deaths.
  5. End of game → review renders.
  6. Arena → no build data.
- **CI compliance grep passes.**

## 10. What changed from the original draft (so it doesn't come back)
- **OP.GG arguments:** the endpoint is `mcp-api.op.gg/mcp` with full JSON-RPC, not `https://op.gg`. Arguments are `champion` (uppercase Data Dragon id), `position`, `game_mode`; there is no `champion_id`/`lane`. The analysis tool replies in class notation, not JSON.
- **OP.GG timing:** "50 ms" is replaced by prefetch plus cache.
- **Comp-aware data:** builds use the matchup tool and the comp profile; no source has full five-champion comp win rates.
- **Live Client limits:** enemy full runes aren't available, so Cosmic Insight is a manual toggle. Items have no descriptions, so the haste regex runs on Data Dragon and the pattern changed.
- **Windows and TLS:** the target is Windows only, with Borderless required. TLS is pinned to `riotgames.pem` rather than accepting any certificate. Process discovery is replaced with discovery from Riot's install-settings file.
- **Layout:** the overlay only shows gold diff and timers. Purchase alerts moved to the companion feed. Click-through comes from TAB-gated panel visibility, not cursor hit-testing.
- **Compliance:** the policy rules and feature classes in section 2 now apply.
- **Profile (after the designs):** the embedded OP.GG webview became a native screen built from League client data. The designs' "Send runes to client" and "Save item set" buttons became read-only copy/open actions.
