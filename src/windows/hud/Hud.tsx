import { createEffect, createSignal, For, Show } from "solid-js";
import { AssetIcon } from "../../components/AssetIcon";
import { Logo } from "../../components/Logo";
import { eventSignal } from "../../lib/events";
import { getLive, getOverlayMode, moveTimers } from "../../lib/ipc";
import { countdown } from "../../lib/timer";
import "./hud.css";

// Display only and click-through, except during calibration:
// - Running summoner timers stay visible here while the scoreboard key isn't held.
// - During calibration a sample timer list can be dragged into place, and a banner shows. The
//   banner doubles as the window-mode check: if you can read it over the game, overlays work.
export function Hud() {
  const mode = eventSignal("overlay-mode", getOverlayMode);
  const live = eventSignal("live", getLive);
  const calibrating = () => mode()?.calibrating ?? false;

  // The list's top-RIGHT corner as a share of the screen (default: under League's FPS / ping).
  // Local while dragging, then reported to the backend.
  const [pos, setPos] = createSignal<[number, number]>([0.993, 0.052]);
  createEffect(() => {
    const p = mode()?.timers_pos;
    if (p) setPos([p[0], p[1]]);
  });

  const running = () =>
    (live()?.enemies ?? []).flatMap((e) =>
      e.spells.filter((s) => s.remaining != null).map((s) => ({ champion: e.champion, spell: s.icon, left: s.remaining! })),
    );

  // Drag keeps the grab point under the cursor; the stored point is the box's top-right corner.
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
      <Show when={calibrating() || running().length > 0}>
        <div
          class={`hud-timers ${calibrating() ? "hud-timers--calibrating" : ""}`}
          style={{ right: `${(1 - pos()[0]) * 100}%`, top: `${pos()[1] * 100}%` }}
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
    </>
  );
}
