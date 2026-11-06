# Pix

Personal, Windows-only League of Legends companion and overlay (Tauri v2 + Rust).

- Own profile and match history when idle
- Champion select builds ranked by win rate against the enemy team comp
- In-game overlay: gold diff and summoner spell timers (clickable while holding TAB)
- Second-screen companion: pre-game build, threat card, enemy purchase feed, post-game review

**Status:** M1. Pix finds and connects to the League client (read-only, TLS pinned to Riot's root),
follows the game phase, and shows your own profile, ranked stats and match history with client
icons. Settings has the feature flags (all off by default). Draft, Live and Review are placeholders
until M2+. UI designs are in [design/](design/README.md), exported from Claude Design.

See [SPEC.md](SPEC.md) for the full specification, compliance rules and milestones (M0–M6).

## Requirements (Windows)

- Node.js LTS, Rust stable (MSVC), Visual Studio 2022 Build Tools with the C++ workload
- WebView2 (built into Windows 11)

```powershell
winget install OpenJS.NodeJS.LTS Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

## Commands

```bash
npm install
npm run tauri dev                              # run the app
cargo test --manifest-path src-tauri/Cargo.toml  # Rust tests; also regenerates src/lib/types
cargo test --manifest-path src-tauri/Cargo.toml --lib -- --ignored live_connects  # against a running client
npm run compliance                             # banned-API and GET-only check (SPEC §2)
npm run tauri build                            # release .exe and installer
```

CI (`.github/workflows/ci.yml`) runs the compliance check, the tests and a release build on
Windows, and uploads the `.exe` as an artifact.

## Layout

```
src/                 SolidJS frontend (one page; each window picks its view by label)
  windows/companion  main window: header, tabs, Settings, About
  windows/hud        click-through overlay (gold strip, timers)
  windows/tabpanel   overlay shown while TAB is held
  lib/               IPC and event helpers, generated types
  styles/            design tokens
src-tauri/src/       Rust backend
  lcu/               League client: discovery, lockfile, GET-only HTTP, WebSocket, supervisor
  riot_tls.rs        TLS pinned to Riot's root certificate (certs/riotgames.pem)
  profile.rs         native profile view
  asset_protocol.rs  lcu-asset:// icons (allowlisted, cached, Data Dragon fallback)
  ddragon.rs         Data Dragon download and disk cache
design/              Claude Design mockups and tokens
scripts/             compliance check
```

---

Pix isn't endorsed by Riot Games and doesn't reflect the views or opinions of Riot Games or anyone officially involved in producing or managing Riot Games properties. Riot Games, and all associated properties are trademarks or registered trademarks of Riot Games, Inc.
