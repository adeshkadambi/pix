import { openUrl } from "@tauri-apps/plugin-opener";
import { createSignal, For, Match, Show, Switch } from "solid-js";
import { AssetIcon } from "../../components/AssetIcon";
import { eventSignal } from "../../lib/events";
import { getDraft, setEnemyRole, setMyRole } from "../../lib/ipc";
import type { AllyView } from "../../lib/types/AllyView";
import type { BuildView } from "../../lib/types/BuildView";
import type { DraftView } from "../../lib/types/DraftView";
import type { EnemyView } from "../../lib/types/EnemyView";
import type { Icon } from "../../lib/types/Icon";
import type { IconOption } from "../../lib/types/IconOption";
import type { Role } from "../../lib/types/Role";
import type { RunePageView } from "../../lib/types/RunePageView";
import type { SuggestionsView } from "../../lib/types/SuggestionsView";
import type { SuggestionView } from "../../lib/types/SuggestionView";
import { Placeholder } from "./Placeholder";
import "./draft.css";

const ROLES: Role[] = ["top", "jungle", "mid", "adc", "support"];
const ROLE_LABEL: Record<Role, string> = { top: "Top", jungle: "Jungle", mid: "Mid", adc: "Bot", support: "Support" };
const LATER_LABEL = ["4th item", "5th item", "6th item"];

/** Actual win rate and sample, e.g. "52.4% · 1,204 games". Options are ranked by smoothed rate. */
function stat(o: { play: number; win: number }): string {
  const wr = o.play > 0 ? ((o.win / o.play) * 100).toFixed(1) : "–";
  return `${wr}% · ${o.play.toLocaleString()} games`;
}

export function Draft() {
  const draft = eventSignal("draft", getDraft);
  return (
    <Show
      when={draft()}
      fallback={
        <Placeholder title="Draft">
          In champ select, Pix shows ranked builds for your champion, adjusted for the enemy team.
        </Placeholder>
      }
    >
      {(d) => <DraftScreen draft={d()} />}
    </Show>
  );
}

function DraftScreen(props: { draft: DraftView }) {
  const d = () => props.draft;
  return (
    <div class="draft">
      <section class="bans" aria-label="Bans">
        <BanList label="Your bans" bans={d().ally_bans} />
        <BanList label="Enemy bans" bans={d().enemy_bans} />
      </section>
      <div class="draft-cols">
        <section class="team" aria-label="Your team">
          <h2 class="team-title">Your team</h2>
          <For each={d().allies}>{(a) => <AllyRow ally={a} />}</For>
        </section>
        <BuildPanel draft={d()} />
        <section class="team" aria-label="Enemy team">
          <h2 class="team-title">Enemy team</h2>
          <For each={d().enemies}>{(e) => <EnemyRow enemy={e} />}</For>
          <p class="muted small">Enemy roles are estimated from how often each champion plays each role. Click a role to change it.</p>
        </section>
      </div>
    </div>
  );
}

function BanList(props: { label: string; bans: Icon[] }) {
  return (
    <div class="ban-list">
      <span class="eyebrow">{props.label}</span>
      <Show when={props.bans.length > 0} fallback={<span class="muted small">none</span>}>
        <For each={props.bans}>
          {(b) => (
            <span class="ban-chip">
              <AssetIcon path={b.icon} name={b.name} class="ban-icon" decorative />
              <span>{b.name}</span>
            </span>
          )}
        </For>
      </Show>
    </div>
  );
}

function AllyRow(props: { ally: AllyView }) {
  const a = () => props.ally;
  return (
    <div class={`pick ${a().is_me ? "pick--me" : ""}`}>
      <Show when={a().champion} fallback={<span class="pick-icon pick-icon--empty" aria-hidden="true" />}>
        {(c) => <AssetIcon path={c().icon} name={c().name} class="pick-icon" decorative />}
      </Show>
      <div class="pick-text">
        <span class="strong">{a().champion?.name ?? "Picking…"}</span>
        <span class={a().is_me ? "pick-sub pick-sub--me" : "pick-sub"}>
          {a().role ? ROLE_LABEL[a().role!] : "No role"}
          {a().is_me ? " · You" : ""}
        </span>
      </div>
      <span class="muted small">{a().champion ? (a().locked ? "Locked" : "Hovering") : ""}</span>
    </div>
  );
}

