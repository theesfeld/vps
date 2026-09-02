# Status

Phase: iced picker + user-chosen TTY.

- Close window / laptop: SSH dies, daemon detaches, PTY stays.
- Super+Shift+Return lists sessions (`vpsd list`). If any exist, iced picker. Enter / `n` / zero-session fast path spawn the **chosen terminal** running `ssh -tt grok vpsd attach`.
- First run (empty `[terminal].program`): chooser of known terminals on PATH; writes `~/.config/vps/config.toml`. If that binary is deleted or not executable, the chooser comes back. Switch later with picker `t` or `vps settings`.
- iced_term is gone. It could not ingest Grok’s alt-screen flood; reconnect dumped back to “attach ended — pick the session again”.
- Picker label: `grok [generated_title]` when vpsd fills `SessionInfo.title`.
- `open` always creates; reconnect is `attach --id`.
- `cargo test` / `clippy -D warnings` must pass before install.
