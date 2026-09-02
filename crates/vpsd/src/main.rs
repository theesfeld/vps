//! vpsd — remote PTY owner.
//!
//! Listen: Unix socket (`$XDG_RUNTIME_DIR/vpsd.sock`) only. Never TCP/UDP.
//! `vpsd attach` (under `ssh -tt`) talks to that socket so the PTY survives
//! the client going away.

use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

use clap::{Parser, Subcommand};
use vps_protocol::{decode_line, encode_line, Listen, Message};

mod config;
mod hub;
mod pty;

use config::DaemonConfig;
use hub::{new_shared, SharedHub};

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
        /// `unix:/path` or an absolute path. Overrides config `listen`.
        #[arg(long)]
        listen: Option<String>,
    },
    /// Run under `ssh -tt`: splice this tty to a daemon-owned PTY.
    Attach {
        #[arg(long)]
        cols: Option<u16>,
        #[arg(long)]
        rows: Option<u16>,
        #[arg(long)]
        shell: Option<String>,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("vpsd: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = DaemonConfig::load();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Daemon { listen } => {
            let spec = listen.unwrap_or_else(|| cfg.socket_path().display().to_string());
            daemon(&spec, cfg.shell.clone(), cfg.mode_bits())?
        }
        Cmd::Attach { cols, rows, shell } => attach(
            cols,
            rows,
            shell.as_deref().unwrap_or(cfg.shell.as_str()),
            &cfg,
        )?,
    }
    Ok(())
}

fn daemon(spec: &str, shell: String, mode: u32) -> Result<(), Box<dyn std::error::Error>> {
    match Listen::parse(spec)? {
        Listen::Stdio => {
            return Err("daemon cannot listen on stdio; use `vpsd attach`".into());
        }
        Listen::Unix(path) => bind_unix(&path, new_shared(shell), mode)?,
    }
    Ok(())
}

fn bind_unix(path: &Path, hub: SharedHub, mode: u32) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(mode);
        std::fs::set_permissions(path, perms)?;
    }
    eprintln!("vpsd: listening on unix:{}", path.display());
    loop {
        let (stream, _) = listener.accept()?;
        let hub = hub.clone();
        std::thread::spawn(move || {
            if let Err(e) = handle_unix_client(stream, hub) {
                eprintln!("vpsd: client: {e}");
            }
        });
    }
}

