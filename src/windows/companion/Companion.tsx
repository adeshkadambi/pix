import { createEffect, createSignal, For, Match, on, onCleanup, Switch } from "solid-js";
import { Logo } from "../../components/Logo";
import { eventSignal, on as on_ } from "../../lib/events";
import { getClientStatus } from "../../lib/ipc";
import type { Phase } from "../../lib/types/Phase";
import { About } from "./About";
import { Draft } from "./Draft";
import { InGame } from "./InGame";
import { PostGame } from "./PostGame";
import { Profile } from "./Profile";
import { Settings } from "./Settings";
import { StatusPill } from "./StatusPill";
import "./companion.css";

type Tab = "profile" | "draft" | "live" | "review";
type View = Tab | "settings" | "about";

const TABS: { id: Tab; label: string }[] = [
  { id: "profile", label: "Profile" },
  { id: "draft", label: "Draft" },
  { id: "live", label: "Live" },
  { id: "review", label: "Review" },
];

/** Which tab the companion jumps to when the client enters a phase. */
function tabForPhase(phase: Phase): Tab {
  switch (phase) {
    case "ChampSelect":
      return "draft";
    case "InProgress":
      return "live";
    case "EndOfGame":
      return "review";
    default:
      return "profile";
  }
}

export function Companion() {
  const status = eventSignal("client-status", getClientStatus);
  const [view, setView] = createSignal<View>("profile");
  // Which game the Review tab shows: null is the latest review.
  const [reviewId, setReviewId] = createSignal<number | null>(null);
  const openReview = (gameId: number) => {
    setReviewId(gameId);
    setView("review");
  };

  // A new draft view (champ select) brings the Draft tab forward from Profile.
  const unlistenDraft = on_("draft", (d) => {
    if (d && view() === "profile") setView("draft");
  });
  onCleanup(() => void unlistenDraft.then((fn) => fn()));
  const unlistenReview = on_("review", () => setReviewId(null));
  onCleanup(() => void unlistenReview.then((fn) => fn()));
  const unlistenLive = on_("live", (v) => {
    if (v && (view() === "profile" || view() === "draft")) setView("live");
  });
  onCleanup(() => void unlistenLive.then((fn) => fn()));

  // Follow the game phase, but only when it changes, so a manual tab choice sticks.
  createEffect(
    on(
      () => status()?.phase,
      (phase) => phase && setView(tabForPhase(phase)),
      { defer: true },
    ),
  );

  return (
    <div class="companion">
      <header class="topbar">
        <div class="brand">
          <Logo />
          <span class="brand-name">Pix</span>
        </div>
        <nav class="tabs" aria-label="Main">
          <For each={TABS}>
            {(tab) => (
              <button
                type="button"
                class="tab"
                aria-current={view() === tab.id ? "page" : undefined}
                onClick={() => {
                  if (tab.id === "review") setReviewId(null);
                  setView(tab.id);
                }}
              >
                {tab.label}
              </button>
            )}
          </For>
        </nav>
        <div class="topbar-end">
          <StatusPill status={status()} />
          <button
            type="button"
            class="tab"
            aria-current={view() === "settings" ? "page" : undefined}
            onClick={() => setView("settings")}
          >
            Settings
          </button>
          <button
            type="button"
            class="tab"
            aria-current={view() === "about" ? "page" : undefined}
            onClick={() => setView("about")}
          >
            About
          </button>
        </div>
      </header>

      <main class="page">
        <Switch>
          <Match when={view() === "profile"}>
            <Profile connected={status()?.connected ?? false} onOpenReview={openReview} />
          </Match>
          <Match when={view() === "draft"}>
            <Draft />
          </Match>
          <Match when={view() === "live"}>
            <InGame />
          </Match>
          <Match when={view() === "review"}>
            <PostGame gameId={reviewId()} />
          </Match>
          <Match when={view() === "settings"}>
            <Settings />
          </Match>
          <Match when={view() === "about"}>
            <About />
          </Match>
        </Switch>
      </main>
    </div>
  );
}
