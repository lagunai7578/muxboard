# tmux plugin

Muxboard can be installed as a TPM plugin from this repo. A separate plugin repo
is optional; TPM only needs a GitHub repo with an executable `*.tmux` entrypoint,
and this repo now has `muxboard.tmux` at the root.

The plugin does not install the Rust binary. Install muxboard first:

```bash
cargo install --git https://github.com/aanari/muxboard --locked
```

Then add muxboard to your tmux config:

```tmux
set -g @plugin 'tmux-plugins/tpm'
set -g @plugin 'aanari/muxboard'
```

Press the TPM install key, usually `prefix` + `I`.

## Default behavior

Press `prefix` + `M` to open muxboard.

By default the plugin opens muxboard in a popup at the selected pane's current
directory. If the tmux server is too old for popups, it opens a normal tmux
window instead.

## Choose the shape by task

Use the peek drawer when you want to peek, act, and get back to work. It floats
over tmux, so it never changes pane sizes.

Use the sidebar when muxboard should stay beside your work. It is a real tmux
dock, so it pushes the workspace over and `prefix` + `M` closes it again.

Use a window for a persistent control room. Use split only when you want manual
tmux placement.

## Presets

Start with a preset. It is the easiest way to get the intended shape without
thinking about tmux coordinates.

Default command center:

```tmux
set -g @muxboard-open-preset 'center'
```

Peek drawer, floating:

```tmux
set -g @muxboard-open-preset 'drawer'
```

The peek drawer opens on the right. It floats over panes and never pushes or
resizes them. The show-in-tmux key, default `g`, shows the selected pane and
closes the peek drawer. Moving, searching, opening output, marking panes,
sending commands, refreshing, and Help stay inside muxboard.

Docked sidecar, real pane:

```tmux
set -g @muxboard-open-preset 'dock'
```

Dock opens muxboard as a real full-height tmux sidebar, left by default. It
pushes the current window content over and sizes itself to a useful width. Press
`prefix` + `M` again to close the dock. It is not split inside the currently
selected pane.

Peek drawer shortcut while sidebar is the default:

```tmux
set -g @muxboard-open-preset 'dock'
set -g @muxboard-drawer-key 'P'
```

Now `prefix` + `M` toggles the sidebar, while `prefix` + `P` toggles the peek
drawer. The peek drawer floats and never changes the tmux layout. The sidebar is
a real tmux layout and pushes the workspace over.

Persistent control room:

```tmux
set -g @muxboard-open-preset 'window'
```

Raw split:

```tmux
set -g @muxboard-open-preset 'split'
```

## Options

```tmux
# Default binding. Set @muxboard-bind off if you want to bind it yourself.
set -g @muxboard-key 'M'
set -g @muxboard-bind 'on'

# Optional peek drawer binding. Empty means no extra binding.
set -g @muxboard-drawer-key ''
set -g @muxboard-drawer-bind 'on'

# Command to run. Useful for local development or wrappers.
set -g @muxboard-command 'muxboard'
set -g @muxboard-extra-args ''

# center, drawer, top, bottom, left, right, dock, window, or split.
set -g @muxboard-open-preset 'center'

# popup, window, or split.
# Kept for backward compatibility. @muxboard-open-preset wins when set.
set -g @muxboard-open-mode 'popup'

# Popup placement and sizing.
set -g @muxboard-popup-placement 'center'
set -g @muxboard-popup-width '90%'
set -g @muxboard-popup-height '85%'

# Close muxboard after showing a pane in tmux. Drawer defaults this on.
set -g @muxboard-close-after-jump 'off'

# Mark terminal review events seen when you focus their pane in tmux.
set -g @muxboard-mark-seen-on-focus 'on'

# Window behavior.
set -g @muxboard-window-name 'muxboard'
set -g @muxboard-reuse-window 'on'

# Split behavior.
set -g @muxboard-split-percent '45'
set -g @muxboard-split-direction 'horizontal'

# Dock behavior.
set -g @muxboard-dock-side 'left'
set -g @muxboard-dock-width ''
set -g @muxboard-dock-percent ''

# Empty means use the selected pane's directory.
set -g @muxboard-start-directory ''
```

For local development without installing the binary:

```tmux
set -g @muxboard-command 'cargo run --manifest-path $HOME/Projects/muxboard/Cargo.toml --'
```

For a persistent dashboard window instead of a popup:

```tmux
set -g @muxboard-open-preset 'window'
set -g @muxboard-window-name 'muxboard'
set -g @muxboard-reuse-window 'on'
```

For a docked sidebar that pushes content over:

```tmux
set -g @muxboard-open-preset 'dock'
set -g @muxboard-dock-side 'left'
```

The dock chooses a useful width automatically: roughly 52 columns on normal
terminals, narrower on small terminals, and capped on very wide monitors.
Override only if you really want to:

