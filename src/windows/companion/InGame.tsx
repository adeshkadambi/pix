import { For, Show } from "solid-js";
import { AssetIcon } from "../../components/AssetIcon";
import { eventSignal } from "../../lib/events";
import { getLive } from "../../lib/ipc";
import type { LiveView } from "../../lib/types/LiveView";
import type { StepState } from "../../lib/types/StepState";
import { Placeholder } from "./Placeholder";
import "./live.css";
import "./review.css";

const STEP_LABEL: Record<StepState, string> = { Owned: "Owned", Next: "Next", Later: "Later" };

function clock(secs: number): string {
  const s = Math.max(0, Math.floor(secs));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export function InGame() {
  const live = eventSignal("live", getLive);
  return (
    <Show
      when={live()}
      fallback={
        <Placeholder title="Live">
          During a game, this shows your build path, the threat card and what the enemies are buying.
        </Placeholder>
      }
    >
      {(v) => <LiveScreen view={v()} />}
    </Show>
  );
}

function LiveScreen(props: { view: LiveView }) {
  const v = () => props.view;
  return (
    <div class="live">
      <div class="live-head">
        <Show when={v().me}>
          {(me) => (
            <div class="live-me">
              <AssetIcon path={me().champion.icon} name={me().champion.name} class="live-me-icon" decorative />
              <div class="why-text">
                <span class="strong">
                  {me().champion.name} · Lv {me().level}
                </span>
                <span class="muted small">
                  {me().kills} / {me().deaths} / {me().assists} · {me().cs} CS
                </span>
              </div>
              <div class="live-gold">
                <span class="live-gold-value">{me().gold.toLocaleString()} g</span>
                <span class="muted small">on hand</span>
              </div>
            </div>
          )}
        </Show>
        <span class="live-pill">
          <span class="live-dot" />
          Live <span class="live-clock">{clock(v().game_time)}</span>
        </span>
      </div>

      <Show when={v().window_mode === "FullScreen"}>
        <div class="notice" role="note">
          League is set to <strong>Full Screen</strong>. Pix's overlay may not show over it. To check, open Settings and
          use <strong>Position the tab panel</strong> while the game is running. If you can't see it, switch League to
          Borderless.
        </div>
      </Show>

      <Show when={v().advice}>
        {(a) => (
          <section class="card stack-12 advice" aria-label="Next buy">
            <span class="eyebrow eyebrow--accent">Next buy · from OP.GG stats for this game</span>
            <Show when={a().picks.length > 0} fallback={<p class="muted small-13">No enemy is a clear threat yet.</p>}>
              <For each={a().picks}>
                {(p, i) => (
                  <div class="why-row">
                    <AssetIcon path={p.item.icon} name={p.item.name} class={i() === 0 ? "item-tile" : "item-tile item-tile--small"} decorative />
                    <span class="why-text">
                      <span>
                        <strong>{p.item.name}</strong> <span class="muted">· {p.reason}</span>
                      </span>
                      <span class="muted small">
                        {((p.win / Math.max(1, p.play)) * 100).toFixed(1)}% · {p.play.toLocaleString()} games
                      </span>
                    </span>
                  </div>
                )}
              </For>
            </Show>
            <Show when={a().log.length > 0}>
              <h3 class="panel-title">Why it changed</h3>
              <For each={a().log}>
                {(l) => (
                  <div class="advice-log">
                    <span class="advice-time">{clock(l.game_time)}</span>
                    <span class="muted">{l.text}</span>
                  </div>
                )}
              </For>
            </Show>
          </section>
        )}
      </Show>

      <div class="live-cols">
        <section class="card stack-12" aria-label="Build path">
          <h2 class="card-title">Build path</h2>
          <Show when={v().build} fallback={<p class="muted small-13">{v().build_note}</p>}>
            {(b) => (
              <>
                <Show when={b().runes}>
                  {(r) => (
                    <div class="live-runes">
                      <AssetIcon path={r().perks[0]?.icon} name={r().perks[0]?.name ?? "Keystone"} class="keystone keystone--small" />
                      <span class="small-13">
                        {r().perks[0]?.name} <span class="muted">· {r().primary_style.name} + {r().sub_style.name}</span>
                      </span>
                    </div>
                  )}
                </Show>
                <For each={b().steps}>
                  {(s, i) => (
                    <div class={`step step--${s.state.toLowerCase()}`}>
                      <span class="step-n">{i() + 1}</span>
                      <AssetIcon path={s.item.icon} name={s.item.name} class="item-tile item-tile--small" decorative />
                      <span class="step-name">{s.item.name}</span>
                      <span class={`step-tag step-tag--${s.state.toLowerCase()}`}>{STEP_LABEL[s.state]}</span>
                    </div>
                  )}
                </For>
                <p class="muted small">Your build from champ select (or the standard one for your champion). "Next" is the first item you don't own yet.</p>
              </>
            )}
          </Show>
        </section>

        <div class="stack">
          <Show when={v().threats.length > 0}>
            <section class="card stack-12" aria-label="Threat card">
              <h2 class="card-title">Threat card</h2>
              <For each={v().threats}>
                {(t) => (
                  <div class="threat-row">
                    <AssetIcon path={t.champion.icon} name={t.champion.name} class="threat-icon" decorative />
                    <span class="strong threat-name">{t.champion.name}</span>
                    <span class="threat-answers">
                      <For each={t.answers}>
                        {(a) => <AssetIcon path={a.item.icon} name={`${a.item.name}: ${a.reason}`} class="item-tile item-tile--small" />}
                      </For>
                    </span>
                  </div>
                )}
              </For>
            </section>
          </Show>

          <section class="card stack-12" aria-label="Enemy item completions">
            <h2 class="card-title">Enemy item completions</h2>
            <Show when={v().purchases.length > 0} fallback={<p class="muted small-13">Finished items enemies complete show up here and briefly on your game screen.</p>}>
              <For each={v().purchases}>
                {(p) => (
                  <div class="purchase">
                    <span class="purchase-time">{clock(p.game_time)}</span>
                    <AssetIcon path={p.champion.icon} name={p.champion.name} class="purchase-champ" decorative />
                    <span class="small-13">
                      <span class="strong">{p.champion.name}</span> <span class="muted">completed</span>
                    </span>
                    <For each={p.items}>
                      {(it) => <AssetIcon path={it.icon} name={it.name} class="item-tile item-tile--small" decorative />}
                    </For>
                    <span class="small-13 purchase-names">{p.items.map((it) => it.name).join(" · ")}</span>
                  </div>
                )}
              </For>
            </Show>
          </section>
        </div>
      </div>
    </div>
  );
}
