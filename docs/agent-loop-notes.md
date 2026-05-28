## 2026-05-24 - Make opt-in session dots stay useful when all quiet

STATUS: DONE.

EVIDENCE: Follow-up competitive pass against tmux-agent-indicator found a small
but user-visible mismatch: muxboard documented `#{muxboard_session_dots}` as one
mark per session, but the helper returned nothing until at least one pane had an
agent bridge event. That made the status placeholder feel broken on a calm tmux,
exactly when session wayfinding should be boring and stable. The Rust status
helper and TPM shell helper now render quiet/current dots even with no agent
events, while `#{muxboard_status}` stays empty so the status line does not invent
activity. Added Rust coverage for an all-quiet snapshot, plugin smoke coverage
for quiet sessions, and live tmux coverage proving the real helper prints quiet
plus current dots before any agent state exists.

Checks run: `cargo fmt --check`; `cargo test --lib session_dots --
--nocapture`; `just tmux-plugin-check`; `cargo test --test live_e2e
tmux_plugin_status_widgets_render_agent_names_and_custom_dots_live -- --ignored
--nocapture`; `cargo test --test architecture_guards
tmux_plugin_entrypoint_stays_tpm_compatible -- --nocapture`; `cargo clippy
--all-targets --all-features -- -D warnings`; `just ux`; `just ci`; `just
dogfood`; `git diff --check`.

RISK: low. The change only affects users who opted into the dots placeholder;
the default tmux status line remains untouched, and the textual status segment
still stays blank when there is no agent activity.

NEXT: keep the next competitor pass focused on one visible, local tmux-native
gap; do not add VCS, cloud, daemon, or broad workflow scope.

## 2026-05-24 - Clear review markers when users visit panes directly

STATUS: DONE.

EVIDENCE: Competitive pass against tmux-agent-indicator and OpenSessions found
one lifecycle gap after the prior ambient-status work: muxboard cleared terminal
review markers when users opened Output or used `G show`, but a user who simply
visited the pane with normal tmux navigation could leave `#{muxboard_status}`
and `#{muxboard_session_dots}` flashing. The TPM plugin now registers focused
pane hooks by default through `@muxboard-mark-seen-on-focus 'on'`. The new
`muxboard-mark-seen` helper marks explicit `done`, `error`, and `stuck` review
events seen for both muxboard-native and tmux-agent-indicator-compatible env
keys, then refreshes tmux status. It deliberately does not clear `waiting`
states, because a prompt still needs an answer even after the pane is focused.
Docs now describe the behavior and the OpenSessions gap audit records the
borrowed lifecycle idea.

Checks run: `bash -n muxboard.tmux extras/tmux/scripts/muxboard-mark-seen
extras/tmux/scripts/muxboard-plugin-smoke`; `just tmux-plugin-check`; `cargo
test --test architecture_guards tmux_plugin_entrypoint_stays_tpm_compatible --
--nocapture`; `cargo test --test live_e2e
tmux_plugin_focus_marks_terminal_review_seen_live -- --ignored --nocapture`;
`cargo test --test architecture_guards
dogfood_stays_aligned_with_non_perf_live_e2e_tests -- --nocapture`; `cargo fmt
--check`; `cargo clippy --all-targets --all-features -- -D warnings`; `just
ux`; `just ci`; `just dogfood`; `git diff --check`. `just ux` initially caught
an inline sleep in the new live test; the test now uses a named tmux state wait.

RISK: low. The hook only clears terminal review states after focus, preserves
waiting attention, is configurable off, and is covered by smoke, architecture,
and live tmux tests.

NEXT: audit whether session-level direct-jump bindings would add value without
making the tmux plugin noisy.

## 2026-05-24 - Make ambient tmux status more informative without noise

STATUS: DONE.

EVIDENCE: Competitive audit against tmux-agent-indicator and OpenSessions found
that muxboard's ambient tmux status segment was still anonymous for the most
common case: one pane needs attention or one agent is running. `#{muxboard_status}`
now keeps aggregate counts for multi-agent states, but names the single useful
agent when that is more informative: `mux ! codex`, `mux + claude`, or
`mux done codex`. Generic/missing agent names still fall back to counts, so
legacy hooks do not produce noisy `agent` labels. Added Rust status-segment
coverage, CLI smoke coverage, tmux plugin smoke coverage for native and
tmux-agent-indicator style env keys, and architecture guard coverage for the
documented output. The same pass closed the low-risk session-dot customization
gap from tmux-agent-indicator: `#{muxboard_session_dots}` still defaults to
plain `!+*.` ASCII marks, but users can optionally set dot symbols and tmux
foreground colors without muxboard recoloring panes or window titles. Added a
live tmux regression proving the real helpers read tmux environment state and
custom dot options from an actual server.

Checks run: `cargo test --lib status_segment -- --nocapture`;
`cargo test --test cli_smoke status_subcommands_render_agent_bridge_state --
--nocapture`; `cargo test --test architecture_guards
tmux_plugin_entrypoint_stays_tpm_compatible -- --nocapture`; `bash -n
extras/tmux/scripts/muxboard-status extras/tmux/scripts/muxboard-plugin-smoke`;
`extras/tmux/scripts/muxboard-plugin-smoke`; `cargo fmt --check`; `cargo
clippy --all-targets --all-features -- -D warnings`; `just ux`; `just ci`;
`just dogfood`; repeated focused plugin smoke and guard checks after adding
session-dot customization; repeated `cargo clippy --all-targets --all-features
-- -D warnings`, `just ux`, `just ci`, and `just dogfood`; `cargo test --test
live_e2e tmux_plugin_status_widgets_render_agent_names_and_custom_dots_live --
--ignored --nocapture`; `cargo test --test architecture_guards
dogfood_stays_aligned_with_non_perf_live_e2e_tests -- --nocapture`; final
`git diff --check`, `just ci`, and `just dogfood`.

RISK: low. The changes are limited to opt-in tmux status-line text and symbols,
with defaults preserved and full plugin, UX, CI, and live dogfood validation
green.

NEXT: consider whether status-right should expose a compact selected-session
tooltip later, without adding another default status segment.

## 2026-05-24 - Clear ambient review markers when the user inspects them

STATUS: DONE.

EVIDENCE: Competitive hardening pass against tmux-agent-indicator and OpenSessions found a subtle ambient-status gap: explicit bridge `done`, `error`, and `stuck` events could keep `#{muxboard_status}` and `#{muxboard_session_dots}` flashing after the user intentionally opened Output or jumped to the pane. Muxboard now marks those explicit bridge review events seen on Output open and successful `G show`, updates the in-memory snapshot immediately, and writes both muxboard-native and tmux-agent-indicator-compatible `UNSEEN=0` env keys before refreshing the tmux status line. Added fake-tmux app tests for Output and jump paths plus a live tmux E2E proving a real explicit review marker is cleared by opening Output. Updated the tmux plugin docs and OpenSessions gap audit.

Checks run: `cargo fmt --check`; `cargo test --lib bridge_review_seen -- --nocapture`; `cargo test --test live_e2e opening_output_marks_explicit_agent_review_seen_live -- --ignored --nocapture`.

RISK: low. The change only touches explicit bridge review events after intentional inspection; waiting/running states still remain visible, and failures to update tmux env do not block the user's Output or jump action.

NEXT: run `cargo clippy --all-targets --all-features -- -D warnings`, `just ux`, `just ci`, and `just dogfood` because the change affects live tmux status behavior.

## 2026-05-23 - Make the default theme belong to the terminal

STATUS: DONE.

EVIDENCE: Product/Design/QA pass on `/tmp/muxboard-color-goal.md` found the
actual mismatch: fresh muxboard and generated dotfile examples still treated
Light/Catppuccin Latte as the first-run/default path, while terminal-native
System Colors was only an option. The default `ThemePreset` and generated config
now use `TerminalNative`; first-run onboarding highlights System Colors first;
`Esc` on first run keeps System Colors; `muxboard --theme default` resolves to
the terminal-native preset; README and the theme audit describe the same default.
Terminal-native selection now uses native reverse video instead of a hardcoded
blue background, while warning/error/target/watch states still use semantic ANSI
slots and non-color shape cues. Added X-ray tests proving default cells use
terminal foreground/background reset, selected rows use reverse video, attention
markers keep warning semantics, and theme/config/docs guards all agree. Captured
the current wide, narrow, mixed, and working golden grids before closing; text
goldens did not need changes because the fix is style/config/onboarding.
Checks run: `cargo test --lib theme -- --nocapture`; targeted config, startup,
CLI, README, theme-audit, and architecture-guard tests; `cargo run --quiet --
--print-config-example`; `cargo fmt --check`; `cargo clippy --all-targets
--all-features -- -D warnings`; `just tui-golden`; `just ux`; `just ci`; `git
diff --check`.

RISK: medium-low. The change affects first-run theme choice, generated config,
and default selection styling, but not tmux IO, provider inference, command
dispatch, or layout. The main compatibility risk is users who expected `default`
to mean Calm; `calm` remains explicit, while `default` now matches the actual
first-run behavior.

NEXT: audit the color goal line by line before marking complete.

## 2026-05-23 - Make Fleet attention states visible without shouting

STATUS: DONE.

EVIDENCE: Product/Design/QA pass on `/tmp/muxboard-next-goal.md` captured the
current waiting, mixed-fleet, working, selected, and narrow golden grids before
changing the state treatment. The highest-friction issue was that Fleet rows
could look muddy when selection, send-list targeting, and attention overlapped:
targeted rows could mask "needs you", and selected attention rows did not keep
semantic marker/latest styling. Fleet row tones now separate waiting attention,
error/stuck alerts, watching, targeted, staged, and selected states. Waiting
rows use the warning slot, error/stuck rows use danger, watching rows are muted,
and selected attention rows keep selected background while preserving warning
marker/latest text. Targeting no longer hides a row that needs attention. Added
cell-level renderer coverage across every theme preset, row-tone priority tests,
and a regression proving targeted waiting rows still render as attention. Checks
run: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; targeted theme/row-tone/attention tests; targeted Command Center
tests; full `cargo test --lib`; `just ux`; `just ci`; `git diff --check`.

RISK: low. The changes are semantic presentation/style decisions in Fleet row
rendering and row-tone priority; tmux IO, dispatch, key routing, provider
inference, and command execution are unchanged. The main risk is theme contrast,
covered by all-theme cell-level assertions and existing no-color/dumb terminal
guards.

NEXT: run the full release loop and then decide whether attention should also
drive tmux plugin-level badges or popup titles.

## 2026-05-23 - Make all-clear Command Center states feel alive

STATUS: DONE.

EVIDENCE: Product/Design/QA pass on `/tmp/muxboard-next-goal.md` found that a
running fleet with no attention still made `: send` look like the primary next
step. Command Center now treats that as a calm all-clear state: it leads with
`All clear: 1 agent working`, makes `Enter output` the primary safe action for
the selected running pane, and removes `: send` from the all-clear footer so the
surface does not create fake action pressure. A single cleared attention item
now renders `Watching: ...` plus `Action: G show in tmux`, keeping the deliberate
tmux jump obvious without implying that continue/reply/answer attached. Added
app-state coverage for all-clear running copy, strengthened the one-item
watching guard, and updated renderer/action-contract tests for multi-running
all-clear, footer truth, and Enter behavior. Checks run: `cargo fmt --check`; `cargo clippy
--all-targets --all-features -- -D warnings`; targeted Command Center tests;
full `cargo test --lib`; `just ux`; `just ci`; `git diff --check`.

RISK: low. The change is presentation and key-contract routing for the
Command Center all-clear state; tmux send, jump, reply, answer, and continue
primitives are unchanged. The main risk is over-hiding `: send` where send is
the true primary action, covered by marked-target and lane Command Center tests.

NEXT: audit the active goal against the actual artifacts before completion.

## 2026-05-22 - Make Command Center actions feel like clearing an inbox

STATUS: DONE.

EVIDENCE: Product/Design/QA pass on `/tmp/muxboard-next-goal.md` found that
safe Command Center actions could send the right input but leave the surface
looking as if nothing had changed until tmux output refreshed. Added an
in-memory watching state for acted-on attention panes: after `A continue`,
Command Center promotes the next actionable item, keeps the acted item visible
under `Watching`, and reports either `Next: ...` or `Watching for update`.
Reply and yes/no answer actions from Command Center get the same post-action
feedback without attaching to tmux. Added app-state tests for promotion,
watching visibility, and clearing on new output, plus renderer/X-ray coverage
proving the next queue item renders above the watching item. Checks run:
targeted Command Center tests; full `cargo test --lib`; `cargo fmt --check`;
`cargo clippy --all-targets --all-features -- -D warnings`; `just ux`; `just
ci`; `git diff --check`.

RISK: medium-low. The change is in-memory app state and presentation; tmux send
primitives are unchanged. The main risk is stale watching state, covered by
fingerprinting recent pane output and reconciling on output, tick, and refresh.

NEXT: audit the goal checklist against actual artifacts before marking
complete.

## 2026-05-22 - Make Command Center queue rows explain why now

STATUS: DONE.

EVIDENCE: Product/Design/QA pass on the active triage goal found the remaining
inbox gap in Command Center: queue rows named the action and target, but not why
the item needed attention. That forced the user to inspect Details or Output to
distinguish `reply`, `answer`, `continue`, and `output` rows. Queue rows now add
a terse reason when useful: `network access`, `approval needed`, `yes/no
choice`, `needs Enter`, or an error/stuck detail. Added app coverage for reply,
answer, continue, error, and reordered multi-pane attention queues, and updated
the exact X-ray goldens for the overview Command Center, off-selection Command
Center, and multi-attention Command Center. Checks run: `cargo fmt --check`;
`cargo clippy --all-targets --all-features -- -D warnings`; targeted attention
queue tests; targeted Command Center tests; exact golden tests; `just ux`;
`just ci`; `git diff --check`.

RISK: low. The change is presentation-only and reuses existing distilled
provider/output summaries, so it does not alter tmux control, command dispatch,
selection, or provider inference.

NEXT: audit `/tmp/muxboard-next-goal.md` against the rendered artifacts before
completing the goal.

## 2026-05-18 - Keep Reply submissions out of command-send mental model

STATUS: DONE.

EVIDENCE: The Reply composer was correctly labeled while typing, but submitting
still went through the generic send path and reported `Sent command ...`, which
made a safe inbox reply look like fleet command dispatch and also polluted recent
command history. Captured the Reply context before closing the input, added a
dedicated reply dispatch wrapper that sends through tmux without staging or
remembering, and changed post-action feedback to `Sent reply to ...`. Added an
app test proving Reply sends literal text plus Enter, does not select or attach
in tmux, and does not enter recent commands. Strengthened the Command Center
off-selection action-contract test so `: reply`, typed text, and `Enter` target
the attention pane and leave muxboard oriented. Checks run: `cargo fmt --check`;
`cargo clippy --all-targets --all-features -- -D warnings`; targeted Reply and
Command Center action-contract tests; `just ux`; `just ci`; `git diff --check`.

RISK: low. The actual tmux send primitive is unchanged; only the Reply path's
status copy, staging policy, and command-history behavior now match the visible
surface.

NEXT: do the final objective audit against `/tmp/muxboard-next-goal.md` and
complete the goal only if every named journey and validation gate is covered by
current evidence.

## 2026-05-18 - Let Command Center answer without attaching

STATUS: DONE.

EVIDENCE: The Command Center still treated off-selection choice and free-form
prompts as `G show waiting`, which made the safest triage path an attach path.
Changed the primary Command Center action so it selects the attention pane and
opens `: reply` or `. answer` directly, while keeping `G show` as the
intentional attach path. Updated the working-selection golden, queue overflow
summaries, action-contract coverage, dogfood recipe coverage, and a live tmux
test proving `. answer` does not switch the target tmux window. Also tightened
empty Browse geometry so no-match recovery renders as a two-line state/action
card instead of a tall empty box. Checks run: `cargo fmt --check`; `cargo
clippy --all-targets --all-features -- -D warnings`; targeted Command Center,
empty Browse, exact golden, action-contract, manifest, and live tmux tests;
`just ux`; `just ci`; `git diff --check`.

RISK: low. The change is limited to Command Center primary action routing and
sparse Browse overlay sizing. Direct tmux attach remains available through `G`,
and reply/answer paths are covered before dispatch.

NEXT: audit the active goal against the rendered triage artifacts and complete
it if no requirement is uncovered.

## 2026-05-17 - Make More list the primary action before peeks

STATUS: DONE.

EVIDENCE: X-ray review of `actions_overlay.txt` showed a subtle hierarchy
break: the card said `Action: : send this pane`, but the first listed View row
was `Enter show output`. That made the modal ask the user to reconcile the
recommendation with a lower-value peek. Reordered More's View prioritization so
`: reply` and `: send text` appear before secondary Output/Details peeks when
they are present. Added a renderer test proving both send and reply More
surfaces list the primary action before `Enter show output`, and updated the
exact `actions_overlay.txt` golden. Checks run: `cargo fmt --check`; `cargo
clippy --all-targets --all-features -- -D warnings`; targeted More hierarchy,
narrow More, sparse Command Center, exact golden, and manifest tests; full
exact golden suite; `just ux`; `just ci`; `git diff --check`.

RISK: low. The key router and action set are unchanged; this only reorders
visible rows inside More so the list matches the recommendation.

NEXT: audit the active goal against the actual artifacts before completing it.

## 2026-05-17 - Make sparse Command Center overlays hug the decision

STATUS: DONE.

EVIDENCE: X-ray review of the empty and no-match Command Center goldens showed
the state/action pair sitting inside a tall, mostly empty modal. That violated
the current goal's empty-state rule: state plus recovery, not dead space. Added
a Command Center overlay kind that keeps normal busy Command Center surfaces
roomy, but lets two-line recovery states render as a tight four-row card. Added
an X-ray renderer test proving both empty and no-match Command Center overlays
contain only the state and recovery action, updated the exact goldens, and
tightened manifest `max_boxed_lines` for those two fixtures to 2 boxed content
rows. Checks run: `cargo fmt --check`; `cargo clippy --all-targets
--all-features -- -D warnings`; targeted sparse Command Center geometry,
golden, and manifest tests; full exact golden suite; `just ux`; `just
perf-smoke`; `just ci`; `git diff --check`.

RISK: low. The change is limited to sparse Command Center overlay geometry; the
normal Command Center, More, Help, Send, Reply, and Output sizing paths stay
unchanged.

NEXT: audit the goal against actual artifacts before completing it.

## 2026-05-17 - Make empty Command Center state-first

STATUS: DONE.

EVIDENCE: The new empty Command Center golden exposed a sparse card that only
said `Action: start tmux panes, then R refresh` and otherwise looked unfinished.
Changed Command Center empty states to show the state before the recovery
action: `No panes yet.` before refresh, and `No matching panes.` before
backspace recovery. Added exact X-ray goldens for `empty_command_center.txt`
and `no_match_command_center.txt`, manifest review metadata for both, and
updated control-line tests so this state/action hierarchy cannot silently
regress. Checks run: targeted Command Center tests; exact golden suite; golden
manifest guard; `cargo fmt --check`; `cargo clippy --all-targets
--all-features -- -D warnings`; `just ux`; `just ci`; `git diff --check`.

RISK: low. The change is presentation-only and preserves the existing `R
refresh` and backspace recovery action contracts.

NEXT: audit the active goal against the rendered artifacts before completing it.

## 2026-05-17 - Put the refresh key in the no-pane first screen

STATUS: DONE.

