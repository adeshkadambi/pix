import { getVersion } from "@tauri-apps/api/app";
import { createResource } from "solid-js";
import { Logo } from "../../components/Logo";

export function About() {
  const [version] = createResource(getVersion);
  return (
    <div class="stack">
      <section class="card stack">
        <div class="brand">
          <Logo size={32} />
          <h1>Pix</h1>
          <span class="muted">{version() ? `v${version()}` : ""}</span>
        </div>
        <p class="muted">
          A personal League of Legends companion: your profile, comp-aware builds in champ select,
          and a second-screen view during games.
        </p>
      </section>
      <section class="card stack">
        <h2 class="card-title">Legal</h2>
        <p class="legal">
          Pix isn't endorsed by Riot Games and doesn't reflect the views or opinions of Riot Games
          or anyone officially involved in producing or managing Riot Games properties. Riot Games,
          and all associated properties are trademarks or registered trademarks of Riot Games, Inc.
        </p>
      </section>
    </div>
  );
}
