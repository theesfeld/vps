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
mod tty;

use config::DaemonConfig;
use hub::{new_shared_with_limit, SharedHub};

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
        /// Reconnect to this session id.
        #[arg(long)]
        id: Option<u64>,
        /// Always create a new PTY (default if `--id` is omitted).
        #[arg(long)]
        new: bool,
    },
    /// Print session list as one JSON line (no tty). Used by the laptop picker.
    List,
}

fn main() {
    // Writes to a closed unix socket must return EPIPE, not kill the daemon.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
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
            daemon(
                &spec,
                cfg.shell.clone(),
                cfg.mode_bits(),
                cfg.scrollback_bytes,
            )?
        }
        Cmd::Attach {
            cols,
            rows,
            shell,
            id,
            new,
        } => attach(
            cols,
            rows,
            shell.as_deref().unwrap_or(cfg.shell.as_str()),
            id,
            new,
            &cfg,
        )?,
        Cmd::List => list_cmd(&cfg)?,
    }
    Ok(())
}

fn daemon(
    spec: &str,
    shell: String,
    mode: u32,
    scrollback_bytes: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    match Listen::parse(spec)? {
        Listen::Stdio => {
            return Err("daemon cannot listen on stdio; use `vpsd attach`".into());
        }
        Listen::Unix(path) => {
            bind_unix(&path, new_shared_with_limit(shell, scrollback_bytes), mode)?
        }
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
        Message::List => {
            let sessions = hub
                .lock()
                .map_err(|e| std::io::Error::other(e.to_string()))?
                .list();
            stream.write_all(encode_line(&Message::Sessions { sessions })?.as_bytes())?;
        }
        Message::Open { cols, rows } => {
            let (id, master) = hub
                .lock()
                .map_err(|e| std::io::Error::other(e.to_string()))?
                .create(cols, rows)?;
            splice_session(&mut stream, hub, id, master)?;
        }
        Message::Attach { id, cols, rows } => {
            let master = hub
                .lock()
                .map_err(|e| std::io::Error::other(e.to_string()))?
                .attach(id, cols, rows)?;
            splice_session(&mut stream, hub, id, master)?;
        }
        other => {
            let _ = stream.write_all(
                encode_line(&Message::Error {
                    msg: format!("expected list, open, or attach, got {other:?}"),
                })?
                .as_bytes(),
            );
        }
    }
    Ok(())
}

fn splice_session(
    stream: &mut UnixStream,
    hub: SharedHub,
    id: u64,
    master: std::os::fd::OwnedFd,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pts = hub
        .lock()
        .ok()
        .and_then(|h| h.pts_name(id).map(str::to_string))
        .unwrap_or_default();
    eprintln!("vpsd: session {id} pts {pts}");
    let hub_out = hub.clone();
    let splice_result = (|| -> std::io::Result<SpliceEnd> {
        let hello = encode_line(&Message::Hello {
            v: vps_protocol::VERSION,
            id,
        })
        .map_err(std::io::Error::other)?;
        write_or_gone(stream, hello.as_bytes())?;
        let replay = hub
            .lock()
            .map_err(|e| std::io::Error::other(e.to_string()))?
            .scrollback(id);
        if !replay.is_empty() {
            write_or_gone(stream, &replay)?;
        }
        hub.lock()
            .map_err(|e| std::io::Error::other(e.to_string()))?
            .winch(id);
        splice_fds(
            stream.as_raw_fd(),
            stream.as_raw_fd(),
            master.as_raw_fd(),
            move |bytes| {
                if let Ok(mut h) = hub_out.lock() {
                    h.push_output(id, bytes);
                }
            },
        )
    })();
    {
        let mut h = hub
            .lock()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        match &splice_result {
            Ok(SpliceEnd::MasterGone) => h.drop_session(id),
            _ => h.detach(id),
        }
    }
    match splice_result {
        Ok(_) => Ok(()),
        Err(e) if is_disconnect(&e) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn is_disconnect(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::UnexpectedEof
    ) || matches!(err.raw_os_error(), Some(libc::EPIPE | libc::ECONNRESET))
}

fn write_or_gone(stream: &mut UnixStream, buf: &[u8]) -> std::io::Result<()> {
    match stream.write_all(buf) {
        Ok(()) => Ok(()),
        Err(e) if is_disconnect(&e) => Err(e),
        Err(e) => Err(e),
    }
}

fn list_cmd(cfg: &DaemonConfig) -> Result<(), Box<dyn std::error::Error>> {
    let sock = cfg.socket_path();
    let mut stream = UnixStream::connect(&sock)?;
    stream.write_all(encode_line(&Message::List)?.as_bytes())?;
    let line = read_line(&mut stream)?;
    println!("{}", line.trim());
    Ok(())
}

fn attach(
    cols: Option<u16>,
    rows: Option<u16>,
    shell: &str,
    id: Option<u64>,
    new: bool,
    cfg: &DaemonConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    if !stdin_is_tty() {
        return Err("attach requires a tty (ssh -tt grok vpsd attach)".into());
    }
    if new && id.is_some() {
        return Err("--new and --id cannot both be set".into());
    }
    let ws = match (cols, rows) {
        (Some(c), Some(r)) => pty::winsize(c, r),
        _ => pty::read_tty_winsize(libc::STDIN_FILENO).unwrap_or_else(|| pty::winsize(80, 24)),
    };
    let sock = cfg.socket_path();
    if sock.exists() {
        let req = if let Some(id) = id {
            Message::Attach {
                id,
                cols: ws.ws_col,
                rows: ws.ws_row,
            }
        } else {
            Message::Open {
                cols: ws.ws_col,
                rows: ws.ws_row,
            }
        };
        attach_via_daemon(&sock, req)?;
        return Ok(());
    }
    if id.is_some() {
        return Err("daemon is not running; cannot attach by id".into());
    }
    attach_oneshot(ws, shell)
}

fn attach_via_daemon(sock: &Path, req: Message) -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = UnixStream::connect(sock)?;
    stream.write_all(encode_line(&req)?.as_bytes())?;
    let hello = read_line(&mut stream)?;
    match decode_line(&hello)? {
        Message::Hello { id, .. } => {
            if std::env::var_os("VPSD_DEBUG").is_some() {
                eprintln!("vpsd: attached session {id}");
            }
        }
        Message::Error { msg } => return Err(msg.into()),
        other => return Err(format!("unexpected {other:?}").into()),
    }
    let _raw = tty::RawTty::enter()?;
    splice_fds(
        libc::STDIN_FILENO,
        libc::STDOUT_FILENO,
        stream.as_raw_fd(),
        |_| {},
    )?;
    Ok(())
}

