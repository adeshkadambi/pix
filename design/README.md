# Pix designs

Exported from the Claude Design artifact "Pix": https://claude.ai/artifact/LqWtyM1SPthro1BN5HWzAg

These are reference mockups, not app code. Each `.dc.html` file is one screen. The markup uses
the Design tool's template tags (`<sc-for>`, `<sc-if>`, `{{...}}`), and the sample data sits in
the `renderVals()` script at the bottom. The files load `./support.js` from the Design runtime,
so they won't render on their own. Open the artifact link to see them live.

## Screens

| File | Size | Screen |
| --- | --- | --- |
| `Main.dc.html` | 1280×920 | Home: profile, ranked stats, match history |
| `Draft.dc.html` | 1280×920 | Champ select: picks, bans, suggested runes and build, "why" notes |
| `Live.dc.html` | 440×920 | In game: second-screen live build companion |
| `Scoreboard.dc.html` | 1920×1080 | In-game overlay (hold Tab): team gold diff and summoner timers |

`canvas.json` holds the canvas layout and board titles.

## Tokens

Fonts: **Space Grotesk** (500/600/700) for headings and numbers, **Inter** (400/500/600) for body.
Numbers use `font-variant-numeric: tabular-nums`.

| Role | Color |
| --- | --- |
| Page background | `#140D21` |
| Header background | `#170F26` |
| Card | `#1E1530` |
| Inner card | `#261A3B` |
| Raised / selected | `#2A1D42` |
| Tile fill | `#31224C` |
| Border | `#2C2043` |
| Border strong | `#3A2B57` |
| Tile border | `#4A3768` / `#6A4A8C` |
| Text | `#F3ECFC` |
| Text secondary | `#CDBFE6` |
| Text muted | `#B4A3D1` |
| Accent (pink) | `#EE7AC8` |
| Accent light | `#F7A8DC` |
| Accent text on pink | `#1A0F24` |
| Secondary accent (violet) | `#A98BF5` |
| Win / positive | `#A6D86E`, text `#C4EA97`, pill `#2E3D22` |
| Loss / negative | text `#F7A8DC`, pill `#3D1F33` |
| Overlay panel | `rgba(20, 13, 33, 0.94)`, border `#5B3A73` |

Radii: 8px buttons and nav, 10–12px small cards, 14–16px cards, 6px item tiles, 999px pills.