EVIDENCE: The first-run/no-pane board showed `Start tmux panes, then refresh.`
in Details while the footer exposed the actual `R refresh` action. That forced
the user to reconcile body copy with the footer instead of seeing the obvious
next key in the main card. Updated the no-pane Details recovery line to
`Start tmux panes, then R refresh.`, added the exact
`empty_tmux_board.txt` X-ray golden, added manifest review metadata, and mapped
the no-tmux journey in `docs/agent-view-audit.md`. Checks run:
`cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; targeted empty-tmux golden, empty-tmux usability, tiny-recovery,
presentation, and golden-manifest tests; exact golden suite; `just ux`; `just
ci`; `git diff --check`.

RISK: low. This is copy plus renderer coverage; the refresh action already
existed and remains action-tested.

NEXT: audit the active goal against actual artifacts before marking complete.

## 2026-05-17 - Put the primary reply before secondary peeks

STATUS: DONE.

EVIDENCE: Product review of the mixed-fleet and selected-waiting goldens showed
the body saying `Action: : reply` while the footer listed `Enter output` first.
Reordered the normal footer only when `:` is the selected pane's primary reply
action, so reply appears before the secondary Output peek without changing send
lists or non-reply panes. Updated the narrow, wide, and mixed-fleet goldens and
added an X-ray footer-order assertion to
`usability_action_hierarchy_keeps_primary_actions_visible_across_surfaces`.
Checks run: `cargo fmt --check`; exact golden suite; targeted action hierarchy
test; `cargo clippy --all-targets --all-features -- -D warnings`; `just ux`;
`just ci`; `git diff --check`.

RISK: low. The change is copy ordering only, and action-contract tests still
prove the advertised keys execute.

NEXT: keep pressure-testing first-load and Command Center screens for any other
case where the footer order conflicts with the body action hierarchy.

## 2026-05-17 - Make empty Output recover instead of advertising movement

STATUS: DONE.

EVIDENCE: X-ray review of `empty_output_overlay.txt` showed an empty Output
surface still promoting `J/K move`, even though Output had no useful content to
read. Added a shared default movement footer helper so empty Output with `No
output yet.` suppresses inert movement copy and leads with `Esc back`, while
useful non-scroll output still keeps normal fleet movement. Strengthened the app
footer contract, renderer assertion, exact golden, and golden manifest so this
empty state must not regress. Checks run: `cargo fmt --check`; `cargo clippy
--all-targets --all-features -- -D warnings`; targeted movement, empty-output,
golden, and previously failing footer hierarchy tests; golden manifest guard;
`just ux`; `just ci`; `git diff --check`.

RISK: low. The change is footer-only; real movement still works if pressed, but
the empty Output layer no longer recommends a low-value action.

NEXT: continue reviewing first-ten-second states for any remaining place where
footer copy promises a less useful action than the screen body.

## 2026-05-17 - Stop empty output detours on checking panes

STATUS: DONE.

EVIDENCE: X-ray review of `mixed_fleet_after_moving_to_checking.txt` showed a
checking job with no useful output still advertised `Enter output` in the
footer, competing with the actual `G show in tmux` next step. The footer now
only advertises Output when the selected pane has useful filtered output or is a
known agent harness where Output can become useful immediately. Generic
checking jobs and prompt-only shells do not get an empty Output detour. Added a
renderer assertion and manifest guard so the checking-pane screen must not show
`Enter output`, updated the golden, and kept the helper cheap enough for the
render loop. Checks run: `cargo fmt --check`; clippy with denied warnings;
targeted checking golden, width-aware footer, layout preset, manifest, and
scroll-render perf tests; full exact golden suite; `just ux`; `git diff --check`
passed; `just ci` passed after the footer/perf adjustment; `just
coverage-full-gate` passed after updating live E2E waits for panes that no
longer advertise empty Output, with 65 live E2E tests and 98.34% lines, 97.68%
regions, and 96.46% functions.

RISK: low. The change is limited to footer copy; `Enter` still opens Output if a
user presses it, but the UI stops recommending a low-value empty detour.

NEXT: review the accumulated dirty tree, then squash or commit when the user is
ready.

## 2026-05-17 - Protect mixed-fleet J/K movement

STATUS: DONE.

EVIDENCE: Closed the rendered-state gap for moving through a mixed fleet. Added
`mixed_fleet_after_moving_to_checking.txt`, rendered after pressing the real `J`
key three times through waiting, working, quiet, and checking panes. The screen
now has exact-grid coverage that the selected card updates to
`demo/pending`, says `State: Checking`, makes `G show in tmux` the primary
action, keeps the global attention count visible, and does not carry stale
`: reply`, fake `Output`, `unknown`, `NEXT=`, or `STATUS=` copy from prior panes. Added
manifest metadata and mapped the journey in `docs/agent-view-audit.md` and
`docs/testing-matrix.md`. Checks run: `cargo fmt --check`; exact golden suite;
golden manifest guard; `cargo clippy --all-targets --all-features --
-D warnings`; `just ux`; `git diff --check`.

RISK: low. This adds renderer/action coverage for existing movement behavior
without changing production app logic.

NEXT: run the final goal completion audit against `/tmp/muxboard-next-goal.md`
and only mark complete if every named journey and validation gate has concrete
evidence.

## 2026-05-17 - Make Command Center primary actions target the promised pane

STATUS: DONE.

EVIDENCE: Reviewed the active goal loop and found a contract bug in the
off-selection Command Center state: the screen could say `G show waiting
demo/approval` while the key router still acted on the selected working pane.
Centralized the Command Center primary action target and routed `A`, `Enter`,
`:`, `.`, and `G` through that target before falling back to normal selected-pane
actions. Added exact renderer proof for opening Command Center while a working
pane is selected and another pane needs attention, added an action-contract test
that presses the promised keys for off-selection continue/output/show, and added
a live tmux E2E proving `G show waiting` focuses the attention pane. Checks run:
`cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`;
`cargo test --lib command_center -- --nocapture`; targeted off-selection action
contract and exact-grid tests; `cargo test --test live_e2e
command_center_show_waiting_targets_attention_pane_when_selection_differs --
--ignored --nocapture`; `just ux`; `git diff --check`.

RISK: low. The change is bounded to Command Center primary action dispatch and
keeps normal Fleet/Details selected-pane actions unchanged.

NEXT: dogfood Command Center with several real Codex/Claude/opencode panes and
watch for any remaining place where visible action copy and the actual target
can drift.

## 2026-05-17 - Claude Agent Manager goal completion audit

STATUS: DONE.

EVIDENCE: Audited `/tmp/muxboard-next-goal.md` against current artifacts. Read
`AGENTS.md`, latest loop notes, `docs/agent-view-audit.md`, and the active git
diff. The explicit rendered journeys are mapped in
`docs/agent-view-audit.md`: mixed first launch, one waiting agent, one working
agent with long output, many waiting agents, empty/no-tmux/no-search recovery,
Command Center, Details/Output scrolling, and tmux plugin modes. The main user
promises are covered by exact goldens (`mixed_fleet_dashboard.txt`,
`working_agent_dashboard.txt`, `multi_attention_command_center.txt`, selected
waiting panels, output/empty/search/command screens), action-contract tests,
provider/parser tests, and live tmux tests. High-impact fixes in the pass made
reply actions unambiguous, made oversized Command Center queues visible, stopped
prompt-only shells from inflating working counts, and added a working-agent
first-screen proof. V1 scope stayed tmux-native and no-VCS, guarded by
`product_scope_keeps_v1_tmux_first_without_vcs_dependency`.

Final checks run: `cargo fmt --check`; `cargo clippy --all-targets
--all-features -- -D warnings`; targeted reply, Command Center, prompt-noise,
working-dashboard, golden, manifest, and live tmux tests; `just ux`; `just ci`;
`just ux-live-startup`; `just ux-live-surfaces`; `just coverage-full-gate` with
64 live E2E tests, 98.36% lines, 97.71% regions, and 96.46% functions; `git
diff --check`.

RISK: low. The remaining work is dogfooding taste, not missing goal coverage.
No production behavior changed after the final full coverage gate except this
audit note.

NEXT: have the user dogfood the current tree, then review/squash/commit the
release-shape UI pass.

## 2026-05-17 - Add working-agent first-screen proof

STATUS: DONE.

EVIDENCE: Completed the rendered journey audit gap for a single working agent
with meaningful recent output. Added `working_agent_dashboard.txt`, a matching
exact-grid renderer test, manifest review metadata, and the audit matrix in
`docs/agent-view-audit.md` that maps the goal's core journeys to concrete
artifacts and one obvious next action. The working screen now has a protected
contract for `1 working`, a distilled current-work line, recent output, and the
two safe next actions: `Enter output` and `G show`. Checks run: targeted working
golden test; full exact golden suite; golden manifest guard; `cargo fmt
--check`; clippy with denied warnings; `just ux`; `just ci`; `just
ux-live-startup`; `just ux-live-surfaces`; `git diff --check`.

RISK: low. This adds renderer proof and documentation only; no production
behavior changed after the prior prompt-noise inference fix.

NEXT: before closing the active goal, run a requirement-by-requirement audit
against `/tmp/muxboard-next-goal.md` and only mark complete if every explicit
journey and validation requirement has evidence.

## 2026-05-17 - Protect the mixed-fleet first screen from prompt noise

STATUS: DONE.

EVIDENCE: Renderer/X-ray review of the first-load mixed-fleet journey exposed a
real escaped bug: a quiet shell showing only a prompt could be counted as
`working`, making the header say `2 working` and the Fleet row say
`working zsh`. Prompt-only shell runtime is now inferred as idle before recent
output recency can promote it to running. Added a core unit test covering common
shell prompt shapes, an exact golden grid for a mixed fleet with one Claude Code
approval, one Codex run, and one quiet shell, manifest guard coverage that bans
`2 working`, `working zsh`, `NEXT=`, `STATUS=`, and unknown jargon on that
screen, and live tmux coverage proving prompt noise stays out of Fleet Latest.
Checks run: `cargo fmt --check`; clippy with denied warnings; prompt-only shell
unit test; mixed-fleet exact golden test; full exact golden suite; live tmux
prompt-noise test; `just ux`; `just ci`; `just coverage-full-gate` with 64 live
E2E tests, 98.36% lines, 97.70% regions, and 96.45% functions;
`git diff --check`.

RISK: low. This narrows shell inference only for prompt-only live runtime lines
and adds exact first-screen coverage. Provider parsing, tmux send/jump, plugin,
and VCS scope are unchanged.

NEXT: dogfood a real local and server tmux fleet with quiet shells beside active
agents, then look for the next first-screen lie before adding any new feature.

## 2026-05-17 - Make oversized attention queues visible in Command Center

STATUS: DONE.

EVIDENCE: Product/Design/QA pass over the multiple-waiting-agent Command Center
journey. The Command Center now labels multi-item attention queues as
`Queue (N)` and adds an explicit action-aware overflow row like
`+ N more need you: continue` when the list is too long to show completely, so a
large fleet cannot silently hide waiting agents below the fold. The overlay
priority code now treats counted queue headings as real section headings,
preserving the queue under height pressure without losing the selected lane, and
keeps the overflow row visible even on compact Command Center surfaces. Added
app-level queue-count and overflow tests, a renderer/X-ray test for eight
waiting agents at roomy and compact sizes, a full golden grid
`multi_attention_command_center.txt`, and manifest guard coverage. The overflow
row is now action-aware, so hidden mixed queues say `show` for off-selection
prompts instead of a generic count. Added a mixed hidden-action app test and
renderer/X-ray test.
Added a live tmux Command Center overflow test and wired it into
`just ux-live-surfaces` so dogfood alignment catches any future ignored live
test that is not in the named live loop. Checks run:
`cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`;
targeted command-center queue, mixed overflow, renderer, and golden tests;
targeted live tmux overflow test; `just ux`; `just ux-live-surfaces`; `just ci`;
`just coverage-full-gate` with 64 live E2E tests, 98.36% lines, 97.70% regions,
and 96.45% functions; `git diff --check`.

RISK: low. This is presentation, overlay prioritization, and renderer fixture
coverage only. No tmux send, jump, plugin, provider parser, or VCS scope changed.

NEXT: dogfood a real multi-agent fleet and check whether `A continue` vs.
`: reply` remains the clearest first action when many agents wait at once.

## 2026-05-17 - Make Command Center reply targets read like a sentence

STATUS: DONE.

EVIDENCE: Product/Design/QA pass over the Command Center waiting-agent journey.
The primary action now reads `Action: : reply to demo / agents`, and the queue
row reads `> reply to demo / agents`, while the footer stays compact as
`: reply`. Secondary selected-pane choices now use `Also:` instead of putting
`G show` under a misleading `Reply:` label. This makes the command deck
unambiguous without adding another hint row or widening the surface. Generic
send surfaces now say `Send to` and `send text` instead of the colder `Command
for` / `send commands` wording, so prompt, shell, and agent sends all use the
same plain verb. Checks run: `cargo fmt --check`; `cargo clippy --all-targets
--all-features -- -D warnings`; `cargo test --lib
control_lines_lead_with_attention_and_target_summary -- --nocapture`; `cargo
test --lib command_center -- --nocapture`; `cargo test --lib command_input --
--nocapture`; `cargo test --lib action_menu -- --nocapture`; `cargo test --lib
selected_pane_lines_expose_reply_without_hiding_the_action -- --nocapture`;
`cargo test --lib selected_pane_reply -- --nocapture`; `cargo test --lib
reply_line -- --nocapture`; live send/reply tests for single-send, multi-send,
and free-form reply; `just tui-golden-bless` followed by `just tui-golden`;
`just ux`; `just ci`; `just dogfood`; `just coverage-full-gate` with 98.36%
lines, 97.70% regions, and 96.42% functions; `git diff --check`.

RISK: low. This is copy, renderer fixture, live-test helper, and guardrail
expectation work. The action key and tmux dispatch behavior are unchanged.

NEXT: run the completion audit against `/tmp/muxboard-next-goal.md`, then decide
whether another narrow polish pass is still needed before completing the goal.

## 2026-05-17 - Make reply and Command Center copy unambiguous

STATUS: DONE.

EVIDENCE: Product/Design/QA pass over the selected waiting-agent and Command
Center journeys. The first screen now says `: reply` in the footer when the
selected agent is waiting for a free-form answer, matching Details, Help, More,
Command Center, and the Reply composer. Help no longer repeats `: command pane`
on that same free-form reply screen; it only shows send-list marking as the
secondary send-list path. Command Center now says `Target:` instead of `Send:`
for the selected scope, so reply/answer/continue actions no longer sit beside a
conflicting send label. Captured states covered the wide and narrow selected
waiting panel, Help, Command Center, action contracts, and the new live
`free_form_reply_journey_uses_reply_copy_and_dispatches_live` path.
Checks run: `cargo fmt --check`; clippy with denied warnings; targeted Help,
footer, Command Center, and exact-grid tests; `just ux`; `just ci`; `just
coverage-full-gate` with 98.36% lines, 97.70% regions, and 96.42% functions;
`just dogfood`.

RISK: low. This is copy, presentation, renderer styling, and test expectation
work only. No tmux command dispatch, plugin, provider, parser, or VCS scope was
changed.

NEXT: review the full diff and decide whether to commit this UX pass or run one
more manual `cargo run` dogfood before squashing.

## 2026-05-17 - Claude Agent Manager-inspired goal closeout

STATUS: DONE.

EVIDENCE: `/tmp/muxboard-next-goal.md` was executed as a bounded product/UX/QA
pass for just over one hour. Visible improvements landed across the selected
agent reply journey, More, Help, Start recovery, and Command Center recovery.
Regression coverage was added at app, action-contract, renderer/X-ray, live
tmux, guard, perf, and coverage-gate layers. Final gates: `cargo fmt --check`;
`cargo clippy --all-targets --all-features -- -D warnings`; targeted tests for
Help, Command Center, More, Reply, and Start; `just ux`; `just ci`; `just
coverage-full-gate` with 98.36% lines, 97.70% regions, and 96.42% functions;
`git diff --check`.

RISK: low. No VCS scope was added. The only infrastructure change makes the full
coverage live tmux suite serial under coverage instrumentation to avoid
test-runner-induced tmux races.

NEXT: dogfood the calmer first-run, reply, and Command Center journeys in a real
tmux session before deciding what to commit.

## 2026-05-17 - Make empty Command Center a single recovery action

Pass: Command Center empty-state audit for the same "single next useful action"
discipline applied to Help. Empty and no-match Command Center states still
showed `Send:` and `Start:` rows even though no visible pane could receive either
action.

Change: when Command Center has no visible panes and no explicit target set, it
now renders only the recovery action: start tmux panes then refresh, or show all
panes. Targeted send lists and lanes keep their target-specific send context.

Checks: app-level Command Center empty/no-match tests; renderer/X-ray no-match
and empty Command Center assertions that `Send:` and `Start:` disappear;
action-contract refresh and show-all recovery tests; `cargo fmt --check`;
`cargo clippy --all-targets --all-features -- -D warnings`; `just ux`; `just
ci`; `just coverage-full-gate` with 98.36% lines, 97.70% regions, and 96.42%
functions.

Remaining risk: low. This removes inert body copy only; existing recovery keys
and tmux refresh behavior are unchanged and covered by action-contract tests.

Next: do the objective completion audit.

## 2026-05-17 - Make empty Help recovery-only

Pass: first-run and no-match Help audit for the "never advertise inert actions"
rule. Help could still describe pane actions like `Enter output`, `G show`,
`: command`, send-list marking, Start, and zoom even when no pane existed or the
current filter hid every pane.

Change: default empty Help now says only to start tmux panes, refresh, or open
More for layout/settings. No-match Help now makes `backspace show all panes` the
primary recovery and keeps search/refresh as secondary recovery. Browse keeps
its existing Browse-specific empty recovery copy. The full coverage gate now
runs the live tmux coverage suite serially so instrumentation load does not make
independent tmux journeys race each other.

Checks: app-level Help recovery assertions; renderer/X-ray Help tests for empty
and no-match states; action-contract test that presses advertised empty/no-match
Help recovery actions; `cargo fmt --check`; `cargo clippy --all-targets
--all-features -- -D warnings`; `just ux`; `just ci`; `just coverage-full-gate`
with 98.36% lines, 97.70% regions, and 96.42% functions.

Remaining risk: low. This is copy and key-contract shaping around existing Help
delegation; no tmux IO paths changed.

Next: audit Command Center empty-state copy for the same "single next useful
action" discipline.

## 2026-05-17 - Hide inert Start when the target pane disappears

Pass: Start recovery audit for the "selected pane vanished while Start is open"
edge. The modal could still show `Enter start` even after the selected target was
gone, which forced the user to press an advertised key just to learn it could not
work.

Change: Start now replaces the destination folder with `Action: Esc cancel, then
choose a pane` when no selected pane remains, and both header/footer hints stop
advertising `Enter start` in that state. Normal Start, Start errors, presets, and
successful launches keep their existing language.

Checks: app-level launch recovery test for missing selected targets; renderer
X-ray test that asserts the action row, footer, line order, and absence of
`Enter start`; `cargo fmt --check`; `cargo clippy --all-targets --all-features
-- -D warnings`; `just ux`; `just ci`.

Remaining risk: low. This is a no-target recovery presentation change; live
launch creation and disappeared-target paths remain covered by existing app and
live launch tests.

Next: audit first-run/no-tmux recovery copy and the empty-state More menu for the
same "never advertise an inert action" rule.

## 2026-05-17 - Make the reply composer unmistakably a reply

Pass: Send/Reply surface audit for the selected waiting-agent journey. The first
screen, Help, Command Center, and More now say `: reply`, but the composer still
opened as generic `Send`, showed `To:`, advertised `Enter send`, and could offer
recent-command replay while the user was answering an agent prompt.

Change: the single selected free-form waiting prompt now opens a dedicated
`Reply` composer with `Reply to: ...`, `Enter reply`, and no recent-command
repeat shortcut. Generic sends, yes/no prompts, send lists, lanes, and review
sends keep the existing Send/Review language. Esc now reports `Closed Reply.`
only for that reply composer.

Checks: app-level reply-context and footer/header tests; renderer/X-ray reply
composer test that inspects title, target row, footer, and line order; action
contract tests that press `:` from Command Center and selected reply states;
`cargo test --lib command_input -- --nocapture`; `cargo fmt --check`; `cargo
clippy --all-targets --all-features -- -D warnings`; `just ux`; `just ci`;
`just coverage-full-gate` with 98.35% lines, 97.68% regions, and 96.36%
functions.

Remaining risk: low. This is presentation and key-contract shaping around the
existing send path; live tmux dispatch behavior is unchanged and still covered by
the send/action live suites.

Next: audit Start failure and first-run recovery copy next, especially what the
user can safely do after target panes or sessions disappear.

## 2026-05-17 - Make More honor reply and attach promises

Pass: More-menu action contract audit for the peek/reply/attach journey. Details,
Help, and Command Center had converged on `: reply` and `G show`, but More still
treated selected free-form prompts like generic send/mute work and did not expose
the attach path when the key was actually available.

Change: selected free-form waiting prompts now render `Action: : reply` in More,
the listed `:` row opens the text composer directly, and More shows `G show in
tmux` when that key is not already reserved for saving a fleet. The key router
now executes `g` from More as a real show-in-tmux action in that state, while
preserving `g` for save-fleet when the send-list action is visible.

Checks: action-menu unit tests; More action-contract tests that press `:` and
`g`; More renderer/X-ray tests; actions golden re-blessed and inspected; live
tmux test `action_menu_show_in_tmux_jumps_to_selected_pane`; `cargo fmt --check`;
`cargo clippy --all-targets --all-features -- -D warnings`; `just ux`;
`just ux-live-actions`; `just guards`; `just ci`; `just coverage-full-gate` with
98.35% lines, 97.67% regions, and 96.35% functions; `just dogfood`;
`git diff --check`.

Remaining risk: low. The new show-in-tmux route uses the existing jump
implementation and now has both fake-tmux and live-tmux coverage.

Next: audit Start/Send recovery copy next, especially hidden targets,
disappeared panes, and launch failure states.

## 2026-05-16 - Make Help honor selected text-reply prompts

Pass: Help truth audit for the reply-in-place journey. The first screen and
Command Center now made free-form replies obvious, but Help still described the
old generic `Enter output` path first for the same selected waiting prompt.

Change: Help now advertises `Now: : reply, Enter output, G show in tmux.` on the
first screen for selected free-form prompts, and `Now: : reply, Esc back, G show
in tmux.` from Command Center or Output. It only uses that copy when `:` really
targets the selected pane, not when a send list or lane fanout would receive the
text.

Checks: help-line unit tests, Help action-contract tests that press the visible
`:` key, Help renderer/golden tests, and selected Help golden manifest coverage;
`cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`;
`just ux`; `just ci`; `just coverage-full-gate` with 98.34% lines, 97.67%
regions, and 96.35% functions; `just dogfood`; `git diff --check`.

Remaining risk: low. This is help copy only; the same command-input path is
already covered by action-contract and live tmux send tests.

Next: audit the next highest-friction surface, likely Send/Start recovery copy
or tmux plugin mode wording, and keep the same visible-key contract discipline.

## 2026-05-16 - Make Details use the same reply language as Command Center

Pass: first-screen reply language and renderer X-ray audit. Details still said
`Action: : send reply` while Command Center said `: reply`, so the same action
had two names. Tightening the copy also exposed that one renderer cell helper
used byte offsets for Unicode-bordered rows, which could inspect the wrong cell
for short words.

Change: free-form waiting Details now render `Action: : reply`, matching the
Command Center. The action-detail deduper treats `reply` as a complete action
so prompt summaries cannot regress into `reply for ...`. Renderer cell helpers
now translate substring byte offsets to character-cell offsets before inspecting
styles, giving the X-ray tests accurate coordinates on boxed TUI rows.

Checks: selected-pane, reply-line action-contract, fixture, semantic-cell, and
exact golden tests; selected waiting goldens were re-blessed and inspected;
`cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; `just ux`; `just ci`; `just dogfood`; `just coverage-full-gate`
with 98.34% lines, 97.67% regions, and 96.35% functions; `git diff --check`.

Remaining risk: low. This is copy plus test-helper coordinate correctness. The
underlying `:` route and tmux dispatch remain unchanged and covered by
action-contract tests.

## 2026-05-16 - Let Command Center reply before attaching for selected prompts

Pass: Command Center agent-manager journey audit. A selected free-form waiting
prompt still made `G show` the primary action, forcing the user to attach to
tmux before answering even when `:` could safely open the reply composer in
place.

Change: Command Center now renders `Action: : reply ...` and `> reply ...` for
the selected waiting pane when there is no explicit send list or lane fanout.
If the waiting prompt is off-selection, it still renders `G show` so muxboard
does not promise a reply to the wrong pane. The footer suppresses duplicate
`: send` copy when the primary action is `: reply`, but keeps `G show` visible
on roomy Command Center screens as the attach escape hatch.

Checks: targeted control-line and Command Center action-contract tests; exact
overview golden re-blessed and inspected; panel fixtures updated; `cargo
fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`;
`just ux`; `just ci`; `just coverage-full-gate` with 98.34% lines, 97.67%
regions, and 96.35% functions; `git diff --check`.

Remaining risk: low. This changes only the advertised Command Center primary
action for selected free-form waiting prompts. Off-selection prompts still
attach first, and action-contract tests now press the visible `: reply` path.

## 2026-05-16 - Keep attach visible on narrow triage screens

Pass: first-screen triage footer audit. The narrow waiting-agent screen made
reply obvious but hid `G show`, so the attach/show path was only discoverable by
opening Help or More.

Change: the default footer now keeps `G show` visible starting at 70 columns,
before lower-priority actions. The narrow selected waiting golden and manifest
now protect `G show` as part of the core footer contract.

Checks: narrow selected waiting golden re-blessed and inspected; `cargo
fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`;
`just ux`; `just ci`; `git diff --check`.

Remaining risk: low. This changes footer prioritization only; the actual tmux
jump/show route is unchanged and remains covered by action-contract and live
tmux tests.

## 2026-05-16 - Keep waiting reply actions short

Pass: Details command-card audit. Waiting panes could render awkward generated
phrases like `Action: : send reply for approve`, which made the primary action
look grammatically broken and forced the user to parse duplicated context.

Change: free-form waiting prompts now keep the primary action as `Action: :
send reply`. The blocker and output rows carry the reason and recent context,
while app, fixture, renderer, and semantic-color tests now assert that
`send reply for ...` does not return.

Checks: targeted selected-pane, fixture, renderer, wrapping, medium-layout, and
semantic-cell tests; selected waiting goldens were re-blessed and inspected;
`cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; `just ux`; `just ci`; `just coverage-full-gate` with 98.34% lines,
97.66% regions, and 96.34% functions; `git diff --check`.

Remaining risk: low. This is display copy only. The same `:` send route and
tmux dispatch behavior are covered by existing action-contract and live tests.

## 2026-05-16 - Make More and Send section headings designed labels

Pass: More/Send hierarchy polish. Several secondary surfaces still used
lowercase section labels like `view`, `pane`, `settings`, `recent`, and
`macros`, making designed command groups look like raw tokens.

Change: More now renders title-case section labels for primary groups
(`View`, `Start`, `Pane`, `Send List`, `Settings`, `Reports`) while preserving
the existing prioritization and compact-mode behavior. Send command sections
now use `Fleets`, `Recent`, `Macros`, `Preview`, and `Reports`. The overlay
prioritizers accept old and new section spellings so fixtures and compatibility
paths stay stable. A false-positive renderer test that matched lowercase
`pane` inside `send list` copy now asserts the real `Pane` heading.

Checks: targeted action-menu, More-overlay, send-priority, truncation, and
golden tests; golden grids were re-blessed and inspected for the More and Send
surfaces; `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; `just ux`; `just ci`; `just coverage-full-gate` with 98.34% lines,
97.66% regions, and 96.34% functions; `git diff --check`.

Remaining risk: low. This is visual hierarchy and prioritizer parsing only;
action execution, tmux dispatch, and provider intelligence are unchanged and
remain covered by action-contract, renderer, and live tmux tests.

## 2026-05-16 - Give Command Center sections real hierarchy

Pass: Command Center visual hierarchy audit. The main command deck still used
lowercase `queue` and `lanes` headings, which made major sections look like raw
tokens rather than designed labels.

Change: Command Center now renders `Queue` and `Lanes` as proper section
headings. Renderer heading detection accepts both old and new spellings, and
the Command Center fixture and golden grid now protect the title-case hierarchy.

Checks: targeted control-panel, control-line, section-heading, short/tiny
Command Center, and golden tests; golden grids were re-blessed and inspected for
the Command Center; `cargo fmt --check`; `cargo clippy --all-targets
--all-features -- -D warnings`; `just ux`; `just ci`; `just
coverage-full-gate` with 98.35% lines, 97.67% regions, and 96.29% functions;
`git diff --check`.

Remaining risk: low. This is display hierarchy only; the underlying queue,
lane, movement, and action routing are unchanged.

## 2026-05-16 - Make Command Center show actions read naturally

Pass: Command Center language audit. The visible action and queue rows said
`show prompt demo / agents`, which exposed an internal prompt classification
instead of a natural tmux action.

Change: Command Center now renders `Action: G show demo / agents`, the footer
uses `G show`, and queue rows render `show demo / agents`. The internal
provider summary can still distinguish prompt waiting, but the command deck no
longer makes users parse "show prompt" as product language.

Checks: targeted control-line, attention-queue, short/tiny Command Center, and
golden tests; golden grids were re-blessed and inspected for the Command Center;
`cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; `just ux`; `just ci`; `just coverage-full-gate` with 98.35% lines,
97.67% regions, and 96.29% functions; `git diff --check`.

Remaining risk: low. This is visible wording only; the same jump/show action
and live tmux routing remain covered by existing action-contract and live tests.

## 2026-05-16 - Make Action rows one-decision prompts

Pass: command-surface scan audit. Fleet, Command Center, Send, and More had
several `Action:` rows with competing alternatives, for example `send this pane,
or add to send list`. That made the primary row read like a mini help menu
instead of the single next decision.

Change: default Action rows now name one primary action: `: send this pane`,
`C mute alert`, `A mute alerts`, `L choose fleet`, or `M answer yes/no` before
opening More. More still lists secondary choices in its sections, but the top
row no longer asks the user to compare alternatives. A golden guard now fails
if visible Action rows reintroduce `or` phrasing.

Checks: targeted command recommendation, rebound choice, Command Center,
More-overlay, action-menu, action-contract, and golden action-row guard tests;
golden grids were re-blessed and inspected for the More overlay; `cargo fmt
--check`; `cargo clippy --all-targets --all-features -- -D warnings`; `just
ux`; `just ci`; `just coverage-full-gate` with 98.35% lines, 97.67% regions,
and 96.29% functions; `git diff --check`.

Remaining risk: low. This is copy and prioritization only; secondary actions
remain visible in More and covered by action-contract tests.

## 2026-05-16 - Make Details actions executable at a glance

Pass: selected Details command-card audit. The selected pane panel could say
`Action: approve`, which named the agent's request but forced the user to
translate it into the actual key. That violated the command-center rule: a
primary action must be executable without reading the footer.

Change: Details now renders keyed primary actions: `A continue`, `. answer
yes/no`, `: send reply`, `Enter output`, or `G show in tmux`, with the prompt
detail only when it adds meaning. The Reply row now appears only for real
alternate reply paths instead of duplicating the primary action. Idle "ready"
details no longer pollute the action row, and the output-tail live test uses
`End` to prove newest-output recovery without key-burst flakiness.

Checks: targeted selected-pane, rebound-key, Details action-contract, CUJ,
golden, live smart-action, output-tail, and shell-prompt tests; `cargo fmt
--check`; `cargo clippy --all-targets --all-features -- -D warnings`; `just
ux`; `just ux-live-actions`; `just ci`; `just coverage-full-gate` with 98.35%
lines, 97.67% regions, and 96.29% functions; `git diff --check`.

Remaining risk: low. This is visible action copy plus regression coverage; the
underlying tmux send, answer, continue, output, and jump routes remain covered
by action-contract and live tests.

## 2026-05-16 - Prove Command Center answers against live tmux

Pass: live action-contract hardening. The Command Center answer route had
renderer and fake-tmux coverage, but not a live proof that `Action: . answer`
actually sent the selected yes/no choice to the target pane. The full coverage
gate also exposed an existing race in the output-tail live test: muxboard could
launch before the target pane had printed the full sentinel tail.

Change: added a live E2E that opens Command Center, sees the answer primary
action, presses `.`, selects `y`, and verifies the target pane receives it
without closing muxboard. The output-tail live test now waits for its target
sentinel before starting muxboard, so the test validates output rendering
instead of racing target setup.

Checks: targeted Command Center live answer and output-tail live tests;
`cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; targeted architecture guard tests; `just ux-live-actions`; `just
ci`; `just coverage-full-gate` with 98.35% lines, 97.67% regions, and 96.33%
functions; `git diff --check`.

Remaining risk: low. This is test and live-flow hardening plus one new live
action proof; the production Command Center answer behavior was already covered
by app and renderer action contracts.

## 2026-05-16 - Make Send recommendations read as actions

Pass: command-surface copy audit. The Send panel still rendered `try ...`
recommendations even after More and Command Center had moved to decisive
`Action:` rows. That made one primary surface feel like tutorial copy instead of
a control card.

Change: idle Send recommendations now render as `Action: ...`, matching More
and Command Center. The action-prioritizer now reads `Action:` directly instead
of carrying old `try` compatibility, and the targeted app/TUI tests now reject
the retired shape.

Checks: targeted command-line recommendation, rebound-key, compose-focus,
recommendation visibility, send-priority, and golden tests; `cargo fmt
--check`; `cargo clippy --all-targets --all-features -- -D warnings`; `just
ux`; `just ci`; `just coverage-full-gate` with 98.35% lines, 97.67% regions,
and 96.33% functions; `git diff --check`.

Remaining risk: low. This is a copy/hierarchy change for idle Send
recommendations; compose, review, and More action routing stayed unchanged.

## 2026-05-16 - Make Command Center say what kind of prompt needs attention

Pass: Agent View-inspired Command Center audit. The biggest blind spots were:
the Command Center still used vague `show` language for waiting prompts, choice
prompts were not the primary action there, and the Agent View audit still had
old `: type` wording. That made the command deck feel less like an agent
manager and more like a status list.

Change: Command Center now distinguishes prompt work. Choice prompts render
`Action: . answer ...` and the queue row `answer ...`; generic waiting prompts
render `G show ...` and `show ...`. The footer no longer advertises
`. more` when `.` is the visible answer action. The Agent View audit doc now
matches `: send` language.

Checks: targeted control-line, attention-queue, Command Center action-contract,
and golden tests; `cargo fmt --check`; `cargo clippy --all-targets
--all-features -- -D warnings`; `just ux`; `just ci`; `just coverage-full-gate`
with 98.35% lines, 97.67% regions, and 96.33% functions; `git diff --check`.

Remaining risk: low. This changes Command Center action copy and footer
prioritization only; fake-tmux action-contract coverage proves answer, continue,
output, send, and refresh routes still execute the advertised action.

## 2026-05-16 - Make Reply rows advertise the actual send action

Pass: waiting-agent reply audit. Details showed `Reply: : type, G show`, which
made the user translate an input mechanic into the real task. Reply rows should
read like actions, not micro-instructions.

Change: waiting Reply rows now use `: send` everywhere: continue prompts,
yes/no prompts, generic waiting prompts, rebound key variants, help copy, and
the waiting-pane goldens. Action-contract tests still press the advertised keys
and prove continue, answer, Send, and show-in-tmux behavior.

Checks: targeted selected-pane reply, rebound-key, reply-action-contract, Help,
golden, and architecture-manifest tests; `cargo fmt --check`; `cargo clippy
--all-targets --all-features -- -D warnings`; `just ux`; `just ci`; `just
coverage-full-gate` with 98.34% lines, 97.66% regions, and 96.33% functions;
`git diff --check`.

