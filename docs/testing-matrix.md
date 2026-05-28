# Testing matrix

This is the practical coverage map for muxboard's critical journeys and layers.

## Architecture guards

Files:

- `tests/architecture_guards.rs`

Protects:

- `core` must not depend on `app`, `tui`, or `tmux`.
- `tui` must not reach into `core` or `tmux`, even if it is later split into a
  `src/tui/` module tree.
- `app` must not import terminal widget crates.
- `app` must use the top-level `crate::core` API, not core internals.
- process spawning must stay in IO boundary modules: `tmux`, `metrics`, and
  `notifications`.
- macOS desktop assumptions must stay behind the notification boundary.
- public copy, app fixtures, and TUI golden grids must avoid retired product
  terms.

## Core contract fixtures

Files:

- `tests/fixtures/core/provider_contracts.json`
- `tests/fixtures/core/runtime_streams.json`
- `src/core/tests.rs`

Protects:

- provider detection,
- status inference,
- native Codex and Claude Code transcript hints,
- synthesized reports,
- summary generation,
- runtime chunk buffering,
- partial-line visibility rules.

## App view-model fixtures

Files:

- `tests/fixtures/app/view_models.json`
- `tests/fixtures/app/panels.json`
- `src/app/tests.rs`

Protects:

- board title composition,
- header context messaging,
- footer/help affordances,
- representative board row summaries,
- selected-pane panel hierarchy,
- live-tail empty states,
- navigator empty states,
- actions and overview panel sections,
- review send panel hierarchy.

## TUI golden-grid fixtures

Files:

- `tests/fixtures/tui/golden/*.txt`
- `src/tui.rs`

Protects:

- exact cell-level output for critical screens,
- terminal fallback rendering for ASCII/no-color server-like environments,
- first-load hierarchy,
- narrow selected-agent layout,
- output, actions, overview, working agents, mixed fleets, mixed-fleet
  movement, off-selection attention, multi-attention queues, command, search,
  and empty states,
- accidental spacing, truncation, footer, or wording regressions.

These are intentionally stricter than normal renderer tests. If a golden changes,
review the full screen like a product surface, not just a test diff.

Commands:

- `just tui-golden` checks exact rendered grids.
- `just tui-golden-bless` refreshes the fixtures after a deliberate UI review.

## App behavior tests

Files:

- `src/app/tests.rs`
- `src/app/presentation.rs` unit tests

Protects:

- send-list behavior,
- marking,
- saved fleet persistence behavior,
- muted alerts,
- send confirmation,
- board projection,
- width-aware presentation behavior.

## CLI smoke tests

Files:

- `tests/cli_smoke.rs`

Protects:

- binary startup,
- config dumping,
- default keybinding dumping,
- general CLI health.

## Live tmux e2e

Files:

- `tests/live_e2e.rs`

Protects:

- real tmux integration,
- search,
- selection,
- send-list send-list behavior,
- review dispatch safety,
- output/tmux focus behavior,
- tmux-native start into a new agent window,
- alert-muting flows,
- same-server behavior,
- SSH-like server terminal compatibility,
- resize churn,
- large fleet navigation,
- tmux plugin dock/sidebar geometry, adaptive width, peek drawer behavior, and
  toggle behavior.

Live recipe commands:

- `just ux-live-actions` covers real keypresses, dispatch, focus, launch, and
  tmux side effects.
- `just ux-live-surfaces` covers first-screen hierarchy and terminal rendering
  across local, narrow, and SSH-like profiles.
- `just ux-live-startup` covers recoverable first-run and broken-config states.
- `just ux-live-persistence` covers restart-backed state.
- `just ux-live-navigation` covers filters, visible targets, and deep selection
  scrolling.
- `just ux-live-churn` covers resize, carriage-return progress, and changing
  attention state.
- `just tmux-plugin-live` covers the TPM helper against real tmux pane geometry,
  adaptive dock sizing, peek drawer toggle safety, close-after-jump wiring, and
  sidebar toggle behavior.
- `just dogfood` runs the named live recipes plus live performance.

## Performance gates

Performance is part of usability. The local perf loop protects:

- input polling below the human lag threshold,
- queued movement keys draining in bursts,
- renderer navigation over dense fleets,
- large-fleet presentation work staying interactive.

Commands:

- `just perf` runs the local performance loop.
- `just perf-smoke` runs the local perf tests directly.
- `just perf-live` runs the live tmux rapid fleet movement journey.

Use `just perf-smoke` for renderer, navigation, sorting, tmux-event, runtime-capture,
and input-loop changes. Use `just perf-live` when a change can affect real tmux
movement timing.

## Local commands

- `just guards`
- `just contracts`
- `just tui-golden`
- `just tui-golden-bless`
- `just test`
- `just test-live`
- `just ux-live-actions`
- `just ux-live-surfaces`
- `just ux-live-startup`
- `just ux-live-persistence`
- `just ux-live-navigation`
- `just ux-live-churn`
- `just tmux-plugin-live`
- `just dogfood`
- `just perf`
- `just perf-smoke`
- `just perf-live`
- `just coverage`
- `just coverage-full`
- `just coverage-full-gate`
- `just release-check`
- `just coverage-missing`
- `just ci`
- `just ci-full`

## Coverage diagnostics

Coverage is an X-ray, not a product-quality score. It helps find whole functions,
branches, files, and error paths that no test exercises. It does not prove the TUI
is understandable, beautiful, or free of stale state. Renderer tests and live tmux
journeys still carry that responsibility.

Commands:

- `just coverage` runs the normal suite under `cargo-llvm-cov`.
- `just coverage-full` runs the normal suite, then the live tmux e2e suite
  serially, then reports merged coverage.
- `just coverage-full-gate` enforces the V1 floor: 95% total lines, 95% total
  regions, and 95% total functions across source files.
- `just coverage-missing` writes uncovered source lines to
  `target/llvm-cov/missing.txt`.

Install the tool with:

```bash
cargo install cargo-llvm-cov --locked
```

Use uncovered lines as review prompts. The important question is not "can we make
the percentage higher?" It is "does this uncovered path represent a real user
journey, dangerous state transition, provider parser edge, or tmux failure mode?"

The gate is intentionally project-level, not per-file. A few thin boundary files
and helper-heavy surfaces can sit below 95% on per-file function or region
coverage while their user-facing paths are covered elsewhere:

- `lib.rs`: real terminal startup is covered by CLI smoke and live tmux journeys,
  not by unit tests that would open the user's terminal.
- `config.rs` and `state.rs`: public store methods are exercised through app,
  CLI, persistence, and live restart flows; the remaining misses are mostly
  duplicate wrappers over the same tested load/save helpers.
- `app/targets.rs` and `tui.rs`: Rust function counts include many tiny helper
  and rendering functions. The meaningful guard is behavior: action-contract,
  renderer/X-ray, golden-grid, performance, and live tmux tests.

These are exceptions to per-file percentage chasing, not exceptions to testing.
If `coverage-missing` exposes an uncovered path that changes user-visible state,
tmux dispatch, persistence, provider parsing, or rendering hierarchy, add a real
regression test at the right layer.

## Coverage philosophy

Muxboard is safest when every bug becomes one of these:

1. an architecture guard,
2. a core transcript fixture,
3. an app view-model fixture,
4. or a live e2e journey.

That is the loop that turns dogfooding into durable reliability.
