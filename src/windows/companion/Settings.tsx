import { createSignal, For, Show } from "solid-js";
import { eventSignal } from "../../lib/events";
import { getConfig, setFlag, startCalibration } from "../../lib/ipc";
import type { Flag } from "../../lib/types/Flag";

// Only the off-policy advisor carries a risk label.
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
    desc: "Suggests your next item during the game based on who is fed and who killed you.",
    risk: "Not Riot-compliant, use at own risk.",
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
    </div>
  );
}
