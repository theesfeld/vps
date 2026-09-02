//! Daemon TOML (`~/.config/vps/vpsd.toml`).

use std::path::{Path, PathBuf};

use serde::Deserialize;
use vps_protocol::default_socket_path;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    /// Unix socket path. Empty → `$XDG_RUNTIME_DIR/vpsd.sock`.
    pub listen: String,
    /// Login shell spawned on each new PTY.
    pub shell: String,
    /// Socket file mode as an octal string (`"0600"`). Never world-accessible.
    pub socket_mode: String,
    /// PTY output kept for reattach, in bytes. Oldest data is dropped.
    pub scrollback_bytes: usize,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            listen: String::new(),
            shell: "/bin/bash".into(),
            socket_mode: "0600".into(),
            scrollback_bytes: 2 * 1024 * 1024,
        }
    }
}

impl DaemonConfig {
    pub fn load() -> Self {
        let path = config_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_default()
    }

    pub fn socket_path(&self) -> PathBuf {
        if self.listen.trim().is_empty() {
            default_socket_path()
        } else {
            PathBuf::from(&self.listen)
        }
    }

    pub fn mode_bits(&self) -> u32 {
        u32::from_str_radix(self.socket_mode.trim_start_matches('0'), 8).unwrap_or(0o600)
    }
}

fn config_path() -> PathBuf {
    config_dir().join("vpsd.toml")
}

fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return Path::new(&dir).join("vps");
        }
    }
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".config/vps"))
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_default_toml_parses() {
        let text = include_str!("../../../config/vpsd.toml");
        let cfg: DaemonConfig = toml::from_str(text).expect("config/vpsd.toml");
        assert_eq!(cfg.shell, "/bin/bash");
        assert_eq!(cfg.socket_mode, "0600");
        assert_eq!(cfg.mode_bits(), 0o600);
        assert_eq!(cfg.scrollback_bytes, 2 * 1024 * 1024);
    }
}
