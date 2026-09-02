# Decisions

| ID | Decision | Why |
|---|---|---|
| D1 | Build this, do not switch to WezTerm | WezTerm mux is the closest existing product; it is a different terminal emulator |
| D2 | Tunnel is SSH, not mosh | mosh is one UDP PTY and must bind 60000–61000; a mux must not |
| D3 | `vpsd` listen = Unix socket or stdio only | User: not available to any outside source |
| D4 | Payloads JSON (NDJSON control); PTY bytes are raw on the attach stream | Control messages are JSON; the TTY itself is a byte pipe once attached |
| D5 | Config TOML | `~/.config/vps/config.toml` |
| D6 | iced + iced_term + alacritty_terminal | Documented Rust terminal widget; child program is ssh |
| D7 | Real PTY via `nix::pty::openpty` + TIOCSCTTY | POSIX pty(7), not a pipe |
| D8 | Daemon owns PTYs; attach is a splice | Close window = detach; next Open reuses idle |
| D9 | Two TOML files, every knob named | Laptop `config.toml`, grok `vpsd.toml`; shipped copies are the defaults |
