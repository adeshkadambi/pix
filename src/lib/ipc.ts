// Typed wrappers around the backend commands in src-tauri/src/lib.rs.
import { invoke } from "@tauri-apps/api/core";
import type { ClientStatus } from "./types/ClientStatus";
import type { Config } from "./types/Config";
import type { Flag } from "./types/Flag";
import type { DraftView } from "./types/DraftView";
import type { LiveView } from "./types/LiveView";
import type { OverlayMode } from "./types/OverlayMode";
import type { ProfileView } from "./types/ProfileView";
import type { Role } from "./types/Role";

export const getConfig = () => invoke<Config>("get_config");

export const setFlag = (flag: Flag, on: boolean) => invoke<Config>("set_flag", { flag, on });

export const getClientStatus = () => invoke<ClientStatus>("get_client_status");

/** The last fetched profile, or null if none yet. */
export const getProfile = () => invoke<ProfileView | null>("get_profile");

/** Fetches the profile from the League client now. */
export const refreshProfile = () => invoke<ProfileView>("refresh_profile");

/** The current champ select view, or null outside champ select. */
export const getDraft = () => invoke<DraftView | null>("get_draft");

/** Override an enemy's inferred role; null clears the override. */
export const setEnemyRole = (cellId: number, role: Role | null) => invoke<void>("set_enemy_role", { cellId, role });

/** Your role in blind pick. */
export const setMyRole = (role: Role) => invoke<void>("set_my_role", { role });

/** Pick which rune page or core set to follow in game. */
export const chooseBuild = (kind: "runes" | "core", index: number) => invoke<void>("choose_build", { kind, index });

/** The current game's view, or null when no game is running. */
export const getLive = () => invoke<LiveView | null>("get_live");

/** Start an enemy's summoner spell timer, or clear it if it's running. */
export const toggleTimer = (champion: string, slot: number) => invoke<void>("toggle_timer", { champion, slot });

/** Tick or untick Cosmic Insight for an enemy (+18 summoner spell haste). */
export const setCosmic = (champion: string, on: boolean) => invoke<void>("set_cosmic", { champion, on });

export const getOverlayMode = () => invoke<OverlayMode>("get_overlay_mode");

/** Show the tab panel (draggable) and the HUD test banner. */
export const startCalibration = () => invoke<void>("start_calibration");

/** The HUD timer list was dragged during calibration (`x`, `y` as a share of the screen). */
export const moveTimers = (x: number, y: number) => invoke<void>("move_timers", { x, y });

/** End calibration; `save` stores the tab panel's and timer list's positions. */
export const finishCalibration = (save: boolean) => invoke<void>("finish_calibration", { save });
