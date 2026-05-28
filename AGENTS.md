## UX Principles: How Users Actually Behave

These principles govern how real humans interact with interfaces. They are observed
behavior, not preferences. Apply them before, during, and after every design decision.

### The Three Laws of Usability

1. **Don't make me think.** Every page should be self-evident. If a user stops
   to think "What do I click?" or "What does this mean?", the design has failed.
   Self-evident > self-explanatory > requires explanation.

2. **Clicks don't matter, thinking does.** Three mindless, unambiguous clicks
   beat one click that requires thought. Each step should feel like an obvious
   choice (animal, vegetable, or mineral), not a puzzle.

3. **Omit, then omit again.** Get rid of half the words on each page, then get
   rid of half of what's left. Happy talk (self-congratulatory text) must die.
   Instructions must die. If they need reading, the design has failed.

### How Users Actually Behave

- **Users scan, they don't read.** Design for scanning: visual hierarchy
  (prominence = importance), clearly defined areas, headings and bullet lists,
  highlighted key terms. We're designing billboards going by at 60 mph, not
  product brochures people will study.
- **Users satisfice.** They pick the first reasonable option, not the best.
  Make the right choice the most visible choice.
- **Users muddle through.** They don't figure out how things work. They wing
  it. If they accomplish their goal by accident, they won't seek the "right" way.
  Once they find something that works, no matter how badly, they stick to it.
- **Users don't read instructions.** They dive in. Guidance must be brief,
  timely, and unavoidable, or it won't be seen.

### Billboard Design for Interfaces

- **Use conventions.** Logo top-left, nav top/left, search = magnifying glass.
  Don't innovate on navigation to be clever. Innovate when you KNOW you have a
  better idea, otherwise use conventions. Even across languages and cultures,
  web conventions let people identify the logo, nav, search, and main content.
- **Visual hierarchy is everything.** Related things are visually grouped. Nested
  things are visually contained. More important = more prominent. If everything
  shouts, nothing is heard. Start with the assumption everything is visual noise,
  guilty until proven innocent.
- **Make clickable things obviously clickable.** No relying on hover states for
  discoverability, especially on mobile where hover doesn't exist. Shape, location,
  and formatting (color, underlining) must signal clickability without interaction.
- **Eliminate noise.** Three sources: too many things shouting for attention
  (shouting), things not organized logically (disorganization), and too much stuff
  (clutter). Fix noise by removal, not addition.
- **Clarity trumps consistency.** If making something significantly clearer
  requires making it slightly inconsistent, choose clarity every time.

### Navigation as Wayfinding

Users on the web have no sense of scale, direction, or location. Navigation
must always answer: What site is this? What page am I on? What are the major
sections? What are my options at this level? Where am I? How can I search?

Persistent navigation on every page. Breadcrumbs for deep hierarchies.
Current section visually indicated. The "trunk test": cover everything except
the navigation. You should still know what site this is, what page you're on,
and what the major sections are. If not, the navigation has failed.

### The Goodwill Reservoir

Users start with a reservoir of goodwill. Every friction point depletes it.

**Deplete faster:** Hiding info users want (pricing, contact, shipping). Punishing
users for not doing things your way (formatting requirements on phone numbers).
Asking for unnecessary information. Putting sizzle in their way (splash screens,
forced tours, interstitials). Unprofessional or sloppy appearance.

**Replenish:** Know what users want to do and make it obvious. Tell them what they
want to know upfront. Save them steps wherever possible. Make it easy to recover
from errors. When in doubt, apologize.

## Muxboard Product Discipline

Muxboard is a tmux-first command center for AI agent fleets. V1 has no VCS, git,
hg, branch, or worktree awareness. It should feel like htop for tmux plus a
conductor-like command center: immediately legible, calm, safe, and useful.

Every non-trivial pass must name the role it is serving:

- **Product:** Does this help users command and understand an agent fleet?
- **Design:** Is the screen obvious, calm, beautiful, and sparse?
- **Engineering:** Can UI, tmux IO, provider intelligence, and app state evolve
  independently?
