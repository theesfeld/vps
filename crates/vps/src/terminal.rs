//! Spawn the user's terminal with `ssh … vpsd attach`.
//!
//! iced is the picker only. Grok's alt-screen flood kills iced_term; a real
//! tty does not. Flags below are from each emulator's current official CLI.

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::{AttachSpec, Config};

/// Basenames we know how to launch with a documented class/app-id flag.
pub const KNOWN: &[&str] = &["kitty", "foot", "alacritty", "ghostty", "wezterm"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    pub name: String,
    pub path: PathBuf,
}

pub fn needs_chooser(cfg: &Config) -> bool {
    !program_is_runnable(cfg.terminal.program.trim())
}

pub fn program_is_runnable(program: &str) -> bool {
    !program.is_empty() && resolve_program(program).is_ok()
}

/// Why the chooser is showing. `None` if `program` is empty (first run) or fine.
pub fn missing_terminal_message(cfg: &Config) -> Option<String> {
    let program = cfg.terminal.program.trim();
    if program.is_empty() {
        return None;
    }
    resolve_program(program)
        .err()
        .map(|e| format!("{e} — pick another terminal"))
}

pub fn detect() -> Vec<Detected> {
    detect_in(&std::env::var("PATH").unwrap_or_default())
}

pub fn detect_in(path: &str) -> Vec<Detected> {
    let dirs: Vec<PathBuf> = std::env::split_paths(path).collect();
    let mut out = Vec::new();
    for name in KNOWN {
        for dir in &dirs {
            let p = dir.join(name);
            if is_runnable(&p) {
                out.push(Detected {
                    name: (*name).to_string(),
                    path: p,
                });
                break;
            }
        }
    }
    out
}

pub fn attach_argv(cfg: &Config, spec: AttachSpec) -> Result<Vec<String>, String> {
    let program = cfg.terminal.program.trim();
    if program.is_empty() {
        return Err("no terminal chosen — pick one (t) or open settings".into());
    }
    let name = basename(program);
    let ssh = ssh_cmd(cfg, spec);
    let extra = &cfg.terminal.args;
    let mut a = vec![program.to_string()];
    match name {
        "kitty" => {
            a.extend(kitty_flags(cfg));
            a.extend(extra.iter().cloned());
            a.extend(ssh);
        }
        "foot" | "footclient" => {
            a.extend(foot_flags(cfg));
            a.extend(extra.iter().cloned());
            a.extend(ssh);
        }
        "alacritty" => {
            a.push("--class".into());
            a.push(cfg.window.app_id.clone());
            a.extend(extra.iter().cloned());
            a.push("-e".into());
            a.extend(ssh);
        }
        "ghostty" => {
            a.push(format!("--class={}", cfg.window.app_id));
            a.push(format!("--background={}", cfg.colors.background));
            a.push(format!("--foreground={}", cfg.colors.foreground));
            if cfg.font.size > 0.0 {
                a.push(format!("--font-size={}", cfg.font.size));
            }
            let family = cfg.font.family.trim();
            if !family.is_empty() {
                a.push(format!("--font-family={family}"));
            }
            a.extend(extra.iter().cloned());
            a.push("-e".into());
            a.extend(ssh);
        }
        "wezterm" | "wezterm-gui" => {
            a.push("start".into());
            a.push("--class".into());
            a.push(cfg.window.app_id.clone());
            a.extend(extra.iter().cloned());
            a.push("--".into());
            a.extend(ssh);
        }
        _ => {
            a.extend(extra.iter().cloned());
            a.extend(ssh);
        }
    }
    Ok(a)
}

pub fn spawn_session(cfg: &Config, spec: AttachSpec) -> Result<(), String> {
    let program = resolve_program(cfg.terminal.program.trim())?;
    let mut cfg = cfg.clone();
    cfg.terminal.program = program;
    let argv = attach_argv(&cfg, spec)?;
    let (bin, args) = argv
        .split_first()
        .ok_or_else(|| "empty terminal argv".to_string())?;
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    for (k, v) in cfg.term_env() {
        cmd.env(k, v);
    }
    cmd.spawn().map_err(|e| format!("{bin}: {e}"))?;
    Ok(())
}

