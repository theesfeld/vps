# vps

Native Wayland window onto a real TTY on grok. The daemon never binds TCP or UDP. The laptop reaches it only through SSH.

```
Super+Shift+Return  →  vps  →  ssh -t grok vpsd attach  →  /dev/pts/N
```

See `PLAN.md` and `DECISIONS.md`.
