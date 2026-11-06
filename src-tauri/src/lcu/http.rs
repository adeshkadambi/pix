//! League client HTTP API: GET only (compliance rule 2). CI fails if this file sends anything else.

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::de::DeserializeOwned;

use super::lockfile::Lockfile;

#[derive(Debug, thiserror::Error)]
pub enum LcuError {
    #[error("League client request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("League client returned {status} for {path}")]
    Status { status: u16, path: String },
}

#[derive(Clone)]
pub struct LcuHttp {
    client: reqwest::Client,
    base: String,
    auth: String,
}

impl LcuHttp {
    pub fn new(client: reqwest::Client, lockfile: &Lockfile) -> Self {
        Self { client, base: lockfile.https_base(), auth: lockfile.basic_auth() }
    }

    async fn get(&self, path: &str) -> Result<reqwest::Response, LcuError> {
        let resp = self
            .client
            .get(format!("{}{}", self.base, path))
            .header(AUTHORIZATION, &self.auth)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(LcuError::Status { status: resp.status().as_u16(), path: path.to_owned() });
        }
        Ok(resp)
    }

    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, LcuError> {
        Ok(self.get(path).await?.json().await?)
    }

    /// Raw bytes plus the upstream content type, for the asset protocol.
    pub async fn get_bytes(&self, path: &str) -> Result<(Vec<u8>, Option<String>), LcuError> {
        let resp = self.get(path).await?;
        let content_type =
            resp.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_owned);
        Ok((resp.bytes().await?.to_vec(), content_type))
    }
}
