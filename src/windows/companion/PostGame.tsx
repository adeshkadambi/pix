import { createResource, For, onCleanup, Show } from "solid-js";
import { AssetIcon } from "../../components/AssetIcon";
import { on } from "../../lib/events";
import { duration, tierLabel } from "../../lib/format";
import { getReview } from "../../lib/ipc";
import type { ReviewView } from "../../lib/types/ReviewView";
import { Placeholder } from "./Placeholder";
import "./review.css";

/** Post-game review: the latest game, or `gameId` when opened from match history. */
export function PostGame(props: { gameId: number | null }) {
  const [review, { refetch }] = createResource(() => props.gameId, (id) => getReview(id));
  // A new review (a game just ended) replaces the latest one.
  const unlisten = on("review", () => {
    if (props.gameId == null) void refetch();
  });
  onCleanup(() => void unlisten.then((fn) => fn()));
  return (
    <Show
      when={review()}
      fallback={
        <Placeholder title="Review">
          After a game, this shows who killed you, when, and which items would have countered them. Reviews are saved
          for games Pix was running during.
        </Placeholder>
      }
    >
      {(r) => <ReviewScreen review={r()} />}
    </Show>
  );
}

function ReviewScreen(props: { review: ReviewView }) {
  const r = () => props.review;
  const pct = (t: number) => `${Math.min(100, (t / Math.max(1, r().duration_secs)) * 100)}%`;
  return (
    <div class="review">
      <section class="card review-head">
        <AssetIcon path={r().champion.icon} name={r().champion.name} class="live-me-icon" decorative />
        <div class="why-text">
          <span class="review-title">
            <Show when={r().result}>
              {(res) => <span class={`result result--${res().toLowerCase()}`}>{res()}</span>}
            </Show>
            {r().champion.name}
          </span>
          <span class="muted small-13">
            {r().kills} / {r().deaths} / {r().assists} · {r().cs} CS · {duration(r().duration_secs)}
          </span>
        </div>
        <Show when={r().lp_change}>
          {(lp) => (
            <span class={`lp ${lp().delta >= 0 ? "lp--up" : "lp--down"} review-lp`}>
              {lp().delta >= 0 ? `+${lp().delta}` : `−${-lp().delta}`} LP
              <span class="muted small"> · {tierLabel(lp().tier_after, lp().division_after)} {lp().lp_after} LP</span>
            </span>
          )}
        </Show>
      </section>

      <div class="live-cols">
        <section class="card stack-12" aria-label="Who killed you">
          <h2 class="card-title">Who killed you</h2>
          <Show when={r().deaths_by.length > 0} fallback={<p class="muted small-13">You didn't die this game. Nice.</p>}>
            <For each={r().deaths_by}>
              {(g) => (
                <div class="killer-row">
                  <AssetIcon path={g.killer.icon} name={g.killer.name} class="threat-icon" decorative />
                  <span class="strong killer-name">{g.killer.name}</span>
                  <span class="killer-count">{g.count}×</span>
                  <Show when={g.assists.length > 0}>
                    <span class="killer-assists" title="Assisted">
                      <span class="muted small">with</span>
                      <For each={g.assists}>{(a) => <AssetIcon path={a.icon} name={a.name} class="purchase-champ" />}</For>
                    </span>
                  </Show>
                </div>
              )}
            </For>
            <div class="timeline" aria-label="Death timeline">
              <div class="timeline-bar" />
              <For each={r().timeline}>
                {(d) => (
                  <span class="timeline-mark" style={{ left: pct(d.time) }} title={`${duration(Math.floor(d.time))} · ${d.killer.name}`}>
                    <AssetIcon path={d.killer.icon} name={d.killer.name} class="timeline-icon" decorative />
                  </span>
                )}
              </For>
              <span class="timeline-label timeline-label--start">0:00</span>
              <span class="timeline-label timeline-label--end">{duration(r().duration_secs)}</span>
            </div>
          </Show>
        </section>

        <section class="card stack-12" aria-label="What would have countered them">
          <h2 class="card-title">What would have countered them</h2>
          <Show when={r().counters.length > 0} fallback={<p class="muted small-13">No counter data for this game.</p>}>
            <For each={r().counters}>
              {(c) => (
                <div class="counter">
                  <div class="counter-head">
                    <AssetIcon path={c.killer.icon} name={c.killer.name} class="threat-icon" decorative />
                    <span class="why-text">
                      <span class="strong">vs {c.killer.name}</span>
                      <span class="muted small">
                        {c.win_rate != null
                          ? `Your ${r().champion.name}: ${(c.win_rate * 100).toFixed(1)}% over ${c.games.toLocaleString()} games`
                          : "From your champion's overall builds"}
                      </span>
                    </span>
                  </div>
                  <Show when={c.items.length > 0} fallback={<p class="muted small">No matching items in your champion's builds.</p>}>
                    <For each={c.items}>
                      {(it) => (
                        <div class="why-row">
                          <AssetIcon path={it.item.icon} name={it.item.name} class="item-tile item-tile--small" decorative />
                          <span class="why-text">
                            <span>
                              <strong>{it.item.name}</strong> <span class="muted">· {it.reason}</span>
                            </span>
                            <span class="muted small">
                              {((it.win / Math.max(1, it.play)) * 100).toFixed(1)}% · {it.play.toLocaleString()} games
                            </span>
                          </span>
                        </div>
                      )}
                    </For>
                  </Show>
                </div>
              )}
            </For>
          </Show>
        </section>
      </div>
    </div>
  );
}
