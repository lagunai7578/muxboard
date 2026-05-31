set shell := ["bash", "-euo", "pipefail", "-c"]

default:
  just --list

# Format the codebase.
fmt:
  cargo fmt

# Check formatting without writing changes.
fmt-check:
  cargo fmt --check

# Run the fast local verification suite.
test:
  cargo test -- --test-threads=1

# Run architecture boundary checks only.
guards:
  cargo test --test architecture_guards -- --nocapture

# Run fixture-backed contract tests only.
contracts:
  cargo test fixtures -- --nocapture

# Run exact renderer golden-grid tests only.
tui-golden:
  cargo test --lib exact_grid_matches -- --nocapture

# Run the fast AGENTS.md usability guardrail loop.
ux:
  just guards
  just ux-actions
  cargo test --lib usability_ -- --nocapture
  cargo test --lib scrollbar -- --nocapture
  just tui-golden
  just perf-smoke

# Run deterministic action-contract journeys against the real key router.
ux-actions:
  cargo test --lib usability_action_contract -- --nocapture
  cargo test --lib app_tmux_action_paths_are_exercised_against_fake_tmux_binary -- --nocapture

# Refresh exact renderer golden-grid fixtures after a deliberate UI review.
tui-golden-bless:
  MUXBOARD_BLESS_GOLDEN=1 cargo test --lib exact_grid_matches -- --nocapture

# Run the live tmux smoke suite.
test-live:
  cargo test --test live_e2e -- --ignored --nocapture --test-threads=1

# Run high-signal live tmux actions, not just rendered screens.
ux-live-actions:
  cargo test --test live_e2e enter_opens_output_without_exiting_and_jump_keeps_muxboard_running -- --ignored --nocapture
  cargo test --test live_e2e same_server_enter_keeps_muxboard_visible_and_jump_leaves_it_running -- --ignored --nocapture
  cargo test --test live_e2e same_server_jump_handles_cross_session_targets -- --ignored --nocapture
  cargo test --test live_e2e manual_refresh_survives_target_tmux_server_disappearing -- --ignored --nocapture
  cargo test --test live_e2e manual_refresh_reconnects_live_updates_after_tmux_reappears -- --ignored --nocapture
  cargo test --test live_e2e output_panel_shows_real_tmux_tail_before_metadata -- --ignored --nocapture
  cargo test --test live_e2e output_panel_updates_while_open_after_real_pane_output -- --ignored --nocapture
  cargo test --test live_e2e refresh_recovers_from_stale_waiting_output_after_a_real_state_change -- --ignored --nocapture
  cargo test --test live_e2e live_status_update_replaces_stale_latest_and_next -- --ignored --nocapture
  cargo test --test live_e2e opening_output_marks_explicit_agent_review_seen_live -- --ignored --nocapture
  cargo test --test live_e2e smart_action_sends_enter_to_a_waiting_pane -- --ignored --nocapture
  cargo test --test live_e2e free_form_reply_journey_uses_reply_copy_and_dispatches_live -- --ignored --nocapture
  cargo test --test live_e2e command_center_answer_targets_attention_pane_without_attaching_when_selection_differs -- --ignored --nocapture
  cargo test --test live_e2e lane_smart_action_sends_enter_to_waiting_agents_only -- --ignored --nocapture
  cargo test --test live_e2e single_target_send_labels_enter_send_and_dispatches_immediately -- --ignored --nocapture
  cargo test --test live_e2e review_send_cancel_keeps_targets_safe_and_recovers_cleanly -- --ignored --nocapture
  cargo test --test live_e2e review_send_survives_target_pane_disappearing_before_confirm -- --ignored --nocapture
  cargo test --test live_e2e review_send_recovers_when_every_target_pane_disappears_before_confirm -- --ignored --nocapture
  cargo test --test live_e2e summary_action_sends_one_line_prompt_to_live_tmux -- --ignored --nocapture
  cargo test --test live_e2e zoom_action_toggles_live_tmux_pane_without_leaving_muxboard -- --ignored --nocapture
  cargo test --test live_e2e action_menu_show_in_tmux_jumps_to_selected_pane -- --ignored --nocapture
  cargo test --test live_e2e search_mark_and_confirmed_multi_send_work_against_live_tmux -- --ignored --nocapture
  cargo test --test live_e2e saved_target_group_can_be_reloaded_and_used_for_broadcast -- --ignored --nocapture
  cargo test --test live_e2e stale_saved_fleet_stays_recoverable_after_live_pane_disappears -- --ignored --nocapture
  cargo test --test live_e2e action_menu_clear_marks_resets_targeting_to_the_selected_pane -- --ignored --nocapture
  cargo test --test live_e2e action_menu_can_acknowledge_and_restore_selected_attention -- --ignored --nocapture
  cargo test --test live_e2e action_menu_uses_rebound_secondary_keys -- --ignored --nocapture
  cargo test --test live_e2e notification_settings_persist_across_restart_and_stay_ssh_safe -- --ignored --nocapture
  cargo test --test live_e2e launch_agent_creates_new_tmux_window_without_leaving_muxboard -- --ignored --nocapture
  cargo test --test live_e2e launch_agent_recovers_when_target_server_disappears -- --ignored --nocapture
  cargo test --test live_e2e command_center_escape_returns_to_fleet_details_in_live_tmux -- --ignored --nocapture
  cargo test --test live_e2e browse_escape_returns_to_fleet_details_in_live_tmux -- --ignored --nocapture
  cargo test --test live_e2e browse_enter_scopes_to_live_window_and_backspace_recovers -- --ignored --nocapture
  cargo test --test live_e2e command_center_primary_action_continues_waiting_agent -- --ignored --nocapture
  cargo test --test live_e2e command_center_primary_action_answers_choice_prompt -- --ignored --nocapture

