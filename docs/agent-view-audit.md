# Agent View Lessons For Muxboard

Claude Code Agent View is useful because it turns many running coding sessions
into one attention queue. The relevant ideas are visible in the official
[Agent View announcement](https://claude.com/blog/agent-view-in-claude-code) and
[Agent View docs](https://code.claude.com/docs/en/agent-view): one screen,
clear session state, quick peek, inline reply, and attach only when needed.

Muxboard should copy the product shape, not the Claude-specific machinery.

## Copy

- **Attention-first grouping.** The first screen should answer what needs the
  user, what is still working, and what is quiet or finished.
- **Peek before attach.** The selected pane should summarize state, blocker,
  action, reply path, and recent output before pushing the user into tmux.
- **Reply in place.** If an agent is waiting, muxboard should make the reply path
  visible without a tutorial. `A` continues enter-safe prompts, `.` exposes
  yes/no answers, `:` sends typed text, and `G` shows the real tmux pane.
- **Attach is intentional.** Jumping to tmux should be visually and semantically
  different from opening Output inside muxboard.
- **Dispatch stays close.** Starting or sending work should happen from the same
  command-center surface that shows status.
- **Local organization.** Pins, saved fleets, muting, and local labels are in
  scope when they reduce scanning effort without becoming project management.

## Do Not Copy For V1

- Claude-only background session storage.
- Hidden supervisor daemons.
- VCS, PR, branch, worktree, or review status.
- Automatic cleanup of agent-owned repos or worktrees.
- Assumptions that every pane is Claude Code.

Muxboard's advantage is different: it is cross-agent and tmux-native, observing
and controlling real panes across Codex, Claude Code, opencode, shells, and
generic jobs, locally or over SSH.

## Current V1 Mapping

- Agent View's "needs input" maps to muxboard `needs you`.
- Agent View's "working" maps to muxboard running agent panes.
- Agent View's "peek" maps to Details and Output.
- Agent View's "reply" maps to `A` for safe Enter prompts, `.` for yes/no
  answers, and `:` for typed sends.
- Agent View's "attach" maps to `G show in tmux`.
- Agent View's "dispatch" maps to Start and Send.
- Agent View's "pin/rename/mute" maps to macro pins, saved fleets, and alert
  muting.

## This Pass

Fleet health now keeps working agents visible even when another agent needs the
user, for example `1 needs you, 1 working`. Waiting Details reads like a command
card: the primary `Action:` includes the actual key, enter-safe panes say
`A continue`, choice prompts say `. answer yes/no`, and free-form prompts say
`: reply`. A `Reply:` line appears only when it adds a real alternate path.

The Command Center now treats prompt states as first-class actions too. Choice
prompts surface `. answer`, free-form prompts surface `: reply`, and muxboard
selects the attention pane before opening either reply path. When a working pane
is selected but another agent needs attention, the card says `Selected:` for the
working pane and reserves `Action:` for the waiting agent, so the target of the
safest action stays unmistakable without forcing an attach.

Waiting footers now follow the same hierarchy: when the card says `Action: :
reply`, the footer shows `: reply` before lower-value peeks like `Enter output`.
Empty Output does the inverse: if there is nothing to read yet, it leads with
`Esc back` instead of advertising inert movement.

## End-to-End Rendered Audit

Each row names the first obvious action the screen should invite. These are
backed by exact renderer fixtures unless noted.

| Journey | Artifact | Obvious action |
| --- | --- | --- |
| First launch with multiple agents | `mixed_fleet_dashboard.txt` | Reply to the waiting agent, or move to the working/quiet/checking pane. |
| Move through mixed agents | `mixed_fleet_after_moving_to_checking.txt` | `G show` for a checking pane, without stale reply hints or fake output. |
| One waiting agent | `wide_selected_waiting_panel.txt`, `narrow_selected_waiting_panel.txt` | `: reply`, with `G show` as the intentional attach path. |
| One working agent with long output | `working_agent_dashboard.txt` | `Enter output` to inspect more, or `G show` to attach. |
| Multiple waiting agents | `multi_attention_command_center.txt` | `A continue` for the selected queue item, with hidden waiting work counted. |
| Empty/no tmux/no search match | `empty_tmux_board.txt`, `empty_command_center.txt`, `empty_search_board.txt`, `no_match_command_center.txt`, `empty_output_overlay.txt`, `empty_navigator_overlay.txt`, `no_tmux_server_first_run_explains_recovery` | Backspace, Esc, or `R refresh`, never a fake send or inert movement hint. |
| Command Center | `overview_attention_overlay.txt`, `command_center_from_working_selection.txt`, `empty_command_center.txt`, `no_match_command_center.txt` | Take the shown `Action:` after the state is clear, before browsing secondary lanes. |
| Details and Output scrolling | `opened_output_overlay.txt` plus scrollbar renderer tests | `K older/J newer`, with `Esc back` always visible. |
| tmux plugin modes | live tmux plugin tests | Prefix toggle opens/closes dock or peek without surprising the layout. |

This audit is deliberately V1-scoped: no git, branch, PR, worktree, or daemon
state is allowed into these first-screen promises.