- **QA:** Can a real user complete the critical journey without explanation?
- **Release:** Are checks, risk, docs, and remaining concerns clear?

Autonomous or deep work must stay bounded. Improve one surface at a time. Do not
broaden V1 scope, add VCS awareness, rewrite the UI architecture casually, or let
unreviewed commits pile up past the pending-review limit.

## Evidence-First QA

Do not describe UI issues from memory. Capture the exact visible state:

- critical user journey,
- terminal size,
- mode or overlay,
- selected/focused pane,
- actual rendered result,
- expected rendered result,
- severity and confidence,
- regression test that should prevent a repeat.

For muxboard, renderer/X-ray grids are screenshots. Live tmux E2E captures are
strongest when command dispatch, focus, pane selection, tmux control mode, or
stale state is involved.

## Regression Rule

If a bug escaped once, the fix must add or strengthen a regression test at the
right layer: renderer/X-ray, provider/parser, app-state, architecture guard, or
live tmux E2E. If no useful regression test is possible, record why.

Fixes should be atomic: one user-visible issue, one coherent change, one local
commit when committing is appropriate. If verification reveals a regression,
revert or repair before moving on.

## Escaped-Bug Proactive Loop

When the user catches something obvious, assume the system missed a class of
bugs. Do not stop at the local fix. Convert the miss into:

- a product invariant in plain language,
- a source-level architecture guard when the failure came from code shape,
- a renderer/X-ray test when the failure was visible,
- an action-contract test when the failure involved a key or promise,
- a live tmux test when the failure involved real pane control or stale state,
- a just recipe that keeps the guard in `just ux`, `just ci`, or `just dogfood`.

The goal is not "more tests"; it is fewer ways for a future change to re-break
the same user journey without failing loudly.

## Action Contract QA

Screenshots are not enough. If a footer, help line, menu row, or status hint
advertises a key, a test should press that key and prove the promised action
happened.

- Forward keys must move forward or commit. `Enter` opens, applies, loads, or
  sends; it must not secretly become a back key.
- `Esc` backs out or cancels one layer. It should recover from Output, Send,
  Browse, Command Center, Help, review, and text inputs without losing the user's
  place or send list.
- Footer truth is a contract: if the footer says `Enter output`, pressing Enter
  must show Output; if it says `Esc back`, pressing Esc must go one layer up; if
  a key is inert or surprising in that state, do not advertise it.
- Help truth is a contract too. If Help says a key closes, backs out, or quits,
  that key must work while Help is open.
- More-menu truth is the same contract. If `.` exposes a row, a test should
  press that row's key and prove the visible result or tmux side effect.
- Rebound keys must be real, not cosmetic. If copy updates for custom bindings,
  tests should press the rebound key and prove the old default key is inert.
- Prefer sequence tests over static assertions for interaction bugs. Cover
  flows like `Enter Enter Esc Esc`, `/ query Enter backspace`, `Space J Space :
  text Enter Esc`, `. key Esc`, and real tmux send/jump/launch actions.
- Use `just ux-actions` for fast key-router and fake-tmux action contracts. Use
  `just ux-live-actions` when command dispatch, tmux focus, pane selection, or
  launch behavior must be proven against live tmux.

## Pass Closeout

Every meaningful pass should end with a compact status:

```text
STATUS: DONE | DONE_WITH_CONCERNS | BLOCKED
EVIDENCE: checks or captures used
RISK: low | medium | high
NEXT: one concrete next action
```

Do not say something is verified unless the check actually ran.

## TUI Renderer Verification

- Treat the renderer as a first-class product surface, not just a thin view layer.
  Verify real rendered grids, not only string-producing helpers.
- For any meaningful TUI change, add or update renderer-level tests that prove the
  visible result at the cell level: hierarchy, truncation, spacing, focus, and
  what survives on narrow or short terminals.
