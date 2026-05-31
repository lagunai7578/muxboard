Make muxboard's public demo and launch presentation feel obvious, safe, and impressive.

Follow AGENTS.md. Work as Product, Design, Engineering, QA, and Release in one bounded loop.

Start from the synthetic demo harness, README, docs/demo.md, tmux plugin docs, release page expectations, and GitHub public surface. Do not record or expose real pane data, private paths, secrets, chat transcripts, customer data, or the user's live tmux server.

Goals:
- Make the demo journey explain muxboard in under 45 seconds: see fleet, spot attention, inspect output, continue a waiting agent, select multiple panes, review a send, jump back to tmux.
- Keep setup mindless: one command starts a private demo, one command attaches, one command records, one command stops.
- Improve docs only where they remove thinking. Omit noise.
- Prefer reproducible scripts and guardrails over manual instructions.
- Keep V1 scope: no VCS, daemon, cloud, account, PR, branch, or worktree awareness.

Inspect before editing:
- scripts/demo-session
- docs/demo.md
- README.md
- docs/index.html
- docs/tmux-plugin.md
- justfile
- architecture guards around public surface, goals, and scripts

Implement exactly one coherent improvement to the demo system or launch presentation, then add the smallest durable guard that would catch a regression. Useful targets include safer synthetic panes, better scripted demo timing, cleaner GIF/cast docs, README demo placement, package hygiene, or a public-surface architecture guard.

Validate:
- bash -n scripts/demo-session
- just demo-smoke
- cargo fmt --check
- just guards
- just goal-check
- just ci if practical

Stop with a reviewable diff. Do not commit, tag, publish, force-push, upload recordings, install broad tooling, or rewrite history.