Remaining risk: low. This is a copy/hierarchy change only; the key routing and
tmux actions stayed covered by existing fake-tmux action contracts.

## 2026-05-16 - Make empty results show one obvious recovery action

Pass: empty-state scan audit. No-match screens still said `Backspace shows all
panes.`, which reads like instruction copy. The empty result should make the
single useful recovery action unavoidable.

Change: empty Fleet/Browse states now render `Action: backspace show all panes`.
The app fixture, renderer goldens, golden manifest, and empty-state usability
tests now protect the action-row shape and reject the old sentence copy in
golden screens.

Checks: targeted no-match, empty-state, tiny Browse, hidden-pane, golden, and
architecture-manifest tests; `cargo fmt --check`; `cargo clippy --all-targets
--all-features -- -D warnings`; `just ux`; `just ci`; `just
coverage-full-gate` with 98.34% lines, 97.66% regions, and 96.33% functions;
`git diff --check`.

Remaining risk: low. This changes no-match recovery copy and the scoped Browse
status message only; the key route and footer contract are unchanged.

## 2026-05-16 - Make More read like a command card

Pass: More overlay language audit. The first row still said `try ...`, which
felt like tutorial copy instead of a confident command surface.

Change: More now labels its primary recommendation as `Action: ...`, matching
the Command Center language and avoiding the retired `Next:` wording. The
recommendation prioritizer, action-contract tests, renderer tests, golden grid,
and golden manifest now protect the clearer first row.

Checks: targeted action-menu, More, fixture, architecture-manifest, usability,
and golden tests; `cargo fmt --check`; `cargo clippy --all-targets
--all-features -- -D warnings`; `just ux`; `just ci`; `just
coverage-full-gate` with 98.34% lines, 97.66% regions, and 96.33% functions;
`git diff --check`.

Remaining risk: low. This changes the More recommendation label only; Send and
Details guidance keep their existing wording.

## 2026-05-16 - Remove zero-value Command Center counters

Pass: Command Center noise audit from the rendered golden. The overview still
spent a full row on `Working: none`, and no-match states could show both
`Needs you: none` and `Working: none`. Those rows make the user parse absence
instead of action.

Change: Command Center now renders `Needs you:` only when attention exists and
`Working:` only when an agent is actually running. Empty, filtered, and
attention-only states keep the action, send target, and start recovery visible
without zero-count filler. App-level control-line tests, renderer usability
tests, fixtures, and the Command Center golden protect the quieter hierarchy.

Checks: targeted control-line, Command Center, usability, fixture, and golden
tests; `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; `just ux`; `just ci`; `just coverage-full-gate` with 98.34% lines,
97.66% regions, and 96.33% functions; `git diff --check`.

Remaining risk: low. This changes only Command Center summary rows; top chrome
still carries fleet counts and attention state.

## 2026-05-16 - Make More show its send destination as a form row

Pass: More-menu scan audit. More already exposes power, but the destination row
still read like loose prose: `send to demo / agents`. That made the overlay feel
less like a command surface than Send and Start.

Change: More now labels the active destination as `To: demo / agents`, matching
the form hierarchy used by Send review and compose. The app-level contract,
panel fixture, golden grid, and golden manifest now reject the old unlabelled
`send to demo / agents` row.

Checks: targeted action-menu tests; fixture and architecture-manifest tests;
`just tui-golden`; `cargo fmt --check`; `cargo clippy --all-targets
--all-features -- -D warnings`; `just ux`; `just ci`; `just
coverage-full-gate` with 98.35% lines, 97.67% regions, and 96.33% functions;
`git diff --check`.

Guardrail catch: the first `just ux` run caught that saved-fleet More menus no
longer prioritized `L choose fleet` and `D delete triage` after the label
change. The prioritizer now recognizes `To: fleet ...` and `To: the send list
...`, and the existing action-contract test protects those promised actions.

Remaining risk: low. This changes More prelude labeling and prioritization for
explicit send-list/fleet scopes; stale and recovery states keep their existing
recovery copy.

## 2026-05-16 - Make Help expose the command-center path

Pass: Help CUJ audit. Help was calmer, but it still hid the Browse and Command
Center path behind More, so a user asking "where do I go next?" had to infer too
much.

Change: Help now has a dedicated `Views:` row: `. then [ browse, ] command
center; L layout.` The `More:` row now describes the remaining More actions
without pretending those keys are top-level actions. Action-contract coverage
presses the visible Help -> More -> Browse and Help -> More -> Command Center
paths, and the Help golden/manifest protect the new hierarchy.

Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; targeted help-line, help-overlay, Help action-contract, rebound-key,
fixture, architecture-manifest, and golden tests; `just ux`; `just ci`; `just
coverage-full-gate` with 98.36% lines, 97.67% regions, and 96.28% functions;
`git diff --check`.

Remaining risk: low. This is Help copy and routing coverage only; top-level
footer behavior is unchanged.

## 2026-05-16 - Make Start read like a compact launch card

Pass: Start-agent CUJ audit. Starting a new agent should feel like a tiny form:
where it will run, what folder it inherits, what window it creates, and what
command will execute.

Change: the Start overlay now uses labeled rows: `In:`, `Folder:`, `Window:`,
`Command:`, and `Presets:`. This removes the repeated `Start in ...` sentence,
keeps the top bar calm as `Start agent.`, and spends scarce rows on actual launch
decisions instead of chrome. Renderer goldens, narrow/tiny Start tests,
action-contract coverage, and live tmux launch waits now protect the new shape.

Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; targeted launch/start app and renderer tests; Start golden grid;
architecture launch wait guard; live launch success and recovery tests; `just
ux`; `just ci`; `just coverage-full-gate` with 98.35% lines, 97.67% regions,
and 96.28% functions; `git diff --check`.

Remaining risk: low. The launch command path is unchanged; this is a visible
form hierarchy and test-wait update.

## 2026-05-16 - Rename Command Center attention section to Queue

Pass: Command Center scanning audit. The panel already has a `Needs you:`
summary, so repeating `needs you` as a lower section heading made users parse
the same idea twice.

Change: the actionable attention list is now headed `queue`, matching the
existing Details `Queue: #N` language and separating counts from the ordered work
list. The overview prioritizer, section styling, panel fixture, and golden grid
now protect the clearer hierarchy.

Checks: `cargo fmt --check`; targeted Command Center, overview, section-heading,
fixture, and golden tests; `cargo clippy --all-targets --all-features -- -D
warnings`; `just ux`; `just ci`; `just coverage-full-gate` with 98.35% lines,
97.67% regions, and 96.28% functions; `git diff --check`.

Remaining risk: low. This changes only a section label and keeps the top-level
`Needs you:` count visible.

## 2026-05-16 - Calm Command Center triage copy

Pass: Command Center hierarchy audit. The control surface should read like a
fleet status card, not a raw counter dump.

Change: Command Center now omits zero-count attention categories, keeps idle
work out of the summary, and gives selected queue and lane rows a real gutter
after `>`. The overview golden grid, panel fixture, app-state tests, and
renderer tests now protect the calmer status summary and prevent glued markers
like `>show` or `>codex`.

Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; targeted Command Center, attention queue, fixture, golden, and live
summary tests; `just ux`; `just ci`; `just coverage-full-gate` with 98.35%
lines, 97.67% regions, and 96.28% functions; `git diff --check`.

Remaining risk: low. This removes only zero-count noise; nonzero waiting,
error, stuck, and working counts still stay visible and prioritized.

## 2026-05-16 - Keep Send compose focused on the form

Pass: send-compose noise audit. While a user is typing or reviewing a send, the
surface should spend rows on destination, text, preview, targets, and recovery,
not background agent reports.

Change: Send now hides the `reports` section while command input or review is
active, while preserving reports on idle Send and More surfaces. App and
action-contract tests prove reports still exist when useful, but cannot intrude
into compose or review.

Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; targeted command/review/action-contract tests; `cargo test --lib
command -- --nocapture`; `cargo test --lib send -- --nocapture`; `just ux`;
`just ci`; `just coverage-full-gate` with 98.35% lines, 97.67% regions, and
96.28% functions; `git diff --check`.

Remaining risk: low. This intentionally removes secondary agent status from
active send flows; Details and Command Center still carry operational status.

## 2026-05-16 - Make command compose scan like send review

Pass: command-compose CUJ audit. The send input screen should answer where the
command goes, what will be sent, and what expansion will happen without making
the user parse repeated `send to` prose.

Change: command compose now uses the same quiet form hierarchy as review:
`To:`, `Text:`, and `Preview`, with preview rows visually nested under the
section. Empty compose shows `Text: _` so the editable field is visible before
typing. Renderer, action-contract, golden, app, fixture, and live coverage gates
now protect the destination/text/preview shape across single-pane, lane, saved
fleet, hidden-target, and command-center entry flows.

Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; targeted command/send/fixture tests; `just tui-golden-bless` with
reviewed command-input grid; `just ux`; `just ci`; `just coverage-full-gate`
with 98.35% lines, 97.66% regions, and 96.33% functions.

Remaining risk: low. Existing non-input More and first-step surfaces still use
plain `send to ...` copy where it reads as an action rather than a form field.

## 2026-05-16 - Make send review read like a confirmation card

Pass: dispatch/send CUJ audit. The multi-pane send review should look like a
safe confirmation form, not a raw command log.

Change: the send review overlay now leads with `To:`, `Text:`, and `Targets`
instead of repeating `review`, `send to ...`, and `send ...`. App, renderer,
golden, panel-fixture, and live tmux tests now protect the clearer confirmation
shape, including cancel-and-recover before anything is dispatched.

Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; `cargo test --lib send -- --nocapture`; `cargo test fixtures --
--nocapture`; live `review_send_cancel_keeps_targets_safe_and_recovers_cleanly`;
`just tui-golden-bless` with reviewed confirm-send grid; `just ux`; `just ci`;
`just coverage-full-gate` with 98.35% lines, 97.67% regions, and 96.32%
functions.

Remaining risk: low. The header and footer still carry the action contract,
while the body now spends every row on destination, text, and blast radius.

## 2026-05-16 - Rename selected attention order to Queue

Pass: Details wording audit. A selected waiting pane should explain its place
in the user's work queue, not expose notification jargon.

Change: the selected Details card now says `Queue: #N` instead of `Alert: #N`
when a pane is in the attention queue. The label keeps the same warning styling,
but the wording now matches the command-center mental model. App and renderer
tests prove the old `Alert:` label stays out of selected details, dense layouts,
and golden grids.

Checks: `cargo fmt --check`; targeted selected-pane app and renderer tests;
`just tui-golden-bless` with reviewed waiting-panel grid; `cargo clippy
--all-targets --all-features -- -D warnings`; `just ux`; `git diff --check`;
`just ci`; `just coverage-full-gate` with 98.38% lines, 97.70% regions, and
96.32% functions.

Remaining risk: low. Notification settings and mute/unmute actions still use
alert language where it describes desktop alerts; selected Details now uses
queue language for work ordering.

## 2026-05-16 - Make compact Fleet states human

Pass: first-screen hierarchy audit. The compact Fleet should not make users
decode htop-style abbreviations before they know which agent needs action.

Change: compact Fleet rows now use the same human state language as the rest of
the app: `needs you`, `working`, `failed`, `quiet`, and `checking` instead of
`wait`, `run`, `err`, `idle`, and `check`. Renderer, app, architecture, and live
tmux expectations now protect the copy across compact, standard, golden, and
live surfaces. A direct token guard covers every compact state word so future
changes cannot silently reintroduce abbreviated scan labels.

Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; targeted compact Fleet presentation, renderer, architecture, and live
tmux tests; `just tui-golden-bless` with reviewed waiting-panel grids; `just ux`;
`git diff --check`; `just ci`; `just coverage-full-gate` with 98.38% lines,
97.70% regions, and 96.32% functions.

Remaining risk: low. This spends a little more horizontal space in compact rows,
but the wording is self-evident and the narrow standard layout already keeps the
dedicated `Now` column.

## 2026-05-16 - Trim summary-only Output chrome

Pass: Output density audit. Summary-only Output should feel like a focused card,
not a large empty modal.

Change: Output overlays now use the compact minimum height when there is no
scrollable Latest section, while preserving the taller viewport for real output
tails. The summary-only and empty Output golden grids were reviewed and updated.

Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; targeted Output overlay renderer tests; golden-grid tests; `just ux`;
`just ci`; `just coverage-full-gate` with 98.38% lines, 97.69% regions, and
96.32% functions.

Remaining risk: low. Long Output views with Latest content still keep their
larger viewport and scrollbar behavior.

## 2026-05-16 - Make waiting replies obvious

Pass: Details and More reply-path audit. The selected waiting pane should not
make users guess whether to type, answer yes/no, continue, or jump into tmux.

Change: tightened the Details `Reply:` line so enter-safe prompts show
`A continue`, yes/no prompts point to `.` answer options, and generic prompts
show only `:` type plus `G show`. Help now routes advertised keys through to the
underlying screen instead of swallowing them, and choice-prompt Help says
`. answer yes/no` directly. More now recommends `Y answer yes, N answer no`
ahead of mute/summarize for choice prompts. Details timestamp metadata no longer
repeats `awaiting input` after `State: Waiting`. A later action-row pass removed
the competing `or` phrasing from More's choice recommendation.

Checks: `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D
warnings`; focused reply, Help, and metadata tests; More contextual renderer
test; golden-grid tests; `just ux`; `just ci`; `just coverage-full-gate` with
98.38% lines, 97.69% regions, and 96.32% functions.

Remaining risk: low. This keeps tmux dispatch semantics intact while making
safe reply paths more visible and Help less inert.

## 2026-05-16 - Add Agent View reply affordance

Pass: Agent View/conductor-style V1 audit. The useful shape is not Claude-only
session management or VCS state; it is one calm queue that shows what needs the
user, what is still working, and how to peek, reply, or intentionally attach.

Change: added `docs/agent-view-audit.md`, kept working agents visible in the
Fleet health summary even when another pane needs the user, and added a minimal
`Reply:` line to waiting Details cards. Renderer and action-contract tests now
prove the visible reply keys actually continue, open Send, or show the tmux pane.

Checks: `cargo fmt --check`; focused app and TUI reply tests; TUI golden grids;
architecture guards; `just ux`; `just ci`; `just coverage-full-gate` with
98.38% lines, 97.68% regions, and 96.31% functions; `just dogfood`.

Remaining risk: low. This is intentionally a bounded V1 improvement. Pins,
renames, and deeper lifecycle recovery still need separate product passes before
claiming full Agent View parity.

## 2026-05-15 - Make saved goals mobile-friendly

Pass: SSH/mobile goal setup audit. Copying long `/goal` prompts from an iPhone
is too much friction, so saved goals need one-command paths that work inside
tmux without relying on the phone clipboard.

Change: added a saved goal bank under `docs/goals`, a guarded
`scripts/codex-goal-send` helper for sending a saved goal into exactly one
Codex tmux pane, and `just goal-list`, `goal-show`, `goal-buffer`, `goal-send`,
`goal-run`, and `goal-check`. `goal-check` now runs in `just ci`, and an
architecture guard protects the mobile-friendly workflow and its tmux safety
checks.

Checks: `cargo fmt --check`; `just goal-check`; targeted saved-goal architecture
guard; `just guards`; `just ci`.

Remaining risk: medium-low. `just goal-run agent-view` is the safest phone path
because it avoids interactive paste entirely. `just goal-send agent-view` is
guarded to one Codex pane, but the exact `/goal` paste behavior still depends on
the interactive Codex TUI accepting multi-line pasted text.

## 2026-05-02 - Harden release coverage and perf gates

Pass: release-gate audit after coverage exposed false failures and a large-fleet
sort/perf hotspot. Coverage instrumentation should not fail human-latency
assertions, and large fleets must not recompute unstable window heat or repeated
pane summaries on every rendered line.

Change: precomputed Browse window navigation entries from one visible pane pass,
shared Fleet title health/window summaries within a render, reused visible
entries for Command Center attention counts, added deterministic pane/window
tie-breakers, and fast-pathed static provider/workload inference for panes with
no runtime output. Perf tests now keep live/user paths strict while allowing
coverage-instrumented runs to be slower, and dynamic relative ages are normalized
in the footer action-contract comparison.

Checks: `cargo fmt --check`; targeted failing tests for large-fleet
presentation, navigation burst, renderer navigation, footer action contracts,
and tmux control startup; `just coverage-full-gate`; `just package-check`;
`git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low. Release coverage, packaging, UX, CI, and live perf gates
are green after the fix; full `just release-check` is still the single command
to rerun before tagging.

## 2026-05-02 - Live dogfood Details copy cleanup

Pass: captured a real muxboard TUI inside tmux against a separate live target
tmux server with simulated Codex, Claude Code, Opencode, and job panes. The
screen exposed two scan-level issues: Fleet could render `codex codex ...` when
the latest output already named the provider, and Details could show
`Updated: awaiting input`, which mislabeled state as timestamp metadata.

Change: provider prefixes now avoid repeating a tool name that is already the
first word of the latest summary, while still prefixing generic summaries. Details
only renders `Updated:` when a real output age exists. Added app-level and
renderer/X-ray regressions for both escaped live-dogfood misses.

Checks: live tmux capture at 120x36 before and after the fix; targeted app and
renderer tests for duplicate provider prefixes and attention-only Updated
metadata; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low. The dogfood target was simulated but ran through real tmux
servers and the real muxboard TUI capture path.

## 2026-05-02 - Keep More rows and predicates coupled

Pass: More-menu predicate drift audit. The key router and rendered command model
must not grow separate ideas of when acknowledgement, pane, send-list, or fleet
actions exist.

Change: More row rendering now reuses the same predicates that gate key routing,
including the no-visible-selection case for acknowledgement recovery. Added an
app-state predicate-to-row regression and a renderer/key-router no-match guard
proving hidden acknowledgement keys stay inert when the More menu does not list
them.

Checks: `cargo fmt --check`; `cargo test action_menu_row_predicates_match_command_model -- --nocapture`; `cargo test usability_action_contract_unlisted_keys_do_not_steal_modal_state -- --nocapture`; `cargo test key_router_ -- --nocapture`; `cargo test usability_action_contract_more_menu_actions_execute_visible_promises -- --nocapture`; `cargo test usability_more_menu_is_contextual_instead_of_an_action_dump -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low. More state predicates and command rows are now tied at the
app boundary for the riskiest row families. Very small terminals can still
prioritize away lower-value rows, but state-hidden actions now have direct
regression coverage.

## 2026-05-02 - Guard listed-only More actions

Pass: More-menu action-contract audit. More should behave like a real menu:
keys that are not listed for the current state must not mutate state, close the
menu, or emit fallback status chatter.

Change: added shared predicates for the More rows that depend on visible panes,
send-list state, saved fleets, attention acknowledgements, and waiting-prompt
choices. The key router now uses those predicates for pane, send-list, fleet,
attention, and choice actions. Renderer/key-router regressions prove empty,
no-match, and normal one-pane More states keep unlisted keys inert while listed
actions still execute.

Checks: `cargo fmt --check`; `cargo test key_router_ -- --nocapture`; `cargo test usability_action_contract_unlisted_keys_do_not_steal_modal_state -- --nocapture`; `cargo test usability_action_contract_more_menu_actions_execute_visible_promises -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: medium-low. More command, pane, send-list, fleet, attention,
choice, settings, and navigation actions now share state-level row predicates.
Future passes should watch custom/rebound bindings and viewport-clipped More
rows for the same listed-only invariant.

## 2026-05-02 - Guard unlisted More send actions

Pass: More-menu action-contract audit. If More does not list `:` or `S` in an
empty or no-match recovery state, pressing those keys must not close the menu,
open Send, or show summary status chatter.

Change: More command and summarize handlers now execute only when the menu has
actionable live targets, using the same target predicate as the rendered menu.
Renderer/key-router regressions now prove empty and no-match More keep unlisted
`:` and `S` inert, while normal listed send and summarize actions still work.

Checks: `cargo fmt --check`; `cargo test key_router_covers_safe_no_target_async_actions -- --nocapture`; `cargo test usability_action_contract_unlisted_keys_do_not_steal_modal_state -- --nocapture`; `cargo test empty_more_overlay_is_recovery_not_action_dump -- --nocapture`; `cargo test usability_action_contract_more_menu_actions_execute_visible_promises -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: medium-low. The command-like hidden More actions are now guarded
at render and key-router layers. A broader future pass should apply the same
listed-action predicate discipline to every secondary More key, not only send
and summarize.

## 2026-05-02 - Guard review send surface ownership

Pass: action-contract state audit. A staged multi-pane send must own the visible
surface; More, recent commands, macros, and stale secondary actions must not
leak into review or make `Enter`/`Esc` feel ambiguous.

Change: staging a multi-pane send now clears transient UI layers, opening More
is inert while review is pending, and the shell fails safe to the Send surface if
review and More state ever overlap.

Checks: `cargo fmt --check`; `cargo test --lib app::tests::pending_send_review_owns_the_visible_surface -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low. Review send now has a state-level regression for normal
staging, accidental More overlap, and repeated attempts to open More while
review is pending. Full UX, CI, and live performance gates passed.

## 2026-05-02 - Guard Details feedback movement truth

Pass: status-feedback footer audit. Temporary status messages must not make the
footer lie about whether `J/K` will scroll Details output or move Fleet
selection.

Change: strengthened the movement footer regression so Details with no output
keeps `J/K move` under feedback, while scrollable Details keeps `J/K scroll` and
never falls back to `J/K move`.

Checks: `cargo fmt --check`; `cargo test --lib app::tests::movement_footer_labels_match_j_k_behavior_across_panels -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low. Details feedback now has explicit regressions for both
scrollable and empty output, the status-feedback movement branches are covered,
and the full UX, CI, and live performance gates passed.

## 2026-05-02 - Guard ultra-narrow Fleet Latest truncation

Pass: renderer truncation audit. Fleet Latest wrapping must never overflow or
show mangled ellipsis when the terminal is too narrow to fit a normal `...`.

Change: added a selected Latest regression for tiny column widths. It proves
zero-width ellipsis collapses safely, two-cell truncation stays inside the cell,
and a selected running row wraps to three width-respecting lines without trying
to draw an ellipsis that cannot fit.

Checks: `cargo fmt --check`; `cargo test selected_latest_wrapper_handles_tiny_widths_without_overflow -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low. Tiny Fleet Latest columns now have an explicit renderer
guard, the formerly uncovered ellipsis branches are covered, and the full UX,
CI, and live performance gates passed.

## 2026-05-02 - Guard local navigation burst latency

Pass: performance-as-usability audit. Pressing `j` or arrow keys in a large
fleet must never route through slow tmux work or feel like one redraw per key.

Change: added a deterministic key-router performance regression that drives 160
movement keys through the real TUI key handler against a 96-pane fleet, proves
selection lands where expected, and keeps the burst under a human-lag threshold.
The guard is now part of `just perf-smoke`, and the architecture guard enforces
that it stays there.

Checks: `cargo fmt --check`; `cargo test navigation_key_burst_stays_in_memory_and_below_human_lag_threshold -- --nocapture`; `cargo test --test architecture_guards perf_smoke_covers_input_loop_renderer_and_large_fleets -- --nocapture`; `just perf-smoke`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low. Navigation now has an explicit key-router burst perf guard,
the guard is locked into `just perf-smoke`, and the full UX, CI, and live
performance gates passed.

## 2026-05-02 - Guard Browse footer movement truth

Pass: coverage-guided navigation footer audit. When Details is focused on
Browse, the footer must keep describing window browsing instead of falling back
to generic pane movement, even while status feedback is visible.

Change: strengthened the movement footer regression to prove roomy and feedback
footers say `J/K browse` and `Enter window`, and never `J/K move`, while browsing
windows.

Checks: `cargo fmt --check`; `cargo test --lib app::tests::movement_footer_labels_match_j_k_behavior_across_panels -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low. The Browse footer now has an explicit regression for
window movement copy under status feedback, and the full UX, CI, and live
performance gates passed.

## 2026-05-02 - Guard keybinding conflict diagnostics

Pass: coverage-guided rebind reliability audit. If a user customizes shortcuts,
conflict errors must name the exact earlier action they collided with instead of
falling back to a vague validation failure.

Change: strengthened the config regression so top-level conflicts are checked
against both an early action (`quit`) and a later action (`jump`), proving the
diagnostic stays specific across the binding scope.

Checks: `cargo fmt --check`; `cargo test --lib config::tests::load_ui_settings_rejects_conflicting_top_level_bindings -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low. Both top-level conflict diagnostic branches are now covered,
and the full UX, CI, and live performance gates passed.

## 2026-05-02 - Guard Output tail fallback observability

Pass: coverage-guided Output observability audit. The Output surface must not go
blank just because summarization and cleanup decide every captured line is low
value.

Change: added a regression where a pane has real captured output, no safe summary,
and every line would normally be cleaned as redundant. The Output tail now has a
test proving it still shows the raw line under Latest instead of hiding all
evidence from the user.

Checks: `cargo fmt --check`; `cargo test --lib app::tests::live_tail_falls_back_to_raw_output_when_cleaning_would_blank_it -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low. The blanked Output tail fallback branch is now covered,
and the full UX, CI, and live performance gates passed.

## 2026-05-02 - Guard roomy hidden send-list footer recovery

Pass: coverage-guided footer truth audit. When search or filters hide every
visible pane but a send list still exists, the footer must stay useful at roomy
widths instead of collapsing into vague hidden-target copy or status chatter.

Change: extended the footer regression to cover a 116-column hidden send-list
state with important feedback, proving the footer keeps the full send-list
count, send, clear, show-all, filter, back, more, and quit affordances visible.

Checks: `cargo fmt --check`; `cargo test --lib app::tests::footer_status_feedback_never_buries_recovery_or_keymap -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low. The roomy hidden-send-list footer branch is now covered,
and the full UX, CI, and live performance gates passed.

## 2026-05-02 - Guard startup control-spawn recovery

Pass: coverage-guided startup reliability audit. Muxboard should still open with
clear recovery copy when tmux can be probed from a supplied probe but the control
client cannot be spawned.

Change: added a bootstrap regression that feeds a missing tmux binary through
the startup path, verifies the app stays alive with no panes, records the
control failure as `not connected`, and keeps the empty Fleet recovery actionable.

Checks: `cargo fmt --check`; `cargo test --lib app::tests::startup_bootstrap_recovers_when_control_client_cannot_spawn -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low. The missing control-spawn startup branch is now covered,
and the full UX, CI, and live performance gates passed.

## 2026-05-02 - Guard ellipsis-only progress summaries

Pass: coverage-guided provider-summary audit. A terminal fragment that is only
an ellipsis should not normalize into an empty Fleet or Details summary while
ordinary visual truncation like `building...` still compacts cleanly.

Change: strengthened the core progress regression so ellipsis-only text remains
visible and prompt-like ellipses still stay untouched.

Checks: `cargo fmt --check`; `cargo test --lib core::progress::tests::visual_ellipsis_is_trimmed_without_touching_prompts -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low. The guard is isolated in core progress semantics and the
full UX, CI, and live performance gates passed.

## 2026-05-02 - Pause autoloop after reviewable diffs

Pass: autonomous-loop QA audit. `codex-autoloop` should not stack multiple
unreviewed autopass diffs while the prompt says "Do not commit" and AGENTS.md
requires bounded, reviewable passes.

Change: changed `codex-autoloop` to keep running the required `just ux`,
`just ci`, and `just perf-live` gates after a pass, then pause with explicit
review-and-commit copy if the tree is dirty. Added an architecture guard so the
recipe cannot silently regress back to piling up unreviewed changes.

Checks: `cargo fmt --check`; `just --summary`; `cargo test --test architecture_guards autonomous_loop_runs_the_active_goal_gates -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low after the architecture guard and full goal gates; unattended
loops still require human review before committing the paused diff.

