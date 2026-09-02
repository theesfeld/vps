//! Client TOML (`~/.config/vps/config.toml`).

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy)]
pub enum AttachSpec {
    New,
    Id(u64),
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub ssh: Ssh,
    pub picker: Picker,
    pub window: Window,
    pub font: FontCfg,
    pub term: Term,
    pub colors: Colors,
    pub terminal: Terminal,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Terminal {
    /// Session window. Empty → first-run chooser. iced is the picker only.
    pub program: String,
    /// Extra argv after the emulator's own flags and before `ssh`.
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Ssh {
    /// OpenSSH host alias (`Host` in `~/.ssh/config`).
    pub host: String,
    /// Extra argv inserted after `ssh` and before `host` (e.g. `-tt`).
    pub args: Vec<String>,
    /// Remote attach command as **one** string. OpenSSH runs `$SHELL -c "<this>"`.
    /// `vps` appends ` --new` or ` --id N`.
    pub remote: String,
    /// Extra argv for the list hop (`ssh <list_args...> <host> <list>`). No tty.
    pub list_args: Vec<String>,
    /// Remote list command as **one** string.
    pub list: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Picker {
    /// `when_sessions` — menu if any PTYs exist; `always`; `never` (always `--new`).
    pub mode: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Window {
    pub width: f32,
    pub height: f32,
    /// Wayland app id (niri `app-id`).
    pub app_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct FontCfg {
    /// fontconfig family. Empty → fontconfig `monospace` (kitty's default stack).
    pub family: String,
    /// Glyph size in pixels (iced `Pixels`). kitty IRONGALL is 14.
    pub size: f32,
    /// Line height as a multiple of `size`.
    pub scale: f32,
    /// Extra fontconfig families to load (nerd/symbol glyphs).
    pub extras: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Term {
    pub term: String,
    pub colorterm: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Colors {
    pub foreground: String,
    pub background: String,
    pub black: String,
    pub red: String,
    pub green: String,
    pub yellow: String,
    pub blue: String,
    pub magenta: String,
    pub cyan: String,
    pub white: String,
    pub bright_black: String,
    pub bright_red: String,
    pub bright_green: String,
    pub bright_yellow: String,
    pub bright_blue: String,
    pub bright_magenta: String,
    pub bright_cyan: String,
    pub bright_white: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bright_foreground: Option<String>,
    pub dim_foreground: String,
    pub dim_black: String,
    pub dim_red: String,
    pub dim_green: String,
    pub dim_yellow: String,
    pub dim_blue: String,
    pub dim_magenta: String,
    pub dim_cyan: String,
    pub dim_white: String,
}

impl Default for Ssh {
    fn default() -> Self {
        Self {
            host: "grok".into(),
            args: vec!["-tt".into()],
            remote: "/home/tj/.local/bin/vpsd attach".into(),
            list_args: vec!["-T".into()],
            list: "/home/tj/.local/bin/vpsd list".into(),
        }
    }
}

impl Default for Picker {
    fn default() -> Self {
        Self {
            mode: "when_sessions".into(),
        }
    }
}

impl Default for Window {
    fn default() -> Self {
        Self {
            width: 1280.0,
            height: 800.0,
            app_id: "vps".into(),
        }
    }
}

impl Default for FontCfg {
    fn default() -> Self {
        Self {
            family: String::new(),
            size: 14.0,
            scale: 1.3,
            extras: vec!["MesloLGS Nerd Font".into()],
        }
    }
}

impl Default for Term {
    fn default() -> Self {
        Self {
            term: "xterm-256color".into(),
            colorterm: "truecolor".into(),
        }
    }
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            foreground: "#ede6de".into(),
            background: "#241018".into(),
            black: "#241018".into(),
            red: "#e03818".into(),
            green: "#3d9650".into(),
            yellow: "#e0bc55".into(),
            blue: "#1e8ae8".into(),
            magenta: "#d47a82".into(),
            cyan: "#16a8b6".into(),
            white: "#ede6de".into(),
            bright_black: "#4a2830".into(),
            bright_red: "#e5563b".into(),
            bright_green: "#5aa66a".into(),
            bright_yellow: "#e5c66f".into(),
            bright_blue: "#409ceb".into(),
            bright_magenta: "#da8e95".into(),
            bright_cyan: "#39b5c1".into(),
            bright_white: "#b8c0c8".into(),
            bright_foreground: None,
            dim_foreground: "#8e7a76".into(),
            dim_black: "#14080c".into(),
            dim_red: "#8a2010".into(),
            dim_green: "#245830".into(),
            dim_yellow: "#8a7030".into(),
            dim_blue: "#145080".into(),
            dim_magenta: "#804850".into(),
            dim_cyan: "#0c6870".into(),
            dim_white: "#6e6460".into(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("vps: {path}: {e} — using defaults", path = path.display());
            Self::default()
        })
    }

    pub fn save(&self) -> Result<PathBuf, String> {
        let path = config_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}", dir = dir.display()))?;
        }
        let body = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        let text = format!(
            "# written by `vps settings`. Comments live in the repo copy:\n\
             # https://github.com/theesfeld/vps/blob/main/config/config.toml\n\
             # Font family changes apply the next time vps starts.\n\n\
             {body}"
        );
        std::fs::write(&path, text).map_err(|e| format!("{path}: {e}", path = path.display()))?;
        Ok(path)
    }

    pub fn ssh_attach_argv(&self, spec: AttachSpec) -> Vec<String> {
        let mut a = self.ssh.args.clone();
        a.push(self.ssh.host.clone());
        let cmd = match spec {
            AttachSpec::New => format!("{} --new", self.ssh.remote),
            AttachSpec::Id(id) => format!("{} --id {id}", self.ssh.remote),
        };
        a.push(cmd);
        a
    }

    pub fn ssh_list_argv(&self) -> Vec<String> {
        let mut a = self.ssh.list_args.clone();
        a.push(self.ssh.host.clone());
        a.push(self.ssh.list.clone());
        a
    }

    pub fn want_picker(&self, session_count: usize) -> bool {
        match self.picker.mode.as_str() {
            "never" => false,
            "always" => true,
            _ => session_count > 0,
        }
    }

    pub fn term_env(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert("TERM".into(), self.term.term.clone());
        env.insert("COLORTERM".into(), self.term.colorterm.clone());
        env
    }
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("vps");
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
        let text = include_str!("../../../config/config.toml");
        let cfg: Config = toml::from_str(text).expect("config/config.toml");
        assert_eq!(cfg.ssh.host, "grok");
        assert!(!cfg.ssh.remote.is_empty());
        assert_eq!(cfg.colors.background, "#241018");
        assert!(cfg.font.size > 0.0);
        assert!(
            cfg.terminal.program.is_empty(),
            "empty program triggers the first-run chooser"
        );
    }

    #[test]
    fn ssh_argv_is_tt_host_remote() {
        let cfg = Config::default();
        let argv = cfg.ssh_attach_argv(AttachSpec::New);
        assert_eq!(argv.first().map(String::as_str), Some("-tt"));
        assert_eq!(argv.get(1).map(String::as_str), Some("grok"));
        assert!(argv[2].contains("vpsd attach --new"), "{}", argv[2]);
        let id = cfg.ssh_attach_argv(AttachSpec::Id(4));
        assert!(id[2].contains("--id 4"), "{}", id[2]);
        let list = cfg.ssh_list_argv();
        assert_eq!(list.first().map(String::as_str), Some("-T"));
        assert!(list[2].contains("vpsd list"));
    }

    #[test]
    fn save_roundtrip_toml() {
        let dir = std::env::temp_dir().join(format!("vps-cfg-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        let mut cfg = Config::default();
        cfg.ssh.host = "example".into();
        cfg.font.size = 16.0;
        let body = toml::to_string_pretty(&cfg).unwrap();
        std::fs::write(&path, body).unwrap();
        let loaded: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.ssh.host, "example");
        assert_eq!(loaded.font.size, 16.0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
