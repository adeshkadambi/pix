//! Typed events sent from the backend to the frontend windows.
//!
//! The TypeScript side of these types is generated into `src/lib/types/` by ts-rs
//! when `cargo test` runs.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use ts_rs::TS;

use crate::config::Config;

/// Event names. Keep in sync with `src/lib/events.ts`.
pub const CLIENT_STATUS: &str = "client-status";
pub const CONFIG_CHANGED: &str = "config-changed";

/// League client gameflow phase (SPEC §5.1). Filled in from the client in M1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
#[allow(dead_code, reason = "phases other than None are set by the client connection in M1")]
pub enum Phase {
    #[default]
    None,
    Lobby,
    Matchmaking,
    ReadyCheck,
    ChampSelect,
    InProgress,
    EndOfGame,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ClientStatus {
    pub connected: bool,
    pub phase: Phase,
}

pub fn emit_client_status(app: &AppHandle, status: &ClientStatus) {
    if let Err(e) = app.emit(CLIENT_STATUS, status) {
        tracing::warn!("failed to emit {CLIENT_STATUS}: {e}");
    }
}

pub fn emit_config_changed(app: &AppHandle, config: &Config) {
    if let Err(e) = app.emit(CONFIG_CHANGED, config) {
        tracing::warn!("failed to emit {CONFIG_CHANGED}: {e}");
    }
}
