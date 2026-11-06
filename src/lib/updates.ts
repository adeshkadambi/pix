// App updates from GitHub Releases (Tauri updater). Updates are signed; the app only installs
// ones signed with Pix's key. State is shared by Settings and the header's "Update available".
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { createSignal } from "solid-js";

export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "none" }
  | { kind: "available"; version: string; notes?: string }
  | { kind: "installing"; percent: number | null }
  | { kind: "error"; message: string };

const [state, setState] = createSignal<UpdateState>({ kind: "idle" });
export const updateState = state;

let pending: Update | null = null;

export async function checkForUpdate(): Promise<void> {
  setState({ kind: "checking" });
  try {
    const update = await check();
    pending = update;
    setState(update ? { kind: "available", version: update.version, notes: update.body ?? undefined } : { kind: "none" });
  } catch (e) {
    setState({ kind: "error", message: String(e) });
  }
}

/** Downloads and installs the pending update, then restarts Pix. */
export async function installUpdate(): Promise<void> {
  if (!pending) return;
  let total = 0;
  let done = 0;
  setState({ kind: "installing", percent: null });
  try {
    await pending.downloadAndInstall((ev) => {
      if (ev.event === "Started") total = ev.data.contentLength ?? 0;
      if (ev.event === "Progress") {
        done += ev.data.chunkLength;
        setState({ kind: "installing", percent: total ? Math.round((done / total) * 100) : null });
      }
    });
    await relaunch();
  } catch (e) {
    setState({ kind: "error", message: String(e) });
  }
}
