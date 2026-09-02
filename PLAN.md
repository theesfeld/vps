# vps

Native laptop window for a real TTY on grok. Super+Shift+Return.

## Exists already

| Thing | What it is | Why not |
|---|---|---|
| WezTerm mux over SSH | Rust mux daemon, Unix socket reached *through SSH* | WezTerm as the GUI; version lock on both ends; not this compositor bind |
| Eternal Terminal | Reconnectable TTY | `etserver` listens on TCP :2022 |
| ssh-obi | SSH-stdio broker + Unix socket, early 0.1 | CLI in *your* terminal, not a native window |
| mosh | One UDP PTY, roaming | Not a mux; UDP 60000–61000 is a network listener |
| zellij / tmux | Remote multiplexer | Nested UI, not a local OS window |

None of those is: niri app-id `vps`, red ground, Super+Shift+Return, PTY owned on grok, **no TCP/UDP bind**.

## Architecture

```
laptop  vps (Wayland window)
   │    ssh -t grok vpsd attach     ← only path in
   ▼
grok    vpsd daemon
        Unix $XDG_RUNTIME_DIR/vpsd.sock mode 0600
        never TCP, never UDP
        posix_openpt → real /dev/pts/N → login shell
```

Mosh is the wrong tunnel for a mux (it *is* a PTY, and it listens on UDP). SSH is the tunnel. ControlMaster already on `Host grok`.

## Crates

- `vps-protocol` — JSON messages + listen-address policy (stdio / unix only)
- `vpsd` — daemon + `attach` (run under ssh)
- `vps` — iced + iced_term client

## Done when

- `cargo test` in the workspace is exit 0
- `vpsd` refuses to listen on TCP (tested)
- `vps` opens a red native window whose child is `ssh -t grok vpsd attach`
- Super+Shift+Return spawns `vps`
- grok firewall still has no extra public port
