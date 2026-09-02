//! vpsd — remote PTY owner.
//!
//! Listen: Unix socket (`$XDG_RUNTIME_DIR/vpsd.sock`) or a tty/stdio attach.
//! Never TCP. Never UDP. Reach it with `ssh -t grok vpsd attach`.

use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

use clap::{Parser, Subcommand};
use vps_protocol::{decode_line, default_socket_path, encode_line, Listen, Message};

mod pty;

#[derive(Parser, Debug)]
#[command(
    name = "vpsd",
    about = "PTY daemon for the vps client (SSH tunnel only)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Bind a Unix socket. Refuses anything that looks like a network address.
    Daemon {
        /// `unix:/path` or an absolute path. Default: $XDG_RUNTIME_DIR/vpsd.sock
        #[arg(long)]
        listen: Option<String>,
    },
    /// Run under `ssh -t`: own a real PTY and splice it to this tty.
    Attach {
        #[arg(long)]
        cols: Option<u16>,
        #[arg(long)]
        rows: Option<u16>,
        #[arg(long, default_value = "/bin/bash")]
        shell: String,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("vpsd: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Daemon { listen } => daemon(listen.as_deref())?,
        Cmd::Attach { cols, rows, shell } => attach(cols, rows, &shell)?,
    }
    Ok(())
}

fn daemon(listen: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let spec = listen
        .map(str::to_string)
        .unwrap_or_else(|| default_socket_path().display().to_string());
    match Listen::parse(&spec)? {
        Listen::Stdio => {
            return Err("daemon cannot listen on stdio; use `vpsd attach`".into());
        }
        Listen::Unix(path) => bind_unix(&path)?,
    }
    Ok(())
}

fn bind_unix(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }
    eprintln!("vpsd: listening on unix:{}", path.display());
    loop {
        let (stream, _) = listener.accept()?;
        std::thread::spawn(move || {
            if let Err(e) = handle_unix_client(stream) {
                eprintln!("vpsd: client: {e}");
            }
        });
    }
}

fn handle_unix_client(
    mut stream: UnixStream,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut line = Vec::new();
    loop {
        let mut b = [0u8; 1];
        let n = stream.read(&mut b)?;
        if n == 0 {
            return Ok(());
        }
        if b[0] == b'\n' {
            break;
        }
        line.push(b[0]);
        if line.len() > 4096 {
            return Err("control line too long".into());
        }
    }
    let text = String::from_utf8(line)?;
    match decode_line(&text)? {
        Message::Open { cols, rows } => {
            let session = pty::spawn_login_shell(pty::winsize(cols, rows), "/bin/bash")?;
            let _ = stream.write_all(
                encode_line(&Message::Hello {
                    v: vps_protocol::VERSION,
                })?
                .as_bytes(),
            );
            splice_fds(
                stream.as_raw_fd(),
                stream.as_raw_fd(),
                session.master.as_raw_fd(),
            )?;
            let mut child = session.child;
            let _ = child.wait();
        }
        other => {
            let _ = stream.write_all(
                encode_line(&Message::Error {
                    msg: format!("expected open, got {other:?}"),
                })?
                .as_bytes(),
            );
        }
    }
    Ok(())
}

fn attach(
    cols: Option<u16>,
    rows: Option<u16>,
    shell: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !stdin_is_tty() {
        return Err("attach requires a tty (ssh -t grok vpsd attach)".into());
    }
    let ws = match (cols, rows) {
        (Some(c), Some(r)) => pty::winsize(c, r),
        _ => pty::read_tty_winsize(libc::STDIN_FILENO).unwrap_or_else(|| pty::winsize(80, 24)),
    };
    let mut session = pty::spawn_login_shell(ws, shell)?;
    eprintln!("vpsd: pts {} pid {}", session.pts_name, session.child.id());

    let stdin_fd = libc::STDIN_FILENO;
    let stdout_fd = libc::STDOUT_FILENO;
    let master_fd = session.master.as_raw_fd();

    // Raw-ish: copy tty ↔ PTY master until the shell exits.
    splice_fds(stdin_fd, stdout_fd, master_fd)?;
    let _ = session.child.wait();
    Ok(())
}

fn stdin_is_tty() -> bool {
    unsafe { libc::isatty(libc::STDIN_FILENO) == 1 }
}

fn splice_fds(stdin_fd: i32, stdout_fd: i32, master_fd: i32) -> std::io::Result<()> {
    unsafe {
        let flags = libc::fcntl(master_fd, libc::F_GETFL);
        libc::fcntl(master_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        let flags = libc::fcntl(stdin_fd, libc::F_GETFL);
        libc::fcntl(stdin_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }

    let mut buf = [0u8; 8192];
    loop {
        let mut rfds = unsafe { std::mem::zeroed::<libc::fd_set>() };
        unsafe {
            libc::FD_ZERO(&mut rfds);
            libc::FD_SET(stdin_fd, &mut rfds);
            libc::FD_SET(master_fd, &mut rfds);
        }
        let nfds = stdin_fd.max(master_fd) + 1;
        let rc = unsafe {
            libc::select(
                nfds,
                &mut rfds,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        unsafe {
            if libc::FD_ISSET(stdin_fd, &rfds) {
                let n = libc::read(stdin_fd, buf.as_mut_ptr() as *mut _, buf.len());
                if n == 0 {
                    return Ok(());
                }
                if n < 0 {
                    let err = std::io::Error::last_os_error();
                    if err.kind() != std::io::ErrorKind::WouldBlock {
                        return Err(err);
                    }
                } else if libc::write(master_fd, buf.as_ptr() as *const _, n as usize) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            if libc::FD_ISSET(master_fd, &rfds) {
                let n = libc::read(master_fd, buf.as_mut_ptr() as *mut _, buf.len());
                if n == 0 {
                    return Ok(());
                }
                if n < 0 {
                    let err = std::io::Error::last_os_error();
                    if err.kind() != std::io::ErrorKind::WouldBlock {
                        return Err(err);
                    }
                } else if libc::write(stdout_fd, buf.as_ptr() as *const _, n as usize) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_listen_rejects_tcp() {
        let err = Listen::parse("0.0.0.0:2022").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("never listens on TCP"), "{msg}");
    }

    #[test]
    fn default_socket_is_unix_path() {
        let p = default_socket_path();
        assert!(p.is_absolute() || p.starts_with("/tmp"));
        assert!(p.ends_with("vpsd.sock"));
    }
}
