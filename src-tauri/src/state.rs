//! Shared app state, managed by Tauri and reached through `app.state::<AppState>()`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use tauri::{AppHandle, Manager};

use crate::asset_protocol::AssetCache;
use crate::config::Config;
use crate::db::Db;
use crate::ddragon::DData;
use crate::events::{self, ClientStatus};
use crate::lcu::catalog::Catalog;
use crate::lcu::http::LcuHttp;
use crate::profile::ProfileView;

pub struct AppState {
    pub config: Mutex<Config>,
    pub config_path: PathBuf,
    pub cache_dir: PathBuf,
    pub client: Mutex<ClientStatus>,
    /// Set while connected to the League client.
    pub lcu: Mutex<Option<LcuHttp>>,
    /// Champion, item and spell names and icons from the connected client, fetched once.
    pub catalog: tokio::sync::Mutex<Option<Arc<Catalog>>>,
    pub profile: Mutex<Option<ProfileView>>,
    /// Local game log. `None` if the database couldn't be opened; the app still works without it.
    pub db: Option<Db>,
    pub ddragon: RwLock<Option<Arc<DData>>>,
    pub assets: AssetCache,
    pub public_http: reqwest::Client,
}

impl AppState {
    pub fn new(
        config: Config,
        config_path: PathBuf,
        cache_dir: PathBuf,
        db: Option<Db>,
        public_http: reqwest::Client,
    ) -> Self {
        Self {
            db,
            config: Mutex::new(config),
            config_path,
            cache_dir,
            client: Mutex::new(ClientStatus::default()),
            lcu: Mutex::new(None),
            catalog: tokio::sync::Mutex::new(None),
            profile: Mutex::new(None),
            ddragon: RwLock::new(None),
            assets: AssetCache::new(),
            public_http,
        }
    }

    pub fn lcu_http(&self) -> Option<LcuHttp> {
        self.lcu.lock().unwrap().clone()
    }

    pub fn ddragon(&self) -> Option<Arc<DData>> {
        self.ddragon.read().unwrap().clone()
    }
}

/// Changes the client status and emits it if anything changed.
pub fn update_client_status(app: &AppHandle, change: impl FnOnce(&mut ClientStatus)) {
    let state = app.state::<AppState>();
    let mut status = state.client.lock().unwrap();
    let before = status.clone();
    change(&mut status);
    if *status != before {
        let snapshot = status.clone();
        drop(status);
        events::emit_client_status(app, &snapshot);
    }
}
