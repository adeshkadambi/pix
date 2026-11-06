//! The League client's lockfile: `LeagueClient:<pid>:<port>:<password>:<protocol>`.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

#[derive(Clone, PartialEq, Eq)]
pub struct Lockfile {
    pub pid: u32,
    pub port: u16,
    pub password: String,
    pub protocol: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LockfileError {
    #[error("expected 5 fields separated by ':', got {0}")]
    FieldCount(usize),
    #[error("invalid {0}")]
    Invalid(&'static str),
}

impl Lockfile {
    pub fn parse(text: &str) -> Result<Self, LockfileError> {
        let fields: Vec<&str> = text.trim().split(':').collect();
        let [_name, pid, port, password, protocol] = fields[..] else {
            return Err(LockfileError::FieldCount(fields.len()));
        };
        if password.is_empty() {
            return Err(LockfileError::Invalid("password"));
        }
        Ok(Self {
            pid: pid.parse().map_err(|_| LockfileError::Invalid("pid"))?,
            port: port.parse().map_err(|_| LockfileError::Invalid("port"))?,
            password: password.to_owned(),
            protocol: protocol.to_owned(),
        })
    }

    /// `Authorization` header value: `Basic base64("riot:<password>")`.
    pub fn basic_auth(&self) -> String {
        format!("Basic {}", STANDARD.encode(format!("riot:{}", self.password)))
    }

    pub fn https_base(&self) -> String {
        format!("https://127.0.0.1:{}", self.port)
    }

    pub fn wss_url(&self) -> String {
        format!("wss://127.0.0.1:{}/", self.port)
    }
}

// Never print the password, even in debug logs.
impl fmt::Debug for Lockfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lockfile")
            .field("pid", &self.pid)
            .field("port", &self.port)
            .field("password", &"<redacted>")
            .field("protocol", &self.protocol)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_lockfile() {
        let lf = Lockfile::parse("LeagueClient:12345:54321:s3cr3t-Pw_x:https\n").unwrap();
        assert_eq!(lf.pid, 12345);
        assert_eq!(lf.port, 54321);
        assert_eq!(lf.password, "s3cr3t-Pw_x");
        assert_eq!(lf.protocol, "https");
        assert_eq!(lf.https_base(), "https://127.0.0.1:54321");
        assert_eq!(lf.wss_url(), "wss://127.0.0.1:54321/");
    }

    #[test]
    fn builds_basic_auth() {
        let lf = Lockfile::parse("LeagueClient:1:2:pass:https").unwrap();
        // base64("riot:pass")
        assert_eq!(lf.basic_auth(), "Basic cmlvdDpwYXNz");
    }

    #[test]
    fn rejects_bad_lockfiles() {
        assert_eq!(Lockfile::parse(""), Err(LockfileError::FieldCount(1)));
        assert_eq!(Lockfile::parse("a:b:c"), Err(LockfileError::FieldCount(3)));
        assert_eq!(Lockfile::parse("LeagueClient:x:2:p:https"), Err(LockfileError::Invalid("pid")));
        assert_eq!(Lockfile::parse("LeagueClient:1:99999:p:https"), Err(LockfileError::Invalid("port")));
        assert_eq!(Lockfile::parse("LeagueClient:1:2::https"), Err(LockfileError::Invalid("password")));
    }

    #[test]
    fn debug_hides_password() {
        let lf = Lockfile::parse("LeagueClient:1:2:hunter2:https").unwrap();
        assert!(!format!("{lf:?}").contains("hunter2"));
    }
}
