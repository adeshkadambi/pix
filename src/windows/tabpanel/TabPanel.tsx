import { For, Show } from "solid-js";
import { AssetIcon } from "../../components/AssetIcon";
import { eventSignal } from "../../lib/events";
import { finishCalibration, getLive, getOverlayMode, toggleTimer } from "../../lib/ipc";
import { countdown, goldDiff, goldK } from "../../lib/timer";
import type { LiveEnemy } from "../../lib/types/LiveEnemy";
import type { SpellView } from "../../lib/types/SpellView";
import "./tabpanel.css";

// Compact panel next to the scoreboard: one row per enemy (item gold, champion, two summoner
// spells). Shown while the scoreboard key is held with League in front (summoner timer
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
          <div class="tp-team" title="Item gold (full item prices): blue team vs. red team, like the scoreboard">
            <span>Team</span>
            <GoldCompare yours={t().ally} theirs={t().enemy} red={t().red_side} />
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
              </div>
            )}
          </For>
        }
      >
        <For each={live()!.enemies}>
          {(e) => <EnemyRow enemy={e} timers={live()!.timers_enabled} gold={live()!.team_gold != null} red={live()!.team_gold?.red_side ?? false} />}
        </For>
      </Show>
    </div>
  );
}

/**
 * "8.8K vs. 7.8K (+1.0K)": blue team first, red second, matching the scoreboard's left and right.
 * The difference is always yours minus theirs (green when you're ahead, pink when behind).
 */
function GoldCompare(props: { yours: number; theirs: number; red: boolean }) {
  const diff = () => props.yours - props.theirs;
  const [left, right] = props.red ? [props.theirs, props.yours] : [props.yours, props.theirs];
  return (
    <span class="tp-compare">
      {goldK(left)} vs. {goldK(right)} (
      <span class={diff() >= 0 ? "tp-up" : "tp-down"}>{goldDiff(diff())}</span>)
    </span>
  );
}

// With gold on, a row reads like the scoreboard: [blue champion] blue gold vs. red gold [red champion].
function EnemyRow(props: { enemy: LiveEnemy; timers: boolean; gold: boolean; red: boolean }) {
  const e = () => props.enemy;
  const enemyIcon = () => <AssetIcon path={e().champion.icon} name={e().champion.name} class="tp-champ" decorative />;
  const allyIcon = () => (
    <Show when={e().row_gold} fallback={<span class="tp-champ tp-champ--ally tp-champ--empty" />}>
      {(g) => <AssetIcon path={g().ally.icon} name={g().ally.name} class="tp-champ tp-champ--ally" decorative />}
    </Show>
  );
  return (
    <div class={`tp-row ${e().dead ? "tp-row--dead" : ""}`}>
      <Show when={props.gold} fallback={enemyIcon()}>
        {props.red ? enemyIcon() : allyIcon()}
        <span
          class="tp-gold"
          title={e().row_gold == null ? "No player in this scoreboard row on your team" : `Item gold: ${e().row_gold!.ally.name} vs. ${e().champion.name}`}
        >
          <Show when={e().row_gold} fallback="–">
            {(g) => <GoldCompare yours={g().yours} theirs={g().theirs} red={props.red} />}
          </Show>
        </span>
        {props.red ? allyIcon() : enemyIcon()}
      </Show>
      <Show when={props.timers}>
        <For each={e().spells}>{(s) => <SpellButton champion={e().key} championName={e().champion.name} spell={s} />}</For>
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
