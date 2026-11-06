import { Show } from "solid-js";
import { Logo } from "../../components/Logo";
import { eventSignal } from "../../lib/events";
import { getOverlayMode } from "../../lib/ipc";
import "./hud.css";

// Display only; the window is always click-through. M4 adds the gold strip and running timers.
// During calibration it shows a banner, which doubles as the full-screen check: if you can read
// it over the game, Pix's overlay works in your window mode.
export function Hud() {
  const mode = eventSignal("overlay-mode", getOverlayMode);
  return (
    <Show when={mode()?.calibrating}>
      <div class="hud-banner">
        <Logo size={18} />
        <span>
          <strong>Pix overlay is visible.</strong> If you can read this over the game, overlays work in your window mode.
        </span>
      </div>
    </Show>
  );
}
