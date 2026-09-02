# Status

Phase: native window works. Super+Shift+Return → `vps` → `ssh -tt grok vpsd attach` → `/dev/pts/N` on grok. `vpsd` listens on `$XDG_RUNTIME_DIR/vpsd.sock` only (no TCP/UDP). `cargo test` and `cargo clippy -D warnings` pass.

Left: persistence across client close (daemon already binds; attach is still one-shot per SSH). Release `vps` binary (debug is installed for the live window).