```tmux
set -g @muxboard-dock-width '58'
```

To make that sidebar temporary after a jump, close muxboard after `g` shows the
selected pane:

```tmux
set -g @muxboard-open-preset 'dock'
set -g @muxboard-close-after-jump 'on'
```

The sidebar toggles with `prefix` + `M`. The peek drawer toggles with `prefix` +
`P` when `@muxboard-drawer-key` is set. Window is a persistent control room, and
split is the raw tmux split of the selected pane.

For raw split mode:

```tmux
set -g @muxboard-open-preset 'split'
set -g @muxboard-split-percent '40'
set -g @muxboard-split-direction 'horizontal'
```

## Ambient agent attention

Muxboard can also read explicit agent state from tmux itself. This is for agent
harness hooks that already know when a pane is running, waiting, done, or failed.
It makes waiting panes show up instantly in Fleet and Details without scraping
fragile terminal output.

From inside an agent pane:

```bash
~/.tmux/plugins/muxboard/extras/tmux/scripts/muxboard-agent-state \
  --agent codex \
  --state waiting \
  --summary "approval needed"
```

Hooks can also pass the small pieces that make a fleet legible without scraping
the screen:

```bash
~/.tmux/plugins/muxboard/extras/tmux/scripts/muxboard-agent-state \
  --agent codex \
  --state done \
  --summary "release ready" \
  --thread-name "Ship V1" \
  --progress "10/10 tests" \
  --log "all checks passed"
```

Clear the explicit state:

```bash
~/.tmux/plugins/muxboard/extras/tmux/scripts/muxboard-agent-state --state off
```

Codex notify hooks can call:

```bash
~/.tmux/plugins/muxboard/extras/tmux/scripts/muxboard-codex-notify permission-request
```

The state bridge is intentionally small: it stores pane-local values in tmux's
global environment, and muxboard treats them as authoritative. Supported states
are `running`, `waiting`, `done`, `error`, `stuck`, `idle`, and `off`.
Common hook aliases like `approval`, `blocked`, `tool-running`, `complete`, and
`interrupted` normalize to those same states.
Muxboard also understands tmux-agent-indicator style `TMUX_AGENT_PANE_*` state
keys, including unseen markers, so existing Claude, Codex, and OpenCode hooks
can feed the board and the ambient status line.
Terminal `done`, `error`, and `stuck` events are marked unseen by default so
they can surface as review/attention until you look or mute them. Use `--seen`
when a hook reports historical terminal state that should stay visible but not
flash for attention.
Opening Output in muxboard or using `G show` on one of those review events marks
the explicit tmux bridge event seen, so `#{muxboard_status}` and
`#{muxboard_session_dots}` stop flashing after you intentionally inspect it.
With the TPM plugin installed, focusing that pane directly in tmux does the same
for terminal review states. Waiting states keep showing until you answer them.

For a tiny ambient status indicator outside muxboard, add placeholders to your
tmux status line:

```tmux
set -g status-right '#{muxboard_session_dots} #{muxboard_status} | %H:%M'
```

`#{muxboard_status}` prints compact text like `mux ! codex` for one pane that
needs attention, `mux + claude` for one running pane, or `mux !2` / `mux run3`
when multiple panes are involved.
`#{muxboard_session_dots}` prints one ASCII mark per session: `!` needs you,
`+` is running, `*` is current, and `.` is quiet.
If every session is quiet, the dots still render because adding the placeholder
is an explicit request for session wayfinding.

You can customize the session-dot symbols and colors without changing the calm
default:

```tmux
set -g @muxboard-session-dots-attention '!'
set -g @muxboard-session-dots-running '+'
set -g @muxboard-session-dots-current '*'
set -g @muxboard-session-dots-quiet '.'

set -g @muxboard-session-dots-color ''
set -g @muxboard-session-dots-attention-color 'yellow'
set -g @muxboard-session-dots-running-color 'green'
set -g @muxboard-session-dots-current-color ''
set -g @muxboard-session-dots-quiet-color ''
```

Muxboard does not recolor panes or window titles by default. The plugin keeps
ambient signals opt-in so it does not fight your tmux theme.

## Manual install without TPM

From a local checkout:

```tmux
run-shell -b '$HOME/Projects/muxboard/muxboard.tmux'
```

Or bind the helper directly:

```tmux
bind-key M run-shell -b '$HOME/Projects/muxboard/extras/tmux/scripts/muxboard-open'
```

## Same repo or separate repo?

Use this repo for V1. It keeps the plugin versioned with the binary and lets TPM
users install the integration with one `@plugin` line.

A separate `muxboard-tmux` repo would only be worth it later if we want a tiny
plugin-only repo with release tags independent from the Rust app.
