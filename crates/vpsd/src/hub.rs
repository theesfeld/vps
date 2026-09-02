//! In-memory PTY table. Sessions outlive a client splice.

use std::collections::{HashMap, VecDeque};
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::{Arc, Mutex};

use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use vps_protocol::SessionInfo;

use crate::pty::{self, Session};
use crate::snapshot::Screen;

pub struct Slot {
    pub session: Session,
    pub attached: bool,
    pub scrollback: VecDeque<u8>,
    /// Sticky: once we see alt-screen / dense CSI (Grok logo, vim, …).
    pub tui: bool,
    pub screen: Screen,
}

pub struct Hub {
    next_id: u64,
    slots: HashMap<u64, Slot>,
    shell: String,
    scrollback_limit: usize,
}

impl Hub {
    pub fn new(shell: String, scrollback_limit: usize) -> Self {
        Self {
            next_id: 1,
            slots: HashMap::new(),
            shell,
            scrollback_limit: scrollback_limit.max(4096),
        }
    }

    pub fn create(&mut self, cols: u16, rows: u16) -> std::io::Result<(u64, OwnedFd)> {
        self.reap();
        let ws = pty::winsize(cols, rows);
        let session = pty::spawn_login_shell(ws, &self.shell)?;
        let clone = session.master.try_clone().map_err(std::io::Error::other)?;
        let id = self.next_id;
        self.next_id += 1;
        self.slots.insert(
            id,
            Slot {
                session,
                attached: true,
                scrollback: VecDeque::new(),
                tui: false,
                screen: Screen::new(cols, rows),
            },
        );
        Ok((id, clone))
    }

    pub fn attach(&mut self, id: u64, cols: u16, rows: u16) -> std::io::Result<OwnedFd> {
        self.reap();
        let ws = pty::winsize(cols, rows);
        let slot = self
            .slots
            .get_mut(&id)
            .ok_or_else(|| std::io::Error::other(format!("no session {id}")))?;
        // Steal if a previous client died without clearing the flag (EPIPE).
        slot.attached = true;
        let _ = pty::set_winsize(slot.session.master.as_raw_fd(), &ws);
        slot.screen.resize(cols, rows);
        slot.session
            .master
            .try_clone()
            .map_err(std::io::Error::other)
    }

    /// Shell: replay captured `ls`. TUI (Grok): empty — dump crashed iced;
    /// live splice + a size-change redraw paints the current frame instead.
    pub fn replay_on_attach(&self, id: u64) -> Vec<u8> {
        match self.slots.get(&id) {
            Some(s) if s.tui => Vec::new(),
            Some(s) => s.scrollback.iter().copied().collect(),
            None => Vec::new(),
        }
    }

    pub fn is_tui(&self, id: u64) -> bool {
        self.slots.get(&id).map(|s| s.tui).unwrap_or(false)
    }

    pub fn push_output(&mut self, id: u64, bytes: &[u8]) {
        let Some(slot) = self.slots.get_mut(&id) else {
            return;
        };
        if !slot.tui && looks_like_tui(bytes) {
            slot.tui = true;
        }
        slot.screen.feed(bytes);
        slot.scrollback.extend(bytes);
        let limit = if slot.tui {
            64 * 1024
        } else {
            self.scrollback_limit
        };
        let mut trimmed = false;
        while slot.scrollback.len() > limit {
            slot.scrollback.pop_front();
            trimmed = true;
        }
        if trimmed && !slot.tui {
            while let Some(b) = slot.scrollback.front().copied() {
                slot.scrollback.pop_front();
                if b == b'\n' {
                    break;
                }
            }
        }
    }

    pub fn winch(&self, id: u64) {
        let Some(slot) = self.slots.get(&id) else {
            return;
        };
        let bash = slot.session.child.id();
        let _ = kill(Pid::from_raw(bash as i32), Signal::SIGWINCH);
        for child in children_of(bash) {
            let _ = kill(Pid::from_raw(child as i32), Signal::SIGWINCH);
        }
    }

    pub fn set_size(&self, id: u64, cols: u16, rows: u16) {
        if let Some(slot) = self.slots.get(&id) {
            let _ = pty::set_winsize(slot.session.master.as_raw_fd(), &pty::winsize(cols, rows));
        }
    }