fn ssh_cmd(cfg: &Config, spec: AttachSpec) -> Vec<String> {
    let mut a = vec!["ssh".into()];
    a.extend(cfg.ssh_attach_argv(spec));
    a
}

fn kitty_flags(cfg: &Config) -> Vec<String> {
    let mut a = vec![
        "--class".into(),
        cfg.window.app_id.clone(),
        "--detach".into(),
    ];
    let mut push_o = |k: &str, v: &str| {
        a.push("-o".into());
        a.push(format!("{k}={v}"));
    };
    push_o("remember_window_size", "no");
    push_o(
        "initial_window_width",
        &format!("{}", cfg.window.width.round() as u32),
    );
    push_o(
        "initial_window_height",
        &format!("{}", cfg.window.height.round() as u32),
    );
    push_o("font_size", &format!("{}", cfg.font.size));
    let family = cfg.font.family.trim();
    if !family.is_empty() {
        push_o("font_family", family);
    }
    let c = &cfg.colors;
    push_o("foreground", &c.foreground);
    push_o("background", &c.background);
    push_o("color0", &c.black);
    push_o("color1", &c.red);
    push_o("color2", &c.green);
    push_o("color3", &c.yellow);
    push_o("color4", &c.blue);
    push_o("color5", &c.magenta);
    push_o("color6", &c.cyan);
    push_o("color7", &c.white);
    push_o("color8", &c.bright_black);
    push_o("color9", &c.bright_red);
    push_o("color10", &c.bright_green);
    push_o("color11", &c.bright_yellow);
    push_o("color12", &c.bright_blue);
    push_o("color13", &c.bright_magenta);
    push_o("color14", &c.bright_cyan);
    push_o("color15", &c.bright_white);
    // Do not set close_on_child_death=yes: kitty then destroys the window if
    // ssh exits during startup (ControlMaster / 0-size pty), which looks like
    // "Enter does nothing". Default is no; the user closes the window to detach.
    push_o("confirm_os_window_close", "0");
    a
}

fn foot_flags(cfg: &Config) -> Vec<String> {
    let mut a = vec![format!("--app-id={}", cfg.window.app_id)];
    a.push(format!(
        "--window-size-pixels={}x{}",
        cfg.window.width.round() as u32,
        cfg.window.height.round() as u32
    ));
    let family = cfg.font.family.trim();
    if !family.is_empty() {
        a.push(format!("--font={family}:size={}", cfg.font.size));
    }
    let c = &cfg.colors;
    let mut push_o = |key: &str, val: &str| {
        a.push("-o".into());
        a.push(format!("colors.{key}={}", hex_plain(val)));
    };
    push_o("foreground", &c.foreground);
    push_o("background", &c.background);
    push_o("regular0", &c.black);
    push_o("regular1", &c.red);
    push_o("regular2", &c.green);
    push_o("regular3", &c.yellow);
    push_o("regular4", &c.blue);
    push_o("regular5", &c.magenta);
    push_o("regular6", &c.cyan);
    push_o("regular7", &c.white);
    push_o("bright0", &c.bright_black);
    push_o("bright1", &c.bright_red);
    push_o("bright2", &c.bright_green);
    push_o("bright3", &c.bright_yellow);
    push_o("bright4", &c.bright_blue);
    push_o("bright5", &c.bright_magenta);
    push_o("bright6", &c.bright_cyan);
    push_o("bright7", &c.bright_white);
    a
}

fn hex_plain(s: &str) -> String {
    s.trim().trim_start_matches('#').to_string()
}

fn basename(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
}

pub fn resolve_program(program: &str) -> Result<String, String> {
    let p = Path::new(program);
    if p.components().count() > 1 || program.contains('/') {
        if is_runnable(p) {
            return Ok(program.to_string());
        }
        return Err(format!("{program} is not an executable"));
    }
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        let cand = dir.join(program);
        if is_runnable(&cand) {
            return Ok(cand.display().to_string());
        }
    }
    Err(format!("{program} not found on PATH"))
}

