//! `lcu-asset` URI protocol (SPEC §6): serves League client images to the webviews.
//!
//! The page asks for `http://lcu-asset.localhost/lol-game-data/assets/...`. Only GET, and only
//! paths under `/lol-game-data/assets/`, so a webview can't reach other client endpoints with the
//! user's credentials. Order: memory LRU → disk cache → client → Data Dragon fallback.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use lru::LruCache;
use tauri::http::{Request, Response, StatusCode, header};
use tauri::{AppHandle, Manager};

use crate::state::AppState;

pub const SCHEME: &str = "lcu-asset";
const ALLOWED_PREFIX: &str = "/lol-game-data/assets/";
const MEMORY_BUDGET: usize = 32 * 1024 * 1024;

type Entry = (Arc<Vec<u8>>, String);

/// In-memory LRU bounded by total bytes.
pub struct AssetCache {
    inner: Mutex<(LruCache<String, Entry>, usize)>,
}

impl AssetCache {
    pub fn new() -> Self {
        Self { inner: Mutex::new((LruCache::new(NonZeroUsize::new(4096).unwrap()), 0)) }
    }

    fn get(&self, key: &str) -> Option<Entry> {
        self.inner.lock().unwrap().0.get(key).cloned()
    }

    fn put(&self, key: String, bytes: Arc<Vec<u8>>, content_type: String) {
        let mut guard = self.inner.lock().unwrap();
        let (cache, used) = &mut *guard;
        let size = bytes.len();
        if size > MEMORY_BUDGET {
            return;
        }
        if let Some((old, _)) = cache.put(key, (bytes, content_type)) {
            *used -= old.len();
        }
        *used += size;
        while *used > MEMORY_BUDGET {
            match cache.pop_lru() {
                Some((_, (b, _))) => *used -= b.len(),
                None => break,
            }
        }
    }
}

/// Decodes `%XX` escapes (convertFileSrc encodes the whole path).
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Normalizes a request path and checks the allowlist. Returns the client path to fetch.
pub fn allowed_path(raw: &str) -> Option<String> {
    let decoded = percent_decode(raw)?;
    let path = if decoded.starts_with('/') { decoded } else { format!("/{decoded}") };
    let safe_chars = path.chars().all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c));
    let no_traversal = !path.split('/').any(|seg| seg == "." || seg == "..");
    (path.starts_with(ALLOWED_PREFIX) && safe_chars && no_traversal && !path.contains("//")).then_some(path)
}

fn content_type_for(path: &str) -> &'static str {
    match path.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

/// Data Dragon image for a client asset path, when one maps cleanly.
fn ddragon_fallback(app: &AppHandle, path: &str) -> Option<String> {
    let dd = app.state::<AppState>().ddragon()?;
    let file = path.rsplit('/').next()?;
    let stem = file.split('.').next()?;
    if path.starts_with("/lol-game-data/assets/v1/champion-icons/") {
        let id = dd.champion_ids.get(&stem.parse().ok()?)?;
        return Some(dd.cdn_url(&format!("img/champion/{id}.png")));
    }
    if path.starts_with("/lol-game-data/assets/v1/profile-icons/") {
        let id: u32 = stem.parse().ok()?;
        return Some(dd.cdn_url(&format!("img/profileicon/{id}.png")));
    }
    if path.to_ascii_lowercase().contains("/items/icons2d/") {
        // Client item icons are named "<itemId>_<name>.png".
        let id: u32 = stem.split('_').next()?.parse().ok()?;
        return Some(dd.cdn_url(&format!("img/item/{id}.png")));
    }
    None
}

fn disk_path(app: &AppHandle, path: &str) -> PathBuf {
    app.state::<AppState>().cache_dir.join("assets").join(path.trim_start_matches('/'))
}

