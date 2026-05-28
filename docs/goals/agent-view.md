Audit Claude Code Agent View and adapt only the best V1-safe ideas into muxboard.

Research official Agent View docs, release notes, and user feedback. Produce a concise muxboard design note mapping useful primitives to muxboard equivalents: attention groups, peek/reply, attach/jump, dispatch, filtering, pin/rename, lifecycle, and what not to copy.

Then inspect Fleet, Details, Output, Send, tmux plugin, action-contract, and renderer/X-ray tests. Implement the highest-leverage V1 improvement that makes muxboard feel more like an agent command center without adding VCS/worktree/daemon scope.

Prioritize:
- attention-first grouping: Needs you, Working, Quiet, Done/Error
- minimal peek/reply for selected panes
- clearer attach/jump semantics
- truthful footer/help copy
- renderer/X-ray coverage for visible states
- action-contract tests for advertised keys

Do not add VCS, PR status, hidden daemon supervision, or Claude-only assumptions. Keep muxboard cross-agent and tmux-native.

Validate with fmt, clippy, targeted tests, just ux, just guards, and just ci if practical.