fn attach_oneshot(ws: nix::pty::Winsize, shell: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut session = pty::spawn_login_shell(ws, shell)?;
    if std::env::var_os("VPSD_DEBUG").is_some() {
        eprintln!(
            "vpsd: pts {} pid {} (oneshot)",
            session.pts_name,
            session.child.id()
        );
    }
    let _raw = tty::RawTty::enter()?;
    splice_fds(
        libc::STDIN_FILENO,
        libc::STDOUT_FILENO,
        session.master.as_raw_fd(),
        |_| {},
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

fn splice_fds(
    stdin_fd: i32,
    stdout_fd: i32,
    master_fd: i32,
    mut on_master: impl FnMut(&[u8]),
) -> std::io::Result<SpliceEnd> {
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
                    let err = std::io::Error::last_os_error();
                    if is_disconnect(&err) {
                        return Ok(SpliceEnd::ClientGone);
                    }
                    return Err(err);
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
                } else {
                    on_master(&buf[..n as usize]);
                    if libc::write(stdout_fd, buf.as_ptr() as *const _, n as usize) < 0 {
                        let err = std::io::Error::last_os_error();
                        if is_disconnect(&err) {
                            return Ok(SpliceEnd::ClientGone);
                        }
                        return Err(err);
                    }
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
        let hub = new_shared_with_limit("/bin/bash".into(), 64 * 1024);
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

        let mut listing = UnixStream::connect(&path).unwrap();
        listing
            .write_all(encode_line(&Message::List).unwrap().as_bytes())
            .unwrap();
        let listed = read_line(&mut listing).unwrap();
        drop(listing);
        let Message::Sessions { sessions } = decode_line(&listed).unwrap() else {
            panic!("not sessions: {listed}");
        };
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, id);
        assert!(!sessions[0].attached);

        let mut b = UnixStream::connect(&path).unwrap();
        b.write_all(
            encode_line(&Message::Attach {
                id,
                cols: 80,
                rows: 24,
            })
            .unwrap()
            .as_bytes(),
        )
        .unwrap();
        let hello2 = read_line(&mut b).unwrap();
        let Message::Hello { id: id2, .. } = decode_line(&hello2).unwrap() else {
            panic!("not hello: {hello2}");
        };
        assert_eq!(id, id2, "should reconnect the same session");
        let replayed = read_until(&mut b, b"MARK1", Duration::from_secs(3));
        assert!(
            replayed.windows(b"MARK1".len()).any(|w| w == b"MARK1"),
            "reattach must replay prior output, got {:?}",
            String::from_utf8_lossy(&replayed)
        );
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
