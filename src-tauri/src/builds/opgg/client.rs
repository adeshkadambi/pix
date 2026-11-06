//! Minimal MCP client for `https://mcp-api.op.gg/mcp` (SPEC §5.4): JSON-RPC 2.0 over Streamable
//! HTTP. Session: `initialize` → `notifications/initialized` → `tools/call`. Replies may be plain
//! JSON or SSE. Hand-rolled instead of `rmcp`: we only need `tools/call`.

use std::sync::atomic::{AtomicU64, Ordering};

use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::builds::ProviderError;

const ENDPOINT: &str = "https://mcp-api.op.gg/mcp";
const PROTOCOL_VERSION: &str = "2025-06-18";
const SESSION_HEADER: &str = "Mcp-Session-Id";

pub struct McpClient {
    http: reqwest::Client,
    endpoint: String,
    session: Mutex<Option<String>>,
    next_id: AtomicU64,
}

/// Finds the JSON-RPC response with `id` in a plain JSON or SSE body.
pub fn extract_response(content_type: &str, body: &str, id: u64) -> Option<Value> {
    let matches = |v: &Value| v.get("id").and_then(Value::as_u64) == Some(id);
    if content_type.contains("text/event-stream") {
        // Each event's data lines are joined; the response is the event whose JSON has our id.
        let mut data = String::new();
        let mut found = None;
        for line in body.lines().chain(std::iter::once("")) {
            if let Some(rest) = line.strip_prefix("data:") {
                data.push_str(rest.trim_start());
            } else if line.trim().is_empty() && !data.is_empty() {
                if let Ok(v) = serde_json::from_str::<Value>(&data)
                    && matches(&v)
                {
                    found = Some(v);
                }
                data.clear();
            }
        }
        found
    } else {
        serde_json::from_str::<Value>(body).ok().filter(matches)
    }
}

/// The text content of a `tools/call` result, or the error it carries.
pub fn tool_text(response: &Value) -> Result<String, ProviderError> {
    if let Some(err) = response.get("error") {
        let msg = err.get("message").and_then(Value::as_str).unwrap_or("unknown error");
        return Err(ProviderError::Remote(msg.to_owned()));
    }
    let result = response.get("result").ok_or_else(|| ProviderError::Format("no result".into()))?;
    let text = result
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::Format("no text content".into()))?;
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err(ProviderError::Remote(text.to_owned()));
    }
    Ok(text.to_owned())
}

impl McpClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http, endpoint: ENDPOINT.to_owned(), session: Mutex::new(None), next_id: AtomicU64::new(1) }
    }

    async fn post(&self, session: Option<&str>, body: &Value) -> Result<(u16, HeaderMap, String), ProviderError> {
        let mut req = self
            .http
            .post(&self.endpoint)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .json(body);
        if let Some(id) = session {
            req = req.header(SESSION_HEADER, id);
        }
        let resp = req.send().await?;
        let status = resp.status().as_u16();
        let headers = resp.headers().clone();
        Ok((status, headers, resp.text().await?))
    }

    async fn request(&self, session: Option<&str>, method: &str, params: Value) -> Result<(Value, HeaderMap), ProviderError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let (status, headers, text) = self.post(session, &body).await?;
        if status == 404 || status == 400 {
            return Err(ProviderError::Remote(format!("session rejected ({status})")));
        }
        if !(200..300).contains(&status) {
            return Err(ProviderError::Remote(format!("HTTP {status}")));
        }
        let ct = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
        let value = extract_response(&ct, &text, id).ok_or_else(|| ProviderError::Format(format!("no reply to {method}")))?;
        Ok((value, headers))
    }

    async fn open_session(&self) -> Result<String, ProviderError> {
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "pix", "version": env!("CARGO_PKG_VERSION")}
        });
        let (value, headers) = self.request(None, "initialize", params).await?;
        if let Some(err) = value.get("error") {
            return Err(ProviderError::Remote(err.to_string()));
        }
        let id = headers
            .get(SESSION_HEADER)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ProviderError::Format("no session id".into()))?
            .to_owned();
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        self.post(Some(&id), &note).await?;
        Ok(id)
    }

    async fn session(&self) -> Result<String, ProviderError> {
        let mut slot = self.session.lock().await;
        if let Some(id) = slot.as_ref() {
            return Ok(id.clone());
        }
        let id = self.open_session().await?;
        *slot = Some(id.clone());
        Ok(id)
    }

    /// Calls a tool and returns its text. Reopens the session once if the server dropped it.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<String, ProviderError> {
        let params = json!({"name": name, "arguments": arguments});
        for attempt in 0..2 {
            let session = self.session().await?;
            match self.request(Some(&session), "tools/call", params.clone()).await {
                Err(ProviderError::Remote(msg)) if attempt == 0 && msg.starts_with("session rejected") => {
                    *self.session.lock().await = None;
                }
                Err(e) => return Err(e),
                Ok((value, _)) => return tool_text(&value),
            }
        }
        Err(ProviderError::Remote("session rejected twice".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_plain_json_reply() {
        let body = r#"{"jsonrpc":"2.0","id":7,"result":{"content":[{"type":"text","text":"hi"}]}}"#;
        let v = extract_response("application/json", body, 7).unwrap();
        assert_eq!(tool_text(&v).unwrap(), "hi");
        assert!(extract_response("application/json", body, 8).is_none());
    }

    #[test]
    fn extracts_sse_reply() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n\
                    event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":3,\n\
                    data: \"result\":{\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]}}\n\n";
        let v = extract_response("text/event-stream; charset=utf-8", body, 3).unwrap();
        assert_eq!(tool_text(&v).unwrap(), "ok");
    }

    #[test]
    fn surfaces_errors() {
        let rpc_err = serde_json::json!({"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"Unknown champion provided."}});
        assert!(matches!(tool_text(&rpc_err), Err(ProviderError::Remote(m)) if m == "Unknown champion provided."));
        let tool_err = serde_json::json!({"jsonrpc":"2.0","id":1,"result":{"isError":true,"content":[{"type":"text","text":"bad args"}]}});
        assert!(matches!(tool_text(&tool_err), Err(ProviderError::Remote(m)) if m == "bad args"));
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "calls the live OP.GG MCP API"]
    async fn live_opgg_call() {
        crate::riot_tls::install_crypto_provider();
        let client = McpClient::new(crate::riot_tls::public_http_client());
        let text = client
            .call_tool("lol_get_champion_analysis", serde_json::json!({
                "game_mode": "ranked", "champion": "AHRI", "position": "mid",
                "desired_output_fields": ["data.damage_type"]
            }))
            .await
            .unwrap();
        assert!(text.contains("\"AP\""), "{text}");
    }
}
