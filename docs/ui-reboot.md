# muxboard UI reboot notes

These notes record the design constraints behind muxboard's current TUI shell. The tmux integration, fleet discovery, attention detection, send-list behavior, review dispatch, and live tests are the engine worth protecting.

The shell goals were:

- reduce chrome that does not carry meaning,
- strengthen hierarchy between Fleet, Details, and commands,
- keep advanced views secondary until the user asks for them,
- express state through layout and priority instead of label clutter.

The reboot kept the behavior layer and reshaped the visible TUI shell.

## External references

These are the main references for the reboot:

- Ratatui showcase and app examples: official source for current Ratatui app patterns.
  - https://ratatui.rs/showcase/
  - https://github.com/ratatui/ratatui
- `crates-tui`: a Ratatui reference app for small to medium TUI organization, screenshots, help, and mode-driven actions.
  - https://github.com/ratatui/crates-tui/
- `ccboard`: a current Rust TUI in the same broad space as muxboard, with command palette, contextual help, breadcrumbs, vim keys, and dense information design.
  - https://github.com/florianbruniaux/ccboard

## What we are keeping

- tmux probe and snapshot logic.
- pane insight inference.
- attention queue and alert-muting behavior.
- send list, review dispatch, and saved fleets.
- jump, zoom, enter, yes, no, summaries, refresh, and action handling.
- unit tests and live tmux e2e behavior checks.

## What changed

- the top bar became one quiet line,
- the footer became stable key guidance,
- the main split layout became Fleet plus Details,
- panel cycling stopped being required for basic navigation,
- visible terminology moved toward ordinary words,
- Fleet rows became optimized for quick scanning.

## Product shape

Muxboard should feel like a command center, not a widget buffet.

Default shell:

1. Header.
   - one quiet line.
   - brand on the left.
   - current selection and fleet summary on the right.
   - no instruction soup in the header.

2. Fleet.
   - primary focus of the app.
   - optimized for scanning.
   - rows answer: what is this, does it need me, what is it doing now.

3. Details.
   - always visible.
   - wide enough to be useful.
   - shows only the selected pane and the next recommended move.

4. Footer.
   - always visible.
   - stable keys only.
   - `?` opens help.
   - status messages may supplement hints, not strand the user.

5. Overlays.
   - help.
   - More.
   - command / review send.
   - Browse, Command Center, or Output when needed.

Advanced modes should appear as overlays or intentional secondary views, not as five equal tabs competing for attention.

## Layout rules

- Default split should favor Fleet, but not starve Details. Start around 55/45.
- Fleet owns the left side.
- Details owns the right side.
- On short or narrow terminals, stack Fleet over Details.
- Header and footer stay one line each.

## Naming rules

- Prefer ordinary words over internal words.
- "Details" beats "Inspect".
- "Send" or "Send list" beats "Targets".
- "Show" beats "Jump" in visible copy. The key can still be `g`.
- "Browse" beats "Navigator".
- "Command Center" beats "Control".

## Fleet rules

Every row should scan left to right like this:

- attention marker.
- pane location.
- state.
- latest useful output or report summary.

Avoid exposing internal fields unless the user asks for them.

Default Fleet columns should be minimal:

- marker.
- where.
- state.
- latest.

Wider terminals can add secondary columns, but the compact shape is the canonical shape.

## Details rules

Details should answer five questions in this order:

1. What is selected?
2. What state is it in?
3. What tool is running there?
4. What should I do next?
5. What is the latest useful output?

Everything else is secondary.

## Footer rules

Default footer should teach the app by being present:

- `? help`
- `J/K move`
- `Enter output` from Fleet or Details, and `Esc back` from Output.
- `G show`
- `Space add/remove`
- `: send`
- `. more`
- `/ filter`
- `Q quit`

If a status message exists, keep recovery keys visible. On roomy terminals, the
message may supplement the keymap; it should not replace the user's way out.

## Help rules

Help is a modal, not a hidden concept.

- Opens with `?`.
- Uses a few short task-oriented lines.
- Says what the app does, not what the implementation calls things.
- No dense legend wall unless there is no better way.

## Navigation rules

- One primary selection model.
- J/K moves Fleet selection by default.
- When Details or Output is focused and scrollable, J/K scrolls that surface.
- Enter always means "show more here": Details opens Output, Output returns to Details, Browse opens the selected window.
- `g` always means "show this in tmux".
- `:` always means "prepare a send".
- `.` always means "open secondary actions".
- `Tab` should not be required for basic use.

## Anti-goals

Do not rebuild the current shell with different words.

Do not expose every internal mode in the default view.

Do not make the user remember which panel is active before basic navigation works.

Do not let the header become a cheat sheet again.

## Implementation slice that guided the reboot

1. Introduce a new shell model in the presentation layer.
2. Render a fixed Fleet + Details layout.
3. Move advanced views behind overlays or explicit secondary actions.
4. Keep the old behavior engine intact underneath.
5. Re-run unit, clippy, and live tmux e2e checks after each slice.
