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
| iced_term in the vps window | Rust widget over alacritty_terminal | Dies on Grok alt-screen flood; picker loop “attach ended” |

None of those is: niri app-id `vps`, red ground, Super+Shift+Return, PTY owned on grok, **no TCP/UDP bind**, **Grok reattach that stays up**.

## Architecture

```
laptop  vps (iced picker / settings / first-run terminal chooser)
           │  Enter / n / no-sessions
           ▼
        $terminal --class vps … ssh -tt grok vpsd attach --id N | --new
           ▼
grok    vpsd daemon
        Unix $XDG_RUNTIME_DIR/vpsd.sock mode 0600
        never TCP, never UDP
        posix_openpt → real /dev/pts/N → login shell
```

Mosh is the wrong tunnel for a mux. SSH is the tunnel. ControlMaster already on `Host grok`.

iced cannot host the TTY. Empirically: `ssh -tt grok vpsd attach --id 1` inside a real terminal shows the Grok UI; the same splice inside iced_term returns to the picker with “attach ended — pick the session again (Grok was still drawing)”.

The session window is **the user’s terminal**. Empty `[terminal].program` → first-run chooser (known binaries on PATH, plus a path in settings). `t` in the picker or `vps settings` switches later. Recipes use each emulator’s documented CLI (kitty, foot, alacritty, ghostty, wezterm); anything else is `program args… ssh …`.

## Crates

- `vps-protocol` — JSON messages + listen-address policy (stdio / unix only)
- `vpsd` — daemon + `attach` (run under ssh)
- `vps` — iced picker/settings/chooser; session window is the chosen terminal

## Done when

- `cargo test --workspace` is exit 0
- `cargo clippy --workspace --all-targets -- -D warnings` is exit 0
- `vps` crate does not depend on `iced_term`
- `vpsd` refuses to listen on TCP (tested)
- Empty `terminal.program` → chooser before attach; choice is written to `~/.config/vps/config.toml`
- Settings and picker `t` can change the terminal later
- Super+Shift+Return: if any PTYs exist, iced picker; Enter opens the **chosen** terminal running `ssh -tt grok vpsd attach --id N` and closes the picker
- Zero sessions (and a terminal already chosen): skip the picker, spawn `--new`
- Wayland app id is `window.app_id` (`vps`) on terminals that document a class/app-id flag
- Picker row for a grok session with a title is `grok [TITLE]`
- Close the terminal = detach; PTY stays
- grok firewall still has no extra public port
- `~/.config/vps/config.toml` / `vpsd.toml` document every knob
- README matches this
