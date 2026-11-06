import { createEffect, createSignal, For, Show } from "solid-js";
import { AssetIcon } from "../../components/AssetIcon";
import { Logo } from "../../components/Logo";
import { eventSignal } from "../../lib/events";
import { getLive, getOverlayMode, moveTimers } from "../../lib/ipc";
import { countdown } from "../../lib/timer";
import "./hud.css";

/** How long an enemy's item completion stays on screen, in game seconds. */
const POPUP_SECS = 8;

// Display only and click-through, except during calibration. Top-right column (under League's
// FPS / ping by default):
// - Running summoner timers stay visible while the scoreboard key isn't held.
// - Enemy item completions pop up briefly underneath.
// During calibration a sample list can be dragged into place, and a banner shows. The banner
// doubles as the window-mode check: if you can read it over the game, overlays work.
export function Hud() {
  const mode = eventSignal("overlay-mode", getOverlayMode);
  const live = eventSignal("live", getLive);
  const calibrating = () => mode()?.calibrating ?? false;

  // The column's top-RIGHT corner as a share of the screen. Local while dragging, then reported.
  const [pos, setPos] = createSignal<[number, number]>([0.993, 0.052]);
  createEffect(() => {
    const p = mode()?.timers_pos;
    if (p) setPos([p[0], p[1]]);
  });

  const running = () =>
    (live()?.enemies ?? []).flatMap((e) =>
      e.spells.filter((s) => s.remaining != null).map((s) => ({ champion: e.champion, spell: s.icon, left: s.remaining! })),
    );
  const popups = () => {
    const now = live()?.game_time ?? 0;
    return (live()?.purchases ?? []).filter((p) => now - p.game_time <= POPUP_SECS);
  };

  let drag: { dx: number; dy: number } | undefined;
  const onDown = (ev: PointerEvent) => {
    if (!calibrating()) return;
    const box = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    drag = { dx: box.right - ev.clientX, dy: ev.clientY - box.top };
    (ev.currentTarget as HTMLElement).setPointerCapture(ev.pointerId);
  };
  const onMove = (ev: PointerEvent) => {
    if (!drag) return;
    setPos([(ev.clientX + drag.dx) / window.innerWidth, (ev.clientY - drag.dy) / window.innerHeight]);
  };
  const onUp = () => {
    if (!drag) return;
    drag = undefined;
    const [x, y] = pos();
    void moveTimers(x, y);
  };

  return (
    <>
      <Show when={calibrating()}>
        <div class="hud-banner">
          <Logo size={18} />
          <span>
            <strong>Pix overlay is visible.</strong> Drag the timer list where you want it, then save on the tab panel.
          </span>
        </div>
      </Show>
      <div class="hud-column" style={{ right: `${(1 - pos()[0]) * 100}%`, top: `${pos()[1] * 100}%` }}>
        <Show when={calibrating() || running().length > 0}>
          <div
            class={`hud-timers ${calibrating() ? "hud-timers--calibrating" : ""}`}
            aria-label="Running summoner timers"
            onPointerDown={onDown}
            onPointerMove={onMove}
            onPointerUp={onUp}
          >
            <Show
              when={!calibrating()}
              fallback={
                <For each={["4:37", "2:05", "58"]}>
                  {(t) => (
                    <div class="hud-timer">
                      <span class="hud-champ hud-sample" />
                      <span class="hud-spell hud-sample" />
                      <span class="hud-left">{t}</span>
                    </div>
                  )}
                </For>
              }
            >
              <For each={running()}>
                {(t) => (
                  <div class="hud-timer">
                    <AssetIcon path={t.champion.icon} name={t.champion.name} class="hud-champ" decorative />
                    <AssetIcon path={t.spell.icon} name={t.spell.name} class="hud-spell" decorative />
                    <span class="hud-left">{countdown(t.left)}</span>
                  </div>
                )}
              </For>
            </Show>
          </div>
        </Show>
        <For each={popups()}>
          {(p) => (
            <div class="hud-popup" role="status">
              <AssetIcon path={p.champion.icon} name={p.champion.name} class="hud-champ" decorative />
              <span class="hud-popup-text">
                <strong>{p.champion.name}</strong> completed
              </span>
              <For each={p.items}>{(it) => <AssetIcon path={it.icon} name={it.name} class="hud-item" decorative />}</For>
              <span class="hud-popup-text hud-popup-items">{p.items.map((it) => it.name).join(" · ")}</span>
            </div>
          )}
        </For>
      </div>
    </>
  );
}