- Prefer deterministic one-row-per-line rendering over accidental paragraph
  wrapping. If a line is too long, decide how it should compress and test that
  exact behavior.
- Test stressful states, not just happy paths: narrow widths, short heights,
  dense fleets, overlays, active input modes, rebound keys, and conflicting
  attention states.
- Keep critical journeys backed by golden-grid fixtures in
  `tests/fixtures/tui/golden/`. If a golden changes, inspect the whole screen for
  hierarchy, noise, spacing, copy, and wayfinding before accepting the update.
- Use `just tui-golden` for the fast renderer snapshot loop. Only run
  `just tui-golden-bless` after intentionally reviewing and approving the new
  screen output.
- Use `just ux` before closing meaningful UI work. It runs the AGENTS-driven copy,
  wayfinding, renderer-focus, truncation, and golden-grid guardrails.

## Performance is Usability

- Movement, focus changes, search typing, and opening Output must feel instant.
  If a technical user can feel lag, treat it as a product bug, not polish.
- Simple navigation must stay in-memory. Do not put tmux capture, metrics refresh,
  filesystem IO, process spawning, network calls, or expensive full-fleet
  recomputation on the direct keypress-to-render path.
- Input polling should remain below the human lag threshold, and queued movement
  keys should drain in bursts instead of one slow frame at a time.
- Dirty tmux capture, metrics, provider parsing, and notification work should be
  budgeted, incremental, or idle-time work whenever practical.
- Any TUI, navigation, sorting, rendering, tmux-event, or runtime-capture change
  should run `just perf-smoke` before closeout. If live tmux movement, resize, or
  dense fleet behavior could be affected, run `just perf-live` or `just dogfood`.
- Performance gates belong in the same product loop as usability: `just ux`,
  `just ci`, and `just release-check` must keep exercising the local perf smoke.

## Coverage X-Ray

- When a miss exposes a blind spot, run coverage instead of only patching the
  local symptom.
- Use `just coverage` for normal line/function coverage, `just coverage-full` for
  normal plus live tmux e2e coverage, and `just coverage-missing` to write
  uncovered lines to `target/llvm-cov/missing.txt`.
- Treat coverage as a map of risk, not a score to game. Any uncovered path that
  affects a critical user journey, stale state, command dispatch, provider parsing,
  tmux IO, persistence, or rendering hierarchy needs a real regression test.

## Deep Bounded Passes

Use normal effort for implementation, test writing, refactors, and the edit-run
loop. Use extra-deep reasoning only for bounded passes where exhaustive reasoning
beats speed.

Good deep-pass work:

- find architectural blind spots across the whole repo,
- design a next-generation TUI interaction model,
- audit critical user journeys end to end,
- root-cause subtle state, tmux, parser, or renderer bugs,
- review whether tests protect the right product behavior.

Avoid deep passes for:

- small local edits,
- formatting,
- routine test additions,
- broad unbounded polishing,
- chasing coverage points without a user-risk reason.

Every deep pass should be bounded by one sentence:

> Audit `<surface>` for `<risk>` and make only fixes that directly reduce that
> risk.

Use this loop:

1. State the surface and risk.
2. Inspect the relevant code, fixtures, docs, and tests.
3. Name the biggest blind spots before editing.
4. Make the smallest durable fix.
5. Add or update tests at the right layer.
6. Run the narrow check, then the broad check.
7. Record what remains.

Recommended passes:

- **Renderer golden integrity.** Do golden fixtures act as the single reviewed
  source of truth for critical screens?
- **Critical user journeys.** Can a technical user succeed by scanning, without
  reading docs or understanding implementation words?
- **Architecture blind spots.** Can UI, tmux IO, provider intelligence, and app
  state evolve independently without breaking each other?
- **Provider intelligence drift.** If Codex, Claude Code, or opencode changes
  surface text, do we fail obviously and locally?
- **Tmux failure modes.** Do dangerous tmux actions preserve muxboard, target the
  intended pane, and fail recoverably?