function EnemyRow(props: { enemy: EnemyView }) {
  const e = () => props.enemy;
  // Clicking the role cycles through roles; past the last one it returns to the estimate.
  const cycle = () => {
    const current = e().inferred;
    if (!current) return;
    if (current.manual && current.role === ROLES[ROLES.length - 1]) return void setEnemyRole(e().cell_id, null);
    const next = current.manual ? ROLES[ROLES.indexOf(current.role) + 1] : ROLES[(ROLES.indexOf(current.role) + 1) % ROLES.length];
    void setEnemyRole(e().cell_id, next);
  };
  return (
    <div class={`pick ${e().lane_opponent ? "pick--opponent" : ""}`}>
      <Show when={e().champion} fallback={<span class="pick-icon pick-icon--empty" aria-hidden="true" />}>
        {(c) => <AssetIcon path={c().icon} name={c().name} class="pick-icon" decorative />}
      </Show>
      <div class="pick-text">
        <span class="strong">{e().champion?.name ?? "Picking…"}</span>
        <span class="pick-sub">{e().tags}</span>
      </div>
      <Show when={e().inferred}>
        {(r) => (
          <button
            type="button"
            class={`role-chip ${r().manual ? "role-chip--manual" : ""}`}
            onClick={cycle}
            title={r().manual ? "Set by you. Click to change." : `Estimated (${Math.round(r().confidence * 100)}% sure). Click to change.`}
          >
            {ROLE_LABEL[r().role]}
            <span class="role-conf">{r().manual ? "set" : `${Math.round(r().confidence * 100)}%`}</span>
          </button>
        )}
      </Show>
    </div>
  );
}

function BuildPanel(props: { draft: DraftView }) {
  const d = () => props.draft;
  return (
    <section class="card build" aria-label="Builds">
      <Switch>
        <Match when={d().status === "Ready" && d().build}>{(b) => <BuildDetails build={b()} />}</Match>
        <Match when={d().status === "Suggesting" && d().suggestions}>{(s) => <Suggestions view={s()} />}</Match>
        <Match when={d().status === "NeedRole"}>
          <div class="stack">
            <h1 class="build-title">Choose your role</h1>
            <p class="muted">Blind pick doesn't assign roles. Pick yours to see builds.</p>
            <div class="role-picker">
              <For each={ROLES}>
                {(r) => (
                  <button type="button" class="btn" onClick={() => void setMyRole(r)}>
                    {ROLE_LABEL[r]}
                  </button>
                )}
              </For>
            </div>
          </div>
        </Match>
        <Match when={true}>
          <div class="stack">
            <h1 class="build-title">{d().status === "Off" ? "No build data" : d().status === "Error" ? "Builds unavailable" : "Builds"}</h1>
            <p class={d().status === "Error" ? "error" : "muted"}>
              {d().message ?? (d().status === "Loading" ? "Loading builds…" : "Hover or lock a champion to see builds.")}
            </p>
          </div>
        </Match>
      </Switch>
    </section>
  );
}

function Suggestions(props: { view: SuggestionsView }) {
  const v = () => props.view;
  return (
    <div class="build-body">
      <div class="build-head-text">
        <span class="eyebrow eyebrow--accent">Suggested picks</span>
        <h1 class="build-title">
          {ROLE_LABEL[v().role]}
          <Show when={v().opponent}>{(o) => <span class="muted"> into {o().name}</span>}</Show>
        </h1>
        <span class="muted small-13">
          {v().note ?? "Ranked by win rate against your lane opponent, plus synergy with your team and what it's missing."} Lock in to see
          runes and builds.
        </span>
      </div>
      <Show when={v().yours.length > 0}>
        <div class="why">
          <h3 class="panel-title">Your champions</h3>
          <div class="suggestions">
            <For each={v().yours}>{(s) => <SuggestionCard s={s} />}</For>
          </div>
        </div>
      </Show>
      <div class="why">
        <h3 class="panel-title">Top picks</h3>
        <Show when={v().top.length > 0} fallback={<p class="muted">No pick data for this role yet.</p>}>
          <div class="suggestions">
            <For each={v().top}>{(s) => <SuggestionCard s={s} />}</For>
          </div>
        </Show>
      </div>
    </div>
  );
}

