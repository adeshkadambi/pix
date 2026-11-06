// Typed wrappers around the backend commands in src-tauri/src/lib.rs.
import { invoke } from "@tauri-apps/api/core";
import type { ClientStatus } from "./types/ClientStatus";
import type { Config } from "./types/Config";
import type { Flag } from "./types/Flag";
import type { DraftView } from "./types/DraftView";
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