fn handle_unix_client(
    mut stream: UnixStream,
    hub: SharedHub,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let text = read_line(&mut stream)?;
    match decode_line(&text)? {
        Message::Open { cols, rows } => {
            let (id, master) = hub
                .lock()
                .map_err(|e| std::io::Error::other(e.to_string()))?
                .take_or_create(cols, rows)?;
            let pts = hub
                .lock()
                .ok()
                .and_then(|h| h.pts_name(id).map(str::to_string))
                .unwrap_or_default();
            let _ = stream.write_all(
                encode_line(&Message::Hello {
                    v: vps_protocol::VERSION,
                    id,
                })?
                .as_bytes(),
            );
            eprintln!("vpsd: session {id} pts {pts}");
            let end = splice_fds(stream.as_raw_fd(), stream.as_raw_fd(), master.as_raw_fd())?;
            let mut h = hub
                .lock()
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            match end {
                SpliceEnd::ClientGone => h.detach(id),
                SpliceEnd::MasterGone => h.drop_session(id),
            }
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
    cfg: &DaemonConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    if !stdin_is_tty() {
        return Err("attach requires a tty (ssh -tt grok vpsd attach)".into());
    }
    let ws = match (cols, rows) {
        (Some(c), Some(r)) => pty::winsize(c, r),
        _ => pty::read_tty_winsize(libc::STDIN_FILENO).unwrap_or_else(|| pty::winsize(80, 24)),
    };
    let sock = cfg.socket_path();
    if sock.exists() {
        attach_via_daemon(&sock, ws.ws_col, ws.ws_row)?;
        return Ok(());
    }
    attach_oneshot(ws, shell)
}

fn attach_via_daemon(sock: &Path, cols: u16, rows: u16) -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = UnixStream::connect(sock)?;
    stream.write_all(encode_line(&Message::Open { cols, rows })?.as_bytes())?;
    let hello = read_line(&mut stream)?;
    match decode_line(&hello)? {
        Message::Hello { id, .. } => {
            eprintln!("vpsd: attached session {id}");
        }
        Message::Error { msg } => return Err(msg.into()),
        other => return Err(format!("unexpected {other:?}").into()),
    }
    splice_fds(libc::STDIN_FILENO, libc::STDOUT_FILENO, stream.as_raw_fd())?;
    Ok(())
}

fn attach_oneshot(ws: nix::pty::Winsize, shell: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut session = pty::spawn_login_shell(ws, shell)?;
    eprintln!(
        "vpsd: pts {} pid {} (oneshot)",
        session.pts_name,
        session.child.id()
    );
    splice_fds(
        libc::STDIN_FILENO,
        libc::STDOUT_FILENO,
        session.master.as_raw_fd(),
    )?;
    let _ = session.child.kill();
    let _ = session.child.wait();
    Ok(())
}

fn read_line(stream: &mut UnixStream) -> std::io::Result<String> {
    let mut line = Vec::new();
    loop {
        let mut b = [0u8; 1];
        let n = stream.read(&mut b)?;
        if n == 0 {
            return Err(std::io::Error::other("eof before control line"));
        }
        if b[0] == b'\n' {
            break;
        }
        line.push(b[0]);
        if line.len() > 4096 {
            return Err(std::io::Error::other("control line too long"));
        }
    }
    String::from_utf8(line).map_err(std::io::Error::other)
}

fn stdin_is_tty() -> bool {
    unsafe { libc::isatty(libc::STDIN_FILENO) == 1 }
}

#[derive(Debug, PartialEq, Eq)]
enum SpliceEnd {
    ClientGone,
    MasterGone,
}

fn splice_fds(stdin_fd: i32, stdout_fd: i32, master_fd: i32) -> std::io::Result<SpliceEnd> {
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
                    return Ok(SpliceEnd::ClientGone);
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
                    return Ok(SpliceEnd::MasterGone);
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
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn daemon_listen_rejects_tcp() {
        let err = Listen::parse("0.0.0.0:2022").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("never listens on TCP"), "{msg}");
    }

    #[test]
    fn default_socket_is_unix_path() {
        let p = vps_protocol::default_socket_path();
        assert!(p.is_absolute() || p.starts_with("/tmp"));
        assert!(p.ends_with("vpsd.sock"));
    }

    fn wait_for_sock(path: &Path, timeout: Duration) {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if path.exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("socket {} never appeared", path.display());
    }

    fn read_until(stream: &mut UnixStream, needle: &[u8], timeout: Duration) -> Vec<u8> {
        let start = std::time::Instant::now();
        let mut got = Vec::new();
        let mut buf = [0u8; 1024];
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        while start.elapsed() < timeout {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    got.extend_from_slice(&buf[..n]);
                    if got.windows(needle.len()).any(|w| w == needle) {
                        return got;
                    }
                }
                Err(_) => continue,
            }
        }
        panic!(
            "timeout waiting for {:?} in {:?}",
            String::from_utf8_lossy(needle),
            String::from_utf8_lossy(&got)
        );
    }

    #[test]
    fn persist_session_across_disconnect() {
        let path: PathBuf =
            std::env::temp_dir().join(format!("vpsd-persist-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let hub = new_shared("/bin/bash".into());
        let serve_path = path.clone();
        let serve_hub = hub.clone();
        std::thread::spawn(move || {
            let _ = bind_unix(&serve_path, serve_hub, 0o600);
        });
        wait_for_sock(&path, Duration::from_secs(2));

        let mut a = UnixStream::connect(&path).unwrap();
        a.write_all(
            encode_line(&Message::Open { cols: 80, rows: 24 })
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
        let hello = read_line(&mut a).unwrap();
        let Message::Hello { id, .. } = decode_line(&hello).unwrap() else {
            panic!("not hello: {hello}");
        };
        assert!(id >= 1);
        a.write_all(b"export VPSD_PERSIST=keep\nprintf 'MARK1\\n'\n")
            .unwrap();
        let _ = read_until(&mut a, b"MARK1", Duration::from_secs(3));
        drop(a);
        std::thread::sleep(Duration::from_millis(150));

        let mut b = UnixStream::connect(&path).unwrap();
        b.write_all(
            encode_line(&Message::Open { cols: 80, rows: 24 })
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
        let hello2 = read_line(&mut b).unwrap();
        let Message::Hello { id: id2, .. } = decode_line(&hello2).unwrap() else {
            panic!("not hello: {hello2}");
        };
        assert_eq!(id, id2, "should reuse idle session");
        b.write_all(b"printf 'GOT=%s\\n' \"$VPSD_PERSIST\"\n")
            .unwrap();
        let got = read_until(&mut b, b"GOT=keep", Duration::from_secs(3));
        assert!(
            got.windows(b"GOT=keep".len()).any(|w| w == b"GOT=keep"),
            "{}",
            String::from_utf8_lossy(&got)
        );
        let _ = std::fs::remove_file(&path);
    }
}