# Run high-signal live terminal and surface journeys, not key actions.
ux-live-surfaces:
  cargo test --test live_e2e narrow_terminal_keeps_the_board_scannable -- --ignored --nocapture
  cargo test --test live_e2e ssh_like_dumb_terminal_keeps_the_board_legible -- --ignored --nocapture
  cargo test --test live_e2e fleet_keeps_plain_session_window_locations_readable_live -- --ignored --nocapture
  cargo test --test live_e2e idle_shell_prompt_noise_stays_out_of_fleet_latest -- --ignored --nocapture
  cargo test --test live_e2e visible_agent_thinking_state_is_running_not_idle_live -- --ignored --nocapture
  cargo test --test live_e2e shell_prompt_after_agent_activity_is_idle_not_running_live -- --ignored --nocapture
  cargo test --test live_e2e first_screen_prioritizes_attention_and_hides_secondary_details -- --ignored --nocapture
  cargo test --test live_e2e command_center_large_attention_queue_shows_overflow_live -- --ignored --nocapture

# Run high-signal live startup and recovery journeys.
ux-live-startup:
  cargo test --test live_e2e no_tmux_server_first_run_explains_recovery -- --ignored --nocapture
  cargo test --test live_e2e missing_session_first_run_explains_recovery -- --ignored --nocapture
  cargo test --test live_e2e invalid_config_falls_back_to_defaults_and_still_starts -- --ignored --nocapture

# Run high-signal live persistence journeys across restart.
ux-live-persistence:
  cargo test --test live_e2e acknowledgement_persists_across_restart -- --ignored --nocapture
  cargo test --test live_e2e saved_group_persists_across_restart_and_can_be_reloaded -- --ignored --nocapture

# Run high-signal live navigation journeys across filters, targets, and scroll.
ux-live-navigation:
  cargo test --test live_e2e search_cancel_restores_the_previous_filter -- --ignored --nocapture
  cargo test --test live_e2e target_set_stays_obvious_while_selection_moves -- --ignored --nocapture
  cargo test --test live_e2e small_board_scrolls_to_keep_deep_selections_visible -- --ignored --nocapture

# Run high-signal live churn journeys across resize and changing pane output.
ux-live-churn:
  cargo test --test live_e2e resize_churn_preserves_selection_and_attention_context -- --ignored --nocapture
  cargo test --test live_e2e carriage_return_progress_updates_follow_visible_pane_state -- --ignored --nocapture
  cargo test --test live_e2e multi_pane_churn_keeps_attention_current -- --ignored --nocapture

# Run the high-value UX loop against real tmux instances.
dogfood:
  just tmux-plugin-live
  just ux-live-actions
  just ux-live-surfaces
  just ux-live-startup
  just ux-live-persistence
  just ux-live-navigation
  just ux-live-churn
  just perf-live

