//! Control-plane messages and listen-address policy for vpsd.
//!
//! PTY bytes after attach are raw. Control is JSON (one object per line).
//! The daemon never binds TCP or UDP.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const SOCKET_NAME: &str = "vpsd.sock";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Message {
    Hello { v: u32 },
    Open { cols: u16, rows: u16 },
    Resize { cols: u16, rows: u16 },
    Close,
    Exit { code: i32 },
    Error { msg: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listen {
    /// Attach stream (SSH stdio / a tty). Not a bind.
    Stdio,
    /// Filesystem socket. Mode 0600. Never a network address.
    Unix(PathBuf),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ListenError {
    #[error("vpsd never listens on TCP or UDP (got {0})")]
    Network(String),
    #[error("empty listen address")]
    Empty,
}

impl Listen {
    /// Parse a listen spec. `stdio`, `unix:/path`, or an absolute path.
    /// Anything that looks like a network bind is an error.
    pub fn parse(spec: &str) -> Result<Self, ListenError> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err(ListenError::Empty);
        }
        let lower = spec.to_ascii_lowercase();
        if lower == "stdio" || lower == "-" {
            return Ok(Listen::Stdio);
        }
        if looks_like_network(spec) {
            return Err(ListenError::Network(spec.to_string()));
        }
        let path = if let Some(rest) = spec.strip_prefix("unix:") {
            PathBuf::from(rest)
        } else {
            PathBuf::from(spec)
        };
        if path.as_os_str().is_empty() {
            return Err(ListenError::Empty);
        }
        Ok(Listen::Unix(path))
    }

    pub fn is_bind(&self) -> bool {
        matches!(self, Listen::Unix(_))
    }
}

fn looks_like_network(spec: &str) -> bool {
    let s = spec.trim();
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("tcp:")
        || lower.starts_with("tcp://")
        || lower.starts_with("udp:")
        || lower.starts_with("udp://")
        || lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with('[')
    {
        return true;
    }
    if lower.starts_with("unix:") || s.starts_with('/') || s.starts_with('.') {
        return false;
    }
    // host:port or :port (no slash → not a path)
    if s.contains(':') && !s.contains('/') {
        return true;
    }
    false
}

pub fn default_socket_path() -> PathBuf {
    runtime_dir().join(SOCKET_NAME)
}

fn runtime_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    PathBuf::from("/tmp")
}

pub fn encode_line(msg: &Message) -> Result<String, serde_json::Error> {
    let mut line = serde_json::to_string(msg)?;
    line.push('\n');
    Ok(line)
}

pub fn decode_line(line: &str) -> Result<Message, serde_json::Error> {
    serde_json::from_str(line.trim())
}

pub fn socket_is_unix_path(path: &Path) -> bool {
    path.is_absolute() || path.starts_with(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listen_stdio() {
        assert_eq!(Listen::parse("stdio").unwrap(), Listen::Stdio);
        assert_eq!(Listen::parse("-").unwrap(), Listen::Stdio);
    }

    #[test]
    fn listen_unix_path() {
        match Listen::parse("/run/user/1000/vpsd.sock").unwrap() {
            Listen::Unix(p) => assert_eq!(p, PathBuf::from("/run/user/1000/vpsd.sock")),
            other => panic!("{other:?}"),
        }
        match Listen::parse("unix:/tmp/vpsd.sock").unwrap() {
            Listen::Unix(p) => assert_eq!(p, PathBuf::from("/tmp/vpsd.sock")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn listen_rejects_tcp() {
        for spec in [
            "tcp:127.0.0.1:9",
            "tcp://0.0.0.0:2022",
            "0.0.0.0:2022",
            "127.0.0.1:9",
            ":2022",
            "udp:0.0.0.0:60001",
            "[::]:9",
        ] {
            match Listen::parse(spec) {
                Err(ListenError::Network(_)) => {}
                other => panic!("{spec} should reject, got {other:?}"),
            }
        }
    }

    #[test]
    fn json_roundtrip_open() {
        let msg = Message::Open { cols: 80, rows: 24 };
        let line = encode_line(&msg).unwrap();
        assert!(line.ends_with('\n'));
        assert_eq!(decode_line(&line).unwrap(), msg);
    }

    #[test]
    fn json_hello_tag() {
        let line = encode_line(&Message::Hello { v: VERSION }).unwrap();
        assert!(line.contains("\"t\":\"hello\""));
        assert!(line.contains("\"v\":1"));
    }
}
