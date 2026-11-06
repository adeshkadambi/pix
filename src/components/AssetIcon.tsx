import { createSignal, Show } from "solid-js";
import { assetUrl } from "../lib/assets";
import { initials } from "../lib/format";

/**
 * A client icon that falls back to the name's initials if the image is missing.
 * `class` sets size and shape; the label is always available to screen readers.
 */
export function AssetIcon(props: { path: string | null | undefined; name: string; class: string; decorative?: boolean }) {
  const [failed, setFailed] = createSignal(false);
  const src = () => (failed() ? undefined : assetUrl(props.path));
  return (
    <span class={`asset-icon ${props.class}`} title={props.name}>
      <Show when={src()} fallback={<span aria-hidden="true">{initials(props.name)}</span>}>
        <img src={src()} alt="" loading="lazy" onError={() => setFailed(true)} />
      </Show>
      <Show when={!props.decorative}>
        <span class="sr-only">{props.name}</span>
      </Show>
    </span>
  );
}
