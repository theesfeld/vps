//! Client TOML (`~/.config/vps/config.toml`).

use std::collections::HashMap;
use std::path::PathBuf;

use iced::Font;
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub ssh: Ssh,
    pub window: Window,
    pub font: FontCfg,
    pub term: Term,
    pub colors: Colors,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Ssh {
    /// OpenSSH host alias (`Host` in `~/.ssh/config`).
    pub host: String,
    /// Extra argv inserted after `ssh` and before `host` (e.g. `-tt`).
    pub args: Vec<String>,
    /// Remote command as **one** string. OpenSSH runs `$SHELL -c "<this>"`.
    pub remote: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Window {
    pub width: f32,
    pub height: f32,
    /// Wayland app id (niri `app-id`).
    pub app_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FontCfg {
    /// fontconfig family. Empty → iced built-in monospace.
    pub family: String,
    /// Glyph size in pixels (iced `Pixels`).
    pub size: f32,
    /// Line height as a multiple of `size`.
    pub scale: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Term {
    pub term: String,
    pub colorterm: String,
}

#[derive(Debug, Clone, Deserialize)]
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
            family: "Berkeley Mono".into(),
            size: 18.0,
            scale: 1.3,
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

    pub fn ssh_argv(&self) -> Vec<String> {
        let mut a = self.ssh.args.clone();
        a.push(self.ssh.host.clone());
        a.push(self.ssh.remote.clone());
        a
    }

    pub fn iced_font(&self) -> Font {
        if self.font.family.trim().is_empty() {
            Font::MONOSPACE
        } else {
            Font::with_name(Box::leak(self.font.family.clone().into_boxed_str()))
        }
    }

    pub fn term_env(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert("TERM".into(), self.term.term.clone());
        env.insert("COLORTERM".into(), self.term.colorterm.clone());
        env
    }

    pub fn palette(&self) -> iced_term::ColorPalette {
        let c = &self.colors;
        iced_term::ColorPalette {
            foreground: c.foreground.clone(),
            background: c.background.clone(),
            black: c.black.clone(),
            red: c.red.clone(),
            green: c.green.clone(),
            yellow: c.yellow.clone(),
            blue: c.blue.clone(),
            magenta: c.magenta.clone(),
            cyan: c.cyan.clone(),
            white: c.white.clone(),
            bright_black: c.bright_black.clone(),
            bright_red: c.bright_red.clone(),
            bright_green: c.bright_green.clone(),
            bright_yellow: c.bright_yellow.clone(),
            bright_blue: c.bright_blue.clone(),
            bright_magenta: c.bright_magenta.clone(),
            bright_cyan: c.bright_cyan.clone(),
            bright_white: c.bright_white.clone(),
            bright_foreground: c.bright_foreground.clone(),
            dim_foreground: c.dim_foreground.clone(),
            dim_black: c.dim_black.clone(),
            dim_red: c.dim_red.clone(),
            dim_green: c.dim_green.clone(),
            dim_yellow: c.dim_yellow.clone(),
            dim_blue: c.dim_blue.clone(),
            dim_magenta: c.dim_magenta.clone(),
            dim_cyan: c.dim_cyan.clone(),
            dim_white: c.dim_white.clone(),
        }
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
    }

    #[test]
    fn ssh_argv_is_tt_host_remote() {
        let cfg = Config::default();
        let argv = cfg.ssh_argv();
        assert_eq!(argv.first().map(String::as_str), Some("-tt"));
        assert_eq!(argv.get(1).map(String::as_str), Some("grok"));
        assert!(argv[2].contains("vpsd attach"));
    }
}
