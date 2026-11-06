//! League client connection (SPEC §5.1).

pub mod catalog;
pub mod champ_select;
pub mod discovery;
pub mod gameflow;
pub mod http;
pub mod lockfile;
pub mod supervisor;
pub mod ws;

#[cfg(test)]
mod live_tests {
    //! Run against a real, running League client:
    //! `cargo test --manifest-path src-tauri/Cargo.toml -- --ignored live_`
    use super::*;
    use crate::riot_tls;
    use futures_util::StreamExt;

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "needs a running League client"]
    async fn live_connects_to_client() {
        riot_tls::install_crypto_provider();
        let path = discovery::locate(None).expect("League client lockfile not found");
        let lf = lockfile::Lockfile::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        let http = http::LcuHttp::new(riot_tls::local_http_client(), &lf);
        let phase: String = http.get_json("/lol-gameflow/v1/gameflow-phase").await.unwrap();
        println!("gameflow phase: {phase}");
        let mut socket = ws::connect(&lf).await.expect("websocket connects");
        // The subscribe calls succeeded; poll briefly to prove the socket stays open.
        let next = tokio::time::timeout(std::time::Duration::from_millis(500), socket.next()).await;
        println!("websocket open; first frame within 500ms: {}", next.is_ok());
    }

    /// Saves the profile endpoints' JSON to `$PIX_PROBE_DIR` for building fixtures.
    #[tokio::test(flavor = "current_thread")]
    #[ignore = "needs a running League client"]
    async fn live_probe_profile_endpoints() {
        riot_tls::install_crypto_provider();
        let dir = std::path::PathBuf::from(std::env::var("PIX_PROBE_DIR").expect("set PIX_PROBE_DIR"));
        let path = discovery::locate(None).expect("League client lockfile not found");
        let lf = lockfile::Lockfile::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        let http = http::LcuHttp::new(riot_tls::local_http_client(), &lf);
        for (name, ep) in [
            ("summoner", "/lol-summoner/v1/current-summoner"),
            ("region", "/riotclient/region-locale"),
            ("ranked", "/lol-ranked/v1/current-ranked-stats"),
            ("history", "/lol-match-history/v1/products/lol/current-summoner/matches?begIndex=0&endIndex=20"),
            ("version", "/lol-patch/v1/game-version"),
            ("champions", "/lol-game-data/assets/v1/champion-summary.json"),
            ("items", "/lol-game-data/assets/v1/items.json"),
            ("spells", "/lol-game-data/assets/v1/summoner-spells.json"),
        ] {
            let v: serde_json::Value = http.get_json(ep).await.unwrap();
            std::fs::write(dir.join(format!("{name}.json")), serde_json::to_string_pretty(&v).unwrap()).unwrap();
            println!("saved {name}");
        }
    }
}
