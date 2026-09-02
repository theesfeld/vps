# Decisions

| ID | Decision | Why |
|---|---|---|
| D1 | Build this, do not switch to WezTerm | WezTerm mux is the closest existing product; it is a different terminal emulator |
| D2 | Tunnel is SSH, not mosh | mosh is one UDP PTY and must bind 60000–61000; a mux must not |
| D3 | `vpsd` listen = Unix socket or stdio only | User: not available to any outside source |
| D4 | Payloads JSON (NDJSON control); PTY bytes are raw on the attach stream | Control messages are JSON; the TTY itself is a byte pipe once attached |
| D5 | Config TOML | `~/.config/vps/config.toml` |
| D6 | iced picker + **user-chosen** TTY | iced_term dies on Grok alt-screen flood (picker loop). `ssh -tt grok vpsd attach` in a real tty works. Keep iced for picker/settings/chooser. Session window is whatever `[terminal].program` names. Empty program → first-run chooser. Recipes use each emulator’s documented CLI (kitty, foot, alacritty, ghostty, wezterm). |
| D12 | Picker `niri msg action spawn -- vps attach`; attach execs the TTY | niri kills `app-niri-vps-*.scope` when that process exits. Kitty started *inside* the picker scope dies with the picker. A second niri spawn of `vps attach` execs kitty so that scope *is* the TTY. |
| D7 | Real PTY via `nix::pty::openpty` + TIOCSCTTY | POSIX pty(7), not a pipe |
| D8 | Daemon owns PTYs; attach is a splice | Close window = detach; PTY stays |
| D10 | Super+Shift+Return shows a picker | List idle/live sessions; choose reconnect or new. No silent grab of the first idle PTY. |
| D9 | Two TOML files, every knob named | Laptop `config.toml`, grok `vpsd.toml`; shipped copies are the defaults |