fn is_runnable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    meta.is_file() && meta.permissions().mode() & 0o111 != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(program: &str) -> Config {
        let mut cfg = Config::default();
        cfg.terminal.program = program.into();
        cfg
    }

    fn make_exec(name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "vps-term-{}-{name}-{}",
            std::process::id(),
            name.len()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join(name);
        std::fs::write(&bin, b"").unwrap();
        let mut p = std::fs::metadata(&bin).unwrap().permissions();
        p.set_mode(0o755);
        std::fs::set_permissions(&bin, p).unwrap();
        (dir, bin)
    }

    #[test]
    fn empty_program_needs_chooser() {
        assert!(needs_chooser(&Config::default()));
        assert!(missing_terminal_message(&Config::default()).is_none());
    }

    #[test]
    fn runnable_path_skips_chooser() {
        let (dir, bin) = make_exec("myterm");
        let mut cfg = Config::default();
        cfg.terminal.program = bin.display().to_string();
        assert!(!needs_chooser(&cfg));
        assert!(missing_terminal_message(&cfg).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn deleted_program_needs_chooser() {
        let (dir, bin) = make_exec("gone");
        let mut cfg = Config::default();
        cfg.terminal.program = bin.display().to_string();
        assert!(!needs_chooser(&cfg));
        std::fs::remove_file(&bin).unwrap();
        assert!(needs_chooser(&cfg));
        let msg = missing_terminal_message(&cfg).expect("reason");
        assert!(msg.contains("pick another terminal"), "{msg}");
        assert!(
            msg.contains("gone") || msg.contains("not an executable"),
            "{msg}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn non_executable_needs_chooser() {
        let (dir, bin) = make_exec("noperm");
        let mut p = std::fs::metadata(&bin).unwrap().permissions();
        p.set_mode(0o644);
        std::fs::set_permissions(&bin, p).unwrap();
        let mut cfg = Config::default();
        cfg.terminal.program = bin.display().to_string();
        assert!(needs_chooser(&cfg));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn broken_symlink_needs_chooser() {
        let dir = std::env::temp_dir().join(format!("vps-term-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join("term");
        std::os::unix::fs::symlink(dir.join("missing"), &link).unwrap();
        let mut cfg = Config::default();
        cfg.terminal.program = link.display().to_string();
        assert!(needs_chooser(&cfg));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn basename_not_on_path_needs_chooser() {
        let mut cfg = Config::default();
        cfg.terminal.program = "vps-no-such-terminal-xyz".into();
        assert!(needs_chooser(&cfg));
        let msg = missing_terminal_message(&cfg).expect("reason");
        assert!(msg.contains("not found on PATH"), "{msg}");
    }

    #[test]
    fn empty_program_argv_errors() {
        let err = attach_argv(&Config::default(), AttachSpec::New).unwrap_err();
        assert!(err.contains("no terminal"), "{err}");
    }

    #[test]
    fn kitty_argv_class_detach_ssh_id() {
        let argv = attach_argv(&cfg_with("/usr/bin/kitty"), AttachSpec::Id(1)).unwrap();
        assert_eq!(argv[0], "/usr/bin/kitty");
        assert!(argv.windows(2).any(|w| w == ["--class", "vps"]));
        assert!(argv.iter().any(|a| a == "--detach"));
        assert!(argv.iter().any(|a| a == "ssh"));
        assert!(argv.iter().any(|a| a.contains("vpsd attach --id 1")));
        assert!(argv
            .iter()
            .any(|a| a == "background=#241018" || a.contains("background=#241018")));
        assert!(!argv.iter().any(|a| a.contains("font_family=")));
        assert!(
            !argv.iter().any(|a| a.contains("close_on_child_death")),
            "close_on_child_death=yes closes the window if ssh dies at startup"
        );
    }

    #[test]
    fn kitty_font_family_when_set() {
        let mut cfg = cfg_with("/usr/bin/kitty");
        cfg.font.family = "Berkeley Mono".into();
        let argv = attach_argv(&cfg, AttachSpec::New).unwrap();
        assert!(argv.iter().any(|a| a == "font_family=Berkeley Mono"));
        assert!(argv.iter().any(|a| a.contains("vpsd attach --new")));
    }

    #[test]
    fn foot_argv_app_id_and_trailing_ssh() {
        let argv = attach_argv(&cfg_with("/usr/bin/foot"), AttachSpec::Id(3)).unwrap();
        assert_eq!(argv[0], "/usr/bin/foot");
        assert!(argv.iter().any(|a| a == "--app-id=vps"));
        assert!(argv.iter().any(|a| a.contains("colors.background=241018")));
        let ssh = argv.iter().position(|a| a == "ssh").expect("ssh");
        assert!(argv[ssh + 1..].iter().any(|a| a.contains("--id 3")));
    }

    #[test]
    fn alacritty_uses_e_last() {
        let mut cfg = cfg_with("/usr/bin/alacritty");
        cfg.terminal.args = vec!["--option".into(), "x=1".into()];
        let argv = attach_argv(&cfg, AttachSpec::New).unwrap();
        let e = argv.iter().position(|a| a == "-e").expect("-e");
        assert_eq!(argv[e + 1], "ssh");
        assert!(argv[..e].iter().any(|a| a == "--option"));
        assert!(argv.windows(2).any(|w| w == ["--class", "vps"]));
    }

    #[test]
    fn ghostty_class_and_e() {
        let argv = attach_argv(&cfg_with("/usr/bin/ghostty"), AttachSpec::New).unwrap();
        assert!(argv.iter().any(|a| a == "--class=vps"));
        assert!(argv.iter().any(|a| a == "--background=#241018"));
        let e = argv.iter().position(|a| a == "-e").expect("-e");
        assert_eq!(argv[e + 1], "ssh");
    }

    #[test]
    fn wezterm_start_class_then_prog() {
        let argv = attach_argv(&cfg_with("/usr/bin/wezterm"), AttachSpec::Id(9)).unwrap();
        assert_eq!(argv[1], "start");
        assert!(argv.windows(2).any(|w| w == ["--class", "vps"]));
        let dash = argv.iter().position(|a| a == "--").expect("--");
        assert_eq!(argv[dash + 1], "ssh");
        assert!(argv[dash + 1..].iter().any(|a| a.contains("--id 9")));
    }

    #[test]
    fn unknown_is_program_args_ssh() {
        let mut cfg = cfg_with("/opt/myterm");
        cfg.terminal.args = vec!["-e".into()];
        let argv = attach_argv(&cfg, AttachSpec::New).unwrap();
        assert_eq!(
            argv,
            vec![
                "/opt/myterm".to_string(),
                "-e".to_string(),
                "ssh".to_string(),
                "-tt".to_string(),
                "grok".to_string(),
                "/home/tj/.local/bin/vpsd attach --new".to_string(),
            ]
        );
    }

    #[test]
    fn detect_finds_known_executable() {
        let dir = std::env::temp_dir().join(format!("vps-term-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("foot");
        std::fs::write(&bin, b"").unwrap();
        let mut p = std::fs::metadata(&bin).unwrap().permissions();
        p.set_mode(0o755);
        std::fs::set_permissions(&bin, p).unwrap();
        let found = detect_in(dir.to_str().unwrap());
        assert_eq!(
            found,
            vec![Detected {
                name: "foot".into(),
                path: bin,
            }]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detect_skips_unknown_names() {
        let dir = std::env::temp_dir().join(format!("vps-term-unk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("xterm");
        std::fs::write(&bin, b"").unwrap();
        let mut p = std::fs::metadata(&bin).unwrap().permissions();
        p.set_mode(0o755);
        std::fs::set_permissions(&bin, p).unwrap();
        assert!(detect_in(dir.to_str().unwrap()).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn client_does_not_depend_on_iced_term() {
        let toml = include_str!("../Cargo.toml");
        assert!(
            !toml.contains("iced_term"),
            "iced_term cannot host Grok; session window is the user's terminal"
        );
    }
}
