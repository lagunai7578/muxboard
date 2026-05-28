# Contributing

The fastest way to stay out of trouble is to use the repo commands that already match CI.

## Local verification

```bash
just ci
```

That runs:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --test architecture_guards -- --nocapture`
- `cargo test fixtures -- --nocapture`
- `cargo test`

If you are changing boundaries or provider parsing, run the focused guard and contract layers directly first:

```bash
just guards
just contracts
```

If you touch live tmux behavior, run the live smoke suite too:

```bash
just test-live
```

That runs the ignored `tests/live_e2e.rs` coverage against real tmux servers with isolated XDG config and state.

If you are iterating on UX, run the tighter dogfood loop first:

```bash
just dogfood
```

That loop is intentionally narrow. It checks the default first screen, send-list clarity, deep-board navigation, the next-move path, output vs tmux focus behavior, attention recovery, review-send recovery, refresh recovery after real state changes, and multi-pane churn. When a manual dogfood session reveals a regression, the rule is simple: encode that exact user journey in `tests/live_e2e.rs`, then keep it in `just dogfood` if it is part of the default experience.

If you touch navigation, rendering, sorting, tmux event handling, runtime capture, or anything that might affect input latency, run performance explicitly:

```bash
just perf
```

That covers the input loop threshold, renderer navigation over dense fleets, and large-fleet presentation. For live tmux movement timing, run:

```bash
just perf-live
```

Performance is part of UX. Movement, focus changes, search typing, and opening Output should feel instant. Do not put tmux capture, metrics refresh, filesystem IO, process spawning, or expensive full-fleet recomputation on the direct keypress-to-render path.

If a miss suggests broader blind spots, run coverage as a diagnostic pass:

```bash
cargo install cargo-llvm-cov --locked
just coverage
just coverage-missing
```

For the merged normal-plus-live picture, run:

```bash
just coverage-full
```

Coverage is not a UX score. Use it to find unexercised files, branches, and error paths, then convert any meaningful gap into a unit test, fixture, renderer test, or live journey.

## Expectations

- Keep the default path simple. Do not add complexity to the first-run flow without a clear payoff.
- Prefer small, behavior-focused changes over speculative abstraction.
- If you change key handling, send-list behavior, command dispatch, or tmux integration, add or update tests.
- For pure logic, prefer unit tests in `src/app.rs` or the relevant module.
- For provider or runtime inference, prefer fixture-backed tests in `tests/fixtures/core/`.
- For presentation shaping, prefer fixture-backed tests in `tests/fixtures/app/`.
- If a regression is mostly about panel wording or hierarchy, add a panel fixture in `tests/fixtures/app/panels.json` before reaching for a broader e2e test.
- For real interaction changes, prefer a small live test in `tests/live_e2e.rs`.
- Contributions are accepted under the Apache-2.0 license used by this repository.

## Useful commands

```bash
just fmt
just lint
just guards
just contracts
just test
just dogfood
just perf
just perf-smoke
just perf-live
just coverage
just coverage-full
just coverage-missing
just test-live
just ci
just ci-full
```

## Release checklist

Before calling a build release-shaped, run this stack from a clean working tree if practical:

```bash
just release-check
```

That covers formatting, clippy, unit tests, UX guardrails, renderer goldens, coverage gates, live tmux e2e, dogfood journeys, release build, and package verification. The public release flow lives in [`docs/release.md`](docs/release.md).

## Live tmux tests

The live suite is intentionally small and high-value. It should verify user-facing behavior, not every pixel on screen.

Good live assertions:

- a multi-pane send is reviewed before it is sent,
- the next move advances a waiting pane,
- output and tmux focus do different things,
- a restored saved fleet can really receive a broadcast.

Bad live assertions:

- brittle full-screen snapshots,
- exact spacing that does not matter to behavior,
- timing assumptions tighter than a human user would produce.
