import { createSignal, For, Match, onCleanup, Show, Switch } from "solid-js";
import { AssetIcon } from "../../components/AssetIcon";
import { eventSignal } from "../../lib/events";
import { duration, kdaRatio, perMinute, tierLabel, timeAgo, winRate } from "../../lib/format";
import { getProfile, refreshProfile } from "../../lib/ipc";
import type { LpChange } from "../../lib/types/LpChange";
import type { MatchSummary } from "../../lib/types/MatchSummary";
import type { ProfileView } from "../../lib/types/ProfileView";
import type { RankedQueue } from "../../lib/types/RankedQueue";
import { Placeholder } from "./Placeholder";
import "./profile.css";

const FORM_GAMES = 8;

/** Ticks every 30 s so "x min ago" labels stay current. */
function useNow() {
  const [now, setNow] = createSignal(Date.now());
  const timer = setInterval(() => setNow(Date.now()), 30_000);
  onCleanup(() => clearInterval(timer));
  return now;
}

export function Profile(props: { connected: boolean }) {
  const profile = eventSignal("profile", getProfile);
  return (
    <Switch>
      <Match when={profile()}>{(p) => <ProfileScreen profile={p()} connected={props.connected} />}</Match>
      <Match when={props.connected}>
        <Placeholder title="Loading your profile">Reading your ranked stats and match history from the client.</Placeholder>
      </Match>
      <Match when={!props.connected}>
        <Placeholder title="Waiting for League client">
          Open the League client and your profile and match history will show up here.
        </Placeholder>
      </Match>
    </Switch>
  );
}