# Run the large-fleet interaction perf smoke.
perf-smoke:
  cargo test --lib large_fleet_presentation_perf_smoke -- --nocapture
  cargo test --lib input_loop_stays_below_human_lag_threshold -- --nocapture
  cargo test --lib navigation_key_burst_stays_in_memory_and_below_human_lag_threshold -- --nocapture
  cargo test --lib output_scroll_key_burst_stays_in_memory_and_below_human_lag_threshold -- --nocapture
  cargo test --lib scroll_render_perf_smoke_stays_smooth_for_focused_details_and_output -- --nocapture
  cargo test --lib renderer_navigation_perf_smoke_stays_interactive -- --nocapture

# Run the local performance loop.
perf: perf-smoke

# Run live tmux performance coverage for rapid fleet movement.
perf-live:
  cargo test --test live_e2e large_fleet_navigation_holds_up_with_twenty_panes -- --ignored --nocapture

# Show line and function coverage for the normal test suite.
coverage:
  command -v cargo-llvm-cov >/dev/null || { echo "Install coverage tooling with: cargo install cargo-llvm-cov --locked"; exit 1; }
  cargo llvm-cov --workspace --summary-only --ignore-filename-regex 'tests/|target/'

# Show coverage after both normal tests and live tmux e2e tests.
coverage-full:
  command -v cargo-llvm-cov >/dev/null || { echo "Install coverage tooling with: cargo install cargo-llvm-cov --locked"; exit 1; }
  cargo llvm-cov clean --workspace
  cargo llvm-cov --workspace --no-report
  cargo llvm-cov --test live_e2e --no-report -- --ignored --nocapture --test-threads=1
  cargo llvm-cov report --summary-only --ignore-filename-regex 'tests/|target/'

# Enforce the V1 coverage floor across normal tests and live tmux e2e tests.
coverage-full-gate:
  command -v cargo-llvm-cov >/dev/null || { echo "Install coverage tooling with: cargo install cargo-llvm-cov --locked"; exit 1; }
  cargo llvm-cov clean --workspace
  cargo llvm-cov --workspace --no-report
  cargo llvm-cov --test live_e2e --no-report -- --ignored --nocapture --test-threads=1
  cargo llvm-cov report --summary-only --ignore-filename-regex 'tests/|target/' --fail-under-lines 95 --fail-under-regions 95 --fail-under-functions 95

# Write uncovered source lines to target/llvm-cov/missing.txt.
coverage-missing:
  command -v cargo-llvm-cov >/dev/null || { echo "Install coverage tooling with: cargo install cargo-llvm-cov --locked"; exit 1; }
  mkdir -p target/llvm-cov
  cargo llvm-cov --workspace --text --show-missing-lines --ignore-filename-regex 'tests/|target/' --output-path target/llvm-cov/missing.txt

# Run clippy with warnings denied.
lint:
  cargo clippy --all-targets --all-features -- -D warnings

# Verify the TPM plugin entrypoint and helper scripts.
tmux-plugin-check:
  bash -n muxboard.tmux
  bash -n extras/tmux/scripts/muxboard-open
  bash -n extras/tmux/scripts/muxboard-mark-seen
  bash -n extras/tmux/scripts/muxboard-agent-state
  bash -n extras/tmux/scripts/muxboard-codex-notify
  bash -n extras/tmux/scripts/muxboard-status
  bash -n extras/tmux/scripts/muxboard-session-dots
  bash -n extras/tmux/scripts/muxboard-plugin-smoke
  test -x muxboard.tmux
  test -x extras/tmux/scripts/muxboard-open
  test -x extras/tmux/scripts/muxboard-mark-seen
  test -x extras/tmux/scripts/muxboard-agent-state
  test -x extras/tmux/scripts/muxboard-codex-notify
  test -x extras/tmux/scripts/muxboard-status
  test -x extras/tmux/scripts/muxboard-session-dots
  test -x extras/tmux/scripts/muxboard-plugin-smoke
  extras/tmux/scripts/muxboard-plugin-smoke

