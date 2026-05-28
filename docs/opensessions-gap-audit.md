# OpenSessions gap audit

Muxboard should borrow only the pieces that make a tmux agent fleet easier to
understand and control. It should not become a repo dashboard, a daemon, or a
server-first session manager in V1.

## Brought into muxboard

- tmux plugin shapes: popup, peek drawer, dock, window, split.
- tmux status-line signals: compact status and per-session dots.
- Agent-aware ambient status: a single active bridge event names the agent
  (`mux ! codex`, `mux + claude`) instead of showing only an anonymous count.
- Configurable ambient session-dot symbols and colors, with plain ASCII defaults
  so muxboard does not fight the user's tmux theme.
- Opt-in session dots now behave as session wayfinding even when all agents are
  quiet: they still show current and quiet sessions instead of disappearing.
- Explicit agent bridge: agent, state, summary, thread, progress, log, unseen.
- Review attention: terminal `done`, `error`, and `stuck` states can stay visible
  until the user sees or mutes them.
- Inspect-to-clear review markers: opening Output or intentionally showing the
  pane in tmux marks explicit bridge review events seen so ambient status and
  session dots stop flashing.
- Focus-to-clear review markers: the TPM plugin can mark terminal `done`,
  `error`, and `stuck` bridge events seen when the user focuses the pane
  directly in tmux. Waiting prompts keep attention until they are answered.
- Native local source hints for Codex and Claude Code.

The native source hints are intentionally conservative. Muxboard scans recent
local Codex and Claude Code transcript files, distills status/title metadata, and
maps an event only when exactly one obvious matching tmux pane exists. A plain
shell in the same directory is not silently relabeled. Explicit tmux bridge state always wins over native hints.
Visible terminal state wins over stale native hints when the pane already shows
clear running, waiting, done, error, stuck, or idle provider output.
Codex parsing follows current rollout event names for turns, approvals, raw
response items, and errors. Claude parsing uses safe title/task-summary metadata
and deliberately ignores last prompts, git branches, and project/worktree fields.

## Deferred from V1

- VCS, PR, branch, dirty-tree, worktree, or review status.
- HTTP server APIs for metadata ingestion.
- OpenCode SQLite ingestion, until the value clearly justifies a SQLite
  dependency or a small optional helper.
- Amp cloud/DTW integration.
- Localhost port discovery.
- Sessionizer/new-project launcher.
- Persistent manual session order and hidden sessions.

## Product rule

Useful OpenSessions ideas are acceptable only when they strengthen muxboard's
core promise: a calm tmux command center for local or SSH agent panes. If a
feature needs repo awareness, hidden daemon state, network access, or heavy
runtime dependencies, it belongs after V1 unless it directly fixes a real fleet
control problem.
