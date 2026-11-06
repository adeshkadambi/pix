import { Show } from "solid-js";
import { Placeholder } from "./Placeholder";

// M1: native profile and match history from the League client (design/Main.dc.html).
export function Profile(props: { connected: boolean }) {
  return (
    <Show
      when={props.connected}
      fallback={
        <Placeholder title="Waiting for League client">
          Open the League client and your profile and match history will show up here.
        </Placeholder>
      }
    >
      <Placeholder title="Profile">Your ranked stats and match history arrive in M1.</Placeholder>
    </Show>
  );
}
