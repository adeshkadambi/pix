import type { ClientStatus } from "../../lib/types/ClientStatus";
import type { Phase } from "../../lib/types/Phase";

const PHASE_LABEL: Record<Phase, string> = {
  None: "idle",
  Lobby: "in lobby",
  Matchmaking: "in queue",
  ReadyCheck: "match found",
  ChampSelect: "champ select",
  InProgress: "in game",
  EndOfGame: "post-game",
};

export function StatusPill(props: { status: ClientStatus | undefined }) {
  const connected = () => props.status?.connected ?? false;
  const phase = () => props.status?.phase ?? "None";
  const tone = () => {
    if (!connected()) return "off";
    return phase() === "ChampSelect" || phase() === "InProgress" ? "live" : "on";
  };
  return (
    <div class="status" role="status">
      <span class={`status-dot status-dot--${tone()}`} />
      <span>
        {connected() ? `Client connected · ${PHASE_LABEL[phase()]}` : "Waiting for League client"}
        {props.status?.summoners_rift === false ? " · no build data off Summoner's Rift" : ""}
      </span>
    </div>
  );
}