# Verify tmux plugin behavior against a real tmux server.
tmux-plugin-live:
  cargo test --test live_e2e tmux_plugin_dock_opens_full_height_sidebar_not_quadrant_split -- --ignored --nocapture
  cargo test --test live_e2e tmux_plugin_dock_adapts_width_across_window_sizes -- --ignored --nocapture
  cargo test --test live_e2e tmux_plugin_dock_toggle_closes_only_the_muxboard_sidebar -- --ignored --nocapture
  cargo test --test live_e2e tmux_plugin_dock_close_after_jump_env_still_reaches_muxboard -- --ignored --nocapture
  cargo test --test live_e2e tmux_plugin_drawer_preserves_layout_while_default_is_dock -- --ignored --nocapture
  cargo test --test live_e2e tmux_plugin_drawer_binding_targets_drawer_while_default_is_dock -- --ignored --nocapture
  cargo test --test live_e2e tmux_plugin_status_widgets_render_agent_names_and_custom_dots_live -- --ignored --nocapture
  cargo test --test live_e2e tmux_plugin_focus_marks_terminal_review_seen_live -- --ignored --nocapture
  cargo test --test live_e2e tmux_plugin_peek_toggle_closes_live_muxboard_popup_without_layout_change -- --ignored --nocapture
  cargo test --test live_e2e tmux_plugin_peek_toggle_honors_custom_tmux_prefix -- --ignored --nocapture
  cargo test --test live_e2e tmux_plugin_peek_toggle_honors_tmux_prefix2 -- --ignored --nocapture
  cargo test --test live_e2e tmux_plugin_drawer_close_after_jump_env_defaults_on -- --ignored --nocapture
  cargo test --test live_e2e tmux_plugin_dock_close_after_jump_closes_live_muxboard_pane -- --ignored --nocapture

# Verify release build and package assembly from the current tree.
package-check:
  cargo build --release --locked
  target/release/muxboard --version
  cargo package --allow-dirty --locked
  cargo package --allow-dirty --locked --list >/dev/null

# Verify GitHub wiring before a public push, tag, or release. Non-destructive.
github-preflight:
  command -v gh >/dev/null || { echo "Install GitHub CLI before release."; exit 1; }
  expected="aanari/muxboard"; \
  origin="$(git remote get-url origin 2>/dev/null || true)"; \
  case "$origin" in \
    git@github.com:aanari/muxboard.git|https://github.com/aanari/muxboard.git) ;; \
    "") echo "Missing origin remote. Expected git@github.com:aanari/muxboard.git"; exit 1 ;; \
    *) echo "Origin must point to aanari/muxboard, got: $origin"; exit 1 ;; \
  esac; \
  repo="$(gh repo view "$expected" --json nameWithOwner,visibility,isArchived,defaultBranchRef,url,description,homepageUrl,hasDiscussionsEnabled,repositoryTopics --jq '[.nameWithOwner,.visibility,(.isArchived|tostring),.defaultBranchRef.name,.url,.description,.homepageUrl,(.hasDiscussionsEnabled|tostring),([.repositoryTopics[].name] | sort | join(","))] | @tsv')"; \
  IFS=$'\t' read -r name visibility archived default_branch url description homepage discussions topics <<< "$repo"; \
  test "$name" = "$expected" || { echo "Unexpected GitHub repo: $name"; exit 1; }; \
  test "$visibility" = "PUBLIC" || { echo "$expected must be public before V1 release, got: $visibility"; exit 1; }; \
  test "$archived" = "false" || { echo "$expected must not be archived"; exit 1; }; \
  test -n "$default_branch" || { echo "$expected is missing a default branch"; exit 1; }; \
  test "$description" = "A tmux command center for AI agents, panes, and long-running terminal work." || { echo "$expected description is stale: $description"; exit 1; }; \
  test "$homepage" = "https://github.com/aanari/muxboard" || { echo "$expected homepage must stay canonical until Pages is verified, got: $homepage"; exit 1; }; \
  test "$discussions" = "true" || { echo "$expected should have Discussions enabled"; exit 1; }; \
  case ",$topics," in *,ai-agents,*) ;; *) echo "$expected topics should include ai-agents; got: $topics"; exit 1 ;; esac; \
  case ",$topics," in *,tmux,*) ;; *) echo "$expected topics should include tmux; got: $topics"; exit 1 ;; esac; \
  case ",$topics," in *,tui,*) ;; *) echo "$expected topics should include tui; got: $topics"; exit 1 ;; esac; \
  echo "GitHub repo ok: $url default=$default_branch homepage=$homepage"

# Create a private tmux server with synthetic panes for demos.
demo-start:
  scripts/demo-session start

# Attach to the synthetic muxboard demo UI.
demo-attach:
  scripts/demo-session attach

# Stop the private demo server.
demo-stop:
  scripts/demo-session stop

# Smoke-test the synthetic demo without recording.
demo-smoke:
  scripts/demo-session smoke

# Record a scripted asciinema demo into target/demo/muxboard.cast.
demo-record:
  scripts/demo-session record

# Convert target/demo/muxboard.cast to target/demo/muxboard.gif with agg.
demo-gif:
  scripts/demo-session gif

