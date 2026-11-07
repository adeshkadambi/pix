import { getVersion } from "@tauri-apps/api/app";
import { createResource, createSignal, For, Match, Show, Switch } from "solid-js";
import { eventSignal } from "../../lib/events";
import { getConfig, setCheckUpdates, setFlag, startCalibration } from "../../lib/ipc";
import { checkForUpdate, installUpdate, updateState } from "../../lib/updates";
import type { Flag } from "../../lib/types/Flag";

// Only the live advisor carries a note: Riot's policy is stricter about in-game suggestions.
const FLAGS: { id: Flag; name: string; desc: string; risk?: string }[] = [
  {
    id: "overlay_gold",
    name: "Gold diff overlay",
    desc: "Item gold for each scoreboard row and each team, shown while you hold Tab.",
  },
  {
    id: "overlay_timers",
    name: "Summoner spell timers",
    desc: "Click an enemy's spell while holding Tab to start its cooldown timer.",
  },
  {
    id: "live_advisor",
    name: "Live item advisor",
    desc: "Suggests your next item during the game from OP.GG matchup stats, based on who is ahead and who killed you.",
    risk: "Off by default, since Riot's policy is stricter about in-game suggestions.",
  },
];

export function Settings() {
  const config = eventSignal("config-changed", getConfig);
  const [error, setError] = createSignal<string>();

  const toggle = async (flag: Flag, on: boolean) => {
    setError(undefined);
    try {
      await setFlag(flag, on);
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div class="stack">
      <h1>Settings</h1>
      <section class="card stack">
        <h2 class="card-title">Features</h2>
        <p class="muted">The overlays are on by default; the live advisor is off.</p>
        <For each={FLAGS}>
          {(f) => {
            const checked = () => config()?.flags[f.id] ?? false;
            return (
              <label class="flag">
                <div class="flag-text">
                  <span class="flag-name">{f.name}</span>
                  <span class="muted small">{f.desc}</span>
                  <Show when={f.risk}>
                    <span class="risk risk--off-policy">{f.risk}</span>
                  </Show>
                </div>
                <input
                  type="checkbox"
                  role="switch"
                  class="switch"
                  checked={checked()}
                  disabled={!config()}
                  onChange={(e) => toggle(f.id, e.currentTarget.checked)}
                />
              </label>
            );
          }}
        </For>
        <Show when={error()}>
          <p class="error" role="alert">
            {error()}
          </p>
        </Show>
      </section>
      <section class="card stack">
        <h2 class="card-title">Overlay</h2>
        <p class="muted">
          With summoner timers on, the overlay panel shows while you hold <kbd>{config()?.scoreboard_key ?? "Tab"}</kbd>{" "}
          with League in front, and the game keeps focus.
        </p>
        <p class="muted">
          Position the panel once next to the scoreboard's enemy rows; it's saved for your screen resolution. Doing this
          during a game also checks your window mode: if you can see the "Pix overlay is visible" banner over the game,
          overlays work for you.
        </p>
        <div>
          <button type="button" class="btn" onClick={() => void startCalibration()}>
            Position the tab panel
          </button>
        </div>
      </section>
      <Updates checkOnStart={config()?.check_updates ?? true} />
    </div>
  );
}

function Updates(props: { checkOnStart: boolean }) {
  const [version] = createResource(getVersion);
  const s = updateState;
  return (
    <section class="card stack">
      <h2 class="card-title">Updates</h2>
      <p class="muted">Pix {version() ? `v${version()}` : ""}. Updates come from Pix's GitHub releases and are signed.</p>
      <label class="flag">
        <div class="flag-text">
          <span class="flag-name">Check for updates automatically</span>
          <span class="muted small">When Pix starts. You choose when to install.</span>
        </div>
        <input
          type="checkbox"
          role="switch"
          class="switch"
          checked={props.checkOnStart}
          onChange={(e) => void setCheckUpdates(e.currentTarget.checked)}
        />
      </label>
      <div class="update-row">
        <Switch>
          <Match when={s().kind === "available" && s()}>
            {(a) => (
              <div class="stack-8">
                <button type="button" class="btn btn--primary" onClick={() => void installUpdate()}>
                  Install v{(a() as { version: string }).version} and restart
                </button>
                <Show when={(a() as { notes?: string }).notes}>
                  {(n) => <pre class="update-notes">{n()}</pre>}
                </Show>
              </div>
            )}
          </Match>
          <Match when={s().kind === "installing" && s()}>
            {(i) => {
              const p = () => (i() as { percent: number | null }).percent;
              return <span class="muted">Installing… {p() != null ? `${p()}%` : ""}</span>;
            }}
          </Match>
          <Match when={true}>
            <button type="button" class="btn" disabled={s().kind === "checking"} onClick={() => void checkForUpdate()}>
              {s().kind === "checking" ? "Checking…" : "Check for updates"}
            </button>
            <span class="muted small">
              {s().kind === "none" ? "You're on the latest version." : ""}
            </span>
          </Match>
        </Switch>
      </div>
      <Show when={s().kind === "error" && s()}>
        {(e) => (
          <p class="error small" role="alert">
            Couldn't check for updates: {(e() as { message: string }).message}
          </p>
        )}
      </Show>
    </section>
  );
}
