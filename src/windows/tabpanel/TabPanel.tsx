import { For, Show } from "solid-js";
import { AssetIcon } from "../../components/AssetIcon";
import { eventSignal } from "../../lib/events";
import { finishCalibration, getLive, getOverlayMode } from "../../lib/ipc";
import "./tabpanel.css";

// Compact panel next to the scoreboard: one row per enemy (champion, two summoner spells).
// Shown while the scoreboard key is held with League in front (summoner timer feature on), or
// during calibration. M4 turns the spell icons into clickable timers.
export function TabPanel() {
  const mode = eventSignal("overlay-mode", getOverlayMode);
  const live = eventSignal("live", getLive);
  const calibrating = () => mode()?.calibrating ?? false;

  return (
    <div class={`tabpanel ${calibrating() ? "tabpanel--calibrating" : ""}`}>
      <Show when={calibrating()}>
        <div class="tp-bar" data-tauri-drag-region title="Drag next to the scoreboard's enemy rows">
          <span class="tp-grip" data-tauri-drag-region aria-hidden="true">
            ⠿
          </span>
          <button type="button" class="tp-icon-btn tp-icon-btn--save" title="Save position" onClick={() => void finishCalibration(true)}>
            ✓
          </button>
          <button type="button" class="tp-icon-btn" title="Cancel" onClick={() => void finishCalibration(false)}>
            ✕
          </button>
        </div>
      </Show>
      <Show
        when={(live()?.enemies.length ?? 0) > 0}
        fallback={
          <For each={[0, 1, 2, 3, 4]}>
            {() => (
              <div class="tp-row tp-row--placeholder">
                <span class="tp-champ" />
                <span class="tp-spell" />
                <span class="tp-spell" />
              </div>
            )}
          </For>
        }
      >
        <For each={live()!.enemies}>
          {(e) => (
            <div class={`tp-row ${e.dead ? "tp-row--dead" : ""}`}>
              <AssetIcon path={e.champion.icon} name={e.champion.name} class="tp-champ" decorative />
              <For each={e.spells}>{(s) => <AssetIcon path={s.icon} name={s.name} class="tp-spell" />}</For>
            </div>
          )}
        </For>
      </Show>
    </div>
  );
}
