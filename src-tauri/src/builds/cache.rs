//! Build data cache (SPEC §5.4): memory plus disk at `app_cache_dir/builds/<provider>/<patch>/`,
//! 24 hours. Cold OP.GG calls take ~0.3–1 s; prefetch plus this cache hides that.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

pub const TTL: Duration = Duration::from_secs(24 * 60 * 60);

pub struct BuildCache {
    root: PathBuf,
    ttl: Duration,
    memory: Mutex<HashMap<String, (SystemTime, Value)>>,
}

/// A file-safe cache key, e.g. `base_AHRI_mid`, `matchup_AHRI_mid_vs_ZED`, `traits_JINX`.
pub fn key(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|p| !p.is_empty())
        .map(|p| p.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>())
        .collect::<Vec<_>>()
        .join("_")
}

impl BuildCache {
    pub fn new(root: PathBuf) -> Self {
        Self { root, ttl: TTL, memory: Mutex::new(HashMap::new()) }
    }

    fn path(&self, provider: &str, patch: &str, key: &str) -> PathBuf {
        self.root.join(provider).join(if patch.is_empty() { "unknown" } else { patch }).join(format!("{key}.json"))
    }

    fn fresh(&self, at: SystemTime) -> bool {
        at.elapsed().map(|age| age < self.ttl).unwrap_or(false)
    }

    async fn read(&self, provider: &str, patch: &str, key: &str) -> Option<Value> {
        let mem_key = format!("{provider}/{patch}/{key}");
        if let Some((at, v)) = self.memory.lock().unwrap().get(&mem_key)
            && self.fresh(*at)
        {
            return Some(v.clone());
        }
        let path = self.path(provider, patch, key);
        let at = tokio::fs::metadata(&path).await.ok()?.modified().ok()?;
        if !self.fresh(at) {
            return None;
        }
        let v: Value = serde_json::from_str(&tokio::fs::read_to_string(&path).await.ok()?).ok()?;
        self.memory.lock().unwrap().insert(mem_key, (at, v.clone()));
        Some(v)
    }

    async fn write(&self, provider: &str, patch: &str, key: &str, v: &Value) {
        self.memory.lock().unwrap().insert(format!("{provider}/{patch}/{key}"), (SystemTime::now(), v.clone()));
        let path = self.path(provider, patch, key);
        if let Some(dir) = path.parent()
            && tokio::fs::create_dir_all(dir).await.is_ok()
            && let Err(e) = tokio::fs::write(&path, v.to_string()).await
        {
            tracing::debug!("build cache write failed for {key}: {e}");
        }
    }

    /// Cached value for `key`, or `fetch()` stored for next time.
    pub async fn get_or_fetch<T, E, F>(&self, provider: &str, patch: &str, key: &str, fetch: impl FnOnce() -> F) -> Result<T, E>
    where
        T: Serialize + DeserializeOwned,
        F: Future<Output = Result<T, E>>,
    {
        if let Some(v) = self.read(provider, patch, key).await
            && let Ok(t) = serde_json::from_value(v)
        {
            return Ok(t);
        }
        let value = fetch().await?;
        if let Ok(v) = serde_json::to_value(&value) {
            self.write(provider, patch, key, &v).await;
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn keys_are_file_safe() {
        assert_eq!(key(&["matchup", "KAISA", "adc", "vs", "KOGMAW"]), "matchup_KAISA_adc_vs_KOGMAW");
        assert_eq!(key(&["base", "A/B:C", "", "mid"]), "base_A-B-C_mid");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fetches_once_then_serves_from_memory_and_disk() {
        let dir = tempfile::tempdir().unwrap();
        let calls = AtomicU32::new(0);
        let fetch = || async {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok::<_, String>(vec![1u32, 2, 3])
        };
        let cache = BuildCache::new(dir.path().to_path_buf());
        assert_eq!(cache.get_or_fetch("opgg", "16.22", "k", fetch).await.unwrap(), vec![1, 2, 3]);
        assert_eq!(cache.get_or_fetch("opgg", "16.22", "k", fetch).await.unwrap(), vec![1, 2, 3]);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        // A new cache (fresh app start) reads the disk copy.
        let reopened = BuildCache::new(dir.path().to_path_buf());
        assert_eq!(reopened.get_or_fetch("opgg", "16.22", "k", fetch).await.unwrap(), vec![1, 2, 3]);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(dir.path().join("opgg/16.22/k.json").is_file());
        // Another patch is a separate entry.
        cache.get_or_fetch("opgg", "16.23", "k", fetch).await.unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn expired_entries_are_refetched_and_errors_are_not_cached() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = BuildCache::new(dir.path().to_path_buf());
        cache.ttl = Duration::ZERO;
        let calls = AtomicU32::new(0);
        let fetch = || async {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok::<_, String>(1u32)
        };
        cache.get_or_fetch("opgg", "p", "k", fetch).await.unwrap();
        cache.get_or_fetch("opgg", "p", "k", fetch).await.unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        let failing = || async { Err::<u32, _>("down".to_string()) };
        assert!(cache.get_or_fetch("opgg", "p", "e", failing).await.is_err());
        assert!(!dir.path().join("opgg/p/e.json").exists());
    }
}