## 2026-05-02 - Guard control wait failures

Pass: coverage-guided tmux control exit audit. If the control client process
cannot be waited on cleanly, muxboard should still surface a bounded exit event
instead of losing the failure or hanging the monitor.

Change: extracted control-client wait status mapping into a small helper and
added a direct test for wait errors. Also removed avoidable sleep from the
dropped-receiver fake control script and widened its bounded timeout so the
guard stays stable under full coverage load.

Checks: `cargo fmt --check`; `cargo test --lib tmux::control::tests -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for control wait failure reporting after targeted tests and
coverage, UX, CI, and live performance gates.

## 2026-05-02 - Guard control monitor receiver drops

Pass: coverage-guided tmux control audit. A dropped or replaced control monitor
must not strand a background reader task during reconnect, shutdown, or live
tmux churn.

Change: added a fake control-client test that starts the tmux control monitor,
drops the receiver, and proves the reader task exits within a bounded timeout
without panicking. The coverage pass confirmed the dropped-receiver branch is no
longer missing.

Checks: `cargo fmt --check`; `cargo test --lib tmux::control::tests::control_monitor_finishes_when_receiver_is_dropped -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for dropped receiver cleanup after fake-control coverage,
UX, CI, and live performance gates.

## 2026-05-02 - Full live dogfood after coverage hardening

Pass: live tmux dogfood audit after the literal-send, core-label, partial-line,
and XDG persistence guards. The goal was to verify the recent lower-layer
hardening against real tmux journeys, not only unit, renderer, and fake-tmux
tests.

Change: no product code change. Ran the full `just dogfood` suite, covering live
send, smart actions, action menu contracts, launch, first-run recovery,
notification persistence, output updates, saved fleets, stale fleets, search,
Command Center/Browse escape, narrow and SSH-like terminals, same-server jump,
resize churn, review-send pane disappearance, stale refresh recovery, and
multi-pane attention churn.

Checks: `just dogfood`.

Remaining risk: low for the live journeys covered by `just dogfood`; continue
coverage-guided hardening for still-uncovered startup, control, and renderer
edge branches.

## 2026-05-02 - Guard XDG store entrypoints

Pass: coverage-guided persistence audit. The public config/state store
constructors should stay XDG-style so muxboard keeps local and SSH machines
clean without falling back to `~/.muxboard` or platform-specific paths.

Change: added an environment-serialized path test that sets XDG config/state
roots, clears HOME, and proves `config::Store::new()` and `state::Store::new()`
resolve to `.../muxboard/config.json` and `.../muxboard/state.json`.

Checks: `cargo fmt --check`; `cargo test --lib paths::tests::config_and_state_stores_use_xdg_style_files -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for XDG store entrypoints after targeted path tests,
coverage, UX, CI, and live performance gates.

## 2026-05-02 - Guard live partial filtering against prompt noise

Pass: coverage-guided runtime-fragment audit. Prompt glyphs, tiny terminal echoes,
and short protocol fragments should never leak back into Fleet or Details, while
meaningful in-progress agent text should still reach the corpus.

Change: added core runtime tests for hidden prompt glyphs (`❯`, `>`, `>>`),
one- and two-character echoes, and punctuation-only short fragments. Added a
corpus guard proving prompt noise is excluded but meaningful partial text like
`Reply in exactly one line as: STATUS=running` is retained for provider parsing.

Checks: `cargo fmt --check`; `cargo test --lib live_fragment_filter -- --nocapture`; `cargo test --lib runtime_corpus_uses_meaningful_partial -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for live partial filtering after targeted core tests,
coverage, UX, CI, and live performance gates.

## 2026-05-02 - Guard core labels against unknown jargon

Pass: coverage-guided core model audit. The UI should never regress back to
opaque `unk` or `Unknown` labels for panes it is still checking, and supported
agent families need stable labels below the TUI layer.

Change: added core model tests covering every `WorkloadKind` short/display label
and agent-family classification, plus every `PaneStatus` short/display label.
The status guard explicitly rejects `unk` and `Unknown` so future UI work cannot
reintroduce that escaped wording through the shared model.

Checks: `cargo test --lib core::model::tests -- --nocapture`; `cargo fmt`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for core user-visible labels after targeted model tests,
coverage, UX, CI, and live performance gates.

## 2026-05-02 - Guard literal tmux sends

Pass: coverage-guided tmux IO audit. Summary polling and draft-style sends must
be able to type literal text into a pane without accidentally pressing Enter.

Change: added a fake-tmux wrapper test proving `send_text(..., false)` emits
only the literal `send-keys -l -- <text>` call, while `send_text(..., true)` emits
the literal text plus a separate Enter. This closes the uncovered no-enter branch
and protects the send/request-summary boundary from a high-blast-radius action
regression.

Checks: `cargo fmt --check`; `cargo test --lib tmux::tests::send_text_only_presses_enter_when_requested -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for literal tmux send behavior after targeted fake-tmux,
coverage, UX, CI, and live performance gates.

## 2026-05-02 - Guard stale-fleet footer recovery and fake-tmux isolation

Pass: renderer/X-ray stale-footer audit. A stale saved fleet should never look
sendable, even when the user has filtered the board and moved focus into
Details/Output.

Change: added a renderer guard for the narrowed, details-focused stale-fleet
state. The footer must keep `fleet stale`, `show all`, `Esc back`, More, and
Help visible, while avoiding dead `: send`, output, or focus chatter. While
running the full gate, a fake-tmux reliability gap surfaced, so app, config,
state, tmux, and control-test temp paths now use process-aware, timestamped
names instead of reusable counter-only names. Config and state atomic-write temp
files also include the process id.

Checks: `cargo test --lib stale_active_fleet -- --nocapture`; `cargo test --lib
app::tests:: -- --nocapture`; `cargo test --lib config::tests -- --nocapture`;
`cargo test --lib state::tests -- --nocapture`; `git diff --check && just ux &&
just ci && just perf-live`.

Remaining risk: low for narrowed stale-fleet footer recovery and fake-tmux
fixture isolation after renderer, app-slice, UX, CI, and live performance gates.

## 2026-05-02 - Keep saved-fleet summary feedback named

Pass: command-center copy consistency audit. A loaded saved fleet should stay a
named target across summary polling too, not only manual send and Smart Action.

Change: summary requests against stale or disappearing saved fleets now preserve
the fleet name in the status message. Added app-state guards for a stale loaded
fleet and a fake-tmux all-disappeared summary dispatch.

Checks: `cargo test --lib summary_ -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for saved-fleet summary feedback after summary tests, UX,
CI, and live performance gates.

## 2026-05-02 - Keep saved-fleet Smart Action labeled as the fleet

Pass: coverage-guided smart-action copy audit. A loaded saved fleet is a named
target, not a generic send list; Smart Action feedback must preserve that mental
model when Enter is sent or when every ready pane disappears mid-send.

Change: saved-fleet Smart Action status now prefers `Fleet <name>` over the
generic send-list label for both successful and all-disappeared Enter sends.
Added fake-tmux app-state guards for the success and pane-churn paths.

Checks: `just coverage-missing`; `cargo test --lib fleet_smart_action_reports_named_fleet -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for saved-fleet Smart Action copy after coverage, UX, CI,
and live performance gates.

## 2026-05-02 - Prove stale fleets against live tmux pane loss

Pass: live-tmux stale fleet recovery. The app-state, renderer, and action
contract guards now need a real tmux proof that a saved fleet stays recoverable
when its target pane disappears.

Change: added an ignored live E2E that saves a one-pane fleet, clears the search,
kills that target pane in tmux, refreshes muxboard, proves the stale fleet does
not fall back to `: send`, proves More exposes `choose fleet` and `delete stale`,
and deletes the stale fleet. Added the test to `just dogfood`.

Checks: `cargo test --test live_e2e stale_saved_fleet_stays_recoverable_after_live_pane_disappears -- --ignored --nocapture`; `cargo test --test architecture_guards dogfood_stays_aligned_with_non_perf_live_e2e_tests -- --nocapture`; `cargo test --test architecture_guards ux_action_recipes_exercise_real_key_and_tmux_actions -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`; `just dogfood`.

Remaining risk: low for live stale-fleet recovery after targeted live tmux,
dogfood alignment, full dogfood, UX, CI, and live performance gates.

## 2026-05-02 - Prove stale fleet More actions actually execute

Pass: action-contract follow-up for stale active fleets. If More promises
`choose fleet` or `delete stale`, pressing those keys must perform that recovery
instead of leaving a dead target or silently doing nothing.

Change: extended the saved-fleet More action-contract test to cover a stale
loaded fleet. The guard renders the stale menu, proves send is not advertised,
presses `L` to open Fleets, and presses `D` to delete the stale fleet.

Checks: `cargo test --lib usability_action_contract_more_saved_fleet_rows_execute_visible_actions -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for stale fleet More action contracts after UX, CI, and live
performance gates.

## 2026-05-02 - Keep stale active fleets from falling back to a pane

Pass: stale active-fleet audit. If a loaded saved fleet loses all live panes,
muxboard must keep that stale fleet visible and recoverable; it must not silently
offer commands against the selected pane.

Change: active saved fleets now remain the explicit target even when their live
member count falls to zero. Details show `Target: fleet ... has no live panes`,
command send and smart action stay local with fleet-specific recovery messages,
and More prioritizes `choose fleet` / `delete stale` ahead of lower-value pane
actions. Added app-state and renderer/X-ray guards.

Checks: `cargo test --lib command_panel_keeps_stale_loaded_fleet_visible -- --nocapture`; `cargo test --lib stale_active_fleet_keeps_recovery_visible_and_never_promises_send -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for stale active-fleet recovery after app-state, renderer,
coverage, UX, CI, and live performance gates.

## 2026-05-02 - Keep stale fleet loading out of the primary path

Pass: stale saved-fleet audit. A fleet with zero live panes is recoverable
state, not a useful load target; the picker should make delete/choose obvious
and avoid promoting a dead load.

Change: the Fleets footer now hides `Enter load` when the selected saved fleet
has no live panes and shows `D delete stale` instead. Pressing Enter on that
stale selection keeps the picker open and reports that the fleet has no live
panes. Added app-state and 60x12 renderer guards.

Checks: `cargo test --lib saved_fleet_picker_previous_from_middle_moves_one_row -- --nocapture`; `cargo test --lib tiny_fleet_picker_stale_selection_prioritizes_delete_over_load -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for stale saved-fleet recovery after app-state, renderer,
coverage, UX, CI, and live performance gates.

## 2026-05-02 - Keep tiny empty secondary views recoverable

Pass: cramped no-pane secondary-surface audit. Opening Output, Browse, or
Command Center with no tmux panes should still show how to recover and how to go
back; it should not advertise movement, output, send, filter, or show actions
against an empty fleet.

Change: moved empty snapshot footer recovery ahead of secondary-surface footer
special cases and kept `Esc back` when a secondary surface is open. Added a 60x12
renderer/X-ray guard for empty Output, Browse, and Command Center.

Checks: `cargo test --lib empty_navigator_actions_recover_without_guesswork -- --nocapture`; `cargo test --lib tiny_empty_tmux -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for no-pane secondary surfaces after app-state, renderer,
coverage, UX, CI, and live performance gates.

## 2026-05-02 - Make tiny first-run recovery actionable

Pass: cramped empty-tmux recovery audit. A first-run or broken tmux target must
show the next useful action, not pretend there are panes to move through or send
to.

Change: tightened the empty snapshot footer to show `? help`, `R refresh`, `. more`,
and `Q quit` instead of inert fleet actions. Added a 60x12 renderer/X-ray guard
covering no panes, no server, missing session, and unreadable tmux states.

Checks: `cargo test --lib tiny_empty_tmux_recovery_keeps_refresh_visible_without_fake_actions -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for tiny first-run recovery after renderer, coverage, UX,
CI, and live performance gates.

## 2026-05-02 - Guard tiny empty Browse recovery

Pass: cramped Browse empty-state audit. When Browse has no visible windows, the
surface must behave like recovery, not a dead navigator that advertises inert
window actions.

Change: added a renderer/X-ray guard proving a 60x12 empty Browse keeps
`No matching panes.`, `Action: backspace show all panes`, and `? help` visible while
hiding inert `Enter window`, `G show`, and `J/K browse` actions. No production
change was needed; the current empty Browse rendering already preserves the
recovery path.

Checks: `cargo test --lib tiny_empty_browse_keeps_recovery_visible_without_inert_actions -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for tiny empty Browse recovery after renderer, coverage,
UX, CI, and live performance gates.

## 2026-05-02 - Guard tiny Output usefulness

Pass: cramped Output overlay audit for real observability. Output is the
drill-down surface, so tiny terminals must still show the distilled summary,
fresh tail, and recovery key rather than only chrome or stale history.

Change: added a renderer/X-ray guard proving a 60x12 Output overlay keeps
`Summary`, `Latest`, the distilled handoff, the newest useful tail line, and
`Esc back` visible while dropping the oldest low-value tail. No production
change was needed; the current Output prioritizer already preserves useful
observability.

Checks: `cargo test --lib tiny_output_overlay_keeps_summary_latest_and_recovery_visible -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for tiny Output usefulness after renderer, coverage, UX,
CI, and live performance gates.

## 2026-05-02 - Guard tiny Help task visibility

Pass: cramped Help overlay audit after tightening decision overlays. Help must
remain a task map, not a footer dump, even on tiny terminals where users are
most likely to need clear recovery.

Change: added a renderer/X-ray guard proving a 60x12 Help overlay keeps the
title, `Now:`, `Send:`, `Move:`, and `Esc close` visible. No production change
was needed; the current Help rendering already preserves the core task map.

Checks: `cargo test --lib tiny_help_overlay_keeps_core_tasks_and_recovery_visible -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for tiny Help task visibility after renderer, coverage,
UX, CI, and live performance gates.

## 2026-05-02 - Guard tiny Command Center decision visibility

Pass: cramped Command Center audit for action truth. At tiny sizes the command
surface must still show the primary action, send target, start action, current
attention count, selected action, and recovery keys without turning into a
passive status dump.

Change: added a renderer/X-ray guard proving a 60x12 Command Center keeps
`Action:`, `Send:`, `Start:`, `Needs you:`, and the selected action visible in
the right order. No production change was needed; the current prioritizer
already preserves the decision path.

Checks: `cargo test --lib tiny_command_center_keeps_primary_actions_and_selection_visible -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for tiny Command Center decision visibility after renderer,
coverage, UX, CI, and live performance gates.

## 2026-05-02 - Guard tiny saved-fleet decision visibility

Pass: cramped Fleets overlay audit for action truth. Loading a saved fleet is a
state-changing choice, so the tiny overlay must still show which fleet is
selected, how many targets are live, and the obvious confirm/cancel keys.

Change: added a renderer/X-ray guard proving a 60x12 Fleets overlay keeps the
selected saved fleet, live target count, `Enter load`, and `Esc close` visible.
No production change was needed; the current Fleets rendering already preserves
the decision.

Checks: `cargo test --lib tiny_fleet_picker_overlay_keeps_selection_and_decision_visible -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for tiny Fleets decision visibility after renderer,
coverage, UX, CI, and live performance gates.

## 2026-05-02 - Guard tiny Start decision visibility

Pass: cramped Start overlay audit for footer/action truth. Start can promise
`Enter start` in the footer, so the tiny overlay must still show the destination
and command before the user launches a new agent.

Change: added a renderer/X-ray guard proving a 60x12 Start overlay keeps the
destination, command, `Enter start`, and `Esc cancel` visible. No production
change was needed; the current Start rendering already preserves the launch
decision.

Checks: `cargo test --lib tiny_start_agent_overlay_keeps_destination_command_and_recovery -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for tiny Start decision visibility after renderer,
coverage, UX, CI, and live performance gates.

## 2026-05-02 - Guard tiny Review Send decision visibility

Pass: cramped non-More overlay audit for footer/action truth. Review Send can
promise `Enter send` in the footer, so the tiny overlay must still show what
will be sent and to whom before the user commits.

Change: added a renderer/X-ray guard proving a 60x12 Review Send overlay keeps
`review`, the target, the command text, `Enter send`, and `Esc cancel` visible.
No production change was needed; the current Send prioritizer already preserves
the decision.

Checks: `cargo test tiny_confirm_send_overlay_keeps_the_decision_visible -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for tiny Review Send decision visibility after renderer,
coverage, UX, CI, and live performance gates.

## 2026-05-02 - Keep cramped More recovery above discovery

Pass: renderer priority audit after making `backspace show all panes`
discoverable in More. The tiny More overlay could still spend its only view
action slot on Command Center while hiding the recovery action for a narrowed
view.

Change: when More has only one visible view-action slot, `backspace show all
panes` outranks secondary discovery actions. The normal tiny More path still
keeps Start and Command Center discoverable when there is no narrowed-view
recovery.

Checks: `cargo test tiny_narrowed_more_overlay_keeps_show_all_recovery_visible -- --nocapture`; `cargo test tiny_more_overlay_keeps_start_agent_discoverable -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for cramped More recovery priority after renderer,
coverage, UX, CI, and live performance gates.

## 2026-05-02 - Keep More show-all sparse and discoverable

Pass: renderer/action-contract follow-up for the More recovery surface. The
show-all recovery key needed to be visible whenever a view is narrowed, but it
should not repeat as duplicate visual noise in empty search/filter states.

Change: More now lists `backspace show all panes` exactly once for narrowed
views, including narrowed views that still have visible matches. Pressing it
from More clears the scope, closes More, and restores the full Fleet/Details
view.

Checks: `cargo test action_menu_empty_states_are_recovery_not_inert_send_actions -- --nocapture`; `cargo test usability_action_contract_unlisted_keys_do_not_steal_modal_state -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for duplicate More recovery copy and narrowed-view show-all
behavior after app-state, renderer/action-contract, coverage, UX, CI, and live
performance gates.

## 2026-05-02 - Make More show-all recovery real

Pass: action-contract audit for recovery actions inside More. The empty/no-match
More menu advertised `backspace show all panes`, but the More key router only
handled Esc, listed action keys, and pane actions.

Change: Backspace now works from More when the current view is narrowed, closes
More, clears search/scope/filter, and returns to the normal Fleet/Details view.
Unlisted Backspace in normal More remains inert and protected by the same
action-contract test.

Checks: `cargo test usability_action_contract_unlisted_keys_do_not_steal_modal_state -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for More recovery action truth after targeted action
contracts, coverage, UX, CI, and live performance gates.

## 2026-05-02 - Guard empty recovery golden actions

Pass: escaped-bug guard follow-up for the empty Browse footer leak.

Change: golden-screen wayfinding now rejects empty recovery screens that have
no visible targets but still advertise pane/window actions like `Enter window`,
`Enter output`, `G show`, `J/K browse`, `Space add`, or `: send`.

Checks: `cargo test --test architecture_guards usability_golden_screens_keep_basic_wayfinding -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for this golden regression class after the architecture
guard and full active gates.

## 2026-05-02 - Make empty Browse a recovery surface

Pass: renderer/action-contract audit for Browse after the hidden-pane pass
exposed another footer-truth leak. An empty Browse result still advertised
`J/K browse`, `Enter window`, and `G show` even though no visible window could
receive those actions.

Change: empty Browse now behaves like a recovery surface: footer and Help keep
show-all/filter/more/back/quit actions, hide inert window actions, and accidental
Enter/G presses stay inside Browse with plain `No window selected in Browse.`
feedback. The golden empty Browse screen now protects this hierarchy.

Checks: `cargo test empty_navigator_actions_recover_without_guesswork -- --nocapture`; `cargo test usability_action_contract_unlisted_keys_do_not_steal_modal_state -- --nocapture`; `cargo test help_overlay_matches_secondary_surface_actions -- --nocapture`; `cargo test exact_grid_matches_empty_navigator_overlay -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for empty Browse footer/help/action truth after app-state,
renderer, action-contract, golden, coverage, and full active gates.

## 2026-05-02 - Block hidden pane-only actions

Pass: action-contract audit for no-match and filtered views after direct Send
was guarded. The same hidden selected pane could still respond to unadvertised
pane-only keys like Enter, Space, G, Smart Action, and More-menu pane actions.

Change: pane-only actions now require the selected pane to be visible. Hidden
selection states keep the send list intact, avoid opening Output, avoid tmux
jump/zoom/send side effects, keep More open for unlisted pane actions, and show
plain recovery copy that preserves `backspace show all`.

Checks: `cargo test hidden_selected_pane_actions_recover_instead_of_mutating_hidden_state -- --nocapture`; `cargo test usability_action_contract_unlisted_keys_do_not_steal_modal_state -- --nocapture`; `just ux-actions`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for hidden pane-only action side effects after app-state,
key-router, renderer, coverage, and full active gates.

## 2026-05-02 - Guard filtered-empty Send entry

Pass: action-contract audit for direct Send entry after the hidden-selection
guard covered search no-match states but not filter-only empty views.

Change: pressing `:` in an empty filtered view, such as `needs you` with no
attention panes, now stays out of Send, keeps `Show all panes before sending.`,
and preserves `backspace show all` recovery. Renderer and app-state guards prove
filter-only narrowing gets the same protection as search no-match.

Checks: `cargo test command_input_requires_a_visible_target_and_rejects_empty_text -- --nocapture`; `cargo test usability_action_contract_unlisted_keys_do_not_steal_modal_state -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for direct Send entry in filter-empty states after
app-state, action-contract, renderer, coverage guards, and full active gates.

## 2026-05-02 - Keep hidden send-list recovery under important status

Pass: coverage-guided footer status-feedback audit for hidden send lists when
an important message, like a disappeared target, competes with recovery keys.

Change: hidden send-list status feedback now compacts the target summary before
adding lower-value actions, so the footer can keep the important status plus
`1 pane hidden`, `: send`, `X clear`, and `backspace show all` visible without
falling back to generic pane actions.

Checks: `cargo test footer_status_feedback_never_buries_recovery_or_keymap -- --nocapture`; `cargo test usability_status_feedback_keeps_details_movement_visible -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for hidden send-list status feedback after app-state,
renderer, coverage guards, and full active gates.

## 2026-05-02 - Block hidden selections from opening Send

Pass: action-contract audit for the `:` key after the More-menu fixes exposed
the same hidden-target risk in direct command input.

Change: `:` now self-heals normal selection before opening Send, refuses
unmarked no-match views with `Show all panes before sending.`, and refuses
stale marked send lists with `Add a pane before sending.` The status-feedback
footer now keeps recovery actions in no-match views instead of reintroducing
pane-only actions like output, mark, or send after an invalid keypress.

Checks: `cargo test command_input_requires_a_visible_target_and_rejects_empty_text -- --nocapture`; `cargo test usability_action_contract_unlisted_keys_do_not_steal_modal_state -- --nocapture`; `cargo test empty_more_overlay_is_recovery_not_action_dump -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for direct Send entry in hidden/no-match states after
app-state, action-contract, renderer, coverage guards, and full active gates.

## 2026-05-02 - Stop stale and hidden More menus advertising fake sends

Pass: coverage-guided More-menu action-truth audit for stale marked panes and
no-match searches where the selected pane is hidden from the visible fleet.

Change: stale send lists now say `send list has no live panes` in Send and
More, the footer says `send list empty`, and stale states keep recovery actions
like `Space add` and `X clear` without advertising `: send`, summaries, or
save-fleet actions. While verifying, `just coverage-missing` exposed the
neighboring unmarked no-match state: More could advertise `: send commands`
against a hidden selected pane. More now treats unmarked no-match searches as
recovery-only, and a renderer guard proves the visible overlay offers show-all
recovery without fake send, summarize, or output actions.

Checks: `cargo test command_lines_recommend_selection_when_send_list_targets_disappear -- --nocapture`; `cargo test usability_more_menu_is_contextual_instead_of_an_action_dump -- --nocapture`; `cargo test action_menu_empty_states_are_recovery_not_inert_send_actions -- --nocapture`; `cargo test empty_more_overlay_is_recovery_not_action_dump -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for stale send-list and no-match More action truth after
app-state, renderer, coverage guards, and full active gates.

## 2026-05-02 - Keep More recovery visible for hidden send lists

Pass: coverage-guided More-menu recovery audit for marked send lists hidden by
search or view scope.

Change: when a send list exists but every target is hidden by the current view,
More now keeps `backspace show all panes` in the View section instead of only
showing send-list actions. The renderer guard proves this state exposes send,
clear, and show-all recovery without advertising pane-only actions like output,
zoom, or Space add.

Checks: `cargo test usability_more_menu_is_contextual_instead_of_an_action_dump -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for hidden send-list recovery inside More after renderer
and coverage guards plus full active gates.

## 2026-05-02 - Guard send-list footer recovery on narrow/status screens

Pass: coverage-guided footer audit for marked send lists when Output is open
or status feedback is visible.

Change: strengthened footer tests so narrow send-list footers keep `Esc back`
visible in Output, and status-feedback footers say `Space add` when the
selected pane is not yet in the send list. While verifying, a real action
contract gap appeared: low-value mark feedback could replace the useful
footer, and a hidden send list under a no-match search could advertise inert
pane movement. The fix hides redundant mark feedback from the footer, makes
the Space hint match the visible selection, and gives hidden send lists
recovery-first copy: hidden target count, send, clear, show all.

Checks: `cargo test footer_status_feedback_never_buries_recovery_or_keymap -- --nocapture`; `cargo test no_match_fleet_footer_lists_recovery_not_pane_actions -- --nocapture`; `cargo test send_list_surfaces_targets_hidden_by_current_view -- --nocapture`; `cargo test view_model_fixtures_hold -- --nocapture`; `cargo test usability_action_feedback_never_steals_the_footer_keymap_on_roomy_screens -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for narrow/status send-list footers and hidden send-list
no-match recovery after renderer, app-state, coverage, and full active gates.

## 2026-05-02 - Tighten empty fleet and output header recovery

Pass: coverage-guided empty-state audit for hidden recovery copy and no-pane
panel headers.

Change: changed the empty Fleets copy to `Mark panes, then save a fleet from
More.` and added app-state coverage proving Output headers still explain
`No panes yet.` when no tmux panes exist.

Checks: `cargo test presentation_modes_cover_empty_states_and_overlay_branches -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for no-pane Output headers and empty Fleets recovery copy;
the coverage X-ray no longer lists the targeted empty-state branches.

## 2026-05-02 - Keep stale loaded fleets visible

Pass: coverage-guided Command Center audit for saved fleets whose panes are no
longer live.

Change: added an app-state guard proving a loaded saved fleet with zero live
panes still stays visible as `fleet triage` in the Send panel, instead of
silently collapsing into generic selected-pane targeting.

Checks: `cargo test command_panel_keeps_stale_loaded_fleet_visible -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for stale saved-fleet visibility; the coverage X-ray no
longer lists the active-fleet Send panel branch targeted by this pass.

## 2026-05-02 - Guard Output help contract

Pass: coverage-guided Help audit for the Output surface, where Enter should
not be advertised as a fake action once the user is already in Output.

Change: strengthened the app-state Help tests so Output Help says `Esc back to
details, G show in tmux`, includes `A continue waiting` only when a waiting
pane can safely receive Enter, and never leaks `Enter keeps output`.

