# Status

Phase: persistence + documented TOML.

- Close window detaches; next `Open` reuses the idle PTY (`persist_session_across_disconnect`).
- Client `~/.config/vps/config.toml` and daemon `vpsd.toml` — shipped copies in `config/` are the defaults; every knob is a key.
- README covers architecture, threat model, install, config tables.
- `cargo test` / `clippy -D warnings` pass.
