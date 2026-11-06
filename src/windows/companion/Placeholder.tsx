import type { JSX } from "solid-js";

/** Empty-state card for screens that aren't built yet or have nothing to show. */
export function Placeholder(props: { title: string; children: JSX.Element }) {
  return (
    <section class="card placeholder">
      <h1>{props.title}</h1>
      <p class="muted">{props.children}</p>
    </section>
  );
}
