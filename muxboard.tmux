#!/usr/bin/env bash
set -euo pipefail

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HELPER="$CURRENT_DIR/extras/tmux/scripts/muxboard-open"
MARK_SEEN_HELPER="$CURRENT_DIR/extras/tmux/scripts/muxboard-mark-seen"
STATUS_HELPER="$CURRENT_DIR/extras/tmux/scripts/muxboard-status"
DOTS_HELPER="$CURRENT_DIR/extras/tmux/scripts/muxboard-session-dots"

tmux_get_option() {
    tmux show-option -gqv "$1" 2>/dev/null || true
}

tmux_set_default() {
    local option="$1"
    local value="$2"

    if [ -z "$(tmux_get_option "$option")" ]; then
        tmux set-option -gq "$option" "$value"
    fi
}

tmux_option_or_default() {
    local option="$1"
    local default="$2"
    local value

    value="$(tmux_get_option "$option")"
    printf "%s" "${value:-$default}"
}

shell_quote() {
    printf "%q" "$1"
}

enabled() {
    local normalized

    normalized="$(printf "%s" "${1:-}" | tr "[:upper:]" "[:lower:]")"
    case "$normalized" in
        0 | false | off | no) return 1 ;;
        *) return 0 ;;
    esac
}

do_interpolation() {
    local value="$1"

    value="${value//\#\{muxboard_status\}/#($STATUS_HELPER)}"
    value="${value//\#\{muxboard_session_dots\}/#($DOTS_HELPER '#S')}"
    printf "%s" "$value"
}

update_tmux_option() {
    local option="$1"
    local value

    value="$(tmux_get_option "$option")"
    if [ -n "$value" ]; then
        tmux set-option -gq "$option" "$(do_interpolation "$value")"
    fi
}

unregister_hook_matches() {
    local hook_type="$1"
    local match="${2:-$CURRENT_DIR}"
    local existing_hooks

    existing_hooks="$(tmux show-hooks -g "$hook_type" 2>/dev/null || true)"
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        local existing_name
        existing_name="${line%% *}"
        tmux set-hook -gu "$existing_name" 2>/dev/null || true
    done < <(printf "%s\n" "$existing_hooks" | grep -F "$match" || true)
}

register_hook_once() {
    local hook_type="$1"
    local command="$2"
    local match="${3:-$CURRENT_DIR}"

    unregister_hook_matches "$hook_type" "$match"
    tmux set-hook -ag "$hook_type" "$command"
}

register_focus_seen_hooks() {
    local command

    command="run-shell -b \"$(shell_quote "$MARK_SEEN_HELPER") --pane \\\"#{pane_id}\\\"\""
    register_hook_once "pane-focus-in" "$command"
    register_hook_once "after-select-pane" "$command"
    register_hook_once "after-select-window" "$command"
}

tmux_set_default "@muxboard-key" "M"
tmux_set_default "@muxboard-bind" "on"
tmux_set_default "@muxboard-drawer-key" ""
tmux_set_default "@muxboard-drawer-bind" "on"
tmux_set_default "@muxboard-mark-seen-on-focus" "on"

update_tmux_option "status-right"
update_tmux_option "status-left"
update_tmux_option "@minimal-tmux-status-right"
update_tmux_option "@minimal-tmux-status-left"
update_tmux_option "@minimal-tmux-status-right-extra"
update_tmux_option "@minimal-tmux-status-left-extra"

if enabled "$(tmux_option_or_default "@muxboard-bind" "on")"; then
    tmux bind-key "$(tmux_option_or_default "@muxboard-key" "M")" run-shell -b "$(shell_quote "$HELPER")"
fi

if enabled "$(tmux_option_or_default "@muxboard-mark-seen-on-focus" "on")"; then
    register_focus_seen_hooks
else
    unregister_hook_matches "pane-focus-in"
    unregister_hook_matches "after-select-pane"
    unregister_hook_matches "after-select-window"
fi

drawer_key="$(tmux_get_option "@muxboard-drawer-key")"
if enabled "$(tmux_option_or_default "@muxboard-drawer-bind" "on")" && [ -n "$drawer_key" ]; then
    tmux bind-key "$drawer_key" run-shell -b "$(shell_quote "$HELPER") --toggle-peek"
fi