async fn load(app: &AppHandle, path: &str) -> Option<Entry> {
    let state = app.state::<AppState>();
    if let Some(hit) = state.assets.get(path) {
        return Some(hit);
    }
    let file = disk_path(app, path);
    let entry = if let Ok(bytes) = tokio::fs::read(&file).await {
        Some((Arc::new(bytes), content_type_for(path).to_owned()))
    } else {
        let fetched = match state.lcu_http() {
            Some(http) => http.get_bytes(path).await.ok().map(|(b, ct)| (b, ct.unwrap_or_else(|| content_type_for(path).into()))),
            None => None,
        };
        let fetched = match fetched {
            Some(f) => Some(f),
            None => fetch_fallback(app, path).await,
        };
        let (bytes, ct) = fetched?;
        if let Some(dir) = file.parent()
            && tokio::fs::create_dir_all(dir).await.is_ok()
            && let Err(e) = tokio::fs::write(&file, &bytes).await
        {
            tracing::debug!("couldn't cache asset {path}: {e}");
        }
        Some((Arc::new(bytes), ct))
    }?;
    state.assets.put(path.to_owned(), entry.0.clone(), entry.1.clone());
    Some(entry)
}

async fn fetch_fallback(app: &AppHandle, path: &str) -> Option<(Vec<u8>, String)> {
    let url = ddragon_fallback(app, path)?;
    let resp = app.state::<AppState>().public_http.get(url).send().await.ok()?.error_for_status().ok()?;
    let ct = resp.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("image/png").to_owned();
    Some((resp.bytes().await.ok()?.to_vec(), ct))
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder().status(code).body(Vec::new()).unwrap()
}

pub async fn handle(app: &AppHandle, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    if request.method() != tauri::http::Method::GET {
        return status(StatusCode::METHOD_NOT_ALLOWED);
    }
    let Some(path) = allowed_path(request.uri().path()) else {
        return status(StatusCode::FORBIDDEN);
    };
    match load(app, &path).await {
        Some((bytes, ct)) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, ct)
            .header(header::CACHE_CONTROL, "max-age=86400")
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(bytes.as_ref().clone())
            .unwrap(),
        None => status(StatusCode::NOT_FOUND),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_accepts_asset_paths() {
        assert_eq!(
            allowed_path("/lol-game-data/assets/v1/champion-icons/233.png").as_deref(),
            Some("/lol-game-data/assets/v1/champion-icons/233.png")
        );
        assert_eq!(
            allowed_path("%2Flol-game-data%2Fassets%2FASSETS%2FItems%2FIcons2D%2F1001_Class_T1_BootsofSpeed.png").as_deref(),
            Some("/lol-game-data/assets/ASSETS/Items/Icons2D/1001_Class_T1_BootsofSpeed.png")
        );
    }

    #[test]
    fn allowlist_rejects_non_asset_paths() {
        for bad in [
            "/lol-summoner/v1/current-summoner",
            "/lol-gameflow/v1/session",
            "/riotclient/region-locale",
            "/lol-game-data/assets/../../lol-summoner/v1/current-summoner",
            "/lol-game-data/assets/%2e%2e/%2e%2e/lol-login/v1/session",
            "/lol-game-data/assets/v1/x.png?y=1",
            "/lol-game-data/assets//x.png",
            "/lol-game-data/assetsX/x.png",
            "/lol-game-data/assets/v1/a b.png",
            "/lol-game-data/assets/%zz.png",
            "",
        ] {
            assert_eq!(allowed_path(bad), None, "{bad} should be rejected");
        }
    }

    #[test]
    fn lru_respects_the_byte_budget() {
        let cache = AssetCache::new();
        let chunk = MEMORY_BUDGET / 4 + 1;
        for i in 0..4 {
            cache.put(format!("k{i}"), Arc::new(vec![0; chunk]), "image/png".into());
        }
        assert!(cache.get("k0").is_none(), "oldest entry evicted");
        assert!(cache.get("k3").is_some());
        assert!(cache.inner.lock().unwrap().1 <= MEMORY_BUDGET);
    }

    #[test]
    fn content_types() {
        assert_eq!(content_type_for("/x/1.PNG"), "image/png");
        assert_eq!(content_type_for("/x/1.jpg"), "image/jpeg");
        assert_eq!(content_type_for("/x/1"), "application/octet-stream");
    }
}
