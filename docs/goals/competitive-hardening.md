Make muxboard clearly stronger than adjacent tmux agent tools while staying V1-scoped.

Follow AGENTS.md. Work as Product, Design, Engineering, QA, and Release in one long bounded loop.

Context:
- Muxboard is a tmux-native command center for AI agent fleets.
- Borrow useful ideas from Claude Agent View, OpenSessions, tmux-agent-indicator, cmux, and conductor-like products.
- Do not add VCS, branch, PR, worktree, daemon, cloud, server API, or repo-management scope for V1.

Loop until you have a meaningful reviewable improvement or a hard blocker:

1. Re-audit the nearby tools from current source/docs when practical.
   - Compare only user-visible agent command-center capabilities.
   - Classify gaps as: must-have V1, nice V1 polish, V2/defer, or not muxboard.
   - Update or create a concise audit note only when it changes product judgment.

2. Inspect muxboard's current implementation before editing.
   - Fleet, Details, Output, Command Center, Send, Help, More, tmux plugin, native sources, agent bridge, action contracts, renderer/X-ray tests, and live tmux tests.
   - Identify the highest-risk gap that would make a technical user prefer the other tool.

3. Implement exactly one high-leverage improvement.
   - Prefer agent attention, lifecycle clarity, status badges, pane/session grouping, peek/reply/attach clarity, dock/peek tmux ergonomics, parser accuracy, or live-state reliability.
   - Preserve the calm minimal UI. Omit, then omit again.
   - Keep core/provider/app/tui/tmux boundaries intact.

4. Add durable regression coverage at the right layer.
   - Renderer/X-ray for visible hierarchy, spacing, color, truncation, and empty states.
   - Action-contract tests for any advertised key or menu row.
   - Live tmux tests for jump, send, dock/peek, stale state, resize, and real terminal behavior.
   - Provider/parser tests for Codex, Claude Code, opencode, or bridge ingestion.
   - Architecture guards when a boundary or V1 scope rule could regress.

5. Validate progressively.
   - Run targeted tests first.
   - Run `cargo fmt --check`.
   - Run `cargo clippy --all-targets --all-features -- -D warnings` when code changed.
   - Run `just ux`.
   - Run `just ci`.
   - Run `just dogfood` when tmux behavior, live state, plugin behavior, terminal input, layout, or action journeys changed.
   - Run `just coverage` or `just coverage-missing` if you discover a blind spot.

6. Append a concise note to `docs/agent-loop-notes.md`.
   - Name the competitor-inspired gap.
   - State what changed, what tests prove it, remaining risk, and the next obvious pass.

Stop conditions:
- Stop after one coherent reviewable diff.
- Stop if the repo is in a confusing or risky state.
- Do not commit, force-push, publish, delete broadly, rewrite history, or add VCS awareness.