Checks: `cargo test help_lines_match_secondary_surface_actions -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for Output Help action truth; the coverage X-ray no longer
lists the Output Help branches targeted by this pass.

## 2026-05-02 - Ban stale saved-command recovery copy

Pass: coverage-guided Command Center copy audit after `just coverage-missing`
exposed a stale fallback that said `No saved commands yet.`

Change: replaced the fallback with the action-oriented `Type : to send a
command.` and added `saved commands` to the retired production-copy guard so
nonexistent saved-command language cannot quietly return.

Checks: `cargo test --test architecture_guards production_user_copy_avoids_retired_product_terms -- --nocapture`; `cargo test --test architecture_guards public_copy_and_fixtures_avoid_retired_product_terms -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for stale saved-command language in production copy; the
architecture guards now fail if this retired wording returns.

## 2026-05-02 - Guard lane continue hints

Pass: coverage-guided action-contract pass for the wide footer's `A continue`
hint in lane send mode.

Change: extended the footer test so lane mode does not advertise `A continue`
when no lane pane is enter-safe, and does advertise it when a waiting peer in
the lane can safely receive Enter.

Checks: `cargo test status_hint_shows_continue_only_when_enter_is_safe -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for lane-mode footer continue hints.

## 2026-05-02 - Make saved-fleet list counts readable

Pass: coverage-guided copy pass after `just coverage-missing` exposed the
saved-fleet list branch in Command Center as untested.

Change: changed saved-fleet list rows from bare `triage (2)` style counts to
`triage (2 panes)` / `solo (1 pane)`, with an app-state regression test for the
visible Command Center rows.

Checks: `cargo test command_panel_saved_fleets_use_self_explanatory_pane_counts -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for saved-fleet list count copy in Command Center.

## 2026-05-02 - Guard target-count copy against terse regressions

Pass: escaped-bug architecture guard for target-count copy after live dogfood
proved bare counts like `send list 1` and saved-fleet `(2)` labels are too
easy to reintroduce.

Change: added a source, fixture, live-e2e, and golden-screen guard that rejects
user-visible target counts without `pane` or `panes`, and cleaned up stale test
fixtures that still used `fleet triage (5)` and `lane Codex (3)`.

Checks: `cargo test --test architecture_guards target_count_guard -- --nocapture`; `cargo test --test architecture_guards user_visible_target_counts_stay_self_explanatory -- --nocapture`; `cargo test short_send_overlay_keeps_target_identity_and_confirm_confirmation_before_history_noise -- --nocapture`; `cargo test view_model_fixtures_hold -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for terse target-count regressions across source strings,
fixtures, golden screens, and live-e2e assertions.

## 2026-05-02 - Make saved-fleet and lane counts self-explanatory

Pass: live-dogfood follow-up for target-count copy after `send list 2` became
`send list 2 panes`.

Change: updated saved-fleet, lane, header, review-send, golden, fixture, and
live e2e copy to use `2 panes` instead of bare `(2)` or `send list 1`, after
`just dogfood` caught stale live expectations for `Send: fleet triage (2)` and
`send list 1`.

Checks: `cargo test target_scope_copy_stays_plain_for_empty_and_marked_states -- --nocapture`; `cargo test view_model_fixtures_hold -- --nocapture`; `cargo test panel_fixtures_hold -- --nocapture`; `cargo test exact_grid_matches_confirm_send_overlay -- --nocapture`; `cargo test --test live_e2e saved_group_persists_across_restart_and_can_be_reloaded -- --ignored --nocapture`; `cargo test --test live_e2e review_send_survives_target_pane_disappearing_before_confirm -- --ignored --nocapture`; `just dogfood`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for complete target-count copy across saved fleets, send
lists, lane sends, headers, review screens, renderer fixtures, and live tmux
dogfood.

## 2026-05-02 - Keep focused Details movement visible under status feedback

Pass: renderer-level footer contract pass for status feedback that could make
focused Details or Browse feel inert by hiding the real movement command.

Change: added an X-ray renderer test proving status feedback at 104 columns
keeps `J/K scroll` for Output and `J/K browse` for Browse, keeps recovery hints
visible, and never replaces the footer with vague focus chatter.

Checks: `cargo test usability_status_feedback_keeps_details_movement_visible -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for focused Details and Browse footer movement under status
feedback; broader footer edge paths still remain in the coverage risk map.

## 2026-05-02 - Make send-list counts self-explanatory

Pass: coverage-guided footer/status UX pass for send-list and status-feedback
copy that could force users to decode terse count labels like `send list 2`.

Change: changed no-hidden send-list count summaries from bare numbers to
`1 pane` / `2 panes`, removed a dead footer branch that could never execute,
and added a footer guard proving status feedback does not bury recovery actions
or dense keymaps.

Checks: `cargo test target_scope_copy_stays_plain_for_empty_and_marked_states -- --nocapture`; `cargo test footer_status_feedback_never_buries_recovery_or_keymap -- --nocapture`; `cargo test -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for covered send-list count and footer status-feedback copy;
broader footer and renderer edge paths still remain in the coverage risk map.

## 2026-05-02 - Cover action-menu summary recommendations

Pass: coverage-guided UX guard pass for More-menu recommendation copy that is
easy to miss because it only appears for reported agents without alerts or for
attention queues away from the selected pane.

Change: added app-state tests proving the recommendation line stays useful
across the whole high-risk branch set: no tmux panes, no visible matches, stale
send-list targets, live send lists, enter-safe waiting panes, selected alerts,
reported agents without alerts, off-selection attention, and the default
single-pane send/add state.

Checks: `cargo test command_lines_recommend_ -- --nocapture`;
`just coverage-missing`; `git diff --check && cargo test
command_lines_recommend_ -- --nocapture && just ux && just ci && just
perf-live`.

Remaining risk: low for these recommendation branches; coverage still shows
other presentation and TUI edge lines that should be triaged by user risk rather
than coverage count alone.

## 2026-05-02 - Clarify README sort and filter shortcuts

Pass: public-doc copy pass for More-menu shortcut labels that still described
`t` and `f` as bare "sort" and "filter" actions.

Change: changed README shortcut copy to "change sort order" and "change visible
panes", and added retired-copy guards for the old terse labels.

Checks: `cargo test --test architecture_guards readme_shortcuts_match_generated_default_keybindings -- --nocapture`; `cargo test --test architecture_guards public_copy_and_fixtures_avoid_retired_product_terms -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for README shortcut copy; in-app More labels were already
contextual, for example "sort by ..." and "show ...".

## 2026-05-02 - Make summary request copy read like an action

Pass: product/design copy pass for summary requests that still used "poll" in
the README and "Requested one-line summaries" in status feedback.

Change: changed the shortcut copy to "ask panes for one-line summaries" and
changed the status message to "Asked ... for ..." with singular/plural summary
wording. Added retired-copy guards for the old poll/request phrases.

Checks: `cargo test summary_ -- --nocapture`; `cargo test app_tmux_action_paths_are_exercised_against_fake_tmux_binary -- --nocapture`; `cargo test usability_action_contract_more_menu_actions_execute_visible_promises -- --nocapture`; `cargo test --test architecture_guards readme_shortcuts_match_generated_default_keybindings -- --nocapture`; `cargo test --test architecture_guards production_user_copy_avoids_retired_product_terms -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for covered summary-request copy; the prompt sent to panes
still uses the structured protocol internally, but parser/rendering guards keep
that protocol out of visible Fleet and Details copy.

## 2026-05-02 - Clarify desktop alerts on SSH and terminal-only sessions

Pass: product/design copy pass for desktop-alert settings that still said
"SSH-safe" or "terminal" in parentheses and could imply GUI notifications were
available when muxboard was running over SSH or without a desktop notifier.

Change: changed visible fallback copy to "desktop alerts unavailable on SSH" or
"desktop alerts unavailable here", changed status feedback to explain that the
terminal bell still works, and added retired-copy guards for the old fallback
phrases.

Checks: `cargo test local_settings_toggles_cycle_through_user_visible_states -- --nocapture`; `cargo test usability_ssh_notification_mode_is_visible_and_never_promises_desktop_delivery -- --nocapture`; `cargo test usability_more_menu_is_contextual_instead_of_an_action_dump -- --nocapture`; `cargo test usability_action_contract_more_menu_actions_execute_visible_promises -- --nocapture`; `cargo test exact_grid_matches_actions_overlay -- --nocapture`; `cargo test rendered_primary_journeys_avoid_retired_user_terms -- --nocapture`; `cargo test --test architecture_guards live_notification_settings_stay_covered_at_persistence_boundary -- --nocapture`; `cargo test --test architecture_guards production_user_copy_avoids_retired_product_terms -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`; `cargo test --test live_e2e notification_settings_persist_across_restart_and_stay_ssh_safe -- --ignored --nocapture`.

Remaining risk: low for covered notification fallback copy; full live dogfood was
not rerun, but the changed live notification journey and the live performance
gate both passed.

## 2026-05-02 - Rename bell copy to terminal bell

Pass: product/design copy pass for notification settings that still exposed the
bare word "bell" and forced users to infer that it means their terminal bell,
especially in SSH-safe notification copy.

Change: changed visible bell copy to "terminal bell" across More, README,
status feedback, renderer action contracts, SSH-safe desktop alert fallback
copy, and retired-copy guards.

Checks: `cargo test local_settings_toggles_cycle_through_user_visible_states -- --nocapture`; `cargo test usability_more_menu_is_contextual_instead_of_an_action_dump -- --nocapture`; `cargo test more_overlay -- --nocapture`; `cargo test usability_action_contract_more_menu_actions_execute_visible_promises -- --nocapture`; `cargo test exact_grid_matches_actions_overlay -- --nocapture`; `cargo test --test architecture_guards public_copy_and_fixtures_avoid_retired_product_terms -- --nocapture`; `cargo test --test architecture_guards readme_shortcuts_match_generated_default_keybindings -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for covered visible terminal-bell copy; internal setting and
test names still use bell as implementation language.

## 2026-05-02 - Rename metrics copy to pane CPU/memory

Pass: product/design copy pass for optional host-local pane metrics that still
exposed a vague "metrics" label.

Change: changed visible metrics copy to "pane CPU/memory" or "pane CPU/mem"
across More, README, status feedback, board titles, Details lines, renderer
guards, and retired-copy guards. The README now states that values are local to
the host running muxboard, which keeps SSH behavior clear.

Checks: `cargo test metrics -- --nocapture`; `cargo test usability_more_menu_is_contextual_instead_of_an_action_dump -- --nocapture`; `cargo test more_overlay -- --nocapture`; `cargo test usability_action_contract_more_menu_actions_execute_visible_promises -- --nocapture`; `cargo test exact_grid_matches_actions_overlay -- --nocapture`; `cargo test rendered_primary_journeys_avoid_retired_user_terms -- --nocapture`; `cargo test --test architecture_guards retired_product_terms -- --nocapture`; `cargo test --test architecture_guards readme_shortcuts_match_generated_default_keybindings -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for covered visible CPU/memory copy; internal module and
field names still use metrics as implementation language.

## 2026-05-02 - Rename alert rule copy to alert types

Pass: product/design copy pass for advanced alert settings that still forced the
user to interpret "rule", "policy", and compact status values like "wait+err".

Change: changed visible alert-selection copy to "alert types", changed status
feedback to "Alerts: waiting + errors" style labels, and retired the old
"alert rule" / "Alert policy" phrases from visible copy.

Checks: `cargo test local_settings_toggles_cycle_through_user_visible_states -- --nocapture`; `cargo test formatting_and_binding_helpers_cover_edges -- --nocapture`; `cargo test usability_more_menu_is_contextual_instead_of_an_action_dump -- --nocapture`; `cargo test more_overlay -- --nocapture`; `cargo test usability_action_contract_more_menu_actions_execute_visible_promises -- --nocapture`; `cargo test --test architecture_guards retired_product_terms -- --nocapture`; `cargo test --test architecture_guards readme_shortcuts_match_generated_default_keybindings -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for covered visible alert-type copy; internal `AlertPolicy`
names remain implementation language only.

## 2026-05-02 - Rename alert debounce copy to repeat delay

Pass: product/design copy pass for advanced alert settings that still exposed
the implementation term "debounce."

Change: changed visible alert timing copy to "alert repeat delay" across More,
README shortcuts, and status feedback. Added retired-copy guards for the old
"alert debounce" wording.

Checks: `cargo test local_settings_toggles_cycle_through_user_visible_states -- --nocapture`; `cargo test usability_more_menu_is_contextual_instead_of_an_action_dump -- --nocapture`; `cargo test more_overlay -- --nocapture`; `cargo test usability_action_contract_more_menu_actions_execute_visible_promises -- --nocapture`; `cargo test exact_grid_matches_actions_overlay -- --nocapture`; `cargo test --test architecture_guards retired_product_terms -- --nocapture`; `cargo test --test architecture_guards readme_shortcuts_match_generated_default_keybindings -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for covered user-visible alert timing copy; internal
`debounce_seconds` and function names remain implementation language only.

## 2026-05-02 - Make no-match recovery say what Backspace does

Pass: product/design copy pass for empty search and narrowed Browse states that
still exposed "scope" and "backspace all panes" wording.

Change: changed no-match recovery to "Backspace shows all panes", changed
compact footers/help to "backspace show all", changed narrowed-window status to
"Showing only ...", and added retired-copy guards for the old phrases.

Checks: `cargo test scope -- --nocapture`; `cargo test usability_help_lines_are_task_oriented_instead_of_footer_repetition -- --nocapture`; `cargo test fixtures -- --nocapture`; `cargo test exact_grid_matches_empty_search_board -- --nocapture`; `cargo test exact_grid_matches_empty_navigator_overlay -- --nocapture`; `cargo test exact_grid_matches_help_overlay -- --nocapture`; `cargo test --test architecture_guards retired_product_terms -- --nocapture`; `cargo test --test architecture_guards usability_help_overlay_stays_task_oriented_not_a_footer_dump -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for covered no-match recovery surfaces; internal test names
and `ViewScope` implementation terms remain as code-only language.

## 2026-05-02 - Rename lane fanout copy to lane sends

Pass: product/design copy pass for the remaining "fanout" wording in lane
broadcasts.

Change: changed user-facing fanout language to lane send/sends across status
messages, header context, docs, live e2e expectations, and retired-copy guards.
Details now describes active lane sends as "Lane: send to N panes" instead of
"fanout lane."

Checks: `cargo test fanout_targets_selected_lane_members -- --nocapture`; `cargo test top_level_presentation_metadata_tracks_modes_and_targets -- --nocapture`; `cargo test app_state_reducers_cover_navigation_groups_and_settings_edges -- --nocapture`; `cargo test usability_action_contract_more_menu_actions_execute_visible_promises -- --nocapture`; `cargo test --test live_e2e lane_smart_action_sends_enter_to_waiting_agents_only -- --ignored --nocapture`; `cargo test --test architecture_guards retired_product_terms -- --nocapture`; `cargo test --test architecture_guards public_readme_stays_scan_first_and_current -- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for covered lane-send copy; internal `FanoutMode` remains as
implementation terminology only.

## 2026-05-02 - Make send-list copy read like actions

Pass: product/design pass for remaining send-list wording that still made users
decode "add panes" and "command selected/list".

Change: changed Command Center's idle action to lead with the obvious primary
action, ": send this pane", and changed Help send copy to "command pane or list,
add/remove pane." Added renderer coverage for the Command Center sentence and
retired the old ambiguous phrases in the production/public copy guards.

Checks: `cargo test control_lines_count_all_supported_agent_families -- --nocapture`; `cargo test usability_help_lines_are_task_oriented_instead_of_footer_repetition -- --nocapture`; `cargo test footer_and_help_copy_honor_rebound_keys -- --nocapture`; `cargo test exact_grid_matches_help_overlay -- --nocapture`; `cargo test --test architecture_guards retired_product_terms -- --nocapture`; `cargo test --test architecture_guards usability_help_overlay_stays_task_oriented_not_a_footer_dump -- --nocapture`; `cargo test usability_more_menu_is_contextual_instead_of_an_action_dump -- --nocapture`; `cargo test usability_command_center_target_action_reads_like_a_sentence -- --nocapture`; `git diff --check`; `just ux && just ci && just perf-live`.

Remaining risk: low for covered Command Center and Help copy; a first UX run
caught and we fixed a false low-value-copy guard update before closing.

## 2026-05-02 - Full live dogfood after copy guard passes

Pass: release/live-QA pass after retiring visible target jargon, clarifying Start
recovery copy, and adding production-source copy guardrails.

Change: no product code changes. Ran the full live tmux dogfood suite to verify
that command dispatch, jumping, Start, review-send recovery, persistence,
SSH-like rendering, resize churn, live output updates, and multi-pane attention
churn still work against real tmux instances after the copy and guard changes.

Checks: `just dogfood`.

Remaining risk: low for covered live tmux journeys; the full dogfood suite stayed
green.

## 2026-05-02 - Keep retired-copy guards in one list

Pass: QA maintainability pass after adding production-source copy scanning. The
public-copy and production-copy guards were protecting the same retired phrases
with separate lists, which could drift.

Change: centralized the shared retired product-copy terms in the architecture
guard and left only public-fixture-specific extras in a second list.

Checks: `cargo test --test architecture_guards retired_product_terms -- --nocapture`; `just ux && just ci && just perf-live`.

Remaining risk: low for guard-list drift inside architecture guards; app-level
view-model copy checks still keep their own smaller product-surface assertions.

## 2026-05-02 - Guard retired copy in production source

Pass: QA/system pass for the blind spot that let a hard-coded Start recovery
status avoid public fixture and golden-screen copy checks.

Change: added an architecture guard that scans production app/TUI source for the
retired user-facing phrases before they reach a fixture or screenshot. Test code
can keep internal names, but production copy cannot reintroduce the old terms.

Checks: `cargo test --test architecture_guards production_user_copy_avoids_retired_product_terms -- --nocapture`; `just ux && just ci && just perf-live`.

Remaining risk: low for exact retired-phrase regressions in covered production
app/TUI files; semantic copy regressions still need renderer and action-contract
journey tests.

## 2026-05-02 - Start recovery copy names the pane, not the target

Pass: follow-on product copy pass after the visible "target" cleanup. The Start
flow still had one escaped recovery message that described a disappeared pane as
a disappeared target.

Change: changed the Start recovery status to "Start canceled; pane disappeared.
Refreshed panes." and added that old phrase to the app and public-copy retired
term guards.

Checks: `cargo test start_agent_refreshes_when_selected_session_disappears -- --nocapture`; `cargo test primary_view_model_copy_avoids_retired_user_terms -- --nocapture`; `cargo test --test architecture_guards public_copy_and_fixtures_avoid_retired_product_terms -- --nocapture`; `git diff --check`; `just ux && just ci && just perf-live`.

Remaining risk: low for the covered Start recovery copy; focused app/copy guards
and the standard UX/CI/perf gates stayed green.

## 2026-05-02 - Retire target jargon from visible copy

Pass: product/design pass for the remaining visible "target" wording that made
send-list and recovery states sound like implementation internals.

Change: changed hidden-recipient copy from "target hidden by current view" to
"pane hidden by current view", changed vanished-recipient recovery from "No
target panes remain" to "No panes remain", and changed the Help/demo legend from
"+ send target" to "+ listed". Strengthened app, renderer, golden, and
public-copy guards so those retired phrases stay out of user-visible surfaces.

Checks: `cargo test hidden_targets -- --nocapture`; `cargo test user_visible_edge_copy_helpers_stay_plain -- --nocapture`; `cargo test help_lines_include_board_state_legend -- --nocapture`; `cargo test narrow_help_overlay_wraps_sentences_in_place_without_extra_rows -- --nocapture`; `cargo test --test architecture_guards public_copy_and_fixtures_avoid_retired_product_terms -- --nocapture`; `cargo test --test architecture_guards usability_golden_screens_avoid_internal_protocol_and_retired_words -- --nocapture`; `cargo test exact_grid_matches_help_overlay -- --nocapture`; `cargo test --test live_e2e review_send_recovers_when_every_target_pane_disappears_before_confirm -- --ignored --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for the covered visible-copy and disappeared-pane recovery
surfaces; the full UX/CI/perf gates stayed green.

## 2026-05-02 - More keeps SSH-safe notification controls visible

Pass: escaped-bug QA pass from live dogfood. Moving report rows above settings
made the More overlay hide SSH-safe desktop-alert and bell controls on a normal
terminal while a pane needed attention.

Change: tightened More overlay prioritization so tight report-heavy menus spend
one fewer row on secondary view actions and keep the SSH-safe desktop-alert and
bell rows visible. Added renderer coverage for reports plus SSH-safe settings in
the same More overlay.

Checks: `cargo test usability_more_menu_is_contextual_instead_of_an_action_dump -- --nocapture`; `cargo test more_overlay -- --nocapture`; `cargo test --test live_e2e notification_settings_persist_across_restart_and_stay_ssh_safe -- --ignored --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live && just dogfood`.

Remaining risk: low for the covered More-menu notification controls; the full
active-goal and dogfood gates stayed green after the live failure fix.

## 2026-05-02 - Primary surfaces hide raw tmux identity

Pass: product/QA pass for the broader class behind raw `%...` pane ID leaks:
default user surfaces should explain panes with human session/window labels, not
tmux internals.

Change: added a renderer/X-ray guard that renders the home, Output, Send,
Browse, Command Center, More, Help, and marked-send-list states with raw tmux
pane/session/window IDs in the model and fails if those implementation IDs become
visible.

Checks: `cargo test usability_primary_surfaces_hide_raw_tmux_identity -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for the covered primary surfaces; the active goal gates stayed green.

## 2026-05-02 - More-menu report labels avoid tmux IDs

Pass: product/design pass for the risk that More-menu report rows could reintroduce
raw tmux pane IDs after the main UI stopped explaining panes with `%...` jargon.

Change: report rows now identify targets by the same human session/window label
used elsewhere, with app and renderer/X-ray regression coverage for Claude and
opencode report rows. The More overlay now keeps report rows ahead of lower-value
settings when vertical space is tight.

Checks: `cargo test usability_more_menu_is_contextual_instead_of_an_action_dump -- --nocapture`; `cargo test more_overlay -- --nocapture`; `cargo test active_target_report_lines_use -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for the targeted More-menu report labels; the active goal gates stayed green.

## 2026-05-02 - Full live dogfood after Details simplification

Pass: QA/release pass for the risk that the Details hierarchy simplification
could look fine in renderer and unit tests but regress real tmux actions,
selection, focus, persistence, SSH-like terminal rendering, or live output churn.

Change: no product code changes. Ran the full live tmux dogfood suite after the
committed Details metrics and report-section passes.

Checks: `just dogfood`.

Remaining risk: low for the covered live tmux journeys; dogfood stayed green.

## 2026-05-02 - Details report section omission

Pass: product/design/QA pass for the risk that Details could spend scarce space
on a redundant "Agent report" block instead of the user's actual decision and
recent output.

Change: removed the secondary Details report section and added a regression
proving stored agent report fields surface as top-level Blocked/Action copy while
real Output stays visible.

Checks: `cargo test selected_pane_lines_surface_stored_reports_without_a_second_report_section -- --nocapture`; `cargo test selected_pane_lines_suppress_low_value_report_for_normal_running_state -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for the targeted Details report hierarchy path; the active goal gates stayed green.

## 2026-05-02 - Details metrics hierarchy guard

Pass: product/QA pass for the risk that optional local metrics could crowd ahead
of the agent's actual recent output in Details.

Change: added a presentation regression proving selected-pane Details keeps
useful Output visible before the local metrics line while preserving exact CPU,
memory, pid, and elapsed details when metrics are enabled.

Checks: `cargo test selected_pane_lines_put_local_metrics_after_output_when_available -- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for the targeted Details metrics hierarchy path; the active goal gates stayed green.

## 2026-05-02 - Terminal locale compatibility guard

Pass: local/SSH compatibility pass for the risk that muxboard could choose
Unicode borders or colors from the wrong terminal environment signal.

Change: strengthened terminal-profile coverage for `LC_ALL` precedence, blank
locale fallback to `LC_CTYPE`, padded `CLICOLOR=0`, and explicit color enablement
without adding platform assumptions.

Checks: `cargo test terminal_profile_downgrades_dumb_or_non_utf8_terminals
-- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low for the targeted terminal profile env-matrix path; the
active goal gates stayed green.

## 2026-05-02 - Normal Details suppresses low-value report guard

Pass: coverage-guided product/QA pass for the risk that normal running agents
could let lower-value structured report details crowd ahead of fresh output in
Details.

Change: added a presentation regression proving normal running Details keeps
useful Output visible and suppresses a generic Agent report/action when it adds
no user-meaningful information.

Checks: `cargo test
selected_pane_lines_suppress_low_value_report_for_normal_running_state -- --nocapture`;
`just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for the targeted normal-running Details report-suppression
path; the active goal gates stayed green.

## 2026-05-02 - Tiny Details row-budget guard

Pass: coverage-guided renderer/UX pass for the risk that cramped Details panels
could overproduce prelude rows or expose empty sections instead of a clean,
bounded decision surface.

Change: capped Details prelude rows by the actual visible row budget and removed
an unreachable parser branch. Strengthened helper coverage for tiny Details
budgets and empty Agent report/Command section pruning while keeping useful
Output and roomy metadata placement intact.

Checks: `cargo test priority_helpers_keep_useful_lines_under_severe_height_pressure
-- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low for the targeted cramped Details row-budget path; the
active goal gates stayed green.

## 2026-05-02 - Tiny Output overlay useful-tail guard

Pass: renderer/UX pass for the risk that extremely short Output overlays spend
their scarce rows on identity metadata while hiding the actual agent output.

Change: added a fallback path for Output overlays that are too short to fit
section headings, keeping at most one identity row and then showing distilled
summary plus the newest tail line. Strengthened helper coverage for blank-line
filtering, tiny summary/latest fallback, and one-row latest-only output.

Checks: `cargo test overlay_priority_helpers_cover_sparse_overflow_and_fallback_paths
-- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low for the targeted tiny Output overlay priority path; the
active goal gates stayed green.

## 2026-05-02 - More overlay empty-section guard

Pass: coverage-guided renderer/UX pass for the risk that More shows empty
section headings or lets lower-value sections steal the scarce rows on small
terminals.

Change: removed unreachable empty-section cleanup after proving the More
prioritizer only enters sections that can show at least one item, then added
helper coverage for skipped empty sections, zero-cap action sections, tight
yes/no panes, and one-row Command Center prioritization.

Checks: `cargo test overlay_priority_helpers_cover_sparse_overflow_and_fallback_paths
-- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just
ci && just perf-live`.

Remaining risk: low for the targeted More overlay section-priority path; the
active goal gates stayed green.

## 2026-05-02 - Send overlay vars-copy guard

Pass: coverage-guided renderer/UX pass for the risk that low-value command
template copy either disappears when it is useful or competes with review and
preview content when the user is about to send.

Change: strengthened Send overlay priority coverage so template variables appear
only when there is room and no review/preview is active, while skipped recent and
macro sections stay out of the cramped decision surface.

Checks: `cargo test send_priority_helpers_cover_confirm_preview_vars_and_overflow
-- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just
ci && just perf-live`.

Remaining risk: low for the targeted Send overlay vars-copy path; the active
goal gates stayed green.

## 2026-05-02 - Send overlay row-budget guard

Pass: renderer/UX pass for the risk that a cramped Send overlay keeps too many
summary or fallback lines, crowding out the user's next safe action.

Change: capped the Send overlay's top-priority and fallback rows by the actual
visible row budget, then strengthened helper coverage for severe height pressure
so target identity wins and lower-value sections wait their turn.

Checks: `cargo test send_priority_helpers_cover_confirm_preview_vars_and_overflow
-- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just
ci && just perf-live`.

Remaining risk: low for the targeted cramped Send overlay row-budget path; the
active goal gates stayed green.

## 2026-05-02 - Fleets picker priority guard

