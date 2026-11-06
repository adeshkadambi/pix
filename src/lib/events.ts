// Backend events. Names match the constants in src-tauri/src/events.rs.
// Windows subscribe to these; nothing polls from JavaScript (SPEC §7).
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { createSignal, onCleanup, type Accessor } from "solid-js";
import type { ClientStatus } from "./types/ClientStatus";
import type { Config } from "./types/Config";
import type { DraftView } from "./types/DraftView";
import type { LiveView } from "./types/LiveView";
import type { OverlayMode } from "./types/OverlayMode";
import type { ProfileView } from "./types/ProfileView";

export interface EventMap {
  "client-status": ClientStatus;
  "config-changed": Config;
  profile: ProfileView | null;
  draft: DraftView | null;
  live: LiveView | null;
  "overlay-mode": OverlayMode;
}

export function on<K extends keyof EventMap>(
  name: K,
  handler: (payload: EventMap[K]) => void,
): Promise<UnlistenFn> {
  return listen<EventMap[K]>(name, (e) => handler(e.payload));
}

/**
 * A signal that starts from `initial()` and then follows an event.
 * Must be called inside a component so the listener is removed on cleanup.
 */
export function eventSignal<K extends keyof EventMap>(
  name: K,
  initial: () => Promise<EventMap[K]>,
): Accessor<EventMap[K] | undefined> {
  const [value, setValue] = createSignal<EventMap[K]>();
  let gotEvent = false;
  const unlisten = on(name, (payload) => {
    gotEvent = true;
    setValue(() => payload);
  });
  initial()
    .then((v) => {
      if (!gotEvent) setValue(() => v);
    })
    .catch((e) => console.error(`initial ${name} failed`, e));
  onCleanup(() => void unlisten.then((fn) => fn()));
  return value;
}