function SuggestionCard(props: { s: SuggestionView }) {
  const s = () => props.s;
  return (
    <div class={`suggestion ${s().hovered ? "suggestion--hovered" : ""}`}>
      <AssetIcon path={s().champion.icon} name={s().champion.name} class="pick-icon" decorative />
      <div class="why-text">
        <span class="suggestion-head">
          <span class="strong">{s().champion.name}</span>
          <span class="suggestion-wr">{(s().win_rate * 100).toFixed(1)}%</span>
          <Show when={s().hovered}>
            <span class="tag">Hovering</span>
          </Show>
        </span>
        <span class="muted small">{s().reasons.join(" · ")}</span>
      </div>
    </div>
  );
}

function buildText(b: BuildView): string {
  const names = (icons: Icon[]) => icons.map((i) => i.name).join(" > ");
  const lines = [`${b.champion.name} ${ROLE_LABEL[b.role]}${b.opponent ? ` vs ${b.opponent.name}` : ""} (Pix, OP.GG data)`];
  if (b.runes[0]) lines.push(`Runes: ${b.runes[0].perks.map((p) => p.name).join(", ")} | ${b.runes[0].shards.map((s) => s.name).join(", ")}`);
  if (b.spells[0]) lines.push(`Spells: ${names(b.spells[0].icons)}`);
  if (b.starters[0]) lines.push(`Start: ${names(b.starters[0].icons)}`);
  if (b.core[0]) lines.push(`Core: ${names(b.core[0].icons)}`);
  if (b.boots[0]) lines.push(`Boots: ${names(b.boots[0].icons)}`);
  b.later.forEach((slot, i) => slot[0] && lines.push(`${LATER_LABEL[i]}: ${slot.map((o) => o.icons[0].name).join(" / ")}`));
  if (b.skills[0]) lines.push(`Skills: ${b.skills[0].order.join(" > ")}`);
  return lines.join("\n");
}