Pass: coverage-guided renderer/UX pass for the risk that a long saved-fleet
picker hides the currently selected fleet under height pressure.

Change: added helper coverage proving the Fleets picker centers the selected row
when possible, clamps correctly near the bottom, falls back to top rows when no
row is selected, and keeps zero-height prioritization inert.

Checks: `cargo test fleet_picker_priority_keeps_the_selected_saved_fleet_visible
-- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just
ci && just perf-live`.

Remaining risk: low for the targeted Fleets picker priority path; the active
goal gates stayed green.

## 2026-05-02 - Details output priority guard

Pass: coverage-guided renderer/UX pass for the risk that roomy Details panels
fall back to raw source order, letting metadata or command history crowd out the
recent Output users need to understand an agent.

Change: removed the dead early return that made noncompact Details
prioritization unreachable, then added helper/X-ray coverage for long labels,
tiny widths, empty label values, roomy Details output caps, metadata placement,
and roomy Output-overlay section spacing.

Checks: `cargo test panel_line_wrapper_keeps_labels_and_indents_continuations
-- --nocapture`; `cargo test
priority_helpers_keep_useful_lines_under_severe_height_pressure --
--nocapture`; `cargo test
overlay_priority_helpers_cover_sparse_overflow_and_fallback_paths --
--nocapture`; `just coverage-missing`; `git diff --check && just ux && just
ci && just perf-live`.

Remaining risk: low for the targeted Details/Output priority path; coverage
X-ray removed the dead noncompact Details branches and the active goal gates
stayed green.

## 2026-05-02 - Terminal profile compatibility coverage guard

Pass: coverage-guided local/SSH compatibility pass for the risk that terminal
profile edge cases regress ASCII-border or color behavior outside the happy path.

Change: strengthened the terminal-profile unit coverage for default profiles,
missing locale variables, non-UTF8 locales, and explicit `CLICOLOR=0`, keeping
local color, SSH/dumb fallback, and ASCII-border decisions independent.

Checks: `cargo test terminal_profile_downgrades_dumb_or_non_utf8_terminals
-- --nocapture`; `just coverage-missing`; `git diff --check && just ux &&
just ci && just perf-live`.

Remaining risk: low for the targeted terminal-profile edges; coverage X-ray no
longer lists the default/no-locale branches, and the active goal gates stayed
green.

## 2026-05-02 - Autonomous loop hygiene guard

Pass: release/QA pass for the risk that unattended Codex loops start from a
dirty tree or drift away from AGENTS.md action-contract and coverage discipline.

Change: made `just codex-autoloop` refuse to run on uncommitted changes, then
strengthened the autopass prompt and architecture guard so future autonomous
passes keep visible-key tests, `just coverage-missing`, active goal gates, and
agent-loop notes in the loop.

Checks: `cargo test autonomous_loop_runs_the_active_goal_gates --test
architecture_guards -- --nocapture`; `PASSES=0 just codex-autoloop` on the
dirty tree to prove the refusal path; `git diff --check && just ux && just ci
&& just perf-live`.

Remaining risk: low; this was an autonomous-loop guardrail pass, not a product
surface change, and the active goal gates stayed green.

## 2026-05-02 - Layout preset first-screen guard

Pass: renderer/product pass for the risk that Compact, Standard, or Dense
layout presets drift into unusable first screens on local or SSH terminals.

Change: added a test-only layout setter and a renderer/X-ray test that renders
all layout presets at stacked and split widths, asserting brand, Fleet, Details,
selected pane identity, live work, and core footer actions stay visible.

Checks: `cargo test
usability_layout_presets_keep_wayfinding_and_actions_visible -- --nocapture`
and `just coverage-missing`.

Remaining risk: low for preset-level first-screen wayfinding; the active goal
gates stayed green.

## 2026-05-02 - Unlisted key modal state guard

Pass: coverage-guided action-contract pass for the risk that stray keys mutate
or dismiss visible modal states, while also proving the advertised top-level `K`
movement path.

Change: added a renderer/action test that proves `K` visibly moves selection up,
then opens More and Fleets, presses an unlisted arrow key, and asserts the
rendered grids and active modes stay unchanged.

Checks: `cargo test
usability_action_contract_unlisted_keys_do_not_steal_modal_state --
--nocapture` and `just coverage-missing`.

Remaining risk: low for the targeted inert-key modal routes; the active goal
gates stayed green.

## 2026-05-02 - Text input visibility and stray-key guard

Pass: coverage-guided action-contract pass for Search and Send text inputs,
where typed text must stay visible and unlisted navigation keys must not silently
change state.

Change: added a renderer/action test that opens Search and Send from their
visible shortcuts, verifies each footer contract, types text, presses an
unlisted arrow key, checks the typed value is still visible, exercises delete,
and cancels without leaving the user stranded.

Checks: `cargo test
usability_action_contract_search_and_send_inputs_keep_text_visible_after_stray_keys
-- --nocapture` and `just coverage-missing`.

Remaining risk: low for the targeted text input routes; the active goal gates
stayed green.

## 2026-05-02 - Save fleet visible input contract guard

Pass: coverage-guided product/QA pass for the Save Fleet flow, where the user
must see the fleet name they are typing and every visible text-mode key must do
what the footer promises.

Change: made fleet naming render the current `Name:` value inside the Send
panel, then added a renderer/action test that opens Save Fleet from More,
verifies the visible footer contract, exercises typing, delete, stray-key no-op,
Enter save, and confirms the saved fleet is immediately loadable.

Checks: `cargo test
usability_action_contract_save_fleet_visible_keys_persist_named_fleet --
--nocapture`.

Remaining risk: low for the targeted Save Fleet input route; the pass found and
fixed an invisible-input usability miss, and the active goal gates stayed green.

## 2026-05-02 - Start agent visible action contract guard

Pass: coverage-guided action-contract pass for the Start flow, where visible
keys must edit, choose presets, ignore stray keys, and launch without leaving
the user in a confusing state.

Change: added a renderer/action test that opens Start from More, verifies the
visible Start footer contract, exercises typing, delete, stray-key no-op, preset
cycling, and Enter launch through a fake tmux binary, then proves the new tmux
window appears after refresh.

Checks: `cargo test
usability_action_contract_start_agent_visible_keys_launch_window -- --nocapture`
and `just coverage-missing`.

Remaining risk: low for the targeted Start key route; live tmux behavior was
exercised through a fake tmux binary, and the active goal gates stayed green.

## 2026-05-02 - Review send and refresh action contract guard

Pass: coverage-guided action-contract pass for visible confirmation and More
actions that must execute exactly as advertised.

Change: kept `R refresh` visible in roomy More overlays, expanded the More
action-contract test to press refresh and the More toggle-close key, added a
Help-to-refresh contract test for the top-level `R` route, and added a
renderer/action test proving Review Send ignores stray keys but sends to both
tmux panes when the visible `Enter send` action is pressed.

Checks: focused `cargo test` loop for the More, Help refresh, and Review Send
action-contract tests, plus `just coverage-missing`.

Remaining risk: low for the targeted review confirmation and More refresh
routes; live tmux behavior was exercised through fake tmux binaries, and the
active goal gates stayed green.

## 2026-05-02 - More saved-fleet action contract guard

Pass: product/QA action-contract pass for the risk that More shows saved-fleet
state without keeping the obvious saved-fleet actions visible and executable.

Change: moved saved-fleet More rows above lower-priority send-list maintenance
actions, added action-contract tests that press `L` and `D` from More, and added
an Output-to-More Enter test proving `Enter show details` goes back to Details
instead of doing a surprising focus action.

Checks: focused `cargo test` loop for the new More action-contract tests, `just
coverage-missing`, and `git diff --check && just ux && just ci && just
perf-live`.

Remaining risk: low for the targeted More saved-fleet and Output-return actions;
live tmux behavior was not changed, and `just perf-live` stayed green.

## 2026-05-02 - Footer contract X-ray guard

Pass: product/QA renderer pass for the risk that compact footers and headers
promise confusing or inert actions in Send, saved Fleets, narrowed search, and
Browse feedback states.

Change: added app-level guards for repeat-last Send input, fleet naming, fleet
picker, hidden send-list targets, no-match recovery, and Browse-only feedback
keymaps; added renderer/X-ray and action-contract tests that inspect actual
grids for narrow Send repeat affordances, Browse feedback, and saved Fleets
picker keys; fixed wide modal footers so status chatter cannot replace the
visible picker/action keymap after deleting a saved fleet.

Checks: focused `cargo test` loop for the new footer and Fleet picker tests, `just
coverage-missing`, and `git diff --check && just ux && just ci && just
perf-live`.

Remaining risk: low for these footer contract states; live tmux behavior was not
changed, and `just perf-live` still passed after the renderer and key-router
additions.

## 2026-05-02 - Command Center action sentence guard

Pass: renderer/presentation X-ray pass for Command Center next-action copy,
especially empty, filtered, start-agent, and send-list states.

Change: changed the targeted Command Center action from the awkward
`send send list` phrasing to a sentence-shaped `send to the send list`, added
app-level guards for empty and filtered recovery copy plus start-folder fallback
copy, and added a renderer test that inspects the actual grid to ensure the
visible Command Center action reads correctly.

Checks: `cargo test
command_center_action_lines_stay_plain_for_empty_filtered_and_targeted_states --
--nocapture`, `cargo test
usability_command_center_target_action_reads_like_a_sentence -- --nocapture`,
`just coverage-missing`, and `git diff --check && just ux && just ci && just
perf-live`.

Remaining risk: low for targeted Command Center action copy; the renderer guard
now fails if the old duplicated wording reappears on screen.

## 2026-05-02 - Plain edge-copy helper guard

Pass: coverage-guided copy-helper audit for rare but user-visible empty target,
empty waiting, blank action error, launch naming, startup warning, and lane
priority edges.

Change: added explicit tests that keep fallback action messages plain, keep blank
errors from rendering as empty status text, keep punctuation-only launch commands
named `agent`, and preserve lane ordering for stuck, idle, and empty lanes.

Checks: `cargo test launch_window_names_are_short_human_and_safe -- --nocapture`,
`cargo test user_visible_edge_copy_helpers_stay_plain -- --nocapture`, and `just
coverage-missing`, plus `git diff --check && just ux && just ci && just
perf-live`.

Remaining risk: low for these helper edges; coverage X-ray removed the targeted
plain-copy helper lines from `src/app.rs`.

## 2026-05-02 - Stale target recovery route guard

Pass: coverage-guided stale-state and tmux action recovery audit for jump,
Browse, dirty capture, control disconnect, and mixed send-list Smart Actions.

Change: added route-level jump recovery tests for same-server client switch,
same-server focus fallback, same-server direct focus, and cross-server focus when
the target pane disappears; added a Browse jump guard that chooses a visible pane
inside the selected window when the active pane is filtered out; strengthened
dirty-capture, control-exit, empty-Browse, and mixed send-list disappearance
tests so stale state stays recoverable and plain.

Checks: focused `cargo test` loop for the changed tests, `just
coverage-missing`, and `git diff --check && just ux && just ci && just
perf-live`.

Remaining risk: low for the targeted stale-target routes; coverage X-ray removed
the previously exposed dirty-capture, control-exit, Browse fallback, Smart
Action skipped-while-disappeared, and most jump recovery branches.

## 2026-05-02 - Notification settings failure guard

Pass: coverage-guided notification settings audit for local/SSH-safe alert
toggles when config persistence fails.

Change: strengthened the notification settings save-failure test so bell
notification toggles keep the persistence error visible and do not overwrite it
with a false success message, matching the existing desktop-alert failure guard.

Checks: `cargo test usability_notification_setting_save_failures_stay_visible -- --nocapture`,
`just coverage-missing`, and `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low for notification setting persistence feedback; the coverage
X-ray no longer lists the bell-toggle failure branch targeted by this pass, and
the active goal gates passed.

## 2026-05-02 - Alert persistence failure guard

Pass: coverage-guided alert acknowledgement audit for mute, unmute, mute-all,
and clear-all feedback when state persistence fails.

Change: strengthened the existing persistence-failure test so selected unmute
and mute-all actions both keep the human action result visible while appending
the state-save failure, matching the same goodwill rule already used for single
mute and clear-all.

Checks: `cargo test usability_acknowledgement_save_failures_stay_visible_after_mute_actions -- --nocapture`,
`just coverage-missing`, and `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low for acknowledgement save-failure copy; the coverage X-ray
no longer lists the selected-unmute or mute-all failure branches targeted by
this pass, and the active goal gates passed.

## 2026-05-02 - Smart Action target-loss guard

Pass: coverage-guided Smart Action audit for the advertised `A` path when
send-list, lane, or selected-pane targets are not ready or disappear during the
action.

Change: added app-state and fake-tmux guards proving send-list Smart Action says
when none of its panes are ready, lane fanout reports when all ready panes
disappear, and selected-pane Smart Action reports when a ready pane vanishes
instead of implying Enter was sent.

Checks: `cargo test smart_action -- --nocapture`, `just coverage-missing`, and
`git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for Smart Action no-ready and target-loss feedback; the
coverage X-ray no longer lists the Smart Action branches targeted by this pass,
and the active goal gates passed.

## 2026-05-02 - Saved fleet picker edge guard

Pass: coverage-guided saved fleet picker audit for inert actions, row movement,
delete recovery, active fleet preservation, and persistence-failure visibility.

Change: added app-state guards proving inactive fleet picker methods do not
mutate hidden state, previous movement from the middle moves exactly one row,
picker deletion without an active fleet stays local, deleting a different saved
fleet preserves the active fleet and surfaces save failures, and deleting the
loaded fleet keeps selection on a remaining fleet instead of leaving stale
state.

Checks: `cargo test fleet_picker -- --nocapture`, `cargo test
delete_selected_fleet_keeps_selection_on_remaining_fleet -- --nocapture`,
`just coverage-missing`, and `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low for saved fleet picker and selected fleet deletion edges;
the coverage X-ray no longer lists the saved-fleet picker branches targeted by
this pass, and the active goal gates passed.

## 2026-05-02 - Disappearing pane and inert action guard

Pass: coverage-guided app-state audit for stale panes, empty navigator state,
and inactive input/menu actions that must never surprise the user or mutate
hidden state.

Change: added guards proving stale target sends count disappeared panes without
calling tmux, non-disappearance tmux errors are not swallowed, multi-pane
disappearance prunes selection/runtime with plain recovery copy, empty navigator
jump stays inside muxboard with a clear message, and inactive text/menu actions
leave hidden buffers and status untouched.

Checks: `cargo test inactive_text_and_menu_actions_do_not_mutate_hidden_state -- --nocapture`,
`cargo test disappeared_pane_recovery_paths_are_plain_and_non_destructive -- --nocapture`,
`cargo test empty_navigator_actions_recover_without_guesswork -- --nocapture`,
`just coverage-missing`, and `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low for the covered stale-pane and inert-action paths; the
active goal gates passed.

## 2026-05-02 - Notification boundary guard

Pass: coverage-guided reliability audit for local versus SSH notification
selection so muxboard stays useful on laptops and servers without hidden macOS
assumptions.

Change: split notification environment detection behind a deterministic private
helper and added guards proving SSH markers override desktop hints, local desktop
mode only appears when a real backend is detected, and terminal-only mode remains
the fallback when display hints have no notifier.

Checks: `cargo test notifications::tests -- --nocapture`,
`just coverage-missing`, and `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low for SSH/local notification selection. The only uncovered
notification lines are the live environment entrypoint and process-spawning
boundary, which are intentionally not exercised by unit tests. The active goal
gates passed.

## 2026-05-02 - Target scope durability guard

Pass: coverage-guided usability audit for send-list, lane fanout, saved fleet,
and recent command paths that define what muxboard will act on.

Change: added app-state guards that keep empty target scope copy plain, prove
marked panes describe themselves as a send list without requiring a saved fleet,
verify empty command previews stay silent instead of noisy, ensure existing saved
fleets replace in place without duplicates, and keep recent command history
blank-safe, newest-first, unique, and capped.

Checks: `cargo test target_scope_copy_stays_plain_for_empty_and_marked_states -- --nocapture`,
`cargo test remembering_commands_ignores_blank_input_and_caps_history -- --nocapture`,
`cargo test saving_existing_named_fleet_replaces_it_in_place -- --nocapture`, and
`just coverage-missing`, plus `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low for `src/app/targets.rs`; the coverage X-ray no longer lists
that target/send-list module, and the active goal gates passed.

## 2026-05-02 - Alert acknowledgement durability guard

Pass: coverage-guided usability audit for notification and acknowledgement paths
that can otherwise create alert spam or unbounded status noise.

Change: strengthened acknowledgement reconciliation to prove still-actionable
muted panes stay muted, resolved panes clear their acknowledgement, alert history
stays capped with newest items, and zero debounce truly disables alert
suppression. Also covered the full plain-language attention label set so quiet,
working, complete, and checking states cannot regress to jargon.

Checks: `cargo test attention_labels_are_plain_for_all_states -- --nocapture`,
`cargo test acknowledgement_clears_when_status_changes -- --nocapture`,
`cargo test recent_alerts_keep_newest_items_without_unbounded_growth -- --nocapture`,
`cargo test debounce_suppresses_repeat_alerts_for_same_pane -- --nocapture`,
`just coverage-missing`, and `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low for the attention helper surface; the active goal gates
passed.

## 2026-05-02 - Control stream read-error guard

Pass: coverage-guided reliability audit for tmux control-mode monitor branches
that affect live updates and stale-state recovery.

Change: added a fake-control-client regression proving malformed stdout/stderr
bytes become visible read-error events and the monitor still reports process
exit instead of hanging. Also guarded extended-output summaries, which feed live
Fleet/Details updates.

Checks: `cargo test event_helpers_expose_output_and_loggability -- --nocapture`,
`cargo test control_start_reports_malformed_streams_without_hanging -- --nocapture`,
`just coverage-missing`, and `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low for the covered control-stream read-error and extended-output
summary paths; the active goal gates passed.

## 2026-05-02 - Unreadable tmux recovery guard

Pass: local/SSH recovery audit for the empty first-run surface when tmux exists
but panes cannot be read.

Change: added app and renderer/X-ray guards proving unreadable tmux panes become
plain recovery copy, "Cannot read tmux panes" plus "Check socket/session, then R
refresh", without leaking raw socket/session details or low-level permission
text into the first screen.

Checks: `cargo test empty_first_run_states_show_the_next_recovery_step -- --nocapture`,
`cargo test usability_empty_tmux_state_is_actionable_not_jargon -- --nocapture`,
`just coverage-missing`, and `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low; this branch now has app copy, renderer/X-ray, coverage,
UX, CI, and live perf evidence.

## 2026-05-02 - Command Center stuck-agent guard

Pass: audited `just coverage-missing` for product-relevant blind spots after the
short-terminal guard. The Command Center had strong waiting/error coverage, but
the visible `stuck` count branch was not directly exercised, even though stale
agents are a core fleet-triage state.

Change: added an app-state contract and renderer/X-ray regression proving a
stale Claude pane appears as `1 stuck`, recommends `Enter output`, and does not
count as actively working. Also strengthened the provider fallback case where
Claude-style `Approve Bash` prompts should become a specific approval blocker,
not a generic waiting state. While running the goal gates, fixed a tmux wrapper
test flake where a reused temp log path could append stale calls from a previous
process-id collision.

Checks: `cargo test control_lines_count_stale_agents_as_stuck_needs_you -- --nocapture`;
`cargo test usability_command_center_surfaces_stuck_agents_as_needs_you --
--nocapture`; `cargo test waiting_and_tool_summaries_cover_source_variants --
--nocapture`; `cargo test pane_action_wrappers_pass_socket_and_exact_targets_to_tmux --
--nocapture`; `just coverage-missing`.

Remaining risk: low; this guards the visible Command Center path, not every
possible provider-specific route into `Stuck`.

## 2026-05-02 - Residual parser noise coverage guard

Pass: coverage-guided cleanup on the remaining core parser noise branches that
can affect Fleet/Details summaries.

Change: added guards for empty `Tool:` provider lines and two-character inline
echo fragments such as `x/`, keeping both out of user-facing summaries.

Checks: `cargo test waiting_and_tool_summaries_cover_source_variants -- --nocapture`,
`cargo test helper_priorities_and_formatters_cover_edge_values -- --nocapture`,
and `just coverage-missing`.

Remaining risk: low; this removed the remaining core provider/report parser
entries from the coverage-missing list.

## 2026-05-02 - Empty summary action recovery copy

Pass: action-contract audit for the `S summarize panes` path when no panes are
available.

Change: replaced the implementation-flavored "summary polling" empty-state copy
with plain recovery language, and added app plus TUI action-contract tests so
pressing `S` from More with no panes stays recoverable and obvious.

Checks: `cargo test summary_request_without_targets_uses_plain_recovery_copy -- --nocapture`,
`cargo test usability_action_contract_more_menu_actions_execute_visible_promises -- --nocapture`,
and `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low; the empty target state now has app-level and advertised-key
coverage, plus the active goal gates passed.

## 2026-05-02 - Provider noise parser edge guards

Pass: coverage-guided provider/report parser audit for small branches that can
still leak noise into Fleet or Details if provider output shifts.

Change: added guards for Claude model banners using Sonnet/Opus wording and
fixed malformed `Tool:` lines with no tool name, so `Tool: Input: cargo test`
does not become a fake visible action like `wait for Input: cargo test`.

Checks: `cargo test helper_priorities_and_formatters_cover_edge_values -- --nocapture`,
`cargo test waiting_and_tool_summaries_cover_source_variants -- --nocapture`,
`cargo test malformed_tool_input_lines_do_not_become_visible_tool_names -- --nocapture`,
and `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low after focused parser, app presentation, UX, CI, and live
perf gates.

## 2026-05-02 - Nonstandard agent wrapper inference guard

Pass: audited workload inference for the user-facing risk that agents launched
through a custom wrapper could be treated as ordinary jobs even after emitting an
explicit structured status report.

Change: classified any pane with a valid structured agent report as an agent,
regardless of the process name, and added core, board-row, and renderer/X-ray
tests proving a nonstandard `runner` command appears as an agent instead of
`Job`.

Checks: `cargo test workload_inference_covers_generic_shell_job_and_agent_fallbacks -- --nocapture`,
`cargo test board_rows_treat_nonstandard_structured_status_wrappers_as_agents -- --nocapture`,
`cargo test usability_nonstandard_structured_agent_wrappers_render_as_agents_not_jobs -- --nocapture`,
`git diff --check && just ux && just ci && just perf-live`, and `just coverage-missing`.

Remaining risk: low; this does not guess arbitrary jobs are agents, it only
trusts explicit report protocol already parsed by muxboard.

## 2026-05-02 - User intent report synthesis guard

Pass: continued the coverage-guided audit on core provider/report synthesis. The
user-intent path is one of muxboard's highest-leverage intelligence layers: it
turns noisy agent panes into "what is this pane trying to do?" without binding
that value to any UI generation.

Change: strengthened report synthesis so tests prove real `User:` intent becomes
waiting context, while status-report templates stay invisible and cannot leak
`STATUS=<status>` / `NEXT=<next>` scaffolding back into Fleet or Details.

Checks: `cargo test summary_selection_filters_noise_and_prioritizes_specific_signals -- --nocapture`
and `cargo test synthesis_uses_user_request_as_waiting_context_without_template_leakage -- --nocapture`;
then `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low; this protects the core synthesis layer, including the noisy
template fallback path, and the active goal gates passed.

## 2026-05-02 - Short terminal board capacity guard

Pass: used `just coverage-missing` after the Fleet location fix to check for
newly exposed renderer blind spots. The zero-row Fleet capacity branch was still
uncovered, so added a renderer-level regression proving very short terminals
stay recoverable and still render the core Fleet/Details wayfinding chrome
without panicking.

Checks: `cargo test zero_row_board_capacity_keeps_layout_recoverable --
--nocapture`; `just coverage-missing`.

Remaining risk: low for this branch; this is a narrow guard for severe height
pressure, not a substitute for full responsive golden review.

## 2026-05-02 - Fleet location readability guard

Pass: scripted live tmux dogfood exposed a first-screen readability miss where a
plain `session/window` label like `muxdog/claude` could truncate to
`muxdog/cl...` in the Fleet even on a roomy split screen. Made the Fleet `Where`
column adaptive: it keeps the old compact budget for normal labels, grows only
when visible rows need more room, and caps long labels before they steal useful
Latest space.

Checks: live tmux capture at 120x36 showed `muxdog/claude` in full and no
`muxdog/cl...`; `cargo test board_ -- --nocapture`; `cargo test
split_board_keeps_plain_session_window_locations_readable -- --nocapture`;
`cargo test exact_grid_matches -- --nocapture`; `cargo test --test live_e2e
fleet_keeps_plain_session_window_locations_readable_live -- --ignored
--nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for this escaped visual class. The exact golden grids stayed
unchanged for ordinary `demo/agents` layouts, so the fix avoids adding wasted
space to the common first-run screen.

## 2026-05-02 - Full V1 release check on current HEAD

Pass: ran the single release-confidence gate on current HEAD after the alert
policy guard so CI, UX, live tmux, coverage, packaging, and dogfood evidence all
refer to the same tree.

Checks: `just release-check` passed. It included `ci-full`, `ux`,
`coverage-full-gate`, `package-check`, and `dogfood`; coverage ended at 96.89%
lines, 96.44% regions, and 95.01% functions; the package verifier built
`muxboard 1.0.0` from a 66-file crate package.

Remaining risk: no automated gate can prove the subjective first-run feel. Do
one final human `cargo run` dogfood before tagging or publishing.

## 2026-05-02 - Alert policy coverage guard

Pass: used the coverage map to audit notification behavior for the risk that
the visible `wait+err` alert setting was only copy-tested and not behavior-
tested. Added an app-state regression proving that `wait+err` raises alerts for
both waiting prompts and errors while ignoring non-actionable done output.

Checks: `cargo test alert_policy_ -- --nocapture`; `just coverage-missing`
confirmed `AlertPolicy::ErrorAndWaiting.matches` is no longer uncovered;
`git diff --check`; `just ux && just ci && just perf-live`;
`just coverage-full-gate` passed on this commit with 40 live tmux tests and
96.89% line, 96.44% region, and 95.01% function coverage.

Remaining risk: low for notification policy behavior; live notification
persistence and SSH-safe copy are already covered by the live dogfood suite.

## 2026-05-02 - Full live coverage release gate

Pass: audited release confidence for the risk that normal coverage hides live
tmux regressions in command dispatch, focus, pane selection, and stale-state
recovery. Ran the full coverage gate with live ignored e2e tests enabled.

Checks: `just coverage` reported 96.45% line, 95.99% region, and 94.03%
function coverage for the normal suite. `just coverage-full-gate` passed with
40 live tmux tests and final coverage of 96.89% lines, 96.43% regions, and
95.01% functions against floors of 95%, 94%, and 90%.

Remaining risk: this confirms the current release confidence floor, but it is
still not a substitute for one final human dogfood pass before tagging.

## 2026-05-02 - Keybinding validation guard

Pass: audited user-configurable keybinding validation for the risk that broken
rebinding files fail with vague or untested recovery copy. Added config tests
for empty binding lists and padded tokens so invalid customization fails loudly
before it can make advertised shortcuts cosmetic or inert.

Checks: `cargo test load_ui_settings_rejects_ -- --nocapture`;
`just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: live dogfood was not rerun after this config-only guard pass;
the previous dogfood run passed on the same code paths. `just package-check`
passed after this pass.

## 2026-05-02 - Jump action contract cleanup

Pass: audited the `G show` path for the risk that dead focus/jump branches and
untested tmux calls let a future change make jump surprising or destructive.
Removed the unreachable non-jump branch from the jump implementation and added
fake-tmux action guards proving jump uses `switch-client` plus pane focus, falls
back cleanly when client tty lookup fails, and never issues kill or detach
commands.

Checks: `cargo test jump_to_selected_pane_ -- --nocapture`;
`just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: live dogfood and `just package-check` passed after this pass.
Use `just release-check` before shipping if one single end-to-end gate is needed.

