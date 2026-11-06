//! League client WebSocket (WAMP). Subscribe-only: we never send anything but subscriptions.

use futures_util::SinkExt;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

use super::lockfile::Lockfile;
use crate::riot_tls;

pub type LcuSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// WAMP opcode for subscribe, and for events the client pushes.
const WAMP_SUBSCRIBE: u8 = 5;
const WAMP_EVENT: u64 = 8;

pub const GAMEFLOW_PHASE: &str = "OnJsonApiEvent_lol-gameflow_v1_gameflow-phase";
pub const CHAMP_SELECT_SESSION: &str = "OnJsonApiEvent_lol-champ-select_v1_session";
const SUBSCRIPTIONS: [&str; 2] = [GAMEFLOW_PHASE, CHAMP_SELECT_SESSION];

#[derive(Debug, Clone, PartialEq)]
pub struct LcuEvent {
    pub uri: String,
    pub event_type: String,
    pub data: Value,
}

/// Parses `[8,"<event>",{"data":…,"eventType":"…","uri":"…"}]`. Anything else is ignored.
pub fn parse_frame(text: &str) -> Option<LcuEvent> {
    let frame: Value = serde_json::from_str(text).ok()?;
    let arr = frame.as_array()?;
    if arr.first()?.as_u64()? != WAMP_EVENT {
        return None;
    }
    let payload = arr.get(2)?;
    Some(LcuEvent {
        uri: payload.get("uri")?.as_str()?.to_owned(),
        event_type: payload.get("eventType").and_then(Value::as_str).unwrap_or_default().to_owned(),
        data: payload.get("data").cloned().unwrap_or(Value::Null),
    })
}

pub async fn connect(lockfile: &Lockfile) -> Result<LcuSocket, tokio_tungstenite::tungstenite::Error> {
    let mut request = lockfile.wss_url().into_client_request()?;
    request.headers_mut().insert(
        "Authorization",
        HeaderValue::from_str(&lockfile.basic_auth()).expect("base64 is a valid header value"),
    );
    let connector = Connector::Rustls(riot_tls::riot_config());
    let (mut socket, _) =
        tokio_tungstenite::connect_async_tls_with_config(request, None, false, Some(connector)).await?;
    for event in SUBSCRIPTIONS {
        socket.send(Message::text(format!("[{WAMP_SUBSCRIBE},\"{event}\"]"))).await?;
    }
    Ok(socket)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_event_frame() {
        let text = r#"[8,"OnJsonApiEvent_lol-gameflow_v1_gameflow-phase",{"data":"ChampSelect","eventType":"Update","uri":"/lol-gameflow/v1/gameflow-phase"}]"#;
        let ev = parse_frame(text).unwrap();
        assert_eq!(ev.uri, "/lol-gameflow/v1/gameflow-phase");
        assert_eq!(ev.event_type, "Update");
        assert_eq!(ev.data, Value::String("ChampSelect".into()));
    }

    #[test]
    fn ignores_other_frames() {
        assert_eq!(parse_frame(""), None);
        assert_eq!(parse_frame("not json"), None);
        assert_eq!(parse_frame(r#"[0,"session","1",""]"#), None);
        assert_eq!(parse_frame(r#"[8,"x"]"#), None);
        assert_eq!(parse_frame(r#"{"uri":"/x"}"#), None);
    }

    #[test]
    fn delete_event_has_null_data() {
        let ev = parse_frame(r#"[8,"e",{"eventType":"Delete","uri":"/lol-champ-select/v1/session"}]"#).unwrap();
        assert_eq!(ev.event_type, "Delete");
        assert_eq!(ev.data, Value::Null);
    }
}