function BuildDetails(props: { build: BuildView }) {
  const b = () => props.build;
  const [copied, setCopied] = createSignal(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(buildText(b()));
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (e) {
      console.error("copy failed", e);
    }
  };
  return (
    <div class="build-body">
      <div class="build-head">
        <div class="build-head-text">
          <span class="eyebrow eyebrow--accent">Ranked builds</span>
          <h1 class="build-title">
            {b().champion.name} · {ROLE_LABEL[b().role]}
            <Show when={b().opponent}>{(o) => <span class="muted"> vs {o().name}</span>}</Show>
          </h1>
          <span class="muted small-13">
            {b().opponent ? "Lane matchup" : "All matchups"}: {(b().win_rate * 100).toFixed(1)}% win rate over {b().games.toLocaleString()} games ·
            OP.GG ranked data
          </span>
        </div>
        <div class="build-actions">
          <button type="button" class="btn" onClick={copy}>
            {copied() ? "Copied" : "Copy build"}
          </button>
          <button type="button" class="btn" onClick={() => void openUrl(b().opgg_url)}>
            Open on OP.GG
          </button>
        </div>
      </div>

      <div class="build-grid">
        <div class="panel">
          <h3 class="panel-title">Runes</h3>
          <For each={b().runes}>{(r, i) => <RuneOption page={r} primary={i() === 0} />}</For>
        </div>
        <div class="panel">
          <h3 class="panel-title">Core items</h3>
          <For each={b().core}>{(o) => <OptionRow option={o} arrows />}</For>
          <h3 class="panel-title">Boots</h3>
          <For each={b().boots}>{(o) => <OptionRow option={o} />}</For>
          <For each={b().later}>
            {(slot, i) => (
              <>
                <h3 class="panel-title">{LATER_LABEL[i()]}</h3>
                <div class="inline-options">
                  <For each={slot}>{(o) => <OptionChip option={o} />}</For>
                </div>
              </>
            )}
          </For>
        </div>
        <div class="panel">
          <h3 class="panel-title">Summoner spells</h3>
          <For each={b().spells}>{(o) => <OptionRow option={o} />}</For>
          <h3 class="panel-title">Starting items</h3>
          <For each={b().starters}>{(o) => <OptionRow option={o} />}</For>
          <h3 class="panel-title">Skill order</h3>
          <For each={b().skills}>
            {(s) => (
              <div class="option">
                <span class="skill-order">{s.order.join(" › ")}</span>
                <span class="muted small">{stat(s)}</span>
              </div>
            )}
          </For>
        </div>
      </div>

      <Show when={b().situational.length > 0}>
        <div class="why">
          <h3 class="panel-title">Situational, for this draft</h3>
          <For each={b().situational}>
            {(s) => (
              <div class="why-row">
                <AssetIcon path={s.item.icon} name={s.item.name} class="item-tile" decorative />
                <div class="why-text">
                  <span>
                    <strong>{s.item.name}</strong> <span class="muted">· {s.reason}</span>
                  </span>
                  <span class="muted small">{stat(s)}</span>
                </div>
              </div>
            )}
          </For>
        </div>
      </Show>

      <Show when={b().threats.length > 0}>
        <div class="why">
          <h3 class="panel-title">Threat card</h3>
          <div class="threats">
            <For each={b().threats}>
              {(t) => (
                <div class="threat">
                  <AssetIcon path={t.champion.icon} name={t.champion.name} class="threat-icon" decorative />
                  <div class="why-text">
                    <span class="strong">{t.champion.name}</span>
                    <span class="threat-answers">
                      <For each={t.answers}>
                        {(a) => <AssetIcon path={a.item.icon} name={`${a.item.name}: ${a.reason}`} class="item-tile item-tile--small" />}
                      </For>
                    </span>
                  </div>
                </div>
              )}
            </For>
          </div>
        </div>
      </Show>
    </div>
  );
}

function RuneOption(props: { page: RunePageView; primary: boolean }) {
  const p = () => props.page;
  return (
    <div class={`rune-option ${props.primary ? "rune-option--top" : ""}`}>
      <div class="rune-row">
        <AssetIcon path={p().perks[0]?.icon} name={p().perks[0]?.name ?? "Keystone"} class={props.primary ? "keystone" : "keystone keystone--small"} />
        <div class="rune-minor">
          <For each={p().perks.slice(1)}>{(perk) => <AssetIcon path={perk.icon} name={perk.name} class="rune-icon" />}</For>
          <span class="rune-sep" aria-hidden="true" />
          <For each={p().shards}>{(s) => <AssetIcon path={s.icon} name={s.name} class="shard-icon" />}</For>
        </div>
      </div>
      <span class="muted small">
        {p().perks[0]?.name} · {p().sub_style.name} · {stat(p())}
      </span>
    </div>
  );
}

function OptionRow(props: { option: IconOption; arrows?: boolean }) {
  return (
    <div class="option">
      <span class="option-icons">
        <For each={props.option.icons}>
          {(icon, i) => (
            <>
              <Show when={props.arrows && i() > 0}>
                <span class="arrow" aria-hidden="true">
                  ›
                </span>
              </Show>
              <AssetIcon path={icon.icon} name={icon.name} class="item-tile" />
            </>
          )}
        </For>
      </span>
      <span class="muted small">{stat(props.option)}</span>
    </div>
  );
}

function OptionChip(props: { option: IconOption }) {
  const icon = () => props.option.icons[0];
  return (
    <span class="chip-option" title={`${icon().name}: ${stat(props.option)}`}>
      <AssetIcon path={icon().icon} name={icon().name} class="item-tile" />
      <span class="muted small">{props.option.play > 0 ? `${((props.option.win / props.option.play) * 100).toFixed(0)}%` : ""}</span>
    </span>
  );
}