## 2026-05-02 - Send target disappearance guard

Pass: audited the Send journey for the risk that a target vanishes after the
user opens Send but before submit, leaving the app with an empty target set and
ambiguous recovery copy. Added app-state coverage for the disappearing-target
submit path, the empty preview state, and hidden-target preview slot selection.

Checks: `cargo test command_input_recovers_when_target_disappears_before_submit -- --nocapture`;
`cargo test send_preview_edges_keep_hidden_and_empty_states_obvious -- --nocapture`;
`just coverage-missing`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: live dogfood and `just package-check` passed after this pass.
Use `just release-check` before shipping if one single end-to-end gate is needed.

## 2026-05-01 - Public demo language guard

Pass: audited the README hero SVG for the risk that first-impression marketing
copy lags behind the current calm V1 UI. Replaced old Board/Session tree/Live
tail/protocol text with Fleet, Details, Browse, Command Center, Output, and the
same visible footer grammar used in the app.

Checks: `cargo test public_ --test architecture_guards -- --nocapture`;
`cargo fmt`; `just guards`; `just ux`; `just lint`; `git diff --check`.

Remaining risk: visual SVG layout was reviewed as text only; browser rendering
was not manually inspected in this pass.

## 2026-04-30 - Show-in-tmux action wording

Pass: audited the inactive-pane Details action for the risk that one-off words
force users to infer whether `open` differs from the footer's `G show` action.
Changed idle/unobserved pane action copy to `show in tmux` and normalized report
fallbacks so provider text that says `open in tmux` surfaces with the same verb.

Checks: `cargo test inactive_panes -- --nocapture`; `just ux`; `cargo test`;
`just lint`; `git diff --check`.

Remaining risk: live dogfood was not rerun for this copy-only follow-up; no tmux
dispatch behavior changed.

## 2026-04-30 - Checking copy for unobserved panes

Pass: audited inactive panes with no runtime output for the risk that muxboard
leaks `unknown` or `unk` jargon instead of telling users what is happening.
Changed user-facing copy to `Checking`, made synthesized reports use
`checking`, and prevented the Fleet from rendering redundant `codex: codex`
latest text for unobserved agent panes.

Checks: `cargo test checking -- --nocapture`; `just ux`; `cargo test`;
`just lint`; `git diff --check`; `just dogfood`.

Remaining risk: full coverage and package verification were not rerun after this
copy pass; use `just release-check` before shipping.

## 2026-04-30 - Command Center short-screen action budget

Pass: audited the Command Center overlay for the risk that short terminals spend scarce rows on passive status while hiding the agent launch path. Changed compressed overview prioritization so `Action`, `Needs you`, `Send`, and `Start` survive before `Working`, and added a renderer test for an 80x14 Command Center.

Checks: `cargo test short_command_center_keeps_action_send_and_start_visible -- --nocapture`; `cargo test overview -- --nocapture`; `just tui-golden`; `just ux`; `just test`; `just fmt-check`; `git diff --check`; `just test-live`.

Remaining risk: full release coverage and packaging were not rerun in this pass; use `just release-check` before shipping.

## 2026-04-30 - Compact Command Center lane signal

Pass: audited the short Command Center overlay for the risk that duplicate section chrome hides actionable lane context. In compact overflow, the attention summary now acts as the queue label, so the selected attention row and, when room allows, the first lane fit without adding another `needs you` row. Strengthened the 80x14 renderer test to require the lane signal immediately after the attention row.

Checks: `cargo test usability_short_command_center_keeps_action_send_and_start_visible -- --nocapture`; `cargo test overview -- --nocapture`; `just tui-golden`; `just ux`; `just test`; `just fmt-check`; `git diff --check`; `just lint`; `just test-live`.

Remaining risk: release packaging was not rerun in this pass; use `just release-check` before shipping.

## 2026-05-02 - Live action-contract polish

Pass: live tmux dogfood of the `Enter`, `Esc`, `. more`, `/ filter`, and
`: send` journeys found two action-contract issues: More recommended `C mute
alert` while hiding that row in roomy choice panes, and a narrowed footer could
cut `. more` into `. mor...`. The More pane now keeps recommended choice and
alert actions visible, show-all recovery rows are indented under `view`, and
footer keymaps drop whole trailing actions instead of rendering partial keys.

Checks: real tmux capture against `muxboard-actiondogfood` for More, filtered
Fleet, Send preview, and Send cancel; `cargo test
footer_drops_whole_actions_instead_of_cutting_the_last_key_hint -- --nocapture`;
`cargo test filtered_details_footer_renders_whole_key_hints_at_cell_level --
--nocapture`; `cargo test
usability_action_contract_more_menu_actions_execute_visible_promises --
--nocapture`; `cargo test
usability_more_menu_is_contextual_instead_of_an_action_dump -- --nocapture`;
`just ux-live-actions`.

Remaining risk: `just ux`, `just ci`, and `just perf-live` passed after this
pass. Use `just release-check` before shipping if one single end-to-end gate is
needed.

## 2026-05-03 - Live Search waits prove applied state

Pass: converted Search live E2E waits from generic header/footer text to named
visible-state waits. Search input now waits for the actual edit surface and
applied search now waits for the query badge, recovery affordance, selected
result row, and absence of transient overlays.

Checks: architecture guard `live_search_tests_wait_for_visible_search_state`;
focused live Search, send-list, saved-fleet, review, output/jump, and same-server
flows; broad gate `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low after the focused and broad gates. Future live Search tests
should prove the visible search state they depend on instead of accepting stale
`search:` or `type to filter` text.

## 2026-05-03 - Live Save Fleet waits prove input surface

Pass: tightened live saved-fleet journeys so `G save fleet` waits for the actual
name-input surface, not only the generic explanatory sentence. The wait now
requires the save prompt, `Enter save`, `Esc cancel`, `type name`, and absence of
stale More/Fleets/Send overlays before tests type a fleet name.

Checks: architecture guard `live_saved_fleet_tests_wait_for_picker_and_active_state`;
focused live saved-fleet tests; broad gate `git diff --check && just ux &&
just ci && just perf-live`.

Remaining risk: low after the focused and broad gates. Future live saved-fleet
tests should wait for the concrete input, picker, and active-fleet states they
act on.

## 2026-05-03 - Live Review waits prove confirmation surface

Pass: hardened the Review send live wait so it no longer accepts the generic
`Review send` heading by itself. The helper now waits for the requested target
copy, `Enter send`, `Esc cancel`, and absence of stale Send/More/Fleets overlays
before tests confirm or cancel a multi-pane send.

Checks: architecture guard `live_send_tests_wait_for_visible_send_surfaces_before_typing`;
focused live review tests; broad gate `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low after focused review coverage and the broad gate. Future
review tests should keep confirmation waits tied to the real decision surface,
not heading text that could survive a stale render.

## 2026-05-03 - Live smart action waits prove selected action state

Pass: hardened live smart-action tests so they no longer accept a generic
`Action: continue` string before pressing the primary action. The helper now
requires the action copy and the intended selected Fleet row together, while
excluding transient overlay surfaces.

Checks: architecture guard `live_smart_action_tests_wait_for_selected_action_state`;
focused live smart-action and lane-action tests; broad gate `git diff --check &&
just ux && just ci && just perf-live`.

Remaining risk: low after focused live coverage and the broad gate. Future
primary-action live tests should prove the selected row and promised action are
aligned before pressing the key.

## 2026-05-03 - Live Output waits prove the complete surface

Pass: hardened the live Output helper so it no longer waits for generic
`Esc back` text before asserting the surface. The helper now waits until Output,
the back affordance, and the absence of stale top-level, Send, Review, Command
Center, and Browse surfaces are all true at once.

Checks: architecture guard
`live_output_tests_wait_for_the_output_surface_not_the_generic_heading`; focused
live Output and jump/focus flows; broad gate `git diff --check && just ux &&
just ci && just perf-live`.

Remaining risk: low after focused live coverage and the broad gate. Future
Output live tests should keep waits tied to the whole surface, not a shared
footer affordance.

## 2026-05-03 - Live selected-row setup waits

Pass: replaced generic live waits for `ops/prompt` and `ops/split` text with a
selected-row helper before tests open Send, More, Zoom, or refresh-recovery
paths. The helper proves the intended Fleet row is selected and no transient
overlay is stealing the surface before the test acts.

Checks: architecture guard `live_setup_tests_wait_for_selected_rows_before_actions`;
focused live single-send, summary, zoom, and refresh tests; broad gate
`git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low after focused live coverage and the broad gate. Future live
setup waits should prove the selected row they depend on, not merely that some
location text appeared somewhere on screen.

## 2026-05-03 - Live More action feedback waits

Pass: replaced generic post-action text waits for summary, zoom, and lane-send
More actions with a Fleet feedback helper. The helper proves the feedback is
visible, the intended row is still selected, and no secondary surface is still
stealing the next keypress.

Checks: architecture guard `live_more_action_feedback_waits_are_surface_specific`;
focused live summary, zoom, and lane-send tests; broad gate
`git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low after focused live coverage and the broad gate. Future
post-action waits should prove the returned Fleet state before another key is
sent, not merely that the feedback text appeared somewhere in the pane.

## 2026-05-03 - Live cross-session jump selected row

Pass: replaced the same-server cross-session jump setup wait for `review/prompt`
with the selected-row helper. The test now proves muxboard has selected the
intended cross-session pane before pressing Jump.

Checks: architecture guard `live_setup_tests_wait_for_selected_rows_before_actions`;
focused live same-server cross-session jump test; broad gate
`git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low after focused live coverage and the broad gate. Future
cross-session action tests should prove the visible selection, not just that a
target location string exists somewhere on screen.

## 2026-05-03 - Live selected-row waits prove board chrome

Pass: tightened the selected-row live helper so it requires both `Fleet` and
`Details` chrome along with the selected row. A row marker alone is not enough
proof that muxboard has returned to the main board before the next keypress.

Checks: architecture guard `live_setup_tests_wait_for_selected_rows_before_actions`;
focused live send, summary, zoom, refresh, and same-server jump tests; broad
gate `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low after focused live coverage and the broad gate. Future
selected-row waits should continue to prove the visible application surface, not
just a matching line.

## 2026-05-03 - Live first-board waits use board surface state

Pass: replaced first-screen, narrow-terminal, and dumb-terminal live waits that
looked for broad words like `Waiting` or `Action:` with a main-board helper. The
helper proves Fleet and Details chrome, the selected row, required UX copy, and
no secondary surface before the test asserts the first visible board.

Checks: architecture guard `live_first_board_tests_wait_for_main_board_surface`;
focused live first-screen, narrow-terminal, and dumb-terminal tests; broad gate
`git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low after focused live coverage and the broad gate. Future
first-board tests should wait for the actual board surface, not a generic status
word.

## 2026-05-03 - Live first-run recovery waits prove next steps

Pass: replaced first-run no-server and missing-session waits with a recovery
surface helper. The helper proves the error headline, the exact next step, help
availability, and absence of low-level tmux failure jargon before the test
continues.

Checks: architecture guard `live_first_run_recovery_tests_wait_for_recovery_surface`;
focused live no-server and missing-session first-run tests; broad gate
`git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low after focused live coverage and the broad gate. Future
first-run recovery tests should wait for the whole recovery promise, not just an
error headline.

## 2026-05-03 - Live provider state waits prove selected board

Pass: replaced provider row/state live waits for location readability, shell
prompt filtering, active thinking, and returned shell prompts with main-board
surface waits. These tests now prove the selected row and required provider
copy before inspecting row content.

Checks: architecture guard `live_provider_state_tests_wait_for_main_board_surface`;
focused live provider row/state tests; broad gate
`git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low after focused live coverage and the broad gate. Future
provider state tests should wait for the selected board state they inspect, not
for one generic piece of row or detail text.

## 2026-05-03 - Live resize and navigation waits prove board state

Pass: replaced resize, small-board scroll, and large-fleet navigation waits
that looked for raw counts or row fragments with main-board surface waits. The
large-fleet path keeps its responsive timeout while proving the selected row and
visible range.

Checks: architecture guard `live_resize_and_navigation_tests_wait_for_main_board_surface`;
focused live resize, small-board, and large-fleet navigation tests; broad gate
`git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low after focused live coverage and the broad gate. Future
navigation tests should wait for the selected board state and preserve latency
budgets instead of waiting on isolated text fragments.

## 2026-05-03 - Live launch waits prove start and recovery surfaces

Pass: replaced launch-agent waits for raw start, success, and no-server text
with start-surface, launch-feedback, and recovery-surface helpers. The launch
tests now prove the visible command entry surface before typing and the visible
post-action surface before asserting muxboard survived.

Checks: architecture guard `live_launch_tests_wait_for_start_and_recovery_surfaces`;
focused live launch success and launch recovery tests; broad gate
`git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low after focused live coverage and the broad gate. Future
launch tests should wait for the surface that owns input, not a loose status
line.

## 2026-05-03 - Live acknowledgement and notification waits prove board state

Pass: replaced acknowledgement and SSH notification waits for raw attention
counts and status text with selected board-state waits and post-action feedback
waits. These tests now prove the selected waiting pane and visible feedback
before muting, unmuting, or toggling alert settings.

Checks: architecture guard `live_acknowledgement_and_notification_tests_wait_for_board_state`;
focused live acknowledgement, notification persistence, and restart tests; broad
gate `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low after focused live coverage and the broad gate. Future alert
tests should wait for selected board state and action feedback, not generic
attention words.

## 2026-05-03 - Live review dispatch waits prove result surfaces

Pass: replaced review-send disappearing-target waits for raw result text with a
result-surface helper. The helper proves the expected result copy and forbids
stale review, command, and vanished-target context before assertions continue.

Checks: architecture guard `live_review_dispatch_result_tests_wait_for_result_surface`;
focused live partial-disappear and all-disappear review-send tests; broad gate
`git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low after focused live coverage and the broad gate. Future
review-dispatch tests should prove both required result copy and forbidden stale
context.

## 2026-05-02 - Stale fleet recovery in fast UX gate

Pass: audited the stale saved-fleet recovery path for the risk that a visible
dead target list could regress outside the fast UX loop. Promoted the app and
renderer guards that prove stale fleets stay visible, never promise a dead send,
and keep `. then L choose fleet` / `D delete stale` recovery paths obvious into
the `usability_` suite run by `just ux`.

Checks: `cargo test usability_command_panel_keeps_stale_loaded_fleet_visible --
--nocapture`; `cargo test usability_stale_active_fleet -- --nocapture`.

Remaining risk: `just ux`, `just ci`, and `just perf-live` passed after this
promotion-only follow-up.

## 2026-05-02 - Faster local UX loop

Pass: audited the fast product loop itself after targeted filtered tests spent
time walking unrelated test binaries. Updated `just ux`, `just ux-actions`,
`just tui-golden`, and `just perf-smoke` to run library-only filtered tests for
library-only guards, and aligned `just tui-golden-bless` with the same renderer
path. Integration guards, full `just ci`, and live tmux coverage stay unchanged.

Checks: `command time -p just ux` passed with library-only filtered recipes
(`real 44.02` on this machine after recompiling recipe guards);
`git diff --check`; `just ux`; `just ci`; `just perf-live`.

Remaining risk: recipe-only optimization; the full CI gate still runs all test
binaries.

## 2026-05-02 - Live summary action contract

Pass: audited the `S summarize panes` action because it is a visible command
center promise that sends a real prompt into tmux panes. Added a live tmux E2E
test proving More advertises `S summarize panes`, pressing `S` keeps muxboard
alive, updates status copy, and sends the one-line summary prompt into the
selected live pane. Wired the test into `just ux-live-actions` and the recipe
guard so the live action set cannot silently drop it.

Checks: `cargo test --test live_e2e
summary_action_sends_one_line_prompt_to_live_tmux -- --ignored --nocapture`;
`just ux-live-actions`; `git diff --check`; `just ux`; `just ci`;
`just perf-live`.

Remaining risk: low for summary action dispatch; broader active-goal coverage
still depends on keeping every visible command promise tied to an action guard.

## 2026-05-02 - Live zoom action contract

Pass: audited the `Z zoom pane` action because it is a visible More promise that
mutates real tmux pane layout. Added a live tmux E2E test with a split target
window proving More advertises `Z zoom pane`, pressing `Z` toggles the target
window's tmux zoom flag, leaves muxboard running, and surfaces visible feedback.
Wired the test into `just ux-live-actions` and the recipe guard so the tmux
layout action cannot silently regress outside fake-tmux coverage.

Checks: `cargo test --test live_e2e
zoom_action_toggles_live_tmux_pane_without_leaving_muxboard -- --ignored
--nocapture`; `just ux-live-actions`; `git diff --check`; `just ux`;
`just ci`; `just perf-live`.

Remaining risk: low for live zoom dispatch; broader active-goal coverage still
depends on continuing to bind every visible tmux mutation to a live action
contract.

## 2026-05-02 - Live Browse drill-down contract

Pass: audited the Browse journey because V1 needs an obvious drill-down/drill-up
path across real tmux windows, not just a static dashboard. Added a live tmux
E2E test with two target windows proving More advertises `browse windows`, the
Browse surface advertises `Enter window`, `J` selects another live window,
`Enter` scopes the fleet to that window, `Backspace` restores the full fleet,
and muxboard remains running.

Checks: `cargo test --test live_e2e
browse_enter_scopes_to_live_window_and_backspace_recovers -- --ignored
--nocapture`; `just ux-live-actions`; `git diff --check`; `just ux`;
`just ci`; `just perf-live`.

Remaining risk: low for live Browse scope and recovery; broader active-goal
coverage still depends on keeping drill-down/drill-up journeys live-tested when
they touch real tmux state.

## 2026-05-02 - Live Command Center primary action contract

Pass: audited the Command Center primary action because it is the conductor-like
surface users should trust when an agent needs attention. Added a live tmux E2E
test with a waiting target pane proving More opens Command Center, Command
Center visibly recommends the selected waiting pane action, pressing the smart
action key sends Enter to the live waiting pane, and muxboard remains running.
Wired the test into `just ux-live-actions` and the recipe guard.

Checks: `cargo test --test live_e2e
command_center_primary_action_continues_waiting_agent -- --ignored
--nocapture`; `just ux-live-actions`; `git diff --check`; `just ux`;
`just ci`; `just perf-live`.

Remaining risk: low for Command Center primary continue dispatch; broader
active-goal coverage still depends on live-testing every conductor-level action
that promises to control real panes.

## 2026-05-02 - Saved fleet live action gate

Pass: promoted the saved-fleet broadcast journey from dogfood-only coverage into
the live action gate because saving, reloading, and broadcasting to a fleet is a
core command-center promise. Strengthened the live E2E test so it proves More
visibly advertises `save fleet` and `choose fleet`, the fleet picker reloads the
saved fleet, the command review protects both panes before confirmation, the
broadcast reaches both live target panes, and muxboard remains running.

Checks: `cargo test --test live_e2e
saved_target_group_can_be_reloaded_and_used_for_broadcast -- --ignored
--nocapture`; `just ux-live-actions`; `git diff --check && just ux && just ci
&& just perf-live`.

Remaining risk: low for saved-fleet save, load, review, and broadcast action
flow; broader active-goal coverage still depends on keeping fleet persistence
promises in live gates.

## 2026-05-03 - Stale saved fleet live action gate

Pass: promoted stale saved-fleet recovery from dogfood-only coverage into the
live action gate because a reusable fleet that loses all live panes must be
obviously safe, not a hidden broken send path. Strengthened the live E2E test so
it proves More visibly advertises `save fleet`, the stale fleet state blocks
command entry instead of opening a fake send surface, More exposes `choose
fleet` and `delete stale triage`, the Fleets picker shows `0/1 live` without
promising load, deleting the stale fleet clears the broken target, and muxboard
remains running.

Checks: `cargo test --test live_e2e
stale_saved_fleet_stays_recoverable_after_live_pane_disappears -- --ignored
--nocapture`; `just ux-live-actions`; `git diff --check && just ux && just ci
&& just perf-live`.

Remaining risk: low for stale saved-fleet blocking, picker, and delete recovery;
broader active-goal coverage still depends on keeping destructive fleet actions
inside live gates.

## 2026-05-03 - Review-send live action gate

Pass: promoted the review-send safety journeys from dogfood-only coverage into
the live action gate because multi-pane sends are the highest-blast-radius V1
action. Strengthened the live E2E coverage so review screens must visibly show
the target summary plus `Enter send` and `Esc cancel`, canceling review sends
nothing and returns to the fleet, confirming after one target disappears sends
only to the survivor, confirming after every target disappears sends nowhere,
and muxboard remains running throughout.

Checks: `cargo test --test live_e2e
review_send_cancel_keeps_targets_safe_and_recovers_cleanly -- --ignored
--nocapture`; `cargo test --test live_e2e
review_send_survives_target_pane_disappearing_before_confirm -- --ignored
--nocapture`; `cargo test --test live_e2e
review_send_recovers_when_every_target_pane_disappears_before_confirm --
--ignored --nocapture`; `just ux-live-actions`; `git diff --check && just ux
&& just ci && just perf-live`.

Remaining risk: low for review-send cancel, partial-disappearance dispatch, and
all-disappeared recovery; broader active-goal coverage still depends on keeping
all high-blast-radius tmux mutations in live gates.

## 2026-05-03 - Output observability live action gate

Pass: promoted the Output observability journeys from dogfood-only coverage into
the live action gate because opening a pane should expose useful real tmux
tail output immediately and keep updating while the user watches. Strengthened
the live E2E coverage so Output must show the newest real pane tail before
metadata, never fall back to `No output yet.` or `Updated: no output yet` when
real output exists, refresh while still open after fresh pane writes, and leave
muxboard running.

Checks: `cargo test --test live_e2e
output_panel_shows_real_tmux_tail_before_metadata -- --ignored --nocapture`;
`cargo test --test live_e2e
output_panel_updates_while_open_after_real_pane_output -- --ignored
--nocapture`; `just ux-live-actions`; `git diff --check && just ux && just ci
&& just perf-live`.

Remaining risk: low for Output open-tail and live-refresh observability; broader
active-goal coverage still depends on keeping every pane-observability promise
bound to real tmux output tests.

## 2026-05-03 - Same-server focus and jump live action gate

Pass: promoted same-server focus and jump journeys from dogfood-only coverage
into the live action gate because many users launch muxboard inside the same
tmux server they are controlling. These tests prove Enter opens Output without
exiting muxboard, repeated Enter does not accidentally leave the app, Esc
recovers to Details, jump leaves muxboard running while focusing the target
pane, and cross-session jump focuses the intended pane.

Checks: `cargo test --test live_e2e
same_server_enter_keeps_muxboard_visible_and_jump_leaves_it_running --
--ignored --nocapture`; `cargo test --test live_e2e
same_server_jump_handles_cross_session_targets -- --ignored --nocapture`.
`just ux-live-actions`; `git diff --check && just ux && just ci && just
perf-live`.

Remaining risk: low for same-server Enter, repeated Enter, Esc recovery, and
cross-session jump focus; broader active-goal coverage still depends on keeping
focus-changing tmux actions in live gates.

## 2026-05-03 - Refresh and reconnect live action gate

Pass: promoted manual refresh recovery from dogfood-only coverage into the live
action gate because losing and regaining the target tmux server is a real
control-plane promise, not a passive screenshot. These tests prove `R` turns a
dead target server into a clear recovery state without fake success copy,
reconnects when tmux comes back, resumes live pane observation, and leaves
muxboard running.

Checks: `cargo test --test live_e2e
manual_refresh_survives_target_tmux_server_disappearing -- --ignored
--nocapture`; `cargo test --test live_e2e
manual_refresh_reconnects_live_updates_after_tmux_reappears -- --ignored
--nocapture`. `just ux-live-actions`; `git diff --check && just ux && just ci
&& just perf-live`.

Remaining risk: low for manual refresh and reconnect after target-server loss;
broader active-goal coverage still depends on keeping stale-state and
control-plane recovery in live gates.

## 2026-05-03 - Live status freshness action gate

Pass: promoted live status freshness journeys from dogfood-only coverage into
the live action gate because stale `Latest` and `Now`/`Next` summaries are a
showstopper for an agent command center. Strengthened the live E2E coverage so
real pane changes must replace stale waiting/error text, structured status
updates must replace old `NEXT` values in both Fleet and Details, and muxboard
must remain running after the refresh.

Checks: `cargo test --test live_e2e
refresh_recovers_from_stale_waiting_output_after_a_real_state_change --
--ignored --nocapture`; `cargo test --test live_e2e
live_status_update_replaces_stale_latest_and_next -- --ignored --nocapture`.
`just ux-live-actions`; `git diff --check && just ux && just ci && just
perf-live`.

Remaining risk: low for stale Fleet/Details summary replacement after real pane
updates; broader active-goal coverage still depends on keeping future status
freshness promises bound to live tmux output tests.

## 2026-05-03 - Secondary surface escape live action gate

Pass: promoted Command Center and Browse `Esc back` journeys from dogfood-only
coverage into the live action gate because secondary surfaces must feel safe and
layered, not like traps. Strengthened the live E2E checks to use the shared
running-process assertion after Esc restores Details and removes the overlay.

Checks: `cargo test --test live_e2e
command_center_escape_returns_to_fleet_details_in_live_tmux -- --ignored
--nocapture`; `cargo test --test live_e2e
browse_escape_returns_to_fleet_details_in_live_tmux -- --ignored --nocapture`.
`just ux-live-actions`; `git diff --check && just ux && just ci && just
perf-live`.

Remaining risk: low for live Command Center and Browse escape recovery; broader
active-goal coverage still depends on keeping every advertised secondary-surface
key in action-contract or live-action tests.

## 2026-05-03 - SSH-safe notification settings live action gate

Pass: promoted notification settings from dogfood-only coverage into the live
action gate because local-vs-SSH alert behavior is both a settings action and a
trust promise. The live E2E test already proves an SSH environment shows
terminal-safe desktop alert copy, toggles desktop and bell settings through the
More menu, persists them across restart, and leaves muxboard running; the
architecture guard now requires that journey in `just ux-live-actions`.

Checks: `cargo test --test live_e2e
notification_settings_persist_across_restart_and_stay_ssh_safe -- --ignored
--nocapture`; `just ux-live-actions`; `git diff --check && just ux && just ci
&& just perf-live`.

Remaining risk: low for SSH-safe notification settings persistence; broader
active-goal coverage still depends on keeping terminal-compatibility promises in
renderer or live gates.

## 2026-05-03 - Live terminal surface recipe

Pass: split live terminal and surface rendering journeys into `just
ux-live-surfaces` instead of overloading `just ux-live-actions`. The new recipe
owns narrow terminal scannability, SSH-like dumb terminal legibility, and plain
session/window location readability. `dogfood` now calls both live recipes, and
the architecture guard accounts for both when proving every ignored live E2E
test is covered.

Checks: `cargo test --test live_e2e
narrow_terminal_keeps_the_board_scannable -- --ignored --nocapture`; `cargo
test --test live_e2e ssh_like_dumb_terminal_keeps_the_board_legible -- --ignored
--nocapture`; `cargo test --test live_e2e
fleet_keeps_plain_session_window_locations_readable_live -- --ignored
--nocapture`; `just ux-live-surfaces`; `git diff --check && just ux && just ci
&& just perf-live`.

Remaining risk: low for the recipe boundary and live terminal surface coverage;
future live render promises should go into `ux-live-surfaces`, while tmux
mutations and advertised key actions stay in `ux-live-actions`.

## 2026-05-03 - Live startup recovery recipe

Pass: split live startup and first-run recovery journeys into `just
ux-live-startup` so no-tmux, missing-session, and invalid-config states are not
buried inside the general dogfood list. `dogfood` now calls actions, surfaces,
startup, and perf live gates explicitly, and the architecture guard includes the
new recipe when proving ignored live E2E coverage.

Checks: `cargo test --test live_e2e
no_tmux_server_first_run_explains_recovery -- --ignored --nocapture`; `cargo
test --test live_e2e missing_session_first_run_explains_recovery -- --ignored
--nocapture`; `cargo test --test live_e2e
invalid_config_falls_back_to_defaults_and_still_starts -- --ignored
--nocapture`; `just ux-live-startup`; `git diff --check && just ux && just ci
&& just perf-live`.

Remaining risk: low for startup recovery recipe coverage; future first-run or
recoverable startup promises should go into `ux-live-startup` instead of the
undifferentiated dogfood tail.

## 2026-05-03 - Live persistence recipe

Pass: split restart-backed persistence journeys into `just ux-live-persistence`
so acknowledgement state and saved fleet groups have a clear live owner. The
dogfood recipe now calls actions, surfaces, startup, persistence, and perf live
gates explicitly, and the live E2E coverage guard includes the persistence
recipe when checking ignored tests.

Checks: `cargo test --test live_e2e
acknowledgement_persists_across_restart -- --ignored --nocapture`; `cargo test
--test live_e2e saved_group_persists_across_restart_and_can_be_reloaded --
--ignored --nocapture`; `just ux-live-persistence`; `git diff --check && just
ux && just ci && just perf-live`.

Remaining risk: low for restart-backed acknowledgement and saved fleet
persistence coverage; future XDG/state persistence promises should go into
`ux-live-persistence`.

## 2026-05-03 - Live navigation and churn recipes

Pass: moved the remaining uncategorized dogfood tail into owned live recipes.
`ux-live-navigation` now owns filter recovery, visible target movement, and deep
selection scroll promises. `ux-live-churn` owns resize, carriage-return progress,
and multi-pane attention churn. The first-screen attention hierarchy check moved
into `ux-live-surfaces`, so dogfood now reads as named product promises instead
of a loose list of individual tests.

Checks: `cargo test --test live_e2e
first_screen_prioritizes_attention_and_hides_secondary_details -- --ignored
--nocapture`; `just ux-live-navigation`; `just ux-live-churn`; `git diff
--check && just ux && just ci && just perf-live`.

Remaining risk: low for live recipe ownership. Future navigation, resize,
selection, or attention-churn regressions should enter these recipes instead of
the dogfood body directly.

## 2026-05-03 - Live recipe testing matrix guard

Pass: made the newly organized live recipes discoverable in
`docs/testing-matrix.md` and added an architecture guard so dogfood's named live
recipe structure cannot drift into hidden contributor-only knowledge. The testing
matrix now explains when to use actions, surfaces, startup, persistence,
navigation, churn, dogfood, and live performance recipes.

Checks: `cargo test --test architecture_guards
ux_action_recipes_exercise_real_key_and_tmux_actions -- --nocapture`; `git diff
--check && just ux && just ci && just perf-live`.

Remaining risk: low for live recipe discoverability; future live recipe additions
should update the testing matrix in the same pass.

## 2026-05-03 - Shell prompt noise dogfood fix

Pass: manual human-facing TUI dogfood found an idle shell row spending the Fleet
Latest column on a local shell prompt (`user@host:path$`) instead of useful work.
Prompts and shell startup banners are now treated as runtime noise before
partial fragments, core activity summaries, and presentation recent-line filters
can promote them. Added a live tmux surface regression so realistic interactive
shell prompts and macOS default-shell chatter cannot reappear in Fleet Latest.

Checks: `cargo test --lib activity_summary_ignores_shell_prompt_noise --
--nocapture`; `cargo test --lib activity_summary_ignores_shell_startup_banner_noise
-- --nocapture`; `cargo test --lib shell_prompt_glyphs_are_noise --
--nocapture`; `cargo test --test live_e2e
idle_shell_prompt_noise_stays_out_of_fleet_latest -- --ignored --nocapture`;
manual tmux capture of the mixed Fleet screen after the fix; `git diff --check
&& just ux && just ci && just perf-live`.

Remaining risk: low for terminal chatter. Future prompt or startup-banner
variants that escape should extend `is_terminal_chatter_noise` and the live
surface test.

## 2026-05-03 - Visible thinking status truth

Pass: live dogfood exposed a contradictory selected agent state: Fleet and
Details showed Codex `Thinking`, but Details labeled the pane `State: Idle`.
Agent-visible running hints now map to Running when the screen is current or
captured at startup, while old thinking output can still become Stuck after the
existing stale threshold.

Checks: `cargo test --lib visible_agent_thinking_state_is_running_not_idle --
--nocapture`; `cargo test --lib stale_agent_becomes_stuck -- --nocapture`;
`cargo test --test live_e2e visible_agent_thinking_state_is_running_not_idle_live
-- --ignored --nocapture`; architecture guard checks for live recipe coverage;
`git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for `Thinking`/`Working`/`Running` status contradictions.
Future provider-specific active phrases should extend the running-hint test
instead of relying on age-only inference.