function ProfileScreen(props: { profile: ProfileView; connected: boolean }) {
  const now = useNow();
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const p = () => props.profile;
  const form = () => p().matches.slice(0, FORM_GAMES);
  const formWins = () => form().filter((m) => m.result === "Win").length;
  const formLosses = () => form().filter((m) => m.result === "Loss").length;

  const refresh = async () => {
    setBusy(true);
    setError(undefined);
    try {
      await refreshProfile();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="profile">
      <section aria-label="Profile" class="profile-side">
        <div class="card stack">
          <div class="summoner">
            <AssetIcon path={p().profile_icon} name="Profile icon" class="summoner-icon" decorative />
            <div class="summoner-text">
              <div class="summoner-name">
                {p().game_name} <span class="muted">#{p().tag_line}</span>
              </div>
              <div class="muted small-13">
                {p().region} · Level {p().level}
              </div>
            </div>
          </div>

          <div class="ranked-grid">
            <RankedCard label="Solo / Duo" queue={p().solo} />
            <RankedCard label="Flex" queue={p().flex} />
          </div>

          <Show when={form().length > 0}>
            <div class="form">
              <div class="form-head">
                <span>Last {form().length} games</span>
                <span class="form-record">
                  {formWins()}W {formLosses()}L
                </span>
              </div>
              <div class="form-bar" aria-hidden="true">
                <For each={form()}>{(m) => <span class={`form-tick form-tick--${m.result.toLowerCase()}`} />}</For>
              </div>
            </div>
          </Show>
        </div>

        <div class="card stack-12">
          <h2 class="card-title">Recent ranked champions</h2>
          <Show when={p().champions.length > 0} fallback={<p class="muted small-13">No ranked games in your recent history.</p>}>
            <For each={p().champions}>
              {(c) => (
                <div class="champ-row">
                  <AssetIcon path={c.champion.icon} name={c.champion.name} class="champ-icon" decorative />
                  <div class="champ-text">
                    <span class="strong">{c.champion.name}</span>
                    <span class="muted small">
                      {c.games} {c.games === 1 ? "game" : "games"} · {kdaRatio(c.kills, c.deaths, c.assists)} KDA
                    </span>
                  </div>
                  <span class="champ-wr">{winRate(c.wins, c.games)}</span>
                </div>
              )}
            </For>
          </Show>
        </div>
      </section>

      <section aria-label="Match history" class="profile-main">
        <div class="history-head">
          <h1>Match history</h1>
          <div class="history-actions">
            <span class="muted small-13">from League client · {timeAgo(p().fetched_at_ms, now())}</span>
            <button type="button" class="btn" onClick={refresh} disabled={busy() || !props.connected}>
              {busy() ? "Refreshing…" : "Refresh"}
            </button>
          </div>
        </div>
        <Show when={error()}>
          <p class="error" role="alert">
            {error()}
          </p>
        </Show>
        <Show when={p().matches.length > 0} fallback={<p class="muted">No recent games.</p>}>
          <For each={p().matches}>{(m) => <MatchRow match={m} now={now()} />}</For>
          <p class="muted small-13 history-note">
            LP changes show for ranked games played while Pix is open. Older games come from Pix's
            local game log, kept on this PC.
          </p>
        </Show>
      </section>
    </div>
  );
}

function RankedCard(props: { label: string; queue: RankedQueue | null }) {
  return (
    <div class="ranked">
      <span class="eyebrow">{props.label}</span>
      <Show when={props.queue} fallback={<span class="ranked-tier muted">Unranked</span>}>
        {(q) => (
          <>
            <span class="ranked-tier">{tierLabel(q().tier, q().division)}</span>
            <span class="ranked-lp">{q().lp} LP</span>
            <span class="muted small-13">
              {q().wins}W {q().losses}L · {winRate(q().wins, q().wins + q().losses)}
            </span>
          </>
        )}
      </Show>
    </div>
  );
}

function LpBadge(props: { change: LpChange }) {
  const c = () => props.change;
  const moved = () => c().tier_before !== c().tier_after || c().division_before !== c().division_after;
  const label = () => (c().delta > 0 ? `+${c().delta} LP` : c().delta < 0 ? `−${-c().delta} LP` : "±0 LP");
  const title = () => {
    const rank = tierLabel(c().tier_after, c().division_after);
    return moved() ? `${c().delta > 0 ? "Promoted" : "Demoted"} to ${rank} · ${c().lp_after} LP` : `${rank} · ${c().lp_after} LP`;
  };
  return (
    <span class={`lp ${c().delta >= 0 ? "lp--up" : "lp--down"}`} title={title()}>
      {label()}
      <Show when={moved()}>
        <span class="lp-arrow" aria-label={c().delta > 0 ? "promoted" : "demoted"}>
          {c().delta > 0 ? " ▲" : " ▼"}
        </span>
      </Show>
    </span>
  );
}

function MatchRow(props: { match: MatchSummary; now: number }) {
  const m = () => props.match;
  return (
    <article class="match">
      <div class="match-meta">
        <span class="result-line">
          <span class={`result result--${m().result.toLowerCase()}`}>{m().result}</span>
          <Show when={m().lp_change}>{(lp) => <LpBadge change={lp()} />}</Show>
        </span>
        <span class="muted small">{m().queue}</span>
        <span class="muted small">
          {duration(m().duration_secs)} · {timeAgo(m().created_ms, props.now)}
        </span>
      </div>
      <div class="match-champ">
        <AssetIcon path={m().champion.icon} name={m().champion.name} class="match-champ-icon" decorative />
        <div class="match-champ-text">
          <span class="strong">{m().champion.name}</span>
          <span class="spells">
            <For each={m().spells}>{(s) => <AssetIcon path={s.icon} name={s.name} class="spell-icon" />}</For>
          </span>
        </div>
      </div>
      <div class="match-kda">
        <span class="kda">
          {m().kills} / {m().deaths} / {m().assists}
        </span>
        <span class="muted small">
          {kdaRatio(m().kills, m().deaths, m().assists)}
          {m().deaths === 0 ? "" : " KDA"}
        </span>
      </div>
      <div class="match-cs">
        <span class="medium">{m().cs} CS</span>
        <span class="muted small">{perMinute(m().cs, m().duration_secs)} / min</span>
      </div>
      <div class="match-items" aria-label="Items">
        <For each={m().items}>
          {(item) => (
            <Show when={item} fallback={<span class="item-slot item-slot--empty" aria-hidden="true" />}>
              {(it) => <AssetIcon path={it().icon} name={it().name} class="item-slot" />}
            </Show>
          )}
        </For>
      </div>
    </article>
  );
}
