# Status

Phase: session picker + settings window (`vps settings` or picker `s`).

- Close window / laptop: SSH dies, daemon detaches, PTY stays.
- Super+Shift+Return lists sessions (`vpsd list`) and shows a picker when any exist: reconnect idle, skip live, or `+ new`.
- `open` always creates; reconnect is `attach --id`.
- `cargo test` / `clippy -D warnings` pass.
