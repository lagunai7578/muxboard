You are working on muxboard.

Follow AGENTS.md strictly. Make one deep, bounded improvement pass toward:

- conductor.build-level tmux/agent command-center polish without VCS awareness,
- Steve Jobs-level UI/UX simplicity,
- stronger renderer/X-ray tests,
- stronger live/e2e/user-journey guarantees.

Rules:

1. Inspect before editing.
2. Pick exactly one high-value bounded pass.
3. Name the risk before editing.
4. Make the smallest durable code/test/doc change that reduces that risk.
5. Add or update tests at the right layer.
6. If visible copy, footer, help, menu, status, or action promises changed, press the advertised keys in tests.
7. Use `just coverage-missing` when a coverage-guided risk or escaped bug is part of the pass.
8. Run narrow checks, then `just ux`; run `just ci` and `just perf-live` before stopping when practical. If a gate is too costly or blocked, say exactly why.
9. Append a concise note to docs/agent-loop-notes.md with the pass, checks, and remaining risk.
10. Do not commit, force-push, publish, delete files broadly, or rewrite history.
11. Stop if the repo is already in a risky or confusing state.
