import { For, Show } from "solid-js";
import { AssetIcon } from "../../components/AssetIcon";
import { eventSignal } from "../../lib/events";
import { finishCalibration, getLive, getOverlayMode, setCosmic, toggleTimer } from "../../lib/ipc";
import { countdown, goldDiff } from "../../lib/timer";
import type { LiveEnemy } from "../../lib/types/LiveEnemy";
import type { SpellView } from "../../lib/types/SpellView";
import "./tabpanel.css";

// Compact panel next to the scoreboard: one row per enemy (champion, two summoner spells,
// Cosmic Insight). Shown while the scoreboard key is held with League in front (summoner timer
// feature on), or during calibration. Click a spell to start its timer; click again to clear it.
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
      <Show when={live()?.team_gold}>
        {(t) => (
          <div class="tp-team" title="Item gold (full item prices), your team minus theirs">
            <span>Team</span>
            <span class={t().ally - t().enemy >= 0 ? "tp-up" : "tp-down"}>{goldDiff(t().ally - t().enemy)}</span>
          </div>
        )}
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
                <span class="tp-ci" />
              </div>
            )}
          </For>
        }
      >
        <For each={live()!.enemies}>
          {(e) => <EnemyRow enemy={e} timers={live()!.timers_enabled} gold={live()!.team_gold != null} />}
        </For>
      </Show>
    </div>
  );
}

function EnemyRow(props: { enemy: LiveEnemy; timers: boolean; gold: boolean }) {
  const e = () => props.enemy;
  return (
    <div class={`tp-row ${e().dead ? "tp-row--dead" : ""}`}>
      <AssetIcon path={e().champion.icon} name={e().champion.name} class="tp-champ" decorative />
      <Show when={props.gold}>
        <span
          class={`tp-gold ${e().lane_gold == null ? "" : e().lane_gold! >= 0 ? "tp-up" : "tp-down"}`}
          title={e().lane_gold == null ? "No player in this scoreboard row on your team" : `Item gold: your player in this scoreboard row vs ${e().champion.name}`}
        >
          {e().lane_gold == null ? "–" : goldDiff(e().lane_gold!)}
        </span>
      </Show>
      <Show when={props.timers}>
        <For each={e().spells}>{(s) => <SpellButton champion={e().key} championName={e().champion.name} spell={s} />}</For>
        <label class={`tp-ci ${e().cosmic ? "tp-ci--on" : ""}`} title={`${e().champion.name} has Cosmic Insight (+18 summoner spell haste)`}>
          <input type="checkbox" checked={e().cosmic} onChange={(ev) => void setCosmic(e().key, ev.currentTarget.checked)} />
          <span aria-hidden="true">CI</span>
        </label>
      </Show>
    </div>
  );
}

function SpellButton(props: { champion: string; championName: string; spell: SpellView }) {
  const s = () => props.spell;
  const running = () => s().remaining != null;
  const pct = () => (running() && s().duration ? (1 - s().remaining! / s().duration!) * 100 : 0);
  const label = () =>
    !s().trackable
      ? `${props.championName} ${s().icon.name}: not timed`
      : running()
        ? `${props.championName} ${s().icon.name}: ${countdown(s().remaining!)} left. Click to clear.`
        : `Start ${props.championName} ${s().icon.name} timer (${countdown(s().cooldown)})`;
  return (
    <button
      type="button"
      class={`tp-spell tp-spell-btn ${running() ? "tp-spell--running" : ""} ${s().trackable ? "" : "tp-spell--untracked"}`}
      disabled={!s().trackable}
      title={label()}
      aria-label={label()}
      onClick={() => void toggleTimer(props.champion, s().slot)}
    >
      <AssetIcon path={s().icon.icon} name={s().icon.name} class="tp-spell-icon" decorative />
      <Show when={running()}>
        <span class="tp-time">{countdown(s().remaining!)}</span>
        <span class="tp-progress" style={{ width: `${pct()}%` }} />
      </Show>
    </button>
  );
}