## 2026-05-03 - Prompt-returned agent activity is stale

Pass: status-truth dogfood found the complementary edge to visible thinking:
when an agent-like pane returns to a shell prompt after printing active text,
older `Thinking` or waiting hints must not keep the pane looking active. Status
inference now treats a newer shell prompt as a boundary for older Running and
Waiting hints while still allowing useful finished output to remain visible.
The core matrix now proves older active and waiting hints go stale behind a
prompt, while command errors remain visible.

Checks: `cargo test --lib shell_prompt_after_agent_activity_makes_active_hint_stale
-- --nocapture`; `cargo test --lib shell_prompt_after_waiting_hint_makes_waiting_stale
-- --nocapture`; `cargo test --lib shell_prompt_after_agent_error_keeps_error_visible
-- --nocapture`; `cargo test --lib visible_agent_thinking_state_is_running_not_idle
-- --nocapture`; `cargo test --test live_e2e
shell_prompt_after_agent_activity_is_idle_not_running_live -- --ignored
--nocapture`; architecture guard checks for live recipe coverage; `git diff
--check && just ux && just ci && just perf-live`.

Remaining risk: low for prompt-returned stale activity. Future prompt-boundary
bugs should cover both the core status inference and a live selected Details
screen, because the failure is only obvious when State and Now/Action disagree.

## 2026-05-03 - Error details say Problem, not Blocked

Pass: mixed-state Fleet dogfood showed the priority order was right, but the
selected error pane rendered `Blocked: command failed`. Error states now label
the detail as `Problem:` while waiting and stuck states keep `Blocked:`, making
the Details language match what the user needs to know at a glance.

Checks: `cargo test --lib selected_inspector_error_keeps_blocker_and_action_above_output
-- --nocapture`; `cargo test --lib selected_pane_lines_show_codex_error_synthetic_report
-- --nocapture`; `cargo test --lib selected_pane_lines_show_shell_error_report_after_waiting_prompt
-- --nocapture`; `cargo test --test live_e2e
refresh_recovers_from_stale_waiting_output_after_a_real_state_change --
--ignored --nocapture`; `git diff --check && just ux && just ci && just
perf-live`.

Remaining risk: low for error-detail copy. Future attention-state label changes
should inspect a mixed Fleet capture, because labels that are technically
consistent can still be semantically wrong in context.

## 2026-05-03 - None blockers stay out of Details

Pass: coverage-guided Details audit found stale defensive code around
`BLOCKER=none`. The cleaning layer already treats `none` as empty, so the
presentation path is now simpler and the regression tests explicitly prove that
waiting, idle, and running structured reports never render `Blocked: none`,
`Problem: none`, or raw `BLOCKER=none` in user-facing Details.

Checks: `cargo test --lib selected_pane_lines_hide_none_blockers_from_structured_reports
-- --nocapture`; `cargo test --lib
usability_provider_protocol_is_distilled_before_it_reaches_fleet_or_details --
--nocapture`; `just coverage-missing`; `git diff --check && just ux && just ci
&& just perf-live`.

Remaining risk: low for none-blocker leakage in Details. Future structured
report copy changes should keep the renderer protocol guard broad enough to
catch both raw fields and cleaned-but-meaningless labels.

## 2026-05-03 - Tiny Command Center footer advertises the primary action

Pass: live 60x12 Command Center dogfood showed the body correctly said
`Action: A continue`, but the footer spent scarce space on `Enter output`
instead of the actual primary Command Center action. The Command Center footer
now leads with the same primary action as the body, keeps `Esc back`, and avoids
advertising the unrelated Details output action on tiny terminals.

Checks: `cargo test --lib
tiny_command_center_keeps_primary_actions_and_selection_visible --
--nocapture`; `cargo test --test live_e2e
command_center_primary_action_continues_waiting_agent -- --ignored
--nocapture`; `MUXBOARD_BLESS_GOLDEN=1 cargo test --lib
exact_grid_matches_overview_attention_overlay -- --nocapture`; `cargo test
--lib presentation_modes_cover_empty_states_and_overlay_branches --
--nocapture`; `cargo test --lib
chrome_trunk_test_identifies_location_and_primary_action_across_modes --
--nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for Command Center footer/body drift. Future
secondary-surface footers should be checked against the body action, not just
generic pane controls.

## 2026-05-03 - Command Center footer body contract

Pass: coverage-guided footer audit found that the new Command Center footer was
still vulnerable to duplicate actions and inert actions in less obvious states:
normal work could show `: send` twice, no-match Command Center could advertise
movement or send despite having no visible panes, and the footer/body action
mapping was duplicated in two places. The body and footer now share the same
primary-action mapping, and renderer tests prove waiting, error, idle, marked,
lane, empty, and no-match Command Center states keep one obvious primary action.
The no-match regression also presses Backspace to prove the advertised recovery
action restores the Command Center instead of acting as decorative copy.

Checks: `cargo test --lib usability_command_center_ -- --nocapture`; `cargo
test --lib
usability_command_center_no_match_footer_prioritizes_recovery_over_inert_actions
-- --nocapture`; `just coverage-missing`; `git diff --check && just ux && just
ci && just perf-live`.

Remaining risk: low for covered Command Center footer/action drift. Future
Command Center footer changes should prove both body/footer agreement and the
absence of duplicate or inert actions.

## 2026-05-03 - Command Center continue action contract

Pass: action-contract follow-up for the Command Center primary action. The
Command Center can advertise `A continue` for a waiting agent, so the
renderer/key-router test now renders that promise, presses `A`, proves muxboard
stays open, proves tmux receives Enter for the waiting pane, and rejects
destructive tmux commands in the fake log.

Checks: `cargo test --lib
usability_action_contract_command_center_continue_primary_action_sends_enter --
--nocapture`; `just ux-actions`; `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low for the covered `A continue` Command Center promise. Future
primary-action copy should keep the body, footer, key-router, and tmux dispatch
under the same action-contract test loop.

## 2026-05-03 - Command Center primary surface contracts

Pass: completed the local Command Center primary-action matrix beyond the live
`A continue` path. Renderer/key-router coverage now proves `Enter output`
opens Output for an erroring agent, and `: send` opens the Send surface for an
idle agent, so the Command Center's body and footer promises are not static copy.

Checks: `cargo test --lib
usability_action_contract_command_center_output_and_send_actions_open_promised_surfaces
-- --nocapture`; `just ux-actions`; `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low for covered local Command Center primary actions. The
remaining Command Center no-pane refresh path is lower risk because refresh
already has separate help/action coverage, but it can still be folded into this
matrix if future copy changes make it more prominent.

## 2026-05-03 - Command Center empty refresh contract

Pass: folded the remaining empty Command Center primary action into the
action-contract loop. The empty Command Center advertises `R refresh`; the
renderer/key-router test now renders that promise, presses `R`, proves the app
stays open, keeps the no-pane recovery surface visible, and proves fake tmux was
queried for version and panes.

Checks: `cargo test --lib
usability_action_contract_command_center_empty_refresh_primary_action_runs --
--nocapture`; `just ux-actions`; `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low for Command Center primary actions. The visible primary
action matrix now covers waiting continue, error output, idle send, no-match
show-all recovery, and empty refresh.

## 2026-05-03 - Live Output surface wait contract

Pass: `just dogfood` exposed a live-test blind spot, not a product regression:
the Output action test waited for the generic word `Output`, which is already
visible inside Details. The live E2E suite now waits for the unambiguous Output
surface contract, `Esc back` present and `Enter output` absent, and an
architecture guard rejects future live waits on the generic Output heading.

Checks: `cargo test --test architecture_guards
live_output_tests_wait_for_the_output_surface_not_the_generic_heading --
--nocapture`; `cargo test --test live_e2e
enter_opens_output_without_exiting_and_jump_keeps_muxboard_running -- --ignored
--nocapture`; `just dogfood`.

Remaining risk: low for this escaped class once dogfood is green. Future live
surface tests should wait on transition-specific footer/action copy, not words
that can appear in multiple panes.

## 2026-05-03 - Live muxboard startup wait contract

Pass: broadened the live-test wait audit from Output headings to startup
readiness. Live tests no longer wait for the generic word `muxboard`, because
the shell command line can echo that before the app is actually running. They
now poll until tmux reports `#{pane_current_command}` as `muxboard` and the
rendered surface contains the app chrome. An architecture guard rejects future
generic muxboard text waits in live E2E tests.

Checks: `cargo test --test architecture_guards
live_tests_wait_for_muxboard_process_not_command_echo -- --nocapture`; `cargo
test --test live_e2e enter_opens_output_without_exiting_and_jump_keeps_muxboard_running
-- --ignored --nocapture`; `just dogfood`; `git diff --check && just ux && just
ci && just perf-live`.

Remaining risk: low for launch/readiness false positives. Future live tests
should prove the process is running before sending UI keys, not trust visible
command text.

## 2026-05-03 - Live More menu wait contract

Pass: removed another live-test flake class from More menu journeys. Tests no
longer sleep after pressing `.`, then blindly press a row key. They wait for
the specific visible More row first, such as `mute alert`, `unmute alert`,
`save fleet`, `choose fleet`, or `X clear`. An architecture guard now rejects
future sleep-after-dot patterns in live E2E tests.

Checks: `cargo test --test architecture_guards
live_more_menu_tests_wait_for_visible_rows_before_selecting -- --nocapture`;
focused live tests for acknowledgement, saved fleets, and clear marks; `just
dogfood`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: medium-low for other fixed sleeps in Send/review flows; those
are now the next obvious live-wait hardening target.

## 2026-05-03 - Live Send and review wait contract

Pass: hardened live Send journeys so tests no longer press `:`, sleep, type a
command, sleep again, and hope the Send surface was ready. Direct Send tests now
wait for the visible Send surface, wait for the typed command to render, and
wait for the Review surface before asserting that nothing dispatched early. The
stale-fleet inert `:` path now uses a named stability check instead of a blind
sleep.

Checks: focused live Send/review tests; architecture guard
`live_send_tests_wait_for_visible_send_surfaces_before_typing`; `just dogfood`;
`git diff --check && just ux && just ci && just perf-live`.

Remaining risk: medium-low for fixed sleeps in navigation/churn flows that are
not Send/review readiness waits.

## 2026-05-03 - Live E2E named wait contract

Pass: removed the remaining inline fixed sleeps from live E2E tests. Resize,
search, movement, churn, secondary-view escape, repeated Output Enter, and slow
literal typing now use named waits tied to tmux state, visible screen text, or a
short inert-action stability window.

Checks: architecture guard
`live_e2e_tests_use_named_waits_instead_of_inline_fixed_sleeps`; focused live
resize/search/target/churn/output/secondary-view tests; broad gate
`git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low; deliberate slow typing still uses explicit per-key delays,
but live post-action readiness now goes through named waits.

## 2026-05-03 - Live Escape dismissal wait contract

Pass: hardened live Escape flows so tests prove the dismissed surface is gone
before continuing. Search input, Review send, stale fleet picker, Output,
Command Center, and Browse now wait for the recovery text without the closed
surface instead of accepting generic background text.

Checks: architecture guard
`live_escape_tests_wait_for_dismissed_surfaces_to_disappear`; focused live
Escape journeys; broad gate `git diff --check && just ux && just ci && just
perf-live`.

Remaining risk: low; Details legitimately contains an Output section, so Output
Escape waits use `Esc back` as the closed-surface marker instead of the word
Output.

## 2026-05-03 - Live clear send list reset wait

Pass: replaced stale-prone waits after `X clear send list`. The affected live
tests no longer accept a pre-existing pane label after clearing targets; they
wait for the More overlay to disappear, the two-pane send list to be gone, and
the selected row to become the default send target.

Checks: architecture guard
`live_clear_send_list_tests_wait_for_visible_reset`; focused live clear/reload
tests; broad gate `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low; the helper intentionally keys off the selected row becoming
the default target because clearing the send list returns to selected-pane
targeting rather than showing a one-pane send-list label.

## 2026-05-03 - Live saved fleet picker and load waits

Pass: replaced generic saved-fleet waits with visible state waits. Live tests now
wait for the Fleets picker, selected fleet row, live-count, and load affordance
before pressing Enter, then wait for the picker to close and the fleet target to
become active.

Checks: architecture guard
`live_saved_fleet_tests_wait_for_picker_and_active_state`; focused live saved
fleet tests; broad gate `git diff --check && just ux && just ci && just
perf-live`.

Remaining risk: low; the guard forbids generic `Fleets` and `Send: fleet` waits
in live E2E because those strings can be stale overlay or prior status text.

## 2026-05-03 - Live refresh waits

Pass: replaced generic waits after `R refresh` with named refresh-result waits.
The live refresh tests now wait for the post-refresh state and key forbidden
stale states, instead of accepting pre-existing pane labels or generic error
strings.

Checks: architecture guard
`live_refresh_tests_wait_for_visible_refresh_results`; focused live refresh,
reconnect, churn, and stale-fleet tests; broad gate `git diff --check && just ux
&& just ci && just perf-live`.

Remaining risk: low; refresh assertions are still screen-level because refresh is
observable through the same board/details surface users rely on.

## 2026-05-03 - Live Send command text waits

Pass: replaced generic waits for typed Send command text with a Send-surface wait.
Live tests now prove the command text is visible in the active Send form with the
right footer before pressing Enter, rather than accepting stale pane text or
target output.

Checks: architecture guard
`live_send_command_text_waits_stay_inside_send_surface`; focused live Send,
review, disappearing-target, and saved-fleet send tests; broad gate `git diff
--check && just ux && just ci && just perf-live`.

Remaining risk: low; target-pane assertions still prove dispatch after the Send
surface wait.

## 2026-05-03 - Live send-list targeting waits

Pass: replaced generic waits after `Space` target toggles with send-list state
waits. Live tests now wait for both the send-list count and the selected row
showing as targeted, instead of accepting a stale `send list (N panes)` label.

Checks: architecture guard
`live_send_list_targeting_waits_include_selected_rows`; focused live multi-send,
saved-fleet, clear-target, cancel, disappearing-target, and target-clarity tests;
broad gate `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low; review and dispatch tests still verify target panes receive
or do not receive the command after the visible targeting state is established.

## 2026-05-03 - Live secondary surface waits

Pass: replaced generic heading waits after opening Command Center and Browse with
surface waits. Live tests now wait for the opened surface, its back affordance,
and absence of the More menu instead of accepting stale heading text.

Checks: architecture guard
`live_secondary_surface_tests_wait_for_real_surfaces`; focused live Command
Center and Browse tests; broad gate `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low; follow-up assertions still verify each surface's action
content after the surface wait returns.

## 2026-05-03 - Single-target Send action contract

Pass: covered the simplest Send journey at the renderer/key-router/tmux
boundary. The Send surface can advertise `Enter send` for one visible target;
the new action-contract test opens Send, types a command, renders the footer
promise, presses Enter, proves no review modal appears, and verifies fake tmux
received the literal command plus Enter for the selected pane.

Checks: `cargo test --lib
usability_action_contract_send_enter_dispatches_single_visible_target --
--nocapture`; `just ux-actions`; `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low for the one-pane Send promise. Multi-pane review and live
single-pane send already have separate action/live coverage; this pass closes
the local TUI action-contract gap for the default path.

## 2026-05-03 - Output scroll footer action contract

Pass: action-contract follow-up for Output footer truth. Output can advertise
`J/K scroll`, so the key-router test now presses those keys on a long rendered
Output surface, proves Fleet selection does not move, proves the viewport
visibly changes, and proves `K` returns to the original viewport.

Checks: `cargo test --lib usability_output_footer_scroll_keys_scroll_output_not_fleet
-- --nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for covered Output scroll footer truth. Future Output
footer changes should keep scroll promises tied to real rendered movement, not
only app-state helpers.

## 2026-05-03 - Browse window footer action contract

Pass: action-contract follow-up for Browse footer truth. Browse can advertise
`J/K browse`, `Enter window`, and Backspace recovery while narrowed, so the
renderer/key-router test now opens Browse, moves to another window, presses
Enter, proves the visible Fleet narrows to that window, then presses Backspace
and proves all windows return without stale recovery copy.

Checks: `cargo test --lib
usability_action_contract_browse_enter_window_and_backspace_are_real_footer_actions --
--nocapture`; `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low for covered Browse drill-down and recovery footer truth.
Future Browse footer changes should press the advertised keys and prove the
visible hierarchy changes, not only assert static copy.

## 2026-05-03 - Live stale-state board waits

Pass: hardened the live stale-state and churn journeys so driver-side waits prove
the real Fleet plus Details board is visible before acting. Refresh recovery now
starts from a selected waiting row, status replacement waits for the selected
Fleet block and Details `Now:` summary together, carriage-return progress waits
on the selected job board, and multi-pane churn waits on the selected attention
row instead of accepting accidental text anywhere in the pane.

Checks: architecture guard
`live_stale_state_tests_wait_for_board_specific_surfaces`; focused live refresh,
status, carriage-return, and churn tests; broad gate `git diff --check && just
ux && just ci && just perf-live`.

Remaining risk: low. Target-pane sentinel waits remain allowed where they prove
the external process reached a state before muxboard is refreshed or commanded.

## 2026-05-03 - Live driver waits use named surfaces

Pass: removed the remaining raw driver-pane text waits from live tmux E2E tests.
Driver-side assertions now wait for named muxboard surfaces: main Fleet plus
Details, Output with live text, Browse scope, Command Center, and stale-fleet
recovery. This exposed a real 80-column Browse recovery bug where `Showing all
panes.` replaced the useful footer after Backspace; that status is now treated
as low-value footer copy, and the renderer action-contract test protects the
80-column Browse recovery footer.

Checks: architecture guard `live_driver_ui_waits_use_named_surface_helpers`;
focused live reconnect, Output, stale fleet, Command Center, and Browse scope
tests; renderer action-contract Browse drill-down test; broad gate `git diff
--check && just ux && just ci && just perf-live`.

Remaining risk: low. Raw target-pane waits remain valid for external process
sentinels; raw muxboard driver-pane waits are now blocked by architecture guard.

## 2026-05-03 - Raw live waits classified

Pass: made the previous driver-wait cleanup enforceable across the whole live
E2E file. A new architecture guard allows raw `.wait_for_text(...)` only when it
is an external target-pane sentinel proving the controlled process printed a
marker; any muxboard UI wait must use a named surface helper.

Checks: architecture guard `live_raw_text_waits_are_external_target_sentinels`;
broad gate `git diff --check && just ux && just ci && just perf-live`.

Remaining risk: low. This does not ban target-pane sentinels because those are
process-state checks, not muxboard UI assertions.

## 2026-05-03 - Escape waits prove the returned surface

Pass: removed the generic live E2E helper that only waited for one word to
disappear after Escape. Search cancel now waits for the applied search result,
review cancel waits for the selected send-list target state, stale fleet picker
cancel waits for the real board state, and Output, Command Center, and Browse
cancel paths wait for named board-return helpers. This turns "Esc did something"
into "Esc returned to the intended surface."

Checks: architecture guard
`live_escape_tests_wait_for_dismissed_surfaces_to_disappear`; focused live
Escape journeys for search, review, stale fleet picker, Output, Command Center,
and Browse; broad gate `git diff --check && just ux && just ci && just
perf-live`.

Remaining risk: low. Command Center and Browse returns intentionally allow an
`Esc back` footer because the live main board may surface it as a useful focus
action; the guard verifies the dismissed surface is gone and the selected board
row is visible.

## 2026-05-03 - Refresh waits reject secondary surfaces

Pass: strengthened `wait_for_refresh_result` so refresh live E2E coverage waits
for a top-level muxboard surface and rejects secondary overlays such as More,
Send, Review, Command Center, and Browse. This keeps manual refresh tests from
passing on stale modal text after `R refresh`.

Checks: architecture guard
`live_refresh_tests_wait_for_visible_refresh_results`; focused live refresh
journeys for missing tmux, reconnect, stale waiting-to-error-to-done, multi-pane
churn, and stale saved fleet recovery; broad gate `git diff --check && just ux
&& just ci && just perf-live`.

Remaining risk: low. The helper remains intentionally text-parametric because
refresh can return either a recovery surface or a normal Fleet board.

## 2026-05-03 - Target movement waits on the board

Pass: removed the final one-off live E2E visible-line wait. The target clarity
journey now waits for the full main board surface, selected row, send-list
count, and previously marked target after moving selection. This protects the
"I can see what is selected and what is targeted" journey at the same surface
level as the rest of the live suite.

Checks: architecture guard
`live_send_list_targeting_waits_include_selected_rows`; named-wait guard
`live_e2e_tests_use_named_waits_instead_of_inline_fixed_sleeps`; focused live
target movement test; broad gate `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low. This is a test hardening pass, not a product behavior
change.

## 2026-05-03 - Live resize waits prove the board

Pass: tightened the resize-churn live E2E helper so resizing waits for a real
Fleet board with the selected row still visible, instead of waiting for arbitrary
text after the terminal dimensions settle. This keeps resize coverage aligned
with the product promise: selection and attention context survive layout changes.

Checks: architecture guard
`live_resize_and_navigation_tests_wait_for_main_board_surface`; focused live
resize-churn test; broad gate `git diff --check && just ux && just ci &&
just perf-live`.

Remaining risk: low. The helper intentionally allows narrow board states that
omit Details, but it rejects overlays and requires the selected Fleet row.

## 2026-05-02 - Tiny More recommendation contract

Pass: generalized the escaped More recommendation bug into a renderer invariant:
if More says `Action: <key>` and the footer says `press a listed key`, every
recommended key must be visible as a listed row even on tiny terminals. The
audit found tiny More could recommend `Space add to send list` while hiding the
Space row, and could hide secondary recommendations under row pressure. More now
lists the selected pane add/remove action and tiny overlays flatten scarce rows
around recommendations, the start path, command center, and core pane actions
instead of spending rows on section headings.

Checks: `cargo test usability_more_top_recommendations_are_visible_listed_actions --
--nocapture`; `cargo test tiny_more_overlay_keeps_start_agent_discoverable --
--nocapture`; `cargo test
tiny_narrowed_more_overlay_keeps_show_all_recovery_visible -- --nocapture`;
`cargo test empty_more_overlay_is_recovery_not_action_dump -- --nocapture`;
`cargo test usability_action_contract_more_menu_actions_execute_visible_promises
-- --nocapture`.

Remaining risk: `just ux`, `just ci`, and `just perf-live` passed after this
pass. Use `just release-check` before shipping if one single end-to-end gate is
needed.
