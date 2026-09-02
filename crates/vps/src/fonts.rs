//! Load the fontconfig monospace into iced (the picker, not the TTY).
//!
//! iced does not read fontconfig by itself. `Font::with_name` is only a label;
//! the TTF/OTF bytes have to be registered with `.font(...)`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::config::FontCfg;

pub struct Loaded {
    pub family: &'static str,
    pub faces: Vec<&'static [u8]>,
}

static LOADED: OnceLock<Loaded> = OnceLock::new();

pub fn load(cfg: &FontCfg) -> &'static Loaded {
    LOADED.get_or_init(|| load_inner(cfg))
}

fn load_inner(cfg: &FontCfg) -> Loaded {
    let query = if cfg.family.trim().is_empty() {
        "monospace"
    } else {
        cfg.family.trim()
    };
    let family = fc_format("%{family}", query)
        .and_then(|s| s.split(',').next().map(|p| p.trim().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| query.to_string());
    let family: &'static str = Box::leak(family.into_boxed_str());

    let mut paths: Vec<PathBuf> = Vec::new();
    for pattern in [
        query.to_string(),
        format!("{query}:style=Bold"),
        format!("{query}:style=Oblique"),
        format!("{query}:style=Italic"),
        format!("{query}:style=Bold Oblique"),
        format!("{query}:style=Bold Italic"),
    ] {
        if let Some(p) = fc_file(&pattern) {
            if !paths.iter().any(|e| e == &p) {
                paths.push(p);
            }
        }
    }
    for extra in &cfg.extras {
        if extra.trim().is_empty() {
            continue;
        }
        if let Some(p) = fc_file(extra.trim()) {
            if !paths.iter().any(|e| e == &p) {
                paths.push(p);
            }
        }
    }

    let mut faces = Vec::new();
    for path in paths {
        if let Some(bytes) = read_leak(&path) {
            faces.push(bytes);
        }
    }
    Loaded { family, faces }
}

fn fc_file(pattern: &str) -> Option<PathBuf> {
    let s = fc_format("%{file}", pattern)?;
    let p = PathBuf::from(s);
    p.is_file().then_some(p)
}

fn fc_format(fmt: &str, pattern: &str) -> Option<String> {
    let out = Command::new("fc-match")
        .args(["-f", fmt, pattern])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn read_leak(path: &Path) -> Option<&'static [u8]> {
    let bytes = std::fs::read(path).ok()?;
    Some(Box::leak(bytes.into_boxed_slice()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fc_match_monospace_is_a_real_file() {
        let p = fc_file("monospace").expect("fontconfig monospace");
        assert!(p.is_file(), "{}", p.display());
    }

    #[test]
    fn empty_family_resolves_system_monospace() {
        let loaded = load_inner(&FontCfg {
            family: String::new(),
            size: 14.0,
            scale: 1.3,
            extras: Vec::new(),
        });
        assert!(!loaded.family.is_empty());
        assert!(
            !loaded.faces.is_empty(),
            "expected at least one face for system monospace"
        );
    }
}
