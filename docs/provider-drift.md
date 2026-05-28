# Provider drift notes

Muxboard infers agent state from terminal text. That makes provider output drift the main long-term integration risk.

These are the current high-value surfaces we intentionally rely on.

## Codex

Structured and state-like lines:

- `STATUS=... | BLOCKER=... | NEXT=...`
- `muxboard: status=...; blocker=...; next=...`
- `Pending init`
- `waitingOnApproval`
- `Running`
- `Interrupted`
- `Completed ...`
- `Error ...`
- `Shutdown`
- `Not found`
- `Waiting for ...`

Tool and failure markers:

- `apply_patch`
- `update_plan`
- generic harness output from `node`, `python`, `zsh`, or shell wrappers that
  contains Codex state or tool names
- `agent spawn failed`
- `agent interaction failed`
- `agent resume failed`
- `agent close failed`
- `waitingOnApproval`

## Claude Code

Waiting and prompt markers:

- `approve ...`
- `worker request`
- `sandbox request`
- `dialog open`
- `input needed`
- `Answer questions?`
- `Choose [`
- `Waiting for ... approve network access ...`

Progress and recovery markers:

- `User has answered your questions`
- `Conversation compacted`
- `Tool ... running for ...`
- `Tool '...' still running (...)`
- generic harness output from `node`, `python`, `zsh`, or shell wrappers that
  contains Claude prompt, dialog, worker, sandbox, or tool-running text

## Opencode

Waiting and interaction markers:

- `permission.asked`
- `question.asked`
- `Permission required`
- `Question`
- `Select one answer`
- `Select all answers that apply`
- `Type your answer...`

Continuation and failure markers:

- `permission.replied`
- `question.replied`
- `question.rejected`
- generic harness output from `node`, `python`, `zsh`, or shell wrappers that
  contains opencode event names, including JSON event payloads

## What to do when drift happens

1. Capture the smallest sanitized transcript that reproduces the change.
2. Add it to `tests/fixtures/core/provider_contracts.json` or `tests/fixtures/core/runtime_streams.json`.
3. Decide whether the new output means:
   - a new provider state,
   - a renamed provider state,
   - or UI noise that should be ignored.
4. Update only the relevant core provider/report code.
5. Re-run:
   - `just guards`
   - `just contracts`
   - `just test-live`

The goal is not to memorize provider quirks. The goal is to turn every newly observed quirk into a permanent regression fixture.

## Fixture checklist

Provider contract fixtures should cover both direct CLI commands and generic
harness commands like `node` or `python` when the provider identity only appears
in output. For each high-value drift case, assert all four surfaces:

- workload detection,
- pane status,
- board summary,
- synthesized report.

Runtime stream fixtures should cover terminal mechanics that can corrupt provider
signals before parsing:

- carriage-return rewrites,
- backspace repairs,
- meaningful partial lines,
- hidden single-character echoes.
