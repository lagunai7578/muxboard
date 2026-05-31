# Muxboard Codex Goals

Saved goals keep mobile SSH work short. Use them instead of copying long prompts.

```bash
just goal-list
just goal-show agent-view
just goal-run agent-view
just goal-run competitive-hardening
just goal-run demo-polish
```

`just goal-run agent-view` is the easiest phone path: it runs a bounded Codex pass from the saved goal without using the interactive `/goal` UI.
`just goal-run competitive-hardening` is the longer product/QA loop for comparing muxboard against adjacent tmux agent tools and implementing one high-leverage V1-safe gap.
`just goal-run demo-polish` is the longer GitHub presentation loop: README, landing page, safe demo, screenshots, GIF, MP4, social preview, metadata, and public-surface guards.

If an interactive Codex pane is already open and you want to set its `/goal`, use:

```bash
just goal-send agent-view
```

That command only runs when it can find exactly one Codex tmux pane. If more than one Codex pane exists, pass the pane id it prints.
