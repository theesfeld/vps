//! Real POSIX PTY: openpty(3), login_tty, login shell on the slave.

use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

use nix::pty::{openpty, OpenptyResult, Winsize};
use nix::unistd::setsid;

pub struct Session {
    pub master: OwnedFd,
    pub child: Child,
    pub pts_name: String,
}

pub fn winsize(cols: u16, rows: u16) -> Winsize {
    Winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

pub fn read_tty_winsize(fd: i32) -> Option<Winsize> {
    let mut ws = Winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let rc = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) };
    if rc == 0 && ws.ws_row > 0 && ws.ws_col > 0 {
        Some(ws)
    } else {
        None
    }
}

/// Discard pending master output (queued TUI frames while detached).
pub fn drain(fd: i32, mut on_bytes: impl FnMut(&[u8])) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        let mut buf = [0u8; 8192];
        loop {
            let n = libc::read(fd, buf.as_mut_ptr() as *mut _, buf.len());
            if n <= 0 {
                break;
            }
            on_bytes(&buf[..n as usize]);
        }
    }
}

pub fn set_winsize(fd: i32, ws: &Winsize) -> std::io::Result<()> {
    let rc = unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, ws) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

pub fn spawn_login_shell(ws: Winsize, shell: &str) -> std::io::Result<Session> {
    let OpenptyResult { master, slave } =
        openpty(Some(&ws), None).map_err(|e| std::io::Error::other(format!("openpty: {e}")))?;

    let pts_name = nix::unistd::ttyname(&slave)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "/dev/pts/?".into());

    let slave_fd = slave.into_raw_fd();
    let stdin = unsafe { Stdio::from_raw_fd(dup_or_close(slave_fd)?) };
    let stdout = unsafe { Stdio::from_raw_fd(dup_or_close(slave_fd)?) };
    let stderr = unsafe { Stdio::from_raw_fd(dup_or_close(slave_fd)?) };

    let mut cmd = Command::new(shell);
    cmd.arg("-l")
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr)
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("VPSD", "1")
        // This PTY is not the SSH login; skip ~/.bashrc.d/zellij.sh.
        .env_remove("SSH_CONNECTION")
        .env_remove("SSH_CLIENT")
        .env_remove("SSH_TTY");
    unsafe {
        cmd.pre_exec(move || {
            setsid().map_err(std::io::Error::other)?;
            if libc::ioctl(slave_fd, libc::TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            // slave_fd is still open in the child; close extra copy after dup2 via libc.
            let _ = libc::close(slave_fd);
            Ok(())
        });
    }
    let child = cmd.spawn()?;
    // Parent no longer needs the slave.
    unsafe {
        libc::close(slave_fd);
    }

    Ok(Session {
        master,
        child,
        pts_name,
    })
}

fn dup_or_close(fd: i32) -> std::io::Result<i32> {
    let n = unsafe { libc::dup(fd) };
    if n < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    use std::time::Duration;

    fn master_as_std_file(master: &OwnedFd) -> std::io::Result<std::fs::File> {
        let cloned = master.try_clone().map_err(std::io::Error::other)?;
        Ok(std::fs::File::from(cloned))
    }

    #[test]
    fn openpty_is_a_real_pts() {
        let Session {
            master,
            mut child,
            pts_name,
        } = spawn_login_shell(winsize(80, 24), "/bin/bash").unwrap();
        assert!(
            pts_name.starts_with("/dev/pts/"),
            "expected a pts path, got {pts_name}"
        );
        assert!(master.as_raw_fd() >= 0);

        let mut file = master_as_std_file(&master).unwrap();
        // Drain banner, then echo a marker.
        let _ = file.write_all(b"printf 'VPS_PTY_OK\\n'\n");
        let _ = file.flush();
        let mut buf = [0u8; 4096];
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut got = Vec::new();
        while std::time::Instant::now() < deadline {
            if let Ok(n) = file.read(&mut buf) {
                if n > 0 {
                    got.extend_from_slice(&buf[..n]);
                    if got.windows(b"VPS_PTY_OK".len()).any(|w| w == b"VPS_PTY_OK") {
                        let _ = child.kill();
                        let _ = child.wait();
                        return;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = child.kill();
        let _ = child.wait();
        panic!(
            "did not see VPS_PTY_OK in {:?}",
            String::from_utf8_lossy(&got)
        );
    }
}