    pub fn list(&mut self) -> Vec<SessionInfo> {
        self.reap();
        let mut out: Vec<SessionInfo> = self
            .slots
            .iter()
            .map(|(id, slot)| {
                let pid = slot.session.child.id();
                SessionInfo {
                    id: *id,
                    pts: slot.session.pts_name.clone(),
                    pid,
                    attached: slot.attached,
                    cwd: proc_cwd(pid),
                    command: proc_command(pid),
                }
            })
            .collect();
        out.sort_by_key(|s| s.id);
        out
    }

    pub fn detach(&mut self, id: u64) {
        if let Some(slot) = self.slots.get_mut(&id) {
            slot.attached = false;
        }
    }

    pub fn drop_session(&mut self, id: u64) {
        if let Some(mut slot) = self.slots.remove(&id) {
            let _ = slot.session.child.kill();
            let _ = slot.session.child.wait();
        }
    }

    pub fn pts_name(&self, id: u64) -> Option<&str> {
        self.slots.get(&id).map(|s| s.session.pts_name.as_str())
    }

    fn reap(&mut self) {
        let dead: Vec<u64> = self
            .slots
            .iter_mut()
            .filter_map(|(id, slot)| match slot.session.child.try_wait() {
                Ok(Some(_)) => Some(*id),
                _ => None,
            })
            .collect();
        for id in dead {
            self.slots.remove(&id);
        }
    }
}

fn proc_cwd(pid: u32) -> String {
    std::fs::read_link(format!("/proc/{pid}/cwd"))
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

fn proc_command(pid: u32) -> String {
    if let Some(child) = first_non_shell_child(pid) {
        let cmd = cmdline(child);
        if !cmd.is_empty() {
            return cmd;
        }
    }
    cmdline(pid)
}

fn cmdline(pid: u32) -> String {
    let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    bytes
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

fn children_of(pid: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return out;
    };
    for ent in dir.flatten() {
        let Ok(child) = ent.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if proc_ppid(child) == Some(pid) {
            out.push(child);
        }
    }
    out
}

fn first_non_shell_child(pid: u32) -> Option<u32> {
    let dir = std::fs::read_dir("/proc").ok()?;
    for ent in dir.flatten() {
        let name = ent.file_name();
        let Ok(child) = name.to_string_lossy().parse::<u32>() else {
            continue;
        };
        if proc_ppid(child) != Some(pid) {
            continue;
        }
        let cmd = cmdline(child);
        if cmd.is_empty() {
            continue;
        }
        let base = cmd.split_whitespace().next().unwrap_or("");
        let base = base.rsplit('/').next().unwrap_or(base);
        if matches!(base, "bash" | "sh" | "zsh" | "fish" | "dash") {
            continue;
        }
        return Some(child);
    }
    None
}

fn proc_ppid(pid: u32) -> Option<u32> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("PPid:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

pub type SharedHub = Arc<Mutex<Hub>>;

pub fn new_shared_with_limit(shell: String, scrollback_limit: usize) -> SharedHub {
    Arc::new(Mutex::new(Hub::new(shell, scrollback_limit)))
}

pub fn looks_like_tui(bytes: &[u8]) -> bool {
    if bytes
        .windows(8)
        .any(|w| w == b"\x1b[?1049h" || w == b"\x1b[?1047h")
    {
        return true;
    }
    if bytes.windows(6).any(|w| w == b"\x1b[?25l") {
        return true;
    }
    if bytes.is_empty() {
        return false;
    }
    let esc = bytes.iter().filter(|b| **b == 0x1b).count();
    esc.saturating_mul(8) > bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tui_detects_alt_screen_and_logo_csi() {
        assert!(looks_like_tui(b"\x1b[?1049h\x1b[2Jgrok"));
        assert!(looks_like_tui(b"\x1b[?25l"));
        assert!(!looks_like_tui(b"ls -asl\nfile\n[tj@host ~]$ "));
    }

    #[test]
    fn attach_steals_already_attached() {
        let mut hub = Hub::new("/bin/bash".into(), 4096);
        let (id, fd1) = hub.create(80, 24).unwrap();
        let fd2 = hub.attach(id, 80, 24).expect("steal stale attached");
        drop(fd1);
        drop(fd2);
        hub.drop_session(id);
    }
}