# Convert target/demo/muxboard.gif to target/demo/muxboard.mp4 with ffmpeg.
demo-mp4:
  scripts/demo-session mp4

# Render static demo and social-preview PNGs into target/demo/assets.
demo-assets:
  scripts/demo-session assets

# Render checked-in public PNG assets from their SVG sources.
public-assets:
  scripts/demo-session public-assets

# Verify the demo harness without requiring tmux.
demo-check:
  bash -n scripts/demo-session
  test -x scripts/demo-session

# Run the standard CI checks locally.
ci: fmt-check lint guards contracts test perf-smoke tmux-plugin-check goal-check demo-check

# Run the full local verification stack, including live tmux tests.
ci-full: fmt-check lint guards contracts test perf-smoke test-live tmux-plugin-check goal-check demo-check

# Run the V1 release confidence gate.
release-check: ci-full ux coverage-full-gate package-check dogfood

# Create a local safety snapshot before unattended agent work.
backup:
  stamp="$(date +%Y%m%d-%H%M%S)"; \
  backup="$HOME/Downloads/muxboard-backup-$stamp.zip"; \
  cd .. && zip -qr "$backup" muxboard -x 'muxboard/target/*' -x 'muxboard/.git/lfs/*' -x 'muxboard/.git/objects/pack/tmp_*'; \
  ls -lh "$backup"; \
  shasum -a 256 "$backup"

# List saved Codex goals.
goal-list:
  find docs/goals -maxdepth 1 -type f -name '*.md' ! -name 'README.md' -print | sed 's#^docs/goals/##; s#\.md$##' | sort

# Show a saved Codex goal.
goal-show name:
  goal="docs/goals/{{name}}.md"; \
  test -f "$goal" || { echo "Unknown goal: {{name}}"; just goal-list; exit 1; }; \
  cat "$goal"

# Load a saved Codex goal into the tmux paste buffer.
goal-buffer name:
  command -v tmux >/dev/null || { echo "tmux is required"; exit 1; }; \
  goal="docs/goals/{{name}}.md"; \
  test -f "$goal" || { echo "Unknown goal: {{name}}"; just goal-list; exit 1; }; \
  tmux load-buffer "$goal"; \
  echo "Loaded {{name}}. In Codex: /goal, then prefix + ]."

# Send a saved goal into the one visible Codex tmux pane.
goal-send name target="":
  scripts/codex-goal-send "{{name}}" "{{target}}"

# Run one saved Codex goal without the interactive TUI.
goal-run name:
  goal="docs/goals/{{name}}.md"; \
  test -f "$goal" || { echo "Unknown goal: {{name}}"; just goal-list; exit 1; }; \
  codex exec \
    --cd "$PWD" \
    --sandbox danger-full-access \
    --model gpt-5.5 \
    -c approval_policy=\"never\" \
    -c model_reasoning_effort=\"xhigh\" \
    - < "$goal"; \
  git diff --check

# Verify saved-goal helpers stay safe and available.
goal-check:
  bash -n scripts/codex-goal-send
  test -x scripts/codex-goal-send
  test -f docs/goals/README.md
  test -f docs/goals/agent-view.md
  test -f docs/goals/competitive-hardening.md
  test -f docs/goals/demo-polish.md

# Run one bounded Codex improvement pass. Review the diff before running again.
codex-autopass:
  codex exec \
    --cd "$PWD" \
    --sandbox danger-full-access \
    --model gpt-5.5 \
    -c approval_policy=\"never\" \
    -c model_reasoning_effort=\"xhigh\" \
    "$(cat docs/codex-autopass-prompt.md)"
  git diff --check

# Run a capped autonomous loop with the V1 goal gates. Stops after any pass leaves a reviewable diff.
codex-autoloop:
  status="$(git status --short)"; \
  test -z "$status" || { echo "Refusing codex-autoloop on a dirty tree:"; echo "$status"; exit 1; }
  passes="${PASSES:-3}"; \
  for i in $(seq 1 "$passes"); do \
    echo "== muxboard Codex autopass $i/$passes =="; \
    just codex-autopass; \
    just ux; \
    just ci; \
    just perf-live; \
    status="$(git status --short)"; \
    if [ -n "$status" ]; then \
      echo "codex-autoloop paused with a reviewable diff:"; \
      echo "$status"; \
      echo "Review, test, and commit before running another pass."; \
      exit 0; \
    fi; \
    git status --short; \
  done
