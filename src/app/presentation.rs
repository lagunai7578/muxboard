use super::*;
use crate::core::{classify_fallback_summary, is_terminal_chatter_noise};
use std::{
    cell::RefCell,
    collections::{HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    path::Path,
};

const BOARD_LATEST_DETAIL_MAX_CHARS: usize = 180;

thread_local! {
    static BOARD_LATEST_DETAIL_CACHE: RefCell<Option<CachedBoardLatestDetail>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug)]
struct CachedBoardLatestDetail {
    key: u64,
    value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellPanel {
    Theme,
    Selected,
    Send,
    Launch,
    Fleets,
    Output,
    Browse,
    Overview,
    Actions,
}

impl App {
    pub fn panes_title(&self) -> String {
        let mut parts = vec![String::from("Panes")];

        if !self.search_query.is_empty() {
            parts.push(format!("search: {}", self.search_query));
        }
        if self.search_input_active {
            parts.push(String::from("typing"));
        }
        if self.command_input_active {
            parts.push(format!("cmd: {}", self.command_buffer));
        }
        if self.group_input_active {
            parts.push(format!("fleet: {}", self.group_name_buffer));
        }
        if self.macro_assign_active {
            parts.push(String::from("pin slot"));
        }
        if !self.marked_pane_ids.is_empty() {
            parts.push(format!(
                "send list {}",
                pane_count_label(self.marked_pane_ids.len())
            ));
        }
        if let Some(group_name) = &self.active_group_name {
            parts.push(format!("fleet {group_name}"));
        }
        if self.using_marked_targets() {
            parts.push(String::from("send list"));
        }
        if self.fanout_mode == FanoutMode::Lane {
            parts.push(String::from("lane send"));
        }

        parts.join(" | ")
    }

    pub fn refresh_count(&self) -> u64 {
        self.refresh_count
    }

    pub fn title(&self) -> &str {
        "muxboard"
    }

    pub fn help_overlay_title(&self) -> String {
        String::from("Help")
    }

    pub fn tmux_version(&self) -> &str {
        &self.probe.version
    }

    pub fn tmux_bin(&self) -> &str {
        &self.cli.tmux_bin
    }

    pub fn snapshot(&self) -> &tmux::Snapshot {
        &self.snapshot
    }

    pub fn target(&self) -> &tmux::Target {
        &self.probe.target
    }

    pub fn overview_lines(&self) -> Vec<String> {
        vec![
            format!("tmux version : {}", self.tmux_version()),
            format!("tmux binary  : {}", self.tmux_bin()),
            format!("target       : {}", self.target().display_target()),
            format!("attach cmd   : {}", self.target().command_preview()),
            format!("refreshes    : {}", self.refresh_count()),
            format!("sessions     : {}", self.snapshot.session_count()),
            format!("windows      : {}", self.snapshot.window_count()),
            format!("panes        : {}", self.snapshot.pane_count()),
        ]
    }

    pub fn control_lines(&self) -> Vec<String> {
        let visible = self.visible_pane_entries();
        let mut waiting_count = 0;
        let mut error_count = 0;
        let mut stuck_count = 0;
        let mut review_count = 0;
        let mut active_missions = 0;

        for entry in &visible {
            let pane = &self.snapshot.panes[entry.index];
            let insight = entry.insight;
            if insight.status == PaneStatus::Waiting && !entry.acknowledged {
                if self.is_attention_action_pending(pane, insight.status) {
                    continue;
                }
                waiting_count += 1;
            }
            if insight.status == PaneStatus::Error && !entry.acknowledged {
                if self.is_attention_action_pending(pane, insight.status) {
                    continue;
                }
                error_count += 1;
            }
            if insight.status == PaneStatus::Stuck && !entry.acknowledged {
                if self.is_attention_action_pending(pane, insight.status) {
                    continue;
                }
                stuck_count += 1;
            }
            if insight.status == PaneStatus::Done
                && !entry.acknowledged
                && self.pane_requires_attention(pane, insight.status)
            {
                review_count += 1;
            }
            if insight.workload.is_agent() && insight.status == PaneStatus::Running {
                active_missions += 1;
            }
        }

        let action_line = self.overview_action_line_for_entries(&visible);
        if visible.is_empty()
            && !self.using_explicit_targets()
            && self.fanout_mode != FanoutMode::Lane
        {
            let state_line = if self.snapshot.panes.is_empty() {
                String::from("No panes yet.")
            } else {
                String::from("No matching panes.")
            };
            return vec![state_line, action_line];
        }
        let action_is_watching = action_line.starts_with("Watching:");
        let has_actionable_attention =
            waiting_count > 0 || error_count > 0 || stuck_count > 0 || review_count > 0;
        let uses_passive_output_action = self
            .command_center_passive_output_action_target_label(&visible)
            .is_some();
        let passive_all_clear_action =
            !has_actionable_attention && !action_is_watching && uses_passive_output_action;
        let mut lines = Vec::new();
        if passive_all_clear_action {
            if active_missions > 0 {
                lines.push(format!(
                    "All clear: {} working",
                    agent_count_or_none(active_missions)
                ));
            } else {
                lines.push(String::from("All clear."));
            }
        }
        lines.push(action_line);
        if action_is_watching {
            lines.push(format!(
                "Action: {} show in tmux",
                KeyBindingsConfig::primary_label(&self.ui_settings.keybindings.jump)
            ));
        }
        if has_actionable_attention {
            lines.push(format!(
                "Needs you: {}",
                attention_summary_label(waiting_count, error_count, stuck_count, review_count)
            ));
        } else if !action_is_watching
            && let Some((pane, _, _)) = self.watching_attention_queue().first()
        {
            lines.push(format!("Watching: {}", self.pane_target_label(pane)));
        }
        if active_missions > 0 && !passive_all_clear_action {
            lines.push(format!("Working: {}", agent_count_or_none(active_missions)));
        }
        lines.push(self.overview_scope_line_for_entries(&visible));
        lines.push(self.overview_start_line());
        lines
    }

    pub fn control_title(&self) -> String {
        String::from("Command Center")
    }

    fn send_target_line(&self) -> String {
        if self.using_marked_targets() && self.active_group_name.is_none() {
            self.active_target_description()
        } else {
            format!("send to {}", self.active_target_description())
        }
    }

    fn overview_scope_line_for_entries(&self, visible: &[VisiblePaneEntry]) -> String {
        let active_target = self.active_target_description();
        if !self.using_explicit_targets()
            && self.fanout_mode != FanoutMode::Lane
            && self
                .overview_attention_action_for_entries(visible)
                .is_some_and(|(_, _, attention_target)| attention_target != active_target)
        {
            return format!("Selected: {active_target}");
        }

        format!("Target: {active_target}")
    }

    fn overview_attention_action_for_entries(
        &self,
        _visible: &[VisiblePaneEntry],
    ) -> Option<(String, &'static str, String)> {
        let keys = &self.ui_settings.keybindings;
        self.command_center_primary_attention_action()
            .map(|action| {
                let (key, action_label) = match action.kind {
                    CommandCenterPrimaryActionKind::Continue => (
                        KeyBindingsConfig::primary_label(&keys.smart_action),
                        "continue",
                    ),
                    CommandCenterPrimaryActionKind::Output => {
                        (KeyBindingsConfig::primary_label(&keys.focus), "output")
                    }
                    CommandCenterPrimaryActionKind::Reply => {
                        (KeyBindingsConfig::primary_label(&keys.command), "reply")
                    }
                    CommandCenterPrimaryActionKind::Answer => {
                        (KeyBindingsConfig::primary_label(&keys.actions), "answer")
                    }
                    CommandCenterPrimaryActionKind::ShowWaiting => {
                        (KeyBindingsConfig::primary_label(&keys.jump), "show waiting")
                    }
                };
                (key, action_label, self.pane_target_label(&action.pane))
            })
    }

    fn overview_action_line_for_entries(&self, visible: &[VisiblePaneEntry]) -> String {
        let keys = &self.ui_settings.keybindings;
        if let Some((key, action, target)) = self.overview_attention_action_for_entries(visible) {
            if action == "reply" {
                return format!("Action: {key} reply to {target}");
            }
            return format!("Action: {key} {action} {target}");
        }

        if let Some((pane, _, _)) = self.watching_attention_queue().first() {
            return format!("Watching: {}", self.pane_target_label(pane));
        }

        if let Some(target) = self.command_center_passive_output_action_target_label(visible) {
            return format!(
                "Action: {} output {target}",
                KeyBindingsConfig::primary_label(&keys.focus)
            );
        }

        if self.using_explicit_targets() || self.fanout_mode == FanoutMode::Lane {
            return format!(
                "Action: {} {}",
                KeyBindingsConfig::primary_label(&keys.command),
                send_target_phrase(&self.active_target_description())
            );
        }

        if visible.is_empty() {
            if self.snapshot.panes.is_empty() {
                return format!(
                    "Action: start tmux panes, then {} refresh",
                    KeyBindingsConfig::primary_label(&keys.refresh)
                );
            }
            return String::from("Action: backspace show all panes");
        }

        format!(
            "Action: {} send this pane",
            KeyBindingsConfig::primary_label(&keys.command)
        )
    }

    fn command_center_passive_output_action_target_label(
        &self,
        visible: &[VisiblePaneEntry],
    ) -> Option<String> {
        if self.using_explicit_targets()
            || self.fanout_mode == FanoutMode::Lane
            || visible.is_empty()
        {
            return None;
        }

        let pane = self.selected_pane()?;
        let insight = self.pane_insight(pane);
        let has_visible_output =
            !focus_recent_lines(self.latest_live_output_lines(&pane.id, 8), 1).is_empty();
        if !has_visible_output && insight.status != PaneStatus::Running {
            return None;
        }
        if !visible
            .iter()
            .any(|entry| self.snapshot.panes[entry.index].id == pane.id)
        {
            return None;
        }

        Some(self.pane_target_label(pane))
    }

    fn overview_start_line(&self) -> String {
        let key =
            KeyBindingsConfig::primary_label(&self.ui_settings.keybindings.action_launch_agent);
        if let Some(pane) = self.selected_pane() {
            let folder = path_identity_label(&pane.current_path).unwrap_or_else(|| {
                let fallback = pane.current_path.trim();
                if fallback.is_empty() {
                    String::from("selected folder")
                } else {
                    truncate_for_panel(fallback)
                }
            });
            format!("Start: {key} agent in {folder}")
        } else {
            String::from("Start: select a pane first")
        }
    }

    pub fn help_lines(&self) -> Vec<String> {
        let keys = &self.ui_settings.keybindings;
        let back = KeyBindingsConfig::primary_label(&[String::from("backspace")]);
        let focus = KeyBindingsConfig::primary_label(&keys.focus);
        let jump = KeyBindingsConfig::primary_label(&keys.jump);
        let search = KeyBindingsConfig::primary_label(&keys.search);
        let refresh = KeyBindingsConfig::primary_label(&keys.refresh);
        let actions = KeyBindingsConfig::primary_label(&keys.actions);
        let layout = KeyBindingsConfig::primary_label(&keys.action_layout);
        let close = || {
            format!(
                "Close: Esc closes Help, {} quit muxboard.",
                KeyBindingsConfig::primary_label(&keys.quit)
            )
        };
        if self.pending_dispatch.is_some() {
            return vec![
                String::from("Now: Enter sends, Esc cancels review."),
                close(),
            ];
        }
        if self.fleet_picker_active {
            return vec![
                format!(
                    "Now: {} choose fleet, Enter loads, Esc closes fleets.",
                    keys.move_labels()
                ),
                close(),
            ];
        }
        if self.macro_assign_active {
            let labels = (0..MACRO_SLOT_COUNT)
                .map(|slot| self.ui_settings.keybindings.macro_slot_label(slot))
                .collect::<Vec<_>>()
                .join("/");
            return vec![
                format!("Now: {labels} pins latest command, Esc cancels."),
                close(),
            ];
        }
        if self.action_menu_active {
            return vec![
                String::from("Now: press a listed key, Esc closes More."),
                format!("Layout: {layout} cycles auto, side, stack."),
                close(),
            ];
        }
        if matches!(self.shell_panel(), ShellPanel::Browse)
            && self.window_navigation_targets().is_empty()
        {
            let now = if self.has_view_narrowing() {
                format!("Now: {back} shows all panes, Esc back.")
            } else {
                format!("Now: {refresh} refreshes panes, Esc back.")
            };
            let find = if self.has_view_narrowing() {
                format!("Find: {search} filter, {back} show all, {refresh} refresh.")
            } else {
                format!("Find: {search} filter, {refresh} refresh.")
            };
            return vec![now, find, close()];
        }
        if self.visible_pane_indices().is_empty() && self.snapshot.panes.is_empty() {
            return vec![
                format!("Now: start tmux panes, then {refresh} refresh."),
                format!("More: {actions} layout and settings."),
                close(),
            ];
        }
        if self.visible_pane_indices().is_empty() && self.has_view_narrowing() {
            return vec![
                format!("Now: {back} show all panes."),
                format!("Find: {search} filter, {refresh} refresh."),
                close(),
            ];
        }
        let focus_action = match self.shell_panel() {
            ShellPanel::Browse => "opens window",
            ShellPanel::Output => "keeps output open",
            _ => "output",
        };
        let mut now = if matches!(self.shell_panel(), ShellPanel::Output) {
            format!("Now: Esc back to Fleet, {jump} show in tmux.")
        } else if self.escape_back_is_available() {
            format!("Now: {focus} {focus_action}, Esc back, {jump} show in tmux.")
        } else {
            format!("Now: {focus} {focus_action}, {jump} show in tmux.")
        };
        let selected_can_continue = self.selected_pane().is_some_and(|pane| {
            let insight = self.pane_insight(pane);
            self.recommended_smart_action(pane, insight) == SmartAction::SendEnter
        });
        let selected_can_answer_choice = self.selected_pane().is_some_and(|pane| {
            let insight = self.pane_insight(pane);
            insight.status == PaneStatus::Waiting
                && self.recommended_smart_action(pane, insight) == SmartAction::Focus
                && self.action_menu_can_answer_choice()
        });
        let selected_can_reply_text = matches!(
            self.shell_panel(),
            ShellPanel::Selected | ShellPanel::Output | ShellPanel::Overview
        ) && self.selected_pane_can_reply_text();
        if selected_can_continue || !self.bulk_enter_targets().is_empty() {
            let continue_key = KeyBindingsConfig::primary_label(&keys.smart_action);
            now = if matches!(self.shell_panel(), ShellPanel::Output) {
                format!(
                    "Now: Esc back to Fleet, {jump} show in tmux, {continue_key} continue waiting."
                )
            } else if self.escape_back_is_available() {
                format!(
                    "Now: {focus} {focus_action}, Esc back, {jump} show in tmux, {continue_key} continue waiting."
                )
            } else {
                format!(
                    "Now: {} {}, {} show in tmux, {} continue waiting.",
                    focus, focus_action, jump, continue_key,
                )
            };
        } else if selected_can_answer_choice {
            let actions = KeyBindingsConfig::primary_label(&keys.actions);
            let command = KeyBindingsConfig::primary_label(&keys.command);
            now = if matches!(self.shell_panel(), ShellPanel::Output) {
                format!("Now: {actions} answer yes/no, Esc back, {jump} show in tmux.")
            } else if self.escape_back_is_available() {
                format!(
                    "Now: {actions} answer yes/no, {command} send, Esc back, {jump} show in tmux."
                )
            } else {
                format!("Now: {actions} answer yes/no, {command} send, {jump} show in tmux.")
            };
        } else if selected_can_reply_text {
            let command = KeyBindingsConfig::primary_label(&keys.command);
            now = if matches!(self.shell_panel(), ShellPanel::Output)
                || self.escape_back_is_available()
            {
                format!("Now: {command} reply, Esc back, {jump} show in tmux.")
            } else {
                format!("Now: {command} reply, {focus} {focus_action}, {jump} show in tmux.")
            };
        }
        let move_line = match self.shell_panel() {
            ShellPanel::Browse => format!("Move: {} browse windows.", keys.move_labels()),
            ShellPanel::Overview => format!("Move: {} choose action.", keys.move_labels()),
            ShellPanel::Output if self.details_can_scroll_output() => {
                format!("Move: {} output.", self.scroll_movement_label())
            }
            _ => format!(
                "Move: {} select panes, {} Fleet/Details.",
                keys.move_labels(),
                KeyBindingsConfig::primary_label(&keys.panel_focus),
            ),
        };
        let send_line = if selected_can_reply_text {
            format!(
                "Send: {} add/remove pane for a send list.",
                KeyBindingsConfig::primary_label(&keys.mark)
            )
        } else {
            format!(
                "Send: {} send text, {} add/remove pane.",
                KeyBindingsConfig::primary_label(&keys.command),
                KeyBindingsConfig::primary_label(&keys.mark),
            )
        };
        vec![
            now,
            send_line,
            format!(
                "Find: {} filter, {} show all, {} refresh.",
                KeyBindingsConfig::primary_label(&keys.search),
                back,
                KeyBindingsConfig::primary_label(&keys.refresh)
            ),
            move_line,
            format!(
                "Views: {} then {} browse, {} command center; {} layout.",
                KeyBindingsConfig::primary_label(&keys.actions),
                KeyBindingsConfig::primary_label(&keys.action_view_browse),
                KeyBindingsConfig::primary_label(&keys.action_view_command_center),
                layout,
            ),
            format!(
                "More: {} then {} start agent, {} zoom pane.",
                KeyBindingsConfig::primary_label(&keys.actions),
                KeyBindingsConfig::primary_label(&keys.action_launch_agent),
                KeyBindingsConfig::primary_label(&keys.action_zoom),
            ),
            String::from("Legend: > selected, * active, + listed, ! alert, ~ muted."),
            format!(
                "Close: Esc backs out or closes Help, {} quit muxboard.",
                KeyBindingsConfig::primary_label(&keys.quit)
            ),
        ]
    }

    pub fn command_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();

        if !self.recent_alerts.is_empty() {
            lines.push(String::from("alerts"));
            for alert in self.recent_alerts.iter().take(2) {
                lines.push(format!("! {}", truncate_for_panel(alert)));
            }
        }

        if self.action_menu_active && self.pending_dispatch.is_none() {
            return self.action_menu_lines();
        }

        if self.pending_dispatch.is_none() {
            if self.explicit_targets_have_no_live_panes() {
                lines.push(self.no_live_target_list_line());
                if self.command_input_active {
                    lines.push(self.command_text_line());
                }
            } else if self.command_input_active {
                let target_label = if self.command_input_is_reply_context() {
                    "Reply to: "
                } else {
                    "To: "
                };
                lines.push(format!(
                    "{}{}",
                    target_label,
                    truncate_for_panel(&send_target_object_phrase(
                        &self.active_target_description()
                    ))
                ));
                lines.push(self.command_text_line());
            } else {
                lines.push(truncate_for_panel(&self.send_target_line()));
            }
            if self.group_input_active {
                let name = self.group_name_buffer.trim_end();
                lines.push(if name.is_empty() {
                    String::from("Name:")
                } else {
                    format!("Name: {}", truncate_for_panel(name))
                });
            }
            if !self.marked_pane_ids.is_empty() && !self.explicit_targets_have_no_live_panes() {
                if let Some(note) = self.hidden_target_note() {
                    lines.push(note);
                } else {
                    lines.push(format!(
                        "send list {}",
                        pane_count_label(self.marked_pane_ids.len())
                    ));
                }
            }
            if let Some(group_name) = &self.active_group_name
                && !self.using_marked_targets()
            {
                lines.push(format!("fleet {}", truncate_for_panel(group_name)));
            }
        }
        if !self.command_input_active && self.pending_dispatch.is_none() {
            lines.push(format!("Action: {}", self.recommended_action_menu_line()));
        }

        if let Some(confirm) = &self.pending_dispatch {
            lines.push(format!(
                "To: {}",
                truncate_for_panel(&send_target_object_phrase(&confirm.target_description))
            ));
            if let Some(note) = self.hidden_pending_target_note(&confirm.expanded) {
                lines.push(note);
            }
            lines.push(format!("Text: {}", truncate_for_panel(&confirm.text)));
            lines.push(String::from("Targets"));
            let preview_indices =
                self.preview_indices_with_hidden(&confirm.expanded, 2, |(pane_id, _)| {
                    self.snapshot
                        .panes
                        .iter()
                        .find(|pane| pane.id == *pane_id)
                        .is_some_and(|pane| !self.matches_pane_visibility(pane))
                });
            for index in preview_indices.iter().copied() {
                let (pane_id, preview) = &confirm.expanded[index];
                lines.push(format!(
                    "  {} {}",
                    self.pane_target_label_by_id_for_current_view(pane_id),
                    truncate_for_panel(preview)
                ));
            }
            if confirm.expanded.len() > preview_indices.len() {
                lines.push(format!(
                    "  ... {} more",
                    confirm.expanded.len() - preview_indices.len()
                ));
            }
        }
        if !self.target_groups.is_empty()
            && !self.command_input_active
            && self.pending_dispatch.is_none()
        {
            push_section(&mut lines, "Fleets");
            for (index, group) in self.target_groups.iter().take(3).enumerate() {
                let selected = if self.selected_group_index == Some(index) {
                    ">"
                } else {
                    " "
                };
                lines.push(format!(
                    "{selected} {} ({})",
                    truncate_for_panel(&group.name),
                    pane_count_label(group.members.len())
                ));
            }
        }
        if self.pending_dispatch.is_none()
            && !self.recent_commands.is_empty()
            && (!self.command_input_active || self.command_input_can_repeat_recent())
        {
            push_section(&mut lines, "Recent");
            let repeat =
                KeyBindingsConfig::primary_label(&self.ui_settings.keybindings.repeat_last);
            if let Some(command) = self.recent_commands.front() {
                lines.push(format!("{repeat} repeat {}", truncate_for_panel(command)));
            }
        }

        let show_macros = self.macro_assign_active
            || (self.pending_dispatch.is_none()
                && self.macro_slots.iter().any(Option::is_some)
                && !self.command_input_active);
        if show_macros {
            push_section(&mut lines, "Macros");
            for (index, slot) in self.macro_slots.iter().enumerate() {
                let label = self.ui_settings.keybindings.macro_slot_label(index);
                match slot {
                    Some(command) => {
                        lines.push(format!("{label}: {}", truncate_for_panel(command)))
                    }
                    None => lines.push(format!("{label}: <empty>")),
                }
            }
        }
        if self.command_input_active && !self.command_buffer.trim().is_empty() {
            push_section(&mut lines, "Preview");
            for preview in self.command_preview_lines() {
                lines.push(format!("  {preview}"));
            }
        }
        let report_lines = self.active_target_report_lines();
        if !self.command_input_active && self.pending_dispatch.is_none() && !report_lines.is_empty()
        {
            push_section(&mut lines, "Reports");
            lines.extend(report_lines);
        }

        if lines.is_empty() {
            vec![String::from("Type : to send a command.")]
        } else {
            lines
        }
    }

    pub fn launch_lines(&self) -> Vec<String> {
        let mut lines = vec![format!("In: {}", self.launch_target_description())];
        if let Some(pane) = self.selected_pane() {
            lines.push(format!(
                "Folder: {}",
                truncate_for_panel(&pane.current_path)
            ));
        } else {
            lines.push(String::from("Action: Esc cancel, then choose a pane"));
        }
        lines.push(format!("Window: {}", self.launch_window_preview()));
        lines.push(format!("Command: {}", self.launch_command_display()));
        lines.push(format!("Presets: Tab {}", LAUNCH_PRESETS.join(", ")));

        let message = self.status_message().trim();
        if let Some(error) = message.strip_prefix("Start failed: ") {
            lines.push(format!("Error: {}", truncate_for_panel(error)));
        }

        lines
    }

    fn launch_target_description(&self) -> String {
        self.selected_pane()
            .map(|pane| format!("{} / {}", pane.session_name, pane.window_name))
            .unwrap_or_else(|| String::from("select a pane"))
    }

    fn launch_command_display(&self) -> String {
        let command = self.launch_buffer.trim();
        if command.is_empty() {
            String::from("_")
        } else {
            truncate_for_panel(command)
        }
    }

    fn command_text_line(&self) -> String {
        let command = self.command_buffer.trim_end();
        if command.is_empty() {
            String::from("Text: _")
        } else {
            format!("Text: {}", truncate_for_panel(command))
        }
    }

    fn launch_window_preview(&self) -> String {
        let command = self.launch_buffer.trim();
        if command.is_empty() {
            String::from("agent")
        } else {
            launch_window_name(command)
        }
    }

    pub fn header_hint_line(&self) -> String {
        self.header_hint_line_for_width(u16::MAX)
    }

    pub fn header_hint_line_for_width(&self, width: u16) -> String {
        let back = KeyBindingsConfig::primary_label(&[String::from("backspace")]);

        if self.pending_dispatch.is_some() {
            return String::from("Enter send  Esc cancel");
        }

        if self.search_input_active {
            if width < 64 {
                return format!("type  Enter apply  Esc cancel  {back} delete");
            }
            return format!("type to filter  Enter apply  Esc cancel  {back} delete");
        }

        if self.command_input_active {
            let submit = self.command_submit_action_label();
            if self.command_input_can_repeat_recent() {
                let repeat =
                    KeyBindingsConfig::primary_label(&self.ui_settings.keybindings.repeat_last);
                if width < 72 {
                    return format!("type  {repeat} repeat  Enter {submit}  Esc cancel");
                }
                return format!(
                    "type text  {repeat} repeat latest  Enter {submit}  Esc cancel  {back} delete"
                );
            }
            if width < 64 {
                return format!("type  Enter {submit}  Esc cancel  {back} delete");
            }
            return format!("type text  Enter {submit}  Esc cancel  {back} delete");
        }

        if self.launch_input_active {
            if self.selected_pane().is_none() {
                return String::from("Esc cancel, then choose a pane");
            }
            if width < 64 {
                return format!("type  Tab preset  Enter start  Esc cancel  {back} delete");
            }
            return format!("type command  Tab preset  Enter start  Esc cancel  {back} delete");
        }

        if self.group_input_active {
            if width < 64 {
                return format!("type  Enter save  Esc cancel  {back} delete");
            }
            return format!("type name  Enter save  Esc cancel  {back} delete");
        }

        if self.fleet_picker_active {
            return format!(
                "{} choose  Enter load  {} delete  Esc close",
                self.ui_settings.keybindings.move_labels(),
                KeyBindingsConfig::primary_label(&self.ui_settings.keybindings.action_group_delete)
            );
        }

        if self.macro_assign_active {
            let labels = (0..MACRO_SLOT_COUNT)
                .map(|slot| self.ui_settings.keybindings.macro_slot_label(slot))
                .collect::<Vec<_>>()
                .join("/");
            return format!("{labels} pin latest command  Esc cancel");
        }

        if self.action_menu_active {
            return String::from("press a listed key  Esc close");
        }

        if self.context_pane == ContextPane::Navigator {
            return String::new();
        }

        let _ = width;
        String::new()
    }

    pub fn header_context_line(&self) -> String {
        self.header_context_line_for_width(u16::MAX)
    }

    pub fn header_context_line_for_width(&self, width: u16) -> String {
        if self.help_overlay_active {
            String::from("Help")
        } else if self.theme_picker_active {
            String::from("Choose a theme.")
        } else if let Some(confirm) = &self.pending_dispatch {
            if width < 76 {
                format!(
                    "Review send to {}.",
                    pane_count_label(confirm.expanded.len())
                )
            } else {
                format!(
                    "Review {}. Enter sends, Esc cancels.",
                    send_target_phrase(&confirm.target_description)
                )
            }
        } else if self.search_input_active {
            if self.search_query.is_empty() {
                String::from("Search panes.")
            } else {
                format!("Searching for `{}`.", self.search_query)
            }
        } else if self.command_input_active {
            if width < 76 {
                format!(
                    "{} {}.",
                    self.command_input_context_verb(),
                    truncate_for_panel(&self.active_target_description())
                )
            } else {
                format!(
                    "{} {}.",
                    self.command_input_context_verb(),
                    self.active_target_description()
                )
            }
        } else if self.launch_input_active {
            String::from("Start agent.")
        } else if self.group_input_active {
            String::from("Save this send list as a reusable fleet.")
        } else if self.fleet_picker_active {
            String::from("Choose a saved fleet.")
        } else if self.macro_assign_active {
            String::from("Choose a slot for the latest command.")
        } else if self.action_menu_active {
            String::from("More")
        } else if self.context_pane == ContextPane::Navigator {
            let mut parts = vec![String::from("Browse")];
            if let Some(window) = self.board_window_summary(usize::MAX)
                && window != "0 panes"
            {
                parts.push(window);
            }
            join_and_truncate(parts, width)
        } else if self.context_pane == ContextPane::Tail {
            self.panel_header_context("Output", width)
        } else if self.context_pane == ContextPane::Control {
            self.panel_header_context(&self.control_title(), width)
        } else if self.using_marked_targets() {
            let mut parts = vec![format!(
                "send list {}",
                pane_count_label(self.marked_pane_ids.len())
            )];
            if let Some(pane) = self.selected_pane() {
                parts.push(format!("{}/{}", pane.session_name, pane.window_name));
            }
            if let Some(window) = self.board_window_summary(usize::MAX) {
                parts.push(window);
            }
            parts.push(self.fleet_health_summary());
            join_and_truncate(parts, width)
        } else if let Some(pane) = self.selected_pane() {
            let mut parts = if width < 72 || pane.session_name == pane.window_name {
                vec![pane.window_name.clone()]
            } else {
                vec![format!("{}/{}", pane.session_name, pane.window_name)]
            };
            if let Some(window) = self.board_window_summary(usize::MAX) {
                parts.push(window);
            }
            parts.push(self.fleet_health_summary());
            join_and_truncate(parts, width)
        } else if !self.search_query.is_empty() {
            join_and_truncate(
                vec![String::from("no matches"), self.fleet_health_summary()],
                width,
            )
        } else {
            String::from("No panes yet.")
        }
    }

    fn panel_header_context(&self, panel: &str, width: u16) -> String {
        let mut parts = vec![panel.to_owned()];
        parts.extend(self.selected_pane_header_parts(width));
        join_and_truncate(parts, width)
    }

    fn selected_pane_header_parts(&self, width: u16) -> Vec<String> {
        if let Some(pane) = self.selected_pane() {
            let mut parts = if width < 72 || pane.session_name == pane.window_name {
                vec![pane.window_name.clone()]
            } else {
                vec![format!("{}/{}", pane.session_name, pane.window_name)]
            };
            if let Some(window) = self.board_window_summary(usize::MAX) {
                parts.push(window);
            }
            parts.push(self.fleet_health_summary());
            parts
        } else if !self.search_query.is_empty() {
            vec![String::from("no matches"), self.fleet_health_summary()]
        } else {
            vec![String::from("No panes yet.")]
        }
    }

    pub fn status_hint_line(&self) -> String {
        self.status_hint_line_for_width(u16::MAX)
    }

    pub fn status_hint_line_for_width(&self, width: u16) -> String {
        let keys = &self.ui_settings.keybindings;
        let back = KeyBindingsConfig::primary_label(&[String::from("backspace")]);
        let panel = KeyBindingsConfig::primary_label(&keys.panel_focus);
        let refresh = KeyBindingsConfig::primary_label(&keys.refresh);
        let search = KeyBindingsConfig::primary_label(&keys.search);
        let command = KeyBindingsConfig::primary_label(&keys.command);
        let command_action = self.primary_command_action_label();
        let actions = KeyBindingsConfig::primary_label(&keys.actions);
        let details = KeyBindingsConfig::primary_label(&keys.focus);
        let show = KeyBindingsConfig::primary_label(&keys.jump);
        let mark = KeyBindingsConfig::primary_label(&keys.mark);
        let clear = KeyBindingsConfig::primary_label(&keys.clear_marks);
        let layout = KeyBindingsConfig::primary_label(&keys.action_layout);
        let quit = KeyBindingsConfig::primary_label(&keys.quit);
        let move_keys = keys.move_labels();
        if self.help_overlay_active {
            return format!("Esc close  {quit} quit");
        }
        if self.theme_picker_active {
            let escape = if self.theme_picker_first_run {
                "Esc system"
            } else {
                "Esc keep"
            };
            return format!("{move_keys} choose  Enter save  {escape}  {quit} quit");
        }
        if self.pending_dispatch.is_some() {
            return String::from("? help  Enter send  Esc cancel");
        }
        if self.search_input_active {
            return format!("type to filter  Enter apply  Esc cancel  {back} delete");
        }
        if self.command_input_active {
            if self.command_input_can_repeat_recent() {
                let repeat = KeyBindingsConfig::primary_label(&keys.repeat_last);
                if width < 72 {
                    return format!(
                        "type  {repeat} repeat  Enter {}  Esc cancel",
                        self.command_submit_action_label()
                    );
                }
                return format!(
                    "type text  {repeat} repeat latest  Enter {}  Esc cancel  {back} delete",
                    self.command_submit_action_label()
                );
            }
            return format!(
                "type text  Enter {}  Esc cancel  {back} delete",
                self.command_submit_action_label()
            );
        }
        if self.launch_input_active {
            if self.selected_pane().is_none() {
                return String::from("Esc cancel, then choose a pane");
            }
            return format!("type command  Tab preset  Enter start  Esc cancel  {back} delete");
        }
        if self.group_input_active {
            return format!("type name  Enter save  Esc cancel  {back} delete");
        }
        if self.fleet_picker_active {
            let delete = KeyBindingsConfig::primary_label(&keys.action_group_delete);
            if self.selected_fleet_live_count() == 0 {
                return format!("? help  {move_keys} choose  {delete} delete stale  Esc close");
            }
            return format!("? help  {move_keys} choose  Enter load  {delete} delete  Esc close");
        }
        if self.macro_assign_active {
            let labels = (0..MACRO_SLOT_COUNT)
                .map(|slot| self.ui_settings.keybindings.macro_slot_label(slot))
                .collect::<Vec<_>>()
                .join("/");
            return format!("? help  {labels} pin latest  Esc cancel");
        }
        if self.action_menu_active {
            return String::from("? help  press a listed key  Esc close");
        }
        if self.visible_pane_indices().is_empty() && self.snapshot.panes.is_empty() {
            let mut parts = vec![String::from("? help"), format!("{refresh} refresh")];
            if self.escape_back_is_available() {
                parts.push(String::from("Esc back"));
            }
            if width >= 56 {
                parts.push(format!("{actions} more"));
            }
            parts.push(format!("{quit} quit"));
            return join_and_truncate(parts, width);
        }
        if matches!(self.shell_panel(), ShellPanel::Browse) {
            let mut parts = vec![String::from("? help")];
            let has_window = !self.window_navigation_targets().is_empty();
            if self.has_view_narrowing() && width >= 80 {
                if width < 128 {
                    parts.push(format!("{back} show all"));
                } else {
                    parts.push(format!("{back} shows all panes"));
                }
            }
            if has_window {
                parts.push(format!("{move_keys} browse"));
                parts.push(format!("{details} window"));
                if width >= 84 {
                    parts.push(format!("{show} show"));
                }
            }
            parts.push(format!("{search} filter"));
            if width >= 104 {
                parts.push(format!("{layout} layout"));
            }
            if width >= 64 {
                parts.push(format!("{actions} more"));
            }
            parts.push(String::from("Esc back"));
            parts.push(format!("{quit} quit"));
            return join_and_truncate(parts, width);
        }
        if self.explicit_targets_have_no_live_panes() {
            let mut parts = vec![String::from("? help")];
            if !self.visible_pane_indices().is_empty() {
                parts.push(format!("{move_keys} move"));
                if let Some(mark_action) = self.selected_mark_action_label() {
                    parts.push(format!("{mark} {mark_action}"));
                }
            }
            if self.active_group_name.is_some() {
                parts.push(String::from("fleet stale"));
            } else {
                parts.push(String::from("send list empty"));
                parts.push(format!("{clear} clear"));
            }
            if self.has_view_narrowing() {
                parts.push(format!("{back} show all"));
            }
            if width >= 80 {
                parts.push(format!("{search} filter"));
            }
            if width >= 104 {
                parts.push(format!("{layout} layout"));
            }
            if self.escape_back_is_available() {
                parts.push(String::from("Esc back"));
            }
            if width >= 84 {
                parts.push(format!("{actions} more"));
            }
            parts.push(format!("{quit} quit"));
            return join_and_truncate(parts, width);
        }
        if self.using_marked_targets() && self.visible_pane_indices().is_empty() {
            let mut parts = vec![String::from("? help")];
            parts.push(self.hidden_send_list_footer_summary(width));
            parts.push(format!("{command} send"));
            parts.push(format!("{clear} clear"));
            if self.has_view_narrowing() {
                parts.push(format!("{back} show all"));
            }
            if width >= 80 {
                parts.push(format!("{search} filter"));
            }
            if self.escape_back_is_available() {
                parts.push(String::from("Esc back"));
            }
            if width >= 96 {
                parts.push(format!("{actions} more"));
            }
            parts.push(format!("{quit} quit"));
            return join_and_truncate(parts, width);
        }
        if self.using_marked_targets() {
            let count_summary = self.active_target_count_summary();
            let mark_action = self.selected_mark_action_label();
            if self.active_hidden_target_count() > 0 && width < 96 {
                let mut parts = vec![
                    String::from("? help"),
                    format!("{move_keys} move"),
                    format!("send list {count_summary}"),
                    format!("{command} send"),
                ];
                if let Some(mark_action) = mark_action {
                    parts.push(format!("{mark} {mark_action}"));
                }
                if self.escape_back_is_available() {
                    parts.push(String::from("Esc back"));
                }
                if width >= 72 {
                    parts.push(format!("{clear} clear"));
                }
                if width >= 84 {
                    parts.push(format!("{quit} quit"));
                }
                return join_and_truncate(parts, width);
            }
            if width < 96 {
                let mut parts = vec![
                    String::from("? help"),
                    format!("{move_keys} move"),
                    format!("send list {count_summary}"),
                    format!("{clear} clear"),
                    format!("{command} send"),
                ];
                if let Some(mark_action) = mark_action {
                    parts.insert(3, format!("{mark} {mark_action}"));
                }
                if self.escape_back_is_available() {
                    parts.push(String::from("Esc back"));
                }
                parts.push(format!("{quit} quit"));
                return join_and_truncate(parts, width);
            }
            let mut parts = vec![
                String::from("? help"),
                format!("{move_keys} move"),
                format!("send list {count_summary}"),
                format!("{clear} clear"),
                format!("{command} send"),
            ];
            if let Some(mark_action) = mark_action {
                parts.insert(3, format!("{mark} {mark_action}"));
            }
            if self.escape_back_is_available() {
                parts.push(String::from("Esc back"));
            }
            if width >= 104 {
                parts.push(format!("{layout} layout"));
            }
            parts.push(format!("{actions} more"));
            parts.push(format!("{quit} quit"));
            return join_and_truncate(parts, width);
        }

        if matches!(self.shell_panel(), ShellPanel::Overview) {
            let mut parts = vec![String::from("? help")];
            let show_all_hint = format!("{back} show all");
            let send_hint = format!("{command} send");
            let has_visible_panes = !self.visible_pane_indices().is_empty();
            let visible = self.visible_pane_entries();
            let action = self.command_center_footer_action_hint();
            let action_is_show_all = action.as_deref() == Some(show_all_hint.as_str());
            let action_uses_command_key = action
                .as_deref()
                .is_some_and(|action| action.starts_with(&format!("{command} ")));
            let action_uses_show_key = action
                .as_deref()
                .is_some_and(|action| action.starts_with(&format!("{show} ")));
            let action_uses_more_key = action
                .as_deref()
                .is_some_and(|action| action.starts_with(&format!("{actions} ")));
            let passive_output_action = self
                .overview_attention_action_for_entries(&visible)
                .is_none()
                && self.watching_attention_queue().is_empty()
                && self
                    .command_center_passive_output_action_target_label(&visible)
                    .is_some();
            if let Some(action) = action {
                parts.push(action);
            }
            if self.has_view_narrowing() && width >= 80 && !action_is_show_all {
                if width < 128 {
                    parts.push(show_all_hint);
                } else {
                    parts.push(format!("{back} shows all panes"));
                }
            }
            if has_visible_panes && width >= 72 {
                parts.push(format!("{move_keys} move"));
            }
            if self.escape_back_is_available() {
                parts.push(String::from("Esc back"));
            }
            if has_visible_panes && width >= 72 && !action_uses_show_key {
                parts.push(format!("{show} show"));
            }
            if has_visible_panes && !action_uses_command_key && !passive_output_action {
                parts.push(send_hint);
            }
            if width >= 80 {
                parts.push(format!("{search} filter"));
            }
            if width >= 104 {
                parts.push(format!("{layout} layout"));
            }
            if width >= 96 && !action_uses_more_key {
                parts.push(format!("{actions} more"));
            }
            parts.push(format!("{quit} quit"));
            return join_and_truncate(parts, width);
        }

        if self.visible_pane_indices().is_empty() && self.has_view_narrowing() {
            let mut parts = vec![String::from("? help")];
            if width >= 80 {
                if width < 128 {
                    parts.push(format!("{back} show all"));
                } else {
                    parts.push(format!("{back} shows all panes"));
                }
            }
            parts.push(format!("{search} filter"));
            if width >= 64 {
                parts.push(format!("{actions} more"));
            }
            parts.push(format!("{quit} quit"));
            return join_and_truncate(parts, width);
        }

        let mut parts = Vec::new();
        parts.push(String::from("? help"));
        if self.has_view_narrowing() && width >= 80 {
            if width < 128 {
                parts.push(format!("{back} show all"));
            } else {
                parts.push(format!("{back} shows all panes"));
            }
        }
        if let Some(movement) = self.default_movement_footer_hint(&move_keys) {
            parts.push(movement);
        }
        if self.escape_back_is_available() {
            parts.push(String::from("Esc back"));
        }
        let command_hint = format!("{command} {command_action}");
        let command_is_primary = command_action == "reply";
        if command_is_primary {
            parts.push(command_hint.clone());
        }
        let scroll_is_primary = self.is_details_panel_focused()
            && matches!(self.context_pane, ContextPane::Inspect | ContextPane::Tail)
            && self.details_can_scroll_output();
        if width >= 92 && !matches!(self.shell_panel(), ShellPanel::Overview) && !scroll_is_primary
        {
            parts.push(format!("{panel} focus"));
        }
        let details_label = match self.shell_panel() {
            ShellPanel::Output => "details",
            ShellPanel::Browse => "window",
            _ => "output",
        };
        if !matches!(self.shell_panel(), ShellPanel::Output)
            && (details_label != "output" || self.selected_pane_has_useful_output_action())
        {
            parts.push(format!("{details} {details_label}"));
        }
        if width >= 70 {
            parts.push(format!("{show} show"));
        }
        if width >= 84 {
            parts.push(format!("{mark} add"));
        }
        if !command_is_primary {
            parts.push(command_hint);
        }
        parts.push(format!("{search} filter"));
        if width >= 96
            && (width >= 128
                || matches!(self.shell_panel(), ShellPanel::Output)
                || !self.has_view_narrowing() && !self.is_details_panel_focused())
        {
            parts.push(format!("{layout} layout"));
        }
        if width >= 96
            && let Some(next_move) = self.next_move_hint_for_width(width)
        {
            parts.push(next_move);
        }
        if width >= 64 {
            parts.push(format!("{actions} more"));
        }
        parts.push(format!("{quit} quit"));
        join_and_truncate(parts, width)
    }

    fn command_center_footer_action_hint(&self) -> Option<String> {
        let keys = &self.ui_settings.keybindings;
        let visible = self.visible_pane_entries();
        if let Some((key, action, _target)) = self.overview_attention_action_for_entries(&visible) {
            let action = if action == "show waiting" {
                "show"
            } else {
                action
            };
            return Some(format!("{key} {action}"));
        }

        if !self.watching_attention_queue().is_empty() {
            return Some(format!(
                "{} show",
                KeyBindingsConfig::primary_label(&keys.jump)
            ));
        }

        if self.using_explicit_targets() || self.fanout_mode == FanoutMode::Lane {
            return Some(format!(
                "{} send",
                KeyBindingsConfig::primary_label(&keys.command)
            ));
        }

        if visible.is_empty() {
            return Some(format!(
                "{} show all",
                KeyBindingsConfig::primary_label(&[String::from("backspace")])
            ));
        }

        if self
            .command_center_passive_output_action_target_label(&visible)
            .is_some()
        {
            return Some(format!(
                "{} output",
                KeyBindingsConfig::primary_label(&keys.focus)
            ));
        }

        Some(format!(
            "{} send",
            KeyBindingsConfig::primary_label(&keys.command)
        ))
    }

    pub(super) fn details_can_scroll_output(&self) -> bool {
        self.details_scroll_max_offset() > 0
    }

    pub(crate) fn details_scroll_offset(&self) -> usize {
        self.details_scroll
    }

    pub(crate) fn observe_details_scroll_viewport(
        &self,
        metrics: Option<crate::tui::ScrollMetrics>,
    ) {
        self.rendered_scroll_context.set(self.context_pane);
        self.rendered_scroll_content_lines
            .set(metrics.map(|metrics| metrics.content_len).unwrap_or(0));
        self.rendered_scroll_viewport_lines
            .set(metrics.map(|metrics| metrics.viewport_len).unwrap_or(0));
    }

    fn escape_back_is_available(&self) -> bool {
        matches!(
            self.context_pane,
            ContextPane::Tail
                | ContextPane::Targets
                | ContextPane::Navigator
                | ContextPane::Control
        ) || self.is_details_panel_focused()
    }

    pub(super) fn details_scroll_max_offset(&self) -> usize {
        self.details_scroll_metrics().max_offset()
    }

    pub(super) fn details_scroll_page_size(&self) -> usize {
        self.details_scroll_metrics().viewport_len.max(1)
    }

    fn details_scroll_metrics(&self) -> crate::tui::ScrollMetrics {
        let rendered_content_lines = self.rendered_scroll_content_lines.get();
        let rendered_viewport_lines = self.rendered_scroll_viewport_lines.get();
        if self.rendered_scroll_context.get() == self.context_pane && rendered_viewport_lines > 0 {
            return crate::tui::ScrollMetrics {
                content_len: rendered_content_lines,
                viewport_len: rendered_viewport_lines.min(rendered_content_lines),
            };
        }

        let content_len = self.details_scrollable_output_line_count();
        crate::tui::ScrollMetrics {
            content_len,
            viewport_len: self.details_scroll_viewport_line_count().min(content_len),
        }
    }

    fn details_scroll_viewport_line_count(&self) -> usize {
        match self.context_pane {
            ContextPane::Inspect => DETAILS_OUTPUT_VIEWPORT_LINES,
            ContextPane::Tail => TAIL_OUTPUT_VIEWPORT_LINES,
            _ => 0,
        }
    }

    pub(super) fn details_scrollable_output_line_count(&self) -> usize {
        match self.context_pane {
            ContextPane::Inspect => self.inspect_scrollable_output_line_count(),
            ContextPane::Tail => self.tail_scrollable_output_line_count(),
            _ => 0,
        }
    }

    fn inspect_scrollable_output_line_count(&self) -> usize {
        if self.visible_pane_indices().is_empty() {
            return 0;
        }

        let Some(pane) = self.selected_pane() else {
            return 0;
        };

        let insight = self.pane_insight(pane);
        let next_fallback = self.recommended_action_summary(pane, insight);
        let report = self.effective_agent_report_for_pane(&pane.id);
        let blocker_summary = inspector_blocker_summary(report.as_ref());
        let next_summary = inspector_next_summary(report.as_ref(), &next_fallback);

        inspector_latest_lines(
            insight,
            pane.current_command.as_str(),
            report.as_ref(),
            &focus_recent_lines(
                self.latest_output_lines(&pane.id, MAX_INSPECTOR_OUTPUT_SOURCE_LINES),
                MAX_INSPECTOR_OUTPUT_SOURCE_LINES,
            ),
            InspectorSurface {
                blocker: blocker_summary.as_deref(),
                next: &next_summary,
            },
            MAX_INSPECTOR_OUTPUT_SOURCE_LINES,
        )
        .len()
    }

    fn tail_scrollable_output_line_count(&self) -> usize {
        if self.visible_pane_indices().is_empty() {
            return 0;
        }

        let Some(pane) = self.selected_pane() else {
            return 0;
        };

        let insight = self.pane_insight(pane);
        let report = self.effective_agent_report_for_pane(&pane.id);
        let recent = focus_recent_lines(
            self.latest_live_output_lines(&pane.id, MAX_LIVE_TAIL_SOURCE_LINES),
            MAX_LIVE_TAIL_SOURCE_LINES,
        );
        if recent.is_empty() {
            return 0;
        }

        let summary = board_latest_detail(
            insight,
            pane.current_command.as_str(),
            report.as_ref(),
            &recent,
        );
        let show_summary = !summary.is_empty()
            && !summary.eq_ignore_ascii_case("none")
            && (report.is_some() || !recent.is_empty());

        output_tail_lines(
            pane.current_command.as_str(),
            &recent,
            show_summary.then_some(summary.as_str()),
        )
        .len()
    }

    pub(crate) fn has_view_narrowing(&self) -> bool {
        self.view_scope != ViewScope::All
            || !self.search_query.is_empty()
            || self.filter_mode != FilterMode::All
    }

    fn next_move_hint_for_width(&self, width: u16) -> Option<String> {
        if width < 96 || self.pending_dispatch.is_some() || self.action_menu_active {
            return None;
        }

        let key = KeyBindingsConfig::primary_label(&self.ui_settings.keybindings.smart_action);

        if self.using_marked_targets() || self.fanout_mode == FanoutMode::Lane {
            if self.bulk_enter_targets().is_empty() {
                return None;
            }
            return Some(format!("{key} continue"));
        }

        let pane = self.selected_pane()?;
        let insight = self.pane_insight(pane);
        match self.recommended_smart_action(pane, insight) {
            SmartAction::SendEnter => Some(format!("{key} continue")),
            SmartAction::Focus => None,
        }
    }

    pub fn chrome_line_for_width(&self, width: u16) -> String {
        let mut parts = Vec::new();
        let context = self.header_context_line_for_width(width);
        if !context.is_empty() {
            parts.push(context);
        }
        let hint = self.header_hint_line_for_width(width);
        if !hint.is_empty() {
            parts.push(hint);
        }
        let mut line = parts.join("  ");
        let max_chars = usize::from(width.max(24));
        if line.chars().count() > max_chars {
            line = truncate_for_width(&line, max_chars);
        }
        line
    }

    fn command_submit_action_label(&self) -> &'static str {
        if self.active_target_panes().len() > 1 {
            "review"
        } else if self.command_input_is_reply_context() {
            "reply"
        } else {
            "send"
        }
    }

    fn command_input_context_verb(&self) -> &'static str {
        if self.command_input_is_reply_context() {
            "Reply to"
        } else {
            "Send to"
        }
    }

    fn primary_command_action_label(&self) -> &'static str {
        if self.fanout_mode == FanoutMode::Off
            && !self.using_explicit_targets()
            && self.selected_pane_can_reply_text()
        {
            "reply"
        } else {
            "send"
        }
    }

    pub fn footer_line_for_width(&self, width: u16) -> String {
        let text_entry_footer_mode = self.search_input_active
            || self.command_input_active
            || self.launch_input_active
            || self.group_input_active;
        let compact_footer_mode = text_entry_footer_mode
            || self.theme_picker_active
            || self.fleet_picker_active
            || self.macro_assign_active
            || self.action_menu_active;
        if compact_footer_mode {
            return truncate_for_width(
                &self.status_hint_line_for_width(width),
                usize::from(width.max(24)),
            );
        }
        if self.pending_dispatch.is_some() {
            return truncate_for_width(
                &self.status_hint_line_for_width(width),
                usize::from(width.max(24)),
            );
        }

        let message = self.status_message().trim();
        let message = if is_low_value_footer_status(message) {
            ""
        } else {
            message
        };
        if !message.is_empty() {
            let keymap = self.status_hint_line_for_width(width);
            let keymap_len = keymap.chars().count();
            if self.has_view_narrowing()
                && width >= 80
                && !status_deserves_footer_over_narrowing(message)
            {
                return truncate_for_width(&keymap, usize::from(width.max(24)));
            }
            if status_is_theme_feedback(message) && width >= 72 {
                let feedback_keymap = self.status_feedback_keymap_for_width(width);
                let feedback_len = feedback_keymap.chars().count();
                if feedback_len.saturating_add(16) < usize::from(width) {
                    let message_width = usize::from(width).saturating_sub(feedback_len + 2);
                    return truncate_for_width(
                        &format!(
                            "{}  {feedback_keymap}",
                            truncate_for_width(message, message_width)
                        ),
                        usize::from(width.max(24)),
                    );
                }
            }
            let line = if width >= 112 && keymap_len.saturating_add(24) < usize::from(width) {
                let message_width = usize::from(width).saturating_sub(keymap_len + 2);
                format!("{}  {keymap}", truncate_for_width(message, message_width))
            } else if width >= 96 {
                let feedback_keymap = self.status_feedback_keymap_for_width(width);
                let feedback_len = feedback_keymap.chars().count();
                if feedback_len.saturating_add(18) < usize::from(width) {
                    let message_width = usize::from(width).saturating_sub(feedback_len + 2);
                    format!(
                        "{}  {feedback_keymap}",
                        truncate_for_width(message, message_width)
                    )
                } else if status_deserves_compact_footer_feedback(message) && width >= 56 {
                    format!("{message}  ? help")
                } else {
                    keymap
                }
            } else if width >= 56 {
                format!("{message}  ? help")
            } else {
                message.to_owned()
            };
            return truncate_for_width(&line, usize::from(width.max(24)));
        }

        truncate_for_width(
            &self.status_hint_line_for_width(width),
            usize::from(width.max(24)),
        )
    }

    fn status_feedback_keymap_for_width(&self, width: u16) -> String {
        let keys = &self.ui_settings.keybindings;
        let move_keys = keys.move_labels();
        let details = KeyBindingsConfig::primary_label(&keys.focus);
        let show = KeyBindingsConfig::primary_label(&keys.jump);
        let mark = KeyBindingsConfig::primary_label(&keys.mark);
        let clear = KeyBindingsConfig::primary_label(&keys.clear_marks);
        let command = KeyBindingsConfig::primary_label(&keys.command);
        let command_action = self.primary_command_action_label();
        let search = KeyBindingsConfig::primary_label(&keys.search);
        let actions = KeyBindingsConfig::primary_label(&keys.actions);
        let layout = KeyBindingsConfig::primary_label(&keys.action_layout);
        let quit = KeyBindingsConfig::primary_label(&keys.quit);
        let movement = self.default_movement_footer_hint(&move_keys);
        let details_label = match self.shell_panel() {
            ShellPanel::Output => "details",
            ShellPanel::Browse => "window",
            _ => "output",
        };
        if matches!(self.shell_panel(), ShellPanel::Browse) {
            let has_window = !self.window_navigation_targets().is_empty();
            let mut parts = vec![String::from("? help")];
            if self.has_view_narrowing() {
                let back = KeyBindingsConfig::primary_label(&[String::from("backspace")]);
                parts.push(format!("{back} show all"));
            }
            if has_window {
                parts.push(
                    movement
                        .clone()
                        .unwrap_or_else(|| format!("{move_keys} browse")),
                );
                parts.push(format!("{details} {details_label}"));
                parts.push(format!("{show} show"));
            }
            if width >= 104 {
                parts.push(format!("{search} filter"));
            }
            if width >= 112 {
                parts.push(format!("{layout} layout"));
            }
            parts.push(format!("{actions} more"));
            parts.push(String::from("Esc back"));
            parts.push(format!("{quit} quit"));
            return join_and_truncate(parts, width);
        }
        if self.explicit_targets_have_no_live_panes() {
            return self.status_hint_line_for_width(width);
        }
        if !self.using_marked_targets() && self.visible_pane_indices().is_empty() {
            return self.status_hint_line_for_width(width);
        }
        if self.using_marked_targets() && self.visible_pane_indices().is_empty() {
            let back = KeyBindingsConfig::primary_label(&[String::from("backspace")]);
            let mut parts = vec![String::from("? help")];
            let hidden = self.active_hidden_target_count();
            if width < 112 && hidden > 0 {
                parts.push(format!("{} hidden", pane_count_label(hidden)));
            } else {
                parts.push(self.hidden_send_list_footer_summary(width));
            }
            parts.push(format!("{command} send"));
            parts.push(format!("{clear} clear"));
            if self.has_view_narrowing() {
                parts.push(format!("{back} show all"));
            }
            if width >= 112 {
                parts.push(format!("{search} filter"));
            }
            if width >= 120 {
                parts.push(format!("{layout} layout"));
            }
            if self.escape_back_is_available() {
                parts.push(String::from("Esc back"));
            }
            if width >= 116 {
                parts.push(format!("{actions} more"));
            }
            parts.push(format!("{quit} quit"));
            return join_and_truncate(parts, width);
        }
        if self.using_marked_targets() {
            let mark_action = self.selected_mark_action_label();
            let mut parts = vec![
                String::from("? help"),
                movement
                    .clone()
                    .unwrap_or_else(|| format!("{move_keys} move")),
                format!("send list {}", self.active_target_count_summary()),
            ];
            if let Some(mark_action) = mark_action {
                parts.push(format!("{mark} {mark_action}"));
            }
            parts.push(format!("{clear} clear"));
            parts.push(format!("{command} send"));
            if width >= 112 {
                parts.push(format!("{layout} layout"));
            }
            if self.escape_back_is_available() {
                parts.push(String::from("Esc back"));
            }
            parts.push(format!("{actions} more"));
            parts.push(format!("{quit} quit"));
            return join_and_truncate(parts, width);
        }
        let mut parts = vec![String::from("? help")];
        if let Some(movement) = movement {
            parts.push(movement);
        }
        if self.escape_back_is_available() {
            parts.push(String::from("Esc back"));
        }
        let command_hint = format!("{command} {command_action}");
        let command_is_primary = command_action == "reply";
        if command_is_primary {
            parts.push(command_hint.clone());
        }
        if !matches!(self.shell_panel(), ShellPanel::Output)
            && (details_label != "output" || self.selected_pane_has_useful_output_action())
        {
            parts.push(format!("{details} {details_label}"));
        }
        parts.push(format!("{show} show"));
        if width >= 104 {
            parts.push(format!("{mark} add"));
        }
        if !command_is_primary {
            parts.push(command_hint);
        }
        if width >= 104 {
            parts.push(format!("{search} filter"));
        }
        if width >= 120 {
            parts.push(format!("{layout} layout"));
        }
        parts.push(format!("{actions} more"));
        parts.push(format!("{quit} quit"));
        join_and_truncate(parts, width)
    }

    fn scroll_movement_label(&self) -> String {
        let keys = &self.ui_settings.keybindings;
        format!(
            "{} older/{} newer",
            KeyBindingsConfig::primary_label(&keys.move_up),
            KeyBindingsConfig::primary_label(&keys.move_down)
        )
    }

    fn default_movement_footer_hint(&self, move_keys: &str) -> Option<String> {
        if matches!(self.shell_panel(), ShellPanel::Output)
            && !self.details_can_scroll_output()
            && self
                .live_tail_lines()
                .iter()
                .any(|line| line == "No output yet.")
        {
            return None;
        }

        Some(if self.is_details_panel_focused() {
            match self.context_pane {
                ContextPane::Navigator => format!("{move_keys} browse"),
                ContextPane::Inspect | ContextPane::Tail if self.details_can_scroll_output() => {
                    self.scroll_movement_label()
                }
                _ => format!("{move_keys} move"),
            }
        } else {
            format!("{move_keys} move")
        })
    }

    fn selected_mark_action_label(&self) -> Option<&'static str> {
        let pane = self.selected_pane()?;
        if !self.matches_pane_visibility(pane) {
            return None;
        }
        Some(if self.marked_pane_ids.contains(&pane.id) {
            "remove"
        } else {
            "add"
        })
    }

    fn explicit_targets_have_no_live_panes(&self) -> bool {
        self.using_explicit_targets() && self.active_target_panes().is_empty()
    }

    fn hidden_send_list_footer_summary(&self, width: u16) -> String {
        let hidden = self.active_hidden_target_count();
        if width < 80 && hidden > 0 {
            format!("{} hidden", pane_count_label(hidden))
        } else {
            format!("send list {}", self.active_target_count_summary())
        }
    }

    pub fn status_message_line(&self) -> String {
        self.status_message().to_owned()
    }

    pub(crate) fn inspector_title(&self) -> String {
        self.selected_pane_title()
    }

    pub(crate) fn inspector_lines(&self) -> Vec<String> {
        self.selected_pane_lines()
    }

    pub fn context_panel_title(&self) -> String {
        match self.shell_panel() {
            ShellPanel::Theme => String::from("Choose theme"),
            ShellPanel::Selected => self.selected_pane_title(),
            ShellPanel::Send => self.send_panel_title(),
            ShellPanel::Launch => String::from("Start"),
            ShellPanel::Fleets => String::from("Fleets"),
            ShellPanel::Output => self.live_tail_title(),
            ShellPanel::Browse => self.navigator_title(),
            ShellPanel::Overview => self.control_title(),
            ShellPanel::Actions => String::from("More"),
        }
    }

    pub(crate) fn overlay_panel(&self) -> Option<(String, Vec<String>)> {
        match self.shell_panel() {
            ShellPanel::Theme => Some((String::from("Choose theme"), self.theme_picker_lines())),
            ShellPanel::Selected => None,
            ShellPanel::Send => Some((self.send_panel_title(), self.command_lines())),
            ShellPanel::Launch => Some((String::from("Start"), self.launch_lines())),
            ShellPanel::Fleets => Some((String::from("Fleets"), self.fleet_picker_lines())),
            ShellPanel::Output => Some((self.live_tail_title(), self.live_tail_lines())),
            ShellPanel::Browse => Some((self.navigator_title(), self.navigator_lines())),
            ShellPanel::Overview => Some((self.control_title(), self.control_panel_lines())),
            ShellPanel::Actions => Some((String::from("More"), self.action_menu_lines())),
        }
    }

    fn send_panel_title(&self) -> String {
        if self.command_input_is_reply_context() {
            String::from("Reply")
        } else {
            String::from("Send")
        }
    }

    pub(crate) fn layout_preset(&self) -> LayoutPreset {
        self.ui_settings.layout_preset
    }

    pub(crate) fn layout_visible_pane_count(&self) -> usize {
        self.visible_pane_indices().len()
    }

    pub(crate) fn layout_context_line_count(&self) -> usize {
        if matches!(self.shell_panel(), ShellPanel::Selected) && self.snapshot.panes.is_empty() {
            return self.empty_visible_pane_lines().len();
        }
        if matches!(self.shell_panel(), ShellPanel::Selected)
            && self.has_view_narrowing()
            && (self.selected_pane().is_none() || self.selected_pane_hidden_by_current_view())
        {
            return self.empty_visible_pane_lines().len();
        }

        match self.shell_panel() {
            ShellPanel::Output => self
                .selected_runtime_output_line_count(MAX_LIVE_TAIL_SOURCE_LINES)
                .saturating_add(4),
            ShellPanel::Browse => self.navigator_lines().len(),
            ShellPanel::Overview => self.control_panel_lines().len(),
            _ => self.selected_context_line_count_estimate(),
        }
    }

    fn selected_context_line_count_estimate(&self) -> usize {
        let output_count =
            self.selected_runtime_output_line_count(MAX_INSPECTOR_OUTPUT_SOURCE_LINES);
        let base_rows = if output_count > 0
            && output_count < 8
            && self
                .selected_pane()
                .is_some_and(|pane| self.pane_insight(pane).status == PaneStatus::Waiting)
        {
            12
        } else {
            6
        };

        output_count.saturating_add(base_rows)
    }

    fn selected_runtime_output_line_count(&self, limit: usize) -> usize {
        self.selected_pane()
            .and_then(|pane| self.pane_runtime.get(&pane.id))
            .map(|runtime| runtime.output.len().min(limit))
            .unwrap_or(0)
    }

    fn selected_pane_has_useful_output_action(&self) -> bool {
        let Some(pane) = self.selected_pane() else {
            return false;
        };

        !focus_recent_lines(self.latest_live_output_lines(&pane.id, 8), 1).is_empty()
            || self.pane_insight(pane).workload.is_agent()
    }

    #[cfg(test)]
    pub(crate) fn theme_preset(&self) -> ThemePreset {
        self.ui_settings.active_theme_preset()
    }

    pub(crate) fn ui_settings(&self) -> &UiSettings {
        &self.ui_settings
    }

    pub(crate) fn keybindings(&self) -> &KeyBindingsConfig {
        &self.ui_settings.keybindings
    }

    pub(crate) fn should_emphasize_context_panel(&self) -> bool {
        self.is_details_panel_focused() || !matches!(self.shell_panel(), ShellPanel::Selected)
    }

    pub(crate) fn should_emphasize_fleet_panel(&self) -> bool {
        self.is_fleet_panel_focused()
    }

    pub(crate) fn action_menu_has_actionable_targets(&self) -> bool {
        let has_visible_selection = self.selected_visible_pane_position().is_some();
        let has_live_targets = !self.active_target_panes().is_empty();
        has_live_targets && (has_visible_selection || self.using_marked_targets())
    }

    pub(crate) fn action_menu_has_visible_selection(&self) -> bool {
        self.selected_visible_pane_position().is_some()
    }

    pub(crate) fn action_menu_has_sortable_panes(&self) -> bool {
        !self.snapshot.panes.is_empty()
    }

    pub(crate) fn action_menu_can_clear_marks(&self) -> bool {
        !self.marked_pane_ids.is_empty()
    }

    pub(crate) fn action_menu_can_save_group(&self) -> bool {
        !self.marked_pane_ids.is_empty() && !self.active_target_panes().is_empty()
    }

    pub(crate) fn action_menu_can_load_group(&self) -> bool {
        !self.target_groups.is_empty()
    }

    pub(crate) fn action_menu_can_delete_group(&self) -> bool {
        self.selected_group_index
            .and_then(|index| self.target_groups.get(index))
            .is_some()
    }

    pub(crate) fn action_menu_can_target_lane(&self) -> bool {
        self.action_menu_has_visible_selection()
            && self
                .selected_pane()
                .is_some_and(|pane| self.pane_insight(pane).workload.is_agent())
    }

    pub(crate) fn action_menu_can_ack_selected(&self) -> bool {
        self.action_menu_has_visible_selection()
            && self.selected_pane().is_some_and(|pane| {
                let insight = self.pane_insight(pane);
                self.pane_requires_attention(pane, insight.status)
                    && !self.is_acknowledged(pane, insight.status)
            })
    }

    pub(crate) fn action_menu_can_clear_selected_ack(&self) -> bool {
        self.action_menu_has_visible_selection()
            && self.selected_pane().is_some_and(|pane| {
                let insight = self.pane_insight(pane);
                self.pane_requires_attention(pane, insight.status)
                    && self.is_acknowledged(pane, insight.status)
            })
    }

    pub(crate) fn action_menu_can_ack_all(&self) -> bool {
        self.action_menu_has_visible_selection() && self.attention_queue_len() > 0
    }

    pub(crate) fn action_menu_can_clear_all_acks(&self) -> bool {
        self.action_menu_has_visible_selection() && !self.acknowledged_attention.is_empty()
    }

    pub(crate) fn action_menu_can_continue_waiting(&self) -> bool {
        self.action_menu_has_visible_selection()
            && self.selected_pane().is_some_and(|pane| {
                let insight = self.pane_insight(pane);
                self.recommended_smart_action(pane, insight) == SmartAction::SendEnter
                    || !self.bulk_enter_targets().is_empty()
            })
    }

    pub(crate) fn action_menu_can_answer_choice(&self) -> bool {
        self.action_menu_has_visible_selection()
            && self
                .selected_pane()
                .is_some_and(|pane| self.pane_has_choice_prompt(pane, self.pane_insight(pane)))
    }

    pub(crate) fn selected_pane_can_reply_text(&self) -> bool {
        self.action_menu_has_visible_selection()
            && !self.action_menu_can_continue_waiting()
            && !self.action_menu_can_answer_choice()
            && self.selected_pane().is_some_and(|pane| {
                let insight = self.pane_insight(pane);
                self.command_center_can_reply_to_pane(pane, insight)
            })
    }

    pub(crate) fn command_input_is_reply_context(&self) -> bool {
        self.command_input_active
            && self.fanout_mode == FanoutMode::Off
            && !self.using_explicit_targets()
            && self.active_target_panes().len() == 1
            && self.selected_pane_can_reply_text()
    }

    fn action_menu_lines(&self) -> Vec<String> {
        let keys = &self.ui_settings.keybindings;
        let mut lines = vec![format!("Action: {}", self.recommended_action_menu_line())];
        let has_visible_selection = self.action_menu_has_visible_selection();
        let has_live_targets = !self.active_target_panes().is_empty();
        let has_actionable_targets = self.action_menu_has_actionable_targets();
        let can_reply_text = self.selected_pane_can_reply_text();
        let explicit_targets_have_no_live_panes =
            self.using_explicit_targets() && !has_live_targets;
        let needs_show_all_recovery = !self.snapshot.panes.is_empty() && self.has_view_narrowing();

        if has_actionable_targets {
            lines.push(format!(
                "To: {}",
                send_target_object_phrase(&self.active_target_description())
            ));
        } else if explicit_targets_have_no_live_panes {
            lines.push(self.no_live_target_list_line());
        } else if self.snapshot.panes.is_empty() {
            lines.push(String::from("start tmux panes, then refresh"));
        }

        if let Some(group_name) = &self.active_group_name {
            lines.push(format!("fleet {}", truncate_for_panel(group_name)));
        }
        if !self.target_groups.is_empty() {
            lines.push(format!(
                "saved {}",
                group_count_label(self.target_groups.len())
            ));
        }

        if self.action_menu_has_visible_selection() {
            push_section(&mut lines, "Start");
            lines.push(keys.action_label(&keys.action_launch_agent, "start agent"));
        }

        push_section(&mut lines, "View");
        if needs_show_all_recovery {
            lines.push(keys.action_label(&[String::from("backspace")], "show all panes"));
        }
        let detail_action = match self.context_pane {
            ContextPane::Tail => "show details",
            ContextPane::Navigator => "open window",
            _ => "show output",
        };
        if has_visible_selection {
            lines.push(keys.action_label(&keys.focus, detail_action));
        }
        if has_actionable_targets {
            let command_action = if can_reply_text { "reply" } else { "send text" };
            lines.push(keys.action_label(&keys.command, command_action));
        }
        lines.extend([
            keys.action_label(&keys.action_view_browse, "browse windows"),
            keys.action_label(&keys.action_view_command_center, "command center"),
        ]);
        if has_actionable_targets {
            lines.push(keys.action_label(&keys.summaries, "summarize panes"));
        }
        lines.push(keys.action_label(&keys.refresh, "refresh"));
        lines.push(keys.action_label(
            &keys.action_layout,
            &format!("layout: {}", self.ui_settings.layout_preset.display_label()),
        ));
        if self.action_menu_has_sortable_panes() {
            lines.extend([
                keys.action_label(
                    &keys.action_sort,
                    &format!("sort by {}", self.sort_mode.next().display_label()),
                ),
                keys.action_label(
                    &keys.action_filter,
                    &format!("show {}", self.filter_mode.next().display_label()),
                ),
            ]);
        }

        let pane_actions = if has_visible_selection {
            self.action_menu_pane_lines()
        } else {
            Vec::new()
        };
        if !pane_actions.is_empty() {
            push_section(&mut lines, "Pane");
            lines.extend(pane_actions);
        }

        if self.action_menu_can_clear_marks() || self.action_menu_can_load_group() {
            push_section(&mut lines, "Send List");
            if self.action_menu_can_load_group() {
                lines.push(keys.action_label(&keys.action_group_load, "choose fleet"));
                if let Some((group_name, is_stale)) = self
                    .selected_group_index
                    .and_then(|index| self.target_groups.get(index))
                    .map(|group| (group.name.as_str(), self.live_member_count(group) == 0))
                {
                    let delete_label = if is_stale {
                        format!("delete stale {}", truncate_for_panel(group_name))
                    } else {
                        format!("delete {}", truncate_for_panel(group_name))
                    };
                    lines.push(keys.action_label(&keys.action_group_delete, &delete_label));
                }
            }
            if self.action_menu_can_clear_marks() {
                lines.push(keys.action_label(&keys.clear_marks, "clear send list"));
                if self.action_menu_can_save_group() {
                    lines.push(keys.action_label(&keys.action_group_save, "save fleet"));
                }
            }
        }

        push_section(&mut lines, "Settings");
        lines.extend([
            keys.action_label(&keys.action_metrics, "pane CPU/mem"),
            keys.action_label(
                &keys.action_desktop_notifications,
                &self.desktop_alerts_action_label(),
            ),
            keys.action_label(&keys.action_bell, "terminal bell"),
            keys.action_label(&keys.action_alert_debounce, "alert repeat delay"),
            keys.action_label(&keys.action_alert_policy, "alert types"),
        ]);

        let report_lines = self.active_target_report_lines();
        if !report_lines.is_empty() {
            push_section(&mut lines, "Reports");
            lines.extend(report_lines);
        }

        lines
    }

    fn action_menu_pane_lines(&self) -> Vec<String> {
        let keys = &self.ui_settings.keybindings;
        let mut lines = Vec::new();

        if self.action_menu_has_visible_selection() {
            if self.action_menu_can_continue_waiting() {
                lines.push(keys.action_label(&keys.action_enter_queue, "continue waiting panes"));
            }
            if self.action_menu_can_answer_choice() {
                lines.push(keys.action_label(&keys.action_send_yes, "answer yes"));
                lines.push(keys.action_label(&keys.action_send_no, "answer no"));
            }
            if self.action_menu_can_clear_selected_ack() {
                lines.push(keys.action_label(&keys.action_ack_clear_selected, "unmute alert"));
            } else if self.action_menu_can_ack_selected() {
                lines.push(keys.action_label(&keys.action_ack_selected, "mute alert"));
            }
            if let Some(mark_action) = self.selected_mark_action_label() {
                let label = if mark_action == "add" {
                    "add to send list"
                } else {
                    "remove from send list"
                };
                lines.push(keys.action_label(&keys.mark, label));
            }
            if !self.action_menu_can_save_group() {
                lines.push(keys.action_label(&keys.jump, "show in tmux"));
            }
            lines.extend([
                keys.action_label(&keys.action_zoom, "zoom pane"),
                keys.action_label(&keys.action_send_enter, "send Enter"),
            ]);
            if self.action_menu_can_target_lane() {
                lines.push(keys.action_label(&keys.action_lane_target, "send lane"));
            }
        }

        if self.action_menu_can_ack_all() {
            lines.push(keys.action_label(&keys.action_ack_all, "mute all alerts"));
        }
        if self.action_menu_can_clear_all_acks() {
            lines.push(keys.action_label(&keys.action_ack_clear_all, "unmute all"));
        }

        lines
    }

    fn desktop_alerts_action_label(&self) -> String {
        match self.notifier.mode() {
            notifications::NotificationMode::LocalDesktop => String::from("desktop alerts"),
            notifications::NotificationMode::SshFallback => {
                String::from("desktop alerts unavailable on SSH")
            }
            notifications::NotificationMode::TerminalOnly => {
                String::from("desktop alerts unavailable here")
            }
        }
    }

    fn recommended_action_menu_line(&self) -> String {
        let keys = &self.ui_settings.keybindings;

        if self.visible_pane_indices().is_empty() && !self.using_explicit_targets() {
            if self.snapshot.panes.is_empty() {
                return format!(
                    "{} refresh after starting tmux panes",
                    KeyBindingsConfig::primary_label(&keys.refresh)
                );
            }
            return String::from("backspace show all panes");
        }

        if self.active_target_panes().is_empty() {
            if self.active_group_name.is_some() {
                let load = KeyBindingsConfig::primary_label(&keys.action_group_load);
                if self.action_menu_active {
                    return format!("{load} choose fleet");
                }
                return format!(
                    "{} then {load} choose fleet",
                    KeyBindingsConfig::primary_label(&keys.actions)
                );
            }
            if self.using_marked_targets() {
                if self.visible_pane_indices().is_empty() && self.has_view_narrowing() {
                    return String::from("backspace show all panes");
                }
                if self.selected_visible_pane_position().is_some() {
                    return format!(
                        "{} add a visible pane",
                        KeyBindingsConfig::primary_label(&keys.mark)
                    );
                }
            }
            return format!("{} select a pane", keys.move_labels());
        }

        if !self.marked_pane_ids.is_empty() {
            return format!(
                "{} send list",
                KeyBindingsConfig::primary_label(&keys.command)
            );
        }

        if !self.bulk_enter_targets().is_empty() {
            return format!(
                "{} continue waiting panes",
                KeyBindingsConfig::primary_label(&keys.action_enter_queue)
            );
        }

        if self.action_menu_can_answer_choice() {
            if !self.action_menu_active {
                return format!(
                    "{} answer yes/no",
                    KeyBindingsConfig::primary_label(&keys.actions)
                );
            }
            return format!(
                "{} answer yes, {} answer no",
                KeyBindingsConfig::primary_label(&keys.action_send_yes),
                KeyBindingsConfig::primary_label(&keys.action_send_no)
            );
        }

        if self.selected_pane_can_reply_text() {
            return format!("{} reply", KeyBindingsConfig::primary_label(&keys.command));
        }

        if let Some(pane) = self.selected_pane() {
            let insight = self.pane_insight(pane);
            if self.pane_requires_attention(pane, insight.status)
                && !self.is_acknowledged(pane, insight.status)
            {
                return format!(
                    "{} mute alert",
                    KeyBindingsConfig::primary_label(&keys.action_ack_selected)
                );
            }
        }

        if !self.active_target_report_lines().is_empty() {
            return format!(
                "{} summarize panes",
                KeyBindingsConfig::primary_label(&keys.summaries)
            );
        }

        if self.attention_queue_len() > 0 {
            return format!(
                "{} mute alerts",
                KeyBindingsConfig::primary_label(&keys.action_ack_all)
            );
        }

        format!(
            "{} send this pane",
            KeyBindingsConfig::primary_label(&keys.command)
        )
    }

    pub fn recent_event_lines(&self) -> Vec<String> {
        if self.recent_events.is_empty() {
            return vec![String::from("No tmux events yet.")];
        }

        self.recent_events.iter().cloned().collect()
    }

    pub fn recent_events_title(&self) -> String {
        if self.recent_events.is_empty() {
            String::from("Recent events")
        } else {
            format!("Recent events | {}", self.recent_events.len())
        }
    }

    pub fn navigator_title(&self) -> String {
        String::from("Browse")
    }

    pub fn context_panel_lines(&self) -> Vec<String> {
        match self.shell_panel() {
            ShellPanel::Theme => self.theme_picker_lines(),
            ShellPanel::Selected => self.selected_pane_lines(),
            ShellPanel::Send => self.command_lines(),
            ShellPanel::Launch => self.launch_lines(),
            ShellPanel::Fleets => self.fleet_picker_lines(),
            ShellPanel::Output => self.live_tail_lines(),
            ShellPanel::Browse => self.navigator_lines(),
            ShellPanel::Overview => self.control_panel_lines(),
            ShellPanel::Actions => self.action_menu_lines(),
        }
    }

    pub(crate) fn shell_panel(&self) -> ShellPanel {
        if self.theme_picker_active {
            return ShellPanel::Theme;
        }

        if self.pending_dispatch.is_some()
            || self.command_input_active
            || self.group_input_active
            || self.macro_assign_active
            || self.context_pane == ContextPane::Targets
        {
            return ShellPanel::Send;
        }

        if self.action_menu_active {
            return ShellPanel::Actions;
        }

        if self.fleet_picker_active {
            return ShellPanel::Fleets;
        }

        if self.launch_input_active {
            return ShellPanel::Launch;
        }

        match self.context_pane {
            ContextPane::Inspect => ShellPanel::Selected,
            ContextPane::Tail => ShellPanel::Output,
            ContextPane::Targets => ShellPanel::Send,
            ContextPane::Navigator => ShellPanel::Browse,
            ContextPane::Control => ShellPanel::Overview,
        }
    }

    pub fn command_shortcuts_are_visible(&self) -> bool {
        matches!(self.shell_panel(), ShellPanel::Send)
            && !self.command_input_active
            && self.pending_dispatch.is_none()
    }

    pub fn theme_picker_lines(&self) -> Vec<String> {
        let mut lines = if self.theme_picker_page == ThemePickerPage::Top {
            vec![String::from("Pick a look. You can change it later.")]
        } else {
            vec![String::from("More themes")]
        };

        lines.extend(
            self.theme_picker_options()
                .iter()
                .enumerate()
                .map(|(index, option)| {
                    let marker = if index == self.theme_picker_index {
                        ">"
                    } else {
                        " "
                    };
                    format!("{marker} {:<17} {}", option.label, option.detail)
                }),
        );
        lines
    }

    pub fn fleet_picker_lines(&self) -> Vec<String> {
        if self.target_groups.is_empty() {
            return vec![
                String::from("No saved fleets."),
                String::from("Mark panes, then save a fleet from More."),
            ];
        }

        self.target_groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                let marker = if index == self.fleet_picker_index {
                    ">"
                } else {
                    " "
                };
                let current = if self.active_group_name.as_deref() == Some(group.name.as_str()) {
                    " current"
                } else {
                    ""
                };
                format!(
                    "{marker} {}  {}/{} live{}",
                    truncate_for_panel(&group.name),
                    self.live_member_count(group),
                    group.members.len(),
                    current
                )
            })
            .collect()
    }

    fn live_member_count(&self, group: &TargetGroup) -> usize {
        group
            .members
            .iter()
            .filter(|locator| {
                self.snapshot.panes.iter().any(|pane| {
                    pane.session_name == locator.session_name
                        && pane.window_name == locator.window_name
                        && pane.pane_index == locator.pane_index
                })
            })
            .count()
    }

    fn selected_fleet_live_count(&self) -> usize {
        self.target_groups
            .get(self.fleet_picker_index)
            .map(|group| self.live_member_count(group))
            .unwrap_or(0)
    }

    pub fn navigator_lines(&self) -> Vec<String> {
        let entries = self.window_navigation_entries();
        if entries.is_empty() {
            return self.empty_visible_pane_lines();
        }

        let mut lines = Vec::new();
        let mut current_session = None::<&str>;
        let selected_window_id = self
            .selected_window_id
            .as_deref()
            .filter(|window_id| {
                entries
                    .iter()
                    .any(|entry| self.snapshot.windows[entry.index].id == *window_id)
            })
            .or_else(|| {
                entries
                    .first()
                    .map(|entry| self.snapshot.windows[entry.index].id.as_str())
            });

        for entry in entries {
            let window = &self.snapshot.windows[entry.index];
            if current_session != Some(window.session_name.as_str()) {
                current_session = Some(window.session_name.as_str());
                if !lines.is_empty() {
                    lines.push(String::new());
                }
                lines.push(format!("{}:", window.session_name));
            }

            let scoped =
                matches!(&self.view_scope, ViewScope::Window { id, .. } if *id == window.id);
            let selected = if selected_window_id == Some(window.id.as_str()) {
                ">"
            } else {
                " "
            };
            let scope_marker = if scoped { "*" } else { " " };

            lines.push(format!(
                "{selected}{scope_marker} {} ({} pane{}, h{})",
                window.name,
                entry.pane_count,
                if entry.pane_count == 1 { "" } else { "s" },
                entry.heat,
            ));
        }

        lines
    }

    fn control_panel_lines(&self) -> Vec<String> {
        let mut lines = self.control_lines();
        let attention_count = self.attention_queue_len();
        if attention_count > 0 {
            let attention = self.attention_queue_lines();
            lines.push(attention_queue_heading(attention_count));
            lines.extend(attention);
        }
        let watching = self.watching_attention_lines();
        if !watching.is_empty() {
            push_section(&mut lines, "Watching");
            lines.extend(watching);
        }
        let lanes = self.visible_agent_lanes(5);
        if !lanes.is_empty() {
            push_section(&mut lines, "Lanes");
            lines.extend(lanes.into_iter().map(format_agent_lane_line));
        }
        lines
    }

    pub fn attention_queue_lines(&self) -> Vec<String> {
        let queue = self.attention_queue();

        if queue.is_empty() {
            return vec![String::from("All clear.")];
        }

        let total = queue.len();
        let visible_limit = 6;
        let mut lines = queue
            .iter()
            .take(visible_limit)
            .map(|(pane, insight)| {
                let selected = if self.selected_pane_id.as_deref() == Some(pane.id.as_str()) {
                    ">"
                } else {
                    " "
                };
                let mut line = format!(
                    "{selected} {} {}",
                    self.attention_queue_action_label(pane, *insight),
                    self.pane_target_label(pane)
                );
                if let Some(reason) = self.attention_queue_reason_label(pane, *insight) {
                    line.push_str(": ");
                    line.push_str(&reason);
                }
                line
            })
            .collect::<Vec<_>>();

        let visible = lines.len();
        if total > visible {
            lines.push(self.attention_queue_overflow_line(&queue[visible..]));
        }

        lines
    }

    fn attention_queue_reason_label(
        &self,
        pane: &tmux::Pane,
        insight: PaneInsight,
    ) -> Option<String> {
        let recent = focus_recent_lines(self.latest_output_lines(&pane.id, 8), 6);
        if recent.iter().any(|line| matches_enter_hint(line)) {
            return Some(String::from("needs Enter"));
        }
        if recent.iter().any(|line| matches_choice_hint(line)) {
            return Some(String::from("yes/no choice"));
        }

        let action = self.attention_queue_action_label(pane, insight);
        let report = self.effective_agent_report_for_pane(&pane.id);
        let blocker = inspector_blocker_summary(report.as_ref());
        let fallback = board_latest_detail(
            insight,
            pane.current_command.as_str(),
            report.as_ref(),
            &recent,
        );
        let fallback = (!fallback.is_empty()).then_some(fallback);
        let next = report
            .as_ref()
            .and_then(|report| cleaned_inspector_report_field(report.next.as_str()));

        [blocker, fallback, next]
            .into_iter()
            .flatten()
            .map(|reason| clean_mission_summary(&reason))
            .find(|reason| {
                !reason.is_empty()
                    && !action_detail_is_redundant(action, reason)
                    && !matches!(
                        reason.to_ascii_lowercase().as_str(),
                        "needs enter" | "yes/no choice"
                    )
            })
            .map(|reason| truncate_for_panel(&reason))
            .or_else(|| {
                (self.command_center_can_reply_to_pane(pane, insight))
                    .then(|| String::from("free-form prompt"))
            })
    }

    fn watching_attention_lines(&self) -> Vec<String> {
        let watching = self.watching_attention_queue();
        let visible_limit = 4;
        let mut lines = watching
            .iter()
            .take(visible_limit)
            .map(|(pane, _, pending)| {
                let selected = if self.selected_pane_id.as_deref() == Some(pane.id.as_str()) {
                    ">"
                } else {
                    " "
                };
                format!(
                    "{selected} {}: {}",
                    self.pane_target_label(pane),
                    pending.kind.watching_label()
                )
            })
            .collect::<Vec<_>>();

        if watching.len() > visible_limit {
            lines.push(format!(
                "+ {} more watching",
                watching.len() - visible_limit
            ));
        }

        lines
    }

    fn attention_queue_overflow_line(&self, hidden: &[(&tmux::Pane, PaneInsight)]) -> String {
        let hidden_count = hidden.len();
        let verb = if hidden_count == 1 { "needs" } else { "need" };
        let mut action_counts = Vec::<(&'static str, usize)>::new();

        for (pane, insight) in hidden {
            let action = self.attention_queue_hidden_action_label(pane, *insight);
            if let Some((_, count)) = action_counts
                .iter_mut()
                .find(|(existing, _)| *existing == action)
            {
                *count += 1;
            } else {
                action_counts.push((action, 1));
            }
        }

        let actions = if action_counts.len() == 1 {
            action_counts
                .first()
                .map(|(action, _)| *action)
                .unwrap_or("review")
                .to_owned()
        } else {
            action_counts
                .iter()
                .map(|(action, count)| {
                    if *count == 1 {
                        (*action).to_owned()
                    } else {
                        format!("{action} x{count}")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        };

        format!("+ {hidden_count} more {verb} you: {actions}")
    }

    fn attention_queue_hidden_action_label(
        &self,
        pane: &tmux::Pane,
        insight: PaneInsight,
    ) -> &'static str {
        match self.attention_queue_action_label(pane, insight) {
            "reply to" => "reply",
            "output" => "output",
            "show waiting" => "show",
            "show" => "show",
            "answer" => "answer",
            "continue" => "continue",
            _ => "review",
        }
    }

    fn attention_queue_action_label(
        &self,
        pane: &tmux::Pane,
        insight: PaneInsight,
    ) -> &'static str {
        match self.recommended_action_summary(pane, insight).as_str() {
            "continue" => "continue",
            "answer" => "answer",
            "show prompt" if self.command_center_can_reply_to_pane(pane, insight) => "reply to",
            "show prompt" => "show waiting",
            "show output" => "output",
            _ => attention_action_label(insight.status),
        }
    }

    pub fn pane_lines(&self) -> Vec<String> {
        let visible = self.visible_pane_indices();
        if visible.is_empty() {
            return self.empty_visible_pane_lines();
        }

        visible
            .into_iter()
            .take(18)
            .map(|index| {
                let pane = &self.snapshot.panes[index];
                let insight = self.pane_insight(pane);
                let selected = if self.selected_pane_id.as_deref() == Some(pane.id.as_str()) {
                    ">"
                } else {
                    " "
                };
                let active = if pane.active { "*" } else { " " };
                let queue = if self.pane_requires_attention(pane, insight.status) {
                    if self.is_acknowledged(pane, insight.status) {
                        "~"
                    } else {
                        "!"
                    }
                } else {
                    " "
                };
                format!(
                    "{selected}{active}{queue} {} {} {} / {}",
                    insight.status.short_label(),
                    insight.workload.short_label(),
                    pane.session_name,
                    pane.window_name
                )
            })
            .collect()
    }

    pub fn board_title(&self, limit: usize) -> String {
        let mut parts = vec![String::from("Board")];
        let visible = self.visible_pane_entries();
        let window_summary = self.board_window_summary_for_entries(&visible, limit);
        if self.view_scope != ViewScope::All {
            parts.push(self.view_scope.display_label());
        }
        if let Some(filter) = self.board_filter_label() {
            parts.push(filter.to_owned());
        }
        if let Some(sort) = self.board_sort_label() {
            parts.push(sort.to_owned());
        }
        if let Some(empty_state) = self.empty_board_title_state_from_summary(&window_summary) {
            parts.push(empty_state);
        } else {
            if !self.search_query.is_empty() {
                parts.push(format!("search: {}", self.search_query));
            }
            if let Some(window) = &window_summary {
                parts.push(window.to_owned());
            }
            parts.push(self.fleet_health_summary_for_entries(&visible));
        }

        if !self.marked_pane_ids.is_empty() {
            parts.push(format!(
                "send list {}",
                pane_count_label(self.marked_pane_ids.len())
            ));
        }
        if let Some(confirm) = &self.pending_dispatch {
            parts.push(format!(
                "review {}",
                pane_count_label(confirm.expanded.len())
            ));
        }
        if self.using_marked_targets() {
            parts.push(String::from("send list"));
        }
        if self.metrics_mode == MetricsMode::Local {
            parts.push(String::from("pane CPU/mem"));
        }

        parts.join(" | ")
    }

    pub fn board_title_for_width(&self, limit: usize, width: u16) -> String {
        let mut parts = vec![String::from("Fleet")];
        let visible = self.visible_pane_entries();
        let window_summary = self.board_window_summary_for_entries(&visible, limit);

        if self.view_scope != ViewScope::All && width >= 78 {
            parts.push(self.view_scope.display_label());
        }
        if width >= 78
            && let Some(filter) = self.board_filter_label()
        {
            parts.push(filter.to_owned());
        }
        if width >= 96
            && let Some(sort) = self.board_sort_label()
        {
            parts.push(sort.to_owned());
        }

        if let Some(empty_state) = self.empty_board_title_state_from_summary(&window_summary) {
            parts.push(empty_state);
        } else {
            if !self.search_query.is_empty() {
                parts.push(format!("search: {}", self.search_query));
            }

            if (self.search_query.is_empty() || width >= 64)
                && let Some(window) = &window_summary
            {
                parts.push(window.to_owned());
            }
            parts.push(self.fleet_health_summary_for_entries(&visible));
        }

        if !self.marked_pane_ids.is_empty() && width >= 84 {
            parts.push(format!(
                "send list {}",
                pane_count_label(self.marked_pane_ids.len())
            ));
        }
        if let Some(confirm) = &self.pending_dispatch
            && width >= 84
        {
            parts.push(format!(
                "review {}",
                pane_count_label(confirm.expanded.len())
            ));
        }
        if self.metrics_mode == MetricsMode::Local && width >= 100 {
            parts.push(String::from("CPU/mem"));
        }

        parts.join(" | ")
    }

    fn board_filter_label(&self) -> Option<&'static str> {
        match self.filter_mode {
            FilterMode::All => None,
            FilterMode::Agents => Some("agents"),
            FilterMode::Attention => Some("needs you"),
        }
    }

    fn board_sort_label(&self) -> Option<&'static str> {
        match self.sort_mode {
            SortMode::Attention => None,
            SortMode::Heat => Some("activity sort"),
            SortMode::Natural => Some("tmux order"),
        }
    }

    fn empty_board_title_state_from_summary(
        &self,
        window_summary: &Option<String>,
    ) -> Option<String> {
        if window_summary.as_deref() != Some("0 panes") {
            return None;
        }

        if self.search_query.is_empty() {
            Some(String::from("no panes yet"))
        } else {
            Some(String::from("no matches"))
        }
    }

    pub fn board_rows(&self, limit: usize) -> Vec<BoardRow> {
        if limit == 0 {
            return Vec::new();
        }

        let entries = self.board_row_entries(limit);
        let indices = entries.iter().map(|entry| entry.index).collect::<Vec<_>>();
        let location_counts = board_location_counts(&self.snapshot.panes, &indices);
        let active_target_ids = self
            .active_target_panes()
            .into_iter()
            .map(|pane| pane.id.as_str())
            .collect::<HashSet<_>>();
        let pending_target_ids = self
            .pending_dispatch
            .as_ref()
            .map(|staged| {
                staged
                    .expanded
                    .iter()
                    .map(|(pane_id, _)| pane_id.as_str())
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();

        entries
            .into_iter()
            .map(|entry| {
                let index = entry.index;
                let pane = &self.snapshot.panes[index];
                let insight = entry.insight;
                let metric = self.pane_metrics.get(&pane.id);
                let report = self.effective_agent_report_for_pane_with_insight(&pane.id, insight);
                let acknowledged = entry.acknowledged;
                let pending_attention = self.is_attention_action_pending(pane, insight.status);
                let (command_label, show_command_in_latest) =
                    board_command_label(pane, insight.workload);
                let recent_lines = focus_recent_lines(self.latest_output_lines(&pane.id, 6), 6);
                let mission = agent_mission_summary(
                    insight,
                    pane.current_command.as_str(),
                    report.as_ref(),
                    &recent_lines,
                );

                BoardRow {
                    selected: self.selected_pane_id.as_deref() == Some(pane.id.as_str()),
                    active: pane.active,
                    marked: self.marked_pane_ids.contains(&pane.id),
                    targeted: active_target_ids.contains(pane.id.as_str()),
                    staged: pending_target_ids.contains(pane.id.as_str()),
                    show_command_in_latest,
                    attention: if self.pane_requires_attention(pane, insight.status) {
                        if acknowledged || pending_attention {
                            String::from("~")
                        } else {
                            String::from("!")
                        }
                    } else {
                        String::from(" ")
                    },
                    status: insight.status.display_label().to_ascii_lowercase(),
                    lifecycle: lifecycle_label(
                        insight.status,
                        acknowledged,
                        pending_attention,
                        self.pane_requires_attention(pane, insight.status),
                    )
                    .to_owned(),
                    mission: mission.clone(),
                    heat: pane_heat_score(&ObservedPane::from(pane), insight, acknowledged)
                        .to_string(),
                    age: insight
                        .last_output_age
                        .map(format_age_short)
                        .unwrap_or_else(|| String::from("-")),
                    cpu: metric
                        .map(|metric| format!("{:.1}", metric.cpu_percent))
                        .unwrap_or_else(|| String::from("-")),
                    mem: metric
                        .map(|metric| format!("{:.1}", metric.mem_percent))
                        .unwrap_or_else(|| String::from("-")),
                    lane: insight.workload.short_label().to_owned(),
                    pane: pane.id.clone(),
                    location: board_location_label(pane, &location_counts),
                    command: truncate_for_board(&command_label, 12),
                    title: truncate_for_board(&mission, BOARD_LATEST_DETAIL_MAX_CHARS),
                }
            })
            .collect()
    }

    pub fn agent_lane_lines(&self) -> Vec<String> {
        let lanes = self.visible_agent_lanes(6);
        if lanes.is_empty() {
            return vec![String::from("No agent lanes in this view.")];
        }

        lanes.into_iter().map(format_agent_lane_line).collect()
    }

    fn visible_agent_lanes(&self, limit: usize) -> Vec<AgentLane> {
        let lanes = self.agent_lanes();
        if lanes.len() <= limit {
            return lanes;
        }

        let mut visible = lanes.iter().copied().take(limit).collect::<Vec<_>>();
        if visible.iter().any(|lane| lane.selected) {
            return visible;
        }

        if let Some(selected_lane) = lanes.iter().copied().find(|lane| lane.selected)
            && let Some(last) = visible.last_mut()
        {
            *last = selected_lane;
        }

        visible
    }

    pub fn selected_pane_title(&self) -> String {
        String::from("Details")
    }

    pub fn live_tail_title(&self) -> String {
        String::from("Output")
    }

    pub fn selected_pane_lines(&self) -> Vec<String> {
        if self.visible_pane_indices().is_empty() {
            return self.empty_visible_pane_lines();
        }

        let Some(pane) = self.selected_pane() else {
            return vec![String::from("No pane selected.")];
        };

        let insight = self.pane_insight(pane);
        let next_fallback = self.recommended_action_summary(pane, insight);
        let acknowledged = self.is_acknowledged(pane, insight.status);
        let report = self.effective_agent_report_for_pane(&pane.id);
        let blocker_summary = inspector_blocker_summary(report.as_ref());
        let next_summary = inspector_next_summary(report.as_ref(), &next_fallback);
        let mission = agent_mission_summary(
            insight,
            pane.current_command.as_str(),
            report.as_ref(),
            &focus_recent_lines(self.latest_output_lines(&pane.id, 16), 10),
        );
        let recent = inspector_latest_lines(
            insight,
            pane.current_command.as_str(),
            report.as_ref(),
            &focus_recent_lines(
                self.latest_output_lines(&pane.id, MAX_INSPECTOR_OUTPUT_SOURCE_LINES),
                MAX_INSPECTOR_OUTPUT_SOURCE_LINES,
            ),
            InspectorSurface {
                blocker: blocker_summary.as_deref(),
                next: &next_summary,
            },
            MAX_INSPECTOR_OUTPUT_SOURCE_LINES,
        );
        let metrics_line = self.pane_metrics.get(&pane.id).map(|metric| {
            format!(
                "pane CPU/mem: pid {} | cpu {:.1}% | mem {:.1}% | {}",
                metric.pid, metric.cpu_percent, metric.mem_percent, metric.elapsed
            )
        });
        let mut lines = vec![
            format!("{}/{}", pane.session_name, pane.window_name),
            format!(
                "State: {}   Tool: {}",
                insight.status.display_label(),
                insight.workload.display_label()
            ),
        ];

        let mission_duplicates_blocker = blocker_summary
            .as_deref()
            .is_some_and(|blocker| mission == blocker);

        if self.pane_requires_attention(pane, insight.status)
            || matches!(insight.status, PaneStatus::Idle | PaneStatus::Unknown)
        {
            if let Some(blocker) = blocker_summary {
                lines.push(format!(
                    "{}: {}",
                    attention_problem_label(insight.status),
                    blocker
                ));
            }
            lines.push(self.selected_action_line(pane, insight, &next_summary));
            if let Some(reply) = self.selected_reply_line(pane, insight) {
                lines.push(reply);
            }
            if !mission.is_empty() && mission != next_summary && !mission_duplicates_blocker {
                lines.push(format!("Mission: {}", mission));
            }
        } else {
            let now_summary = if mission.is_empty() {
                next_summary.clone()
            } else {
                mission.clone()
            };
            if !now_summary.is_empty() {
                lines.push(format!("Now: {}", now_summary));
            }
        }

        if self.using_explicit_targets() || self.fanout_mode == FanoutMode::Lane {
            if self.explicit_targets_have_no_live_panes() {
                lines.push(format!("Target: {}", self.no_live_target_list_line()));
            } else {
                lines.push(format!("Send: {}", self.active_target_description()));
            }
        }
        if let Some(pending) = self.pending_attention_action(pane, insight.status) {
            lines.push(format!("Watching: {}", pending.kind.watching_label()));
        } else if let Some(position) = self.attention_queue_position(&pane.id) {
            lines.push(format!("Queue: #{}", position));
        }

        if self.command_input_active && !self.command_buffer.trim().is_empty() {
            push_gap(&mut lines);
            lines.push(String::from("Command"));
            for preview in self.command_preview_lines() {
                lines.push(preview);
            }
        }

        if !recent.is_empty() {
            push_gap(&mut lines);
            lines.push(String::from("Output"));
            for line in &recent {
                lines.push(format!("  {line}"));
            }
        }

        if let Some(age) = insight.last_output_age {
            let mut activity = vec![format_age(age)];
            if acknowledged {
                activity.push(String::from("muted"));
            }
            push_gap(&mut lines);
            lines.push(format!("Updated: {}", activity.join(" | ")));
        }

        if self.fanout_mode == FanoutMode::Lane {
            lines.push(format!("Lane: {}", self.fanout_summary_for_selected()));
        }

        if let Some(metrics_line) = metrics_line {
            lines.push(metrics_line);
        } else if self.metrics_mode == MetricsMode::Local {
            lines.push(format!(
                "pane CPU/mem: unavailable for local pid {}",
                pane.pane_pid
            ));
        }

        if let Some(confirm) = &self.pending_dispatch {
            lines.push(format!(
                "Review: {}",
                send_target_phrase(&confirm.target_description)
            ));
        }

        lines
    }

    fn selected_reply_line(&self, pane: &tmux::Pane, insight: PaneInsight) -> Option<String> {
        if insight.status != PaneStatus::Waiting {
            return None;
        }

        let keys = &self.ui_settings.keybindings;
        let send = KeyBindingsConfig::primary_label(&keys.command);
        let show = KeyBindingsConfig::primary_label(&keys.jump);

        match self.recommended_smart_action(pane, insight) {
            SmartAction::SendEnter => Some(format!("Also: {send} send")),
            SmartAction::Focus if self.action_menu_can_answer_choice() => {
                Some(format!("Also: {send} send, {show} show"))
            }
            SmartAction::Focus => None,
        }
    }

    fn selected_action_line(
        &self,
        pane: &tmux::Pane,
        insight: PaneInsight,
        next_summary: &str,
    ) -> String {
        let keys = &self.ui_settings.keybindings;
        let phrase = match insight.status {
            PaneStatus::Waiting => match self.recommended_smart_action(pane, insight) {
                SmartAction::SendEnter => {
                    format!(
                        "{} continue",
                        KeyBindingsConfig::primary_label(&keys.smart_action)
                    )
                }
                SmartAction::Focus if self.action_menu_can_answer_choice() => {
                    format!(
                        "{} answer yes/no",
                        KeyBindingsConfig::primary_label(&keys.actions)
                    )
                }
                SmartAction::Focus => {
                    format!("{} reply", KeyBindingsConfig::primary_label(&keys.command))
                }
            },
            PaneStatus::Error | PaneStatus::Stuck => {
                format!("{} output", KeyBindingsConfig::primary_label(&keys.focus))
            }
            PaneStatus::Idle | PaneStatus::Unknown => {
                format!(
                    "{} show in tmux",
                    KeyBindingsConfig::primary_label(&keys.jump)
                )
            }
            PaneStatus::Running | PaneStatus::Done => next_summary.to_owned(),
        };

        format!(
            "Action: {}",
            action_phrase_with_detail(&phrase, next_summary)
        )
    }

    pub fn live_tail_lines(&self) -> Vec<String> {
        if self.visible_pane_indices().is_empty() {
            return self.empty_visible_pane_lines();
        }

        let Some(pane) = self.selected_pane() else {
            return vec![String::from("No pane selected.")];
        };

        let insight = self.pane_insight(pane);
        let report = self.effective_agent_report_for_pane(&pane.id);
        let mut lines = vec![
            format!("{} / {}", pane.session_name, pane.window_name),
            match insight.last_output_age {
                Some(age) => format!("{} | {}", insight.status.display_label(), format_age(age)),
                None => insight.status.display_label().to_owned(),
            },
        ];

        let recent = focus_recent_lines(
            self.latest_live_output_lines(&pane.id, MAX_LIVE_TAIL_SOURCE_LINES),
            MAX_LIVE_TAIL_SOURCE_LINES,
        );
        let summary = board_latest_detail(
            insight,
            pane.current_command.as_str(),
            report.as_ref(),
            &recent,
        );

        let show_summary = !summary.is_empty()
            && !summary.eq_ignore_ascii_case("none")
            && (report.is_some() || !recent.is_empty());
        if show_summary {
            lines.push(String::from("Summary"));
            lines.push(format!("  {}", truncate_for_panel(&summary)));
        }

        if recent.is_empty() {
            lines.push(String::from("No output yet."));
            return lines;
        }

        let tail = output_tail_lines(
            pane.current_command.as_str(),
            &recent,
            show_summary.then_some(summary.as_str()),
        );
        if !tail.is_empty() {
            lines.push(String::from("Latest"));
            for line in tail {
                lines.push(format!("  {line}"));
            }
        }

        lines
    }

    fn empty_visible_pane_lines(&self) -> Vec<String> {
        if self.search_query.is_empty() {
            if let Some(lines) = self.startup_recovery_lines() {
                return lines;
            }
            let refresh = KeyBindingsConfig::primary_label(&self.ui_settings.keybindings.refresh);
            vec![
                String::from("No panes yet."),
                format!("Start tmux panes, then {refresh} refresh."),
            ]
        } else {
            vec![
                String::from("No matching panes."),
                String::from("Action: backspace show all panes"),
            ]
        }
    }

    fn startup_recovery_lines(&self) -> Option<Vec<String>> {
        let message = self.status_message().trim();
        let refresh = KeyBindingsConfig::primary_label(&self.ui_settings.keybindings.refresh);

        if message.starts_with("No tmux server found") {
            return Some(vec![
                String::from("No tmux server."),
                format!("Start tmux, then {refresh} refresh."),
            ]);
        }

        if message.starts_with("Session not found") {
            return Some(vec![
                String::from("Session not found."),
                format!("Use another session, then {refresh} refresh."),
            ]);
        }

        if message.starts_with("Could not read tmux panes") {
            return Some(vec![
                String::from("Cannot read tmux panes."),
                format!("Check socket/session, then {refresh} refresh."),
            ]);
        }

        None
    }
}

fn format_agent_lane_line(lane: AgentLane) -> String {
    let selected = if lane.selected { ">" } else { " " };
    let hottest = if lane.error > 0 {
        count_label(lane.error, "error", "errors")
    } else if lane.waiting > 0 {
        count_label(lane.waiting, "waiting", "waiting")
    } else if lane.stuck > 0 {
        count_label(lane.stuck, "stuck", "stuck")
    } else if lane.running > 0 {
        count_label(lane.running, "running", "running")
    } else if lane.done > 0 {
        count_label(lane.done, "done", "done")
    } else if lane.idle > 0 {
        count_label(lane.idle, "idle", "idle")
    } else {
        count_label(lane.unknown, "checking", "checking")
    };

    format!(
        "{selected} {}: {} | {hottest}",
        lane.workload.short_label(),
        pane_count_label(lane.total)
    )
}

fn attention_summary_label(waiting: usize, error: usize, stuck: usize, review: usize) -> String {
    let mut parts = Vec::new();
    if error > 0 {
        parts.push(count_label(error, "error", "errors"));
    }
    if stuck > 0 {
        parts.push(count_label(stuck, "stuck", "stuck"));
    }
    if waiting > 0 {
        parts.push(count_label(waiting, "waiting", "waiting"));
    }
    if review > 0 {
        parts.push(count_label(review, "review", "reviews"));
    }

    if parts.is_empty() {
        String::from("none")
    } else {
        parts.join(", ")
    }
}

fn agent_count_or_none(count: usize) -> String {
    if count == 0 {
        String::from("none")
    } else {
        count_label(count, "agent", "agents")
    }
}

fn attention_action_label(status: PaneStatus) -> &'static str {
    match status {
        PaneStatus::Waiting => "continue",
        PaneStatus::Error | PaneStatus::Stuck => "review",
        PaneStatus::Running => "watch",
        PaneStatus::Done => "review",
        PaneStatus::Idle => "idle",
        PaneStatus::Unknown => "check",
    }
}

fn lifecycle_label(
    status: PaneStatus,
    acknowledged: bool,
    pending_attention: bool,
    attention_pending: bool,
) -> &'static str {
    if pending_attention && attention_pending {
        return "watching";
    }

    if acknowledged && attention_pending {
        return "muted";
    }

    match status {
        PaneStatus::Running => "working",
        PaneStatus::Waiting => "needs you",
        PaneStatus::Done if attention_pending => "review",
        PaneStatus::Done => "done",
        PaneStatus::Error => "failed",
        PaneStatus::Stuck => "stale",
        PaneStatus::Idle => "quiet",
        PaneStatus::Unknown => "checking",
    }
}

impl BoardRow {
    pub(crate) fn flags(&self) -> String {
        let status_flag = if self.staged {
            ":"
        } else if self.attention == "!" {
            "!"
        } else if self.attention == "~" {
            "~"
        } else if self.targeted || self.marked {
            "+"
        } else if self.active {
            "*"
        } else {
            " "
        };

        format!("{}{}", if self.selected { ">" } else { " " }, status_flag)
    }

    pub(crate) fn tone(&self) -> BoardRowTone {
        if self.selected {
            BoardRowTone::Selected
        } else if self.staged {
            BoardRowTone::Staged
        } else if self.attention == "!" {
            if matches!(self.status.as_str(), "waiting" | "done") {
                BoardRowTone::Attention
            } else {
                BoardRowTone::Alert
            }
        } else if self.attention == "~" {
            BoardRowTone::Watching
        } else if self.targeted {
            BoardRowTone::Targeted
        } else if matches!(
            self.status.as_str(),
            "idle" | "done" | "checking" | "unknown"
        ) {
            BoardRowTone::Subdued
        } else {
            BoardRowTone::Default
        }
    }

    pub(crate) fn compact_latest(&self) -> String {
        let latest = self.latest_for_scannable_layout();
        if latest.is_empty() {
            compact_status_token(&self.status).to_owned()
        } else if matches!(self.status.as_str(), "waiting" | "error" | "stuck") {
            format!("{}: {latest}", compact_status_token(&self.status))
        } else if self.show_command_in_latest && is_provider_command_label(&self.command) {
            command_prefixed_latest(&self.command, &self.title, " ")
        } else {
            format!("{} {}", compact_status_token(&self.status), latest)
        }
    }

    pub(crate) fn standard_latest(&self) -> String {
        self.latest_for_scannable_layout()
    }

    fn latest_for_scannable_layout(&self) -> String {
        if self.title.is_empty() {
            return self.command.clone();
        }

        if self.show_command_in_latest
            && matches!(self.status.as_str(), "waiting" | "error" | "stuck")
        {
            return self.title.clone();
        }

        let redundant_status_title = self.title == self.lifecycle
            || self.title == self.status
            || self.title == compact_status_token(&self.status);
        if !self.show_command_in_latest && redundant_status_title {
            return String::new();
        }

        if self.show_command_in_latest {
            command_prefixed_latest(&self.command, &self.title, ": ")
        } else {
            self.title.clone()
        }
    }
}

fn compact_status_token(status: &str) -> &str {
    match status {
        "waiting" => "needs you",
        "running" => "working",
        "error" => "failed",
        "stuck" => "stuck",
        "done" => "done",
        "idle" => "quiet",
        "checking" | "unknown" | "unk" => "checking",
        _ => status,
    }
}

fn is_provider_command_label(command: &str) -> bool {
    matches!(
        command,
        "codex" | "claude" | "opencode" | "aider" | "gemini"
    )
}

fn command_prefixed_latest(command: &str, title: &str, separator: &str) -> String {
    if title_already_starts_with_command(command, title) {
        title.to_owned()
    } else {
        format!("{command}{separator}{title}")
    }
}

fn title_already_starts_with_command(command: &str, title: &str) -> bool {
    let command = command.trim();
    if command.is_empty() {
        return false;
    }

    let normalized_title = title.trim_start().to_ascii_lowercase();
    let normalized_command = command.to_ascii_lowercase();
    let Some(rest) = normalized_title.strip_prefix(&normalized_command) else {
        return false;
    };

    rest.chars()
        .next()
        .is_none_or(|ch| ch.is_whitespace() || matches!(ch, ':' | '|' | '-' | '/'))
}

fn board_command_label(pane: &tmux::Pane, workload: WorkloadKind) -> (String, bool) {
    let base = board_tool_label(workload, pane.current_command.as_str());

    if should_prefix_tool_in_latest(&base) {
        return (base, true);
    }

    if is_generic_launcher_command(pane.current_command.as_str())
        && let Some(identity) = board_context_identity_label(pane)
    {
        return (identity, true);
    }

    (base, false)
}

fn board_tool_label(workload: WorkloadKind, command: &str) -> String {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return workload.short_label().to_owned();
    }

    match workload {
        WorkloadKind::Codex
        | WorkloadKind::ClaudeCode
        | WorkloadKind::Opencode
        | WorkloadKind::Aider
        | WorkloadKind::Gemini => workload.short_label().to_owned(),
        WorkloadKind::Agent if is_generic_launcher_command(trimmed) => String::from("agent"),
        _ => trimmed.to_owned(),
    }
}

fn board_context_identity_label(pane: &tmux::Pane) -> Option<String> {
    path_identity_label(&pane.current_path)
        .filter(|label| is_informative_identity_label(label, pane))
        .or_else(|| {
            let title = pane.title.trim();
            is_informative_identity_label(title, pane).then(|| title.to_owned())
        })
}

fn path_identity_label(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }

    Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(ToOwned::to_owned)
}

fn is_informative_identity_label(label: &str, pane: &tmux::Pane) -> bool {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return false;
    }

    let normalized = trimmed.to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "workspace" | "terminal" | "shell" | "home" | "tmp" | "projects"
    ) {
        return false;
    }

    ![
        pane.current_command.as_str(),
        pane.window_name.as_str(),
        pane.session_name.as_str(),
    ]
    .into_iter()
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .any(|value| normalized == value.to_ascii_lowercase())
}

fn board_latest_detail(
    insight: PaneInsight,
    command: &str,
    report: Option<&AgentReport>,
    recent_lines: &[String],
) -> String {
    let key = board_latest_detail_cache_key(insight, command, report, recent_lines);
    if let Some(value) = BOARD_LATEST_DETAIL_CACHE.with(|cache| {
        let cache = cache.borrow();
        cache
            .as_ref()
            .filter(|cached| cached.key == key)
            .map(|cached| cached.value.clone())
    }) {
        return value;
    }

    let value = board_latest_detail_uncached(insight, command, report, recent_lines);
    BOARD_LATEST_DETAIL_CACHE.with(|cache| {
        *cache.borrow_mut() = Some(CachedBoardLatestDetail {
            key,
            value: value.clone(),
        });
    });
    value
}

fn board_latest_detail_uncached(
    insight: PaneInsight,
    command: &str,
    report: Option<&AgentReport>,
    recent_lines: &[String],
) -> String {
    let summary = activity_summary(insight.workload, command, None, recent_lines);
    let fallback_detail = clean_board_detail(
        &summary
            .split_once(" -> ")
            .map(|(_, tail)| tail.trim().to_owned())
            .filter(|tail| !tail.is_empty())
            .unwrap_or(summary),
    );

    if let Some(report) = report {
        let report_status = infer_status_from_report(report).unwrap_or(insight.status);
        if let Some(detail) =
            board_report_detail(report_status, report, recent_lines, &fallback_detail)
        {
            return detail;
        }
    }

    fallback_detail
}

fn board_latest_detail_cache_key(
    insight: PaneInsight,
    command: &str,
    report: Option<&AgentReport>,
    recent_lines: &[String],
) -> u64 {
    let mut hasher = DefaultHasher::new();
    insight.workload.hash(&mut hasher);
    insight.status.display_label().hash(&mut hasher);
    command.hash(&mut hasher);
    if let Some(report) = report {
        report.status.hash(&mut hasher);
        report.blocker.hash(&mut hasher);
        report.next.hash(&mut hasher);
    }
    recent_lines.len().hash(&mut hasher);
    for line in recent_lines {
        line.hash(&mut hasher);
    }
    hasher.finish()
}

fn agent_mission_summary(
    insight: PaneInsight,
    command: &str,
    report: Option<&AgentReport>,
    recent_lines: &[String],
) -> String {
    let mission =
        clean_mission_summary(&board_latest_detail(insight, command, report, recent_lines));
    if insight.status == PaneStatus::Unknown
        && (mission.is_empty()
            || mission.eq_ignore_ascii_case(command.trim())
            || mission.eq_ignore_ascii_case(insight.workload.short_label()))
    {
        return String::from("checking");
    }

    mission
}

fn clean_mission_summary(summary: &str) -> String {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let normalized = trimmed.to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "none" | "unknown" | "checking" | "idle" | "quiet" | "no output yet"
    ) {
        return String::new();
    }

    trimmed.to_owned()
}

fn is_generic_launcher_command(command: &str) -> bool {
    matches!(
        command.to_ascii_lowercase().as_str(),
        "node" | "python" | "python3" | "bun" | "ruby" | "uv" | "bash" | "zsh" | "sh" | "fish"
    )
}

fn clean_board_detail(detail: &str) -> String {
    let trimmed = detail.trim();
    if let Some(value) = trimmed.strip_prefix("approval: ") {
        return value.trim().to_owned();
    }
    if let Some(value) = trimmed.strip_prefix("error: ") {
        return value.trim().to_owned();
    }
    if let Some(value) = trimmed.strip_prefix("tool: ") {
        return format!("tool {value}");
    }

    let normalized = classify_fallback_summary(trimmed).into_normalized();
    compact_board_progress_phrase(&normalized)
}

fn compact_board_progress_phrase(detail: &str) -> String {
    let stripped = strip_step_prefix(detail).unwrap_or(detail).trim();
    let Some((first, rest)) = stripped.split_once(' ') else {
        return compact_progress_verb(stripped)
            .unwrap_or(stripped)
            .to_owned();
    };
    let rest = compact_progress_object(rest);
    if rest.is_empty() {
        return stripped.to_owned();
    }

    let verb = compact_progress_verb(first);

    verb.map(|verb| format!("{verb} {rest}"))
        .unwrap_or_else(|| stripped.to_owned())
}

fn compact_progress_verb(verb: &str) -> Option<&'static str> {
    match verb.to_ascii_lowercase().as_str() {
        "preparing" => Some("prep"),
        "prepped" => Some("prep"),
        "building" => Some("build"),
        "syncing" => Some("sync"),
        "validating" => Some("check"),
        "notifying" => Some("notify"),
        "loading" => Some("load"),
        "writing" => Some("write"),
        "reading" => Some("read"),
        "running" => Some("run"),
        "compiling" => Some("compile"),
        "installing" => Some("install"),
        "analyzing" | "analysing" => Some("analyze"),
        "searching" => Some("search"),
        "completed" => Some("complete"),
        _ => None,
    }
}

fn compact_progress_object(detail: &str) -> String {
    let mut words = detail.split_whitespace().collect::<Vec<_>>();
    while matches!(
        words.first().copied(),
        Some("the" | "a" | "an" | "this" | "that" | "your" | "our" | "my")
    ) {
        words.remove(0);
    }

    let trimmed = words.join(" ");
    if trimmed.is_empty() {
        return trimmed;
    }

    let clause_markers = [
        " across ",
        " with ",
        " using ",
        " via ",
        " from ",
        " into ",
        " onto ",
        " over ",
        " after ",
        " before ",
        " while ",
        " during ",
        " inside ",
        " outside ",
        " against ",
        " for ",
    ];

    if let Some((index, marker)) = clause_markers
        .iter()
        .filter_map(|marker| trimmed.find(marker).map(|index| (index, *marker)))
        .min_by_key(|(index, _)| *index)
    {
        let head = trimmed[..index].trim();
        if !head.is_empty() {
            let _ = marker;
            return head.to_owned();
        }
    }

    compact_signal_object_phrase(&trimmed)
        .or_else(|| compact_object_modifiers(&trimmed))
        .unwrap_or(trimmed)
}

fn compact_signal_object_phrase(detail: &str) -> Option<String> {
    let semantics = classify_fallback_summary(detail);
    let phrase = semantics.signal_phrase()?;
    let lower = semantics.normalized().to_ascii_lowercase();

    if let Some(index) = lower.find(phrase) {
        return semantics
            .normalized()
            .get(index..index + phrase.len())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
    }

    None
}

fn compact_object_modifiers(detail: &str) -> Option<String> {
    let mut words = detail.split_whitespace().collect::<Vec<_>>();
    while matches!(
        words.first().copied(),
        Some(
            "very"
                | "long"
                | "internal"
                | "external"
                | "final"
                | "local"
                | "remote"
                | "multiple"
                | "multi"
                | "extra"
                | "new"
                | "old"
                | "latest"
                | "current"
        )
    ) {
        words.remove(0);
    }

    let compacted = words.join(" ");
    (compacted != detail && !compacted.is_empty()).then_some(compacted)
}

fn strip_step_prefix(detail: &str) -> Option<&str> {
    let trimmed = detail.trim();
    let lower = trimmed.to_ascii_lowercase();
    let rest = lower.strip_prefix("step ")?;
    let offset = trimmed.len() - rest.len();
    let remainder = trimmed.get(offset..)?.trim_start();
    let mut chars = remainder.chars();
    let first = chars.next()?;

    let remainder = if first.is_ascii_digit() {
        let digits_len = remainder
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .map(char::len_utf8)
            .sum::<usize>();
        remainder.get(digits_len..)?.trim_start()
    } else {
        let word_len = remainder
            .chars()
            .take_while(|ch| ch.is_ascii_alphabetic())
            .map(char::len_utf8)
            .sum::<usize>();
        remainder.get(word_len..)?.trim_start()
    };

    Some(remainder)
}

fn board_report_detail(
    status: PaneStatus,
    report: &AgentReport,
    recent_lines: &[String],
    fallback_detail: &str,
) -> Option<String> {
    if status == PaneStatus::Waiting
        && let Some(prompt_action) = board_prompt_action_detail(recent_lines)
    {
        return Some(prompt_action);
    }

    let blocker = cleaned_inspector_report_field(report.blocker.as_str());
    let next = cleaned_inspector_report_field(report.next.as_str());

    match status {
        PaneStatus::Waiting | PaneStatus::Error | PaneStatus::Stuck => {
            if let Some(blocker) = blocker {
                return Some(blocker);
            }

            if let Some(next) = next {
                if report_next_is_too_generic_for_fallback(&next, fallback_detail) {
                    return (!fallback_detail.is_empty()).then(|| fallback_detail.to_owned());
                }
                return Some(next);
            }
        }
        _ => {
            if let Some(next) = next {
                if report_next_is_too_generic_for_fallback(&next, fallback_detail) {
                    return (!fallback_detail.is_empty()).then(|| fallback_detail.to_owned());
                }
                return Some(next);
            }

            if let Some(blocker) = blocker {
                return Some(blocker);
            }
        }
    }

    None
}

fn board_prompt_action_detail(recent_lines: &[String]) -> Option<String> {
    if recent_lines.iter().any(|line| matches_enter_hint(line)) {
        return Some(String::from("continue"));
    }
    if recent_lines.iter().any(|line| matches_choice_hint(line)) {
        return Some(String::from("answer"));
    }
    None
}

fn should_prefix_tool_in_latest(tool: &str) -> bool {
    matches!(tool, "codex" | "claude" | "opencode" | "aider" | "gemini")
}

fn board_location_counts(
    panes: &[tmux::Pane],
    indices: &[usize],
) -> std::collections::HashMap<(String, String), usize> {
    let mut counts = std::collections::HashMap::new();
    for index in indices {
        let pane = &panes[*index];
        *counts
            .entry((pane.session_name.clone(), pane.window_name.clone()))
            .or_insert(0) += 1;
    }
    counts
}

fn board_location_label(
    pane: &tmux::Pane,
    counts: &std::collections::HashMap<(String, String), usize>,
) -> String {
    let base = format!("{}/{}", pane.session_name, pane.window_name);
    let key = (pane.session_name.clone(), pane.window_name.clone());
    if counts.get(&key).copied().unwrap_or(0) > 1 {
        format!("{base}#{}", pane.pane_index)
    } else {
        base
    }
}

#[derive(Clone, Copy)]
struct InspectorSurface<'a> {
    blocker: Option<&'a str>,
    next: &'a str,
}

fn inspector_latest_lines(
    insight: PaneInsight,
    command: &str,
    report: Option<&AgentReport>,
    recent: &[String],
    surface: InspectorSurface<'_>,
    max_lines: usize,
) -> Vec<String> {
    let mut lines = Vec::new();
    if max_lines == 0 {
        return lines;
    }
    if recent.is_empty() {
        return lines;
    }
    let surface_keys = surface.keys();
    let mut seen_keys = HashSet::new();
    let summary = board_latest_detail(insight, command, report, recent);
    if !summary.is_empty()
        && !summary.eq_ignore_ascii_case("none")
        && !surface_keys.contains(&inspector_latest_line_key(&summary))
    {
        seen_keys.insert(inspector_latest_line_key(&summary));
        lines.push(summary.clone());
    }

    let mut tail = Vec::new();
    for line in recent.iter().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_redundant_inspector_latest_line(trimmed, command, report) {
            continue;
        }
        let display_line = clean_inspector_latest_line(trimmed);
        let display_key = inspector_latest_line_key(&display_line);
        if display_line.is_empty()
            || surface_keys.contains(&display_key)
            || !seen_keys.insert(display_key)
        {
            continue;
        }
        tail.push(display_line);
        if tail.len() >= max_lines {
            break;
        }
    }

    tail.reverse();
    lines.extend(tail);
    if lines.len() < max_lines {
        for line in recent.iter().rev() {
            let trimmed = line.trim();
            let normalized = trimmed.to_ascii_lowercase();
            if trimmed.is_empty()
                || is_agent_report_protocol_line(trimmed)
                || trimmed.eq_ignore_ascii_case(command)
                || trimmed
                    .eq_ignore_ascii_case(board_tool_label(WorkloadKind::Job, command).as_str())
                || matches!(
                    normalized.as_str(),
                    "codex" | "claude" | "opencode" | "aider" | "gemini" | "agent"
                )
            {
                continue;
            }
            let display_line = clean_inspector_latest_line(trimmed);
            let display_key = inspector_latest_line_key(&display_line);
            if display_line.is_empty()
                || surface_keys.contains(&display_key)
                || !seen_keys.insert(display_key)
            {
                continue;
            }
            lines.push(display_line);
            if lines.len() >= max_lines {
                break;
            }
        }
    }
    lines
}

impl InspectorSurface<'_> {
    fn keys(&self) -> HashSet<String> {
        self.blocker
            .into_iter()
            .chain(std::iter::once(self.next))
            .map(inspector_latest_line_key)
            .collect()
    }
}

fn clean_inspector_latest_line(line: &str) -> String {
    if looks_like_summary_request_template(line)
        || looks_like_provider_scaffold_line(line)
        || is_agent_report_protocol_line(line)
    {
        return String::new();
    }

    clean_board_detail(line.trim())
}

fn inspector_latest_line_key(line: &str) -> String {
    clean_board_detail(line)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn is_redundant_inspector_latest_line(
    line: &str,
    command: &str,
    report: Option<&AgentReport>,
) -> bool {
    let normalized = line.trim();
    if looks_like_summary_request_template(normalized)
        || looks_like_provider_scaffold_line(normalized)
        || is_agent_report_protocol_line(normalized)
    {
        return true;
    }

    if normalized.eq_ignore_ascii_case(command)
        || normalized.eq_ignore_ascii_case(board_tool_label(WorkloadKind::Job, command).as_str())
    {
        return true;
    }

    if matches!(
        normalized.to_ascii_lowercase().as_str(),
        "codex" | "claude" | "opencode" | "aider" | "gemini" | "agent"
    ) {
        return true;
    }

    report.is_some_and(|report| {
        normalized.eq_ignore_ascii_case(report.status.as_str())
            || normalized.eq_ignore_ascii_case(report.blocker.as_str())
            || normalized.eq_ignore_ascii_case(report.next.as_str())
    })
}

fn output_tail_lines(command: &str, recent: &[String], summary: Option<&str>) -> Vec<String> {
    let summary_keys = keys_from_summary(summary);
    let mut lines = Vec::new();
    let mut seen_keys = HashSet::new();

    for line in recent {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_redundant_output_tail_line(trimmed, command, summary) {
            continue;
        }

        let key = inspector_latest_line_key(trimmed);
        if summary_keys.contains(&key) || !seen_keys.insert(key) {
            continue;
        }
        lines.push(trimmed.to_owned());
    }

    if lines.is_empty() && summary.is_none() {
        lines = recent
            .iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect();
    }

    lines
}

fn keys_from_summary(summary: Option<&str>) -> HashSet<String> {
    summary.into_iter().map(inspector_latest_line_key).collect()
}

fn inspector_blocker_summary(report: Option<&AgentReport>) -> Option<String> {
    let report = report?;
    cleaned_inspector_report_field(report.blocker.as_str())
}

fn attention_problem_label(status: PaneStatus) -> &'static str {
    if status == PaneStatus::Error {
        "Problem"
    } else {
        "Blocked"
    }
}

fn inspector_next_summary(report: Option<&AgentReport>, fallback: &str) -> String {
    if let Some(report) = report
        && let Some(next) = cleaned_inspector_report_field(report.next.as_str())
    {
        if report_next_is_too_generic_for_fallback(&next, fallback) {
            return fallback.to_owned();
        }
        return next;
    }

    fallback.to_owned()
}

fn report_next_is_too_generic_for_fallback(next: &str, fallback: &str) -> bool {
    let normalized_next = next.trim().to_ascii_lowercase();
    let normalized_fallback = fallback.trim().to_ascii_lowercase();

    let generic_next = matches!(
        normalized_next.as_str(),
        "show output"
            | "show pane"
            | "inspect"
            | "continue work"
            | "running"
            | "open dialog"
            | "show agents"
            | "resume"
            | "answer"
            | "approve"
            | "show prompt"
            | "watch progress"
            | "open in tmux"
            | "show in tmux"
            | "review result"
    );
    let generic_fallback = matches!(
        normalized_fallback.as_str(),
        "show it"
            | "review output"
            | "show output"
            | "inspect it"
            | "show prompt"
            | "watch progress"
            | "open in tmux"
            | "show in tmux"
            | "review result"
    );

    generic_next && !generic_fallback
}

fn action_phrase_with_detail(phrase: &str, detail: &str) -> String {
    let detail = detail.trim();
    if detail.is_empty() || action_detail_is_redundant(phrase, detail) {
        return phrase.to_owned();
    }

    format!("{phrase} for {detail}")
}

fn action_detail_is_redundant(phrase: &str, detail: &str) -> bool {
    let normalized_phrase = phrase.trim().to_ascii_lowercase();
    let normalized_detail = detail.trim().to_ascii_lowercase();
    let phrase_without_key = normalized_phrase
        .split_once(' ')
        .map_or(normalized_phrase.as_str(), |(_, tail)| tail);

    normalized_detail == normalized_phrase
        || normalized_detail == phrase_without_key
        || phrase_without_key == "send reply"
        || phrase_without_key == "reply"
        || matches!(
            normalized_detail.as_str(),
            "answer"
                | "continue"
                | "press enter"
                | "reply"
                | "send reply"
                | "show output"
                | "show prompt"
                | "show in tmux"
                | "wait"
                | "ready"
                | "watch progress"
                | "review result"
        )
        || normalized_detail.starts_with("press enter ")
}

fn cleaned_inspector_report_field(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("none")
        || looks_like_report_placeholder(trimmed)
    {
        return None;
    }

    let cleaned = clean_board_detail(trimmed);
    Some(match cleaned.to_ascii_lowercase().as_str() {
        "inspect" | "inspect it" | "inspect output" | "inspect pane" => String::from("show output"),
        "inspect prompt" => String::from("show prompt"),
        "open in tmux" => String::from("show in tmux"),
        _ => cleaned,
    })
}

fn looks_like_report_placeholder(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "<status>"
            | "<blocker>"
            | "<next>"
            | "status=<status>"
            | "blocker=<blocker>"
            | "next=<next>"
            | "next=<next>."
    )
}

fn is_redundant_output_tail_line(line: &str, command: &str, summary: Option<&str>) -> bool {
    let normalized = line.trim();
    if looks_like_summary_request_template(normalized)
        || looks_like_provider_scaffold_line(normalized)
        || (summary.is_some() && is_agent_report_protocol_line(normalized))
    {
        return true;
    }

    if normalized.eq_ignore_ascii_case(command)
        || normalized.eq_ignore_ascii_case(board_tool_label(WorkloadKind::Job, command).as_str())
    {
        return true;
    }

    matches!(
        normalized.to_ascii_lowercase().as_str(),
        "codex" | "claude" | "opencode" | "aider" | "gemini" | "agent"
    )
}

fn looks_like_summary_request_template(line: &str) -> bool {
    let normalized = line.trim().to_ascii_lowercase();
    normalized.contains("reply in exactly one line as:")
        || normalized.contains("status=<status>")
        || normalized.contains("blocker=<blocker>")
        || normalized.contains("next=<next>")
}

fn looks_like_provider_scaffold_line(line: &str) -> bool {
    let normalized = line.trim().to_ascii_lowercase();
    normalized.contains("background terminal running")
        || normalized.contains("esc to interrupt")
        || normalized.contains("# config change detected")
        || ((normalized.contains("~/")
            || normalized.contains("/users/")
            || normalized.contains("/home/"))
            && (normalized.contains("gpt-")
                || normalized.contains("claude-")
                || normalized.contains("sonnet")
                || normalized.contains("opus")))
}

fn join_and_truncate(parts: Vec<String>, width: u16) -> String {
    let max_width = usize::from(width.max(24));
    let mut line = String::new();
    for part in parts {
        let candidate = if line.is_empty() {
            part
        } else {
            format!("{line}  {part}")
        };
        if candidate.chars().count() <= max_width {
            line = candidate;
        } else if line.is_empty() {
            line = truncate_for_width(&candidate, max_width);
            break;
        } else {
            break;
        }
    }

    line
}

fn is_low_value_footer_status(message: &str) -> bool {
    message.starts_with("Showing output for ")
        || message.starts_with("Showing summary for ")
        || message.starts_with("Already showing ")
        || message == "Showing all panes."
        || message.starts_with("No tmux server found")
        || message.starts_with("Session not found")
        || message.starts_with("Could not read tmux panes")
        || (message.starts_with("Added ") && message.ends_with(" to the send list."))
        || (message.starts_with("Removed ") && message.ends_with(" from the send list."))
        || (message.starts_with("Cleared ") && message.ends_with(" from the send list."))
}

fn status_deserves_footer_over_narrowing(message: &str) -> bool {
    message.starts_with("No panes remain")
        || message == "No window selected in Browse."
        || message.starts_with("Show all panes before ")
        || message.starts_with("Sent `")
        || message.contains(" disappeared.")
}

fn status_deserves_compact_footer_feedback(message: &str) -> bool {
    message.starts_with("Action failed:")
        || message.starts_with("Sent Enter")
        || message.starts_with("Sent reply")
        || message.starts_with("Answered ")
        || message.starts_with("Layout:")
}

fn status_is_theme_feedback(message: &str) -> bool {
    message.starts_with("Theme: ")
}

fn push_gap(lines: &mut Vec<String>) {
    if !lines.is_empty() && lines.last().is_some_and(|line| !line.is_empty()) {
        lines.push(String::new());
    }
}

fn focus_recent_lines(lines: Vec<String>, limit: usize) -> Vec<String> {
    let filtered = lines
        .into_iter()
        .filter(|line| !is_noise_recent_line(line))
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    filtered
        .into_iter()
        .rev()
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn is_noise_recent_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return true;
    }
    if looks_like_inline_echo_fragment(trimmed) {
        return true;
    }
    is_terminal_chatter_noise(trimmed) || matches!(trimmed, "❯" | ">" | ">>")
}

fn looks_like_inline_echo_fragment(line: &str) -> bool {
    let char_count = line.chars().count();
    if char_count <= 1 {
        return true;
    }

    char_count <= 2
        && !line.chars().any(char::is_whitespace)
        && line.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(ch, '_' | '-' | '.' | '/' | ':' | ';' | ',' | '\'' | '"')
        })
}

fn push_section(lines: &mut Vec<String>, heading: &str) {
    push_gap(lines);
    lines.push(String::from(heading));
}

fn attention_queue_heading(count: usize) -> String {
    if count == 1 {
        String::from("Queue")
    } else {
        format!("Queue ({count})")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BoardRow, BoardRowTone, clean_board_detail, compact_status_token, focus_recent_lines,
        is_noise_recent_line, looks_like_inline_echo_fragment, output_tail_lines,
    };

    #[test]
    fn inline_echo_fragments_are_treated_as_noise() {
        assert!(looks_like_inline_echo_fragment("c"));
        assert!(looks_like_inline_echo_fragment("ok"));
        assert!(!looks_like_inline_echo_fragment("reply now"));
        assert!(!looks_like_inline_echo_fragment("STATUS=running"));
    }

    #[test]
    fn recent_lines_drop_single_character_echoes() {
        let lines = focus_recent_lines(
            vec![
                String::from("c"),
                String::from("o"),
                String::from("continue?"),
                String::from("❯"),
            ],
            4,
        );
        assert_eq!(lines, vec![String::from("continue?")]);
    }

    #[test]
    fn shell_prompt_glyphs_are_noise() {
        assert!(is_noise_recent_line("❯"));
        assert!(is_noise_recent_line(">"));
        assert!(is_noise_recent_line(
            "muxboard: ali@tau:~/Projects/muxboard$"
        ));
        assert!(is_noise_recent_line(
            "For more details, please visit https://support.apple.com/kb/HT208050."
        ));
        assert!(!is_noise_recent_line("Waiting for approval. Continue?"));
    }

    #[test]
    fn board_row_flags_and_tone_follow_priority_order() {
        let row = BoardRow {
            selected: false,
            active: true,
            marked: true,
            targeted: true,
            staged: true,
            show_command_in_latest: false,
            attention: String::from("!"),
            status: String::from("waiting"),
            lifecycle: String::from("needs you"),
            mission: String::from("approve"),
            heat: String::new(),
            age: String::new(),
            cpu: String::new(),
            mem: String::new(),
            lane: String::new(),
            pane: String::new(),
            location: String::new(),
            command: String::new(),
            title: String::from("approve"),
        };

        assert_eq!(row.flags(), " :");
        assert_eq!(row.tone(), BoardRowTone::Staged);
        assert_eq!(row.compact_latest(), "needs you: approve");

        let mut targeted_waiting = row.clone();
        targeted_waiting.staged = false;
        assert_eq!(targeted_waiting.flags(), " !");
        assert_eq!(targeted_waiting.tone(), BoardRowTone::Attention);

        let mut targeted_error = targeted_waiting.clone();
        targeted_error.status = String::from("error");
        targeted_error.lifecycle = String::from("failed");
        assert_eq!(targeted_error.flags(), " !");
        assert_eq!(targeted_error.tone(), BoardRowTone::Alert);

        let mut watching = targeted_waiting;
        watching.attention = String::from("~");
        watching.lifecycle = String::from("watching");
        assert_eq!(watching.flags(), " ~");
        assert_eq!(watching.tone(), BoardRowTone::Watching);
    }

    #[test]
    fn compact_status_tokens_are_human_not_abbreviated() {
        assert_eq!(compact_status_token("waiting"), "needs you");
        assert_eq!(compact_status_token("running"), "working");
        assert_eq!(compact_status_token("error"), "failed");
        assert_eq!(compact_status_token("idle"), "quiet");
        assert_eq!(compact_status_token("checking"), "checking");
        assert_eq!(compact_status_token("unknown"), "checking");
    }

    #[test]
    fn scannable_latest_drops_redundant_status_titles() {
        let row = BoardRow {
            selected: false,
            active: false,
            marked: false,
            targeted: false,
            staged: false,
            show_command_in_latest: false,
            attention: String::new(),
            status: String::from("checking"),
            lifecycle: String::from("checking"),
            mission: String::from("checking"),
            heat: String::new(),
            age: String::new(),
            cpu: String::new(),
            mem: String::new(),
            lane: String::new(),
            pane: String::new(),
            location: String::new(),
            command: String::from("job"),
            title: String::from("checking"),
        };

        assert_eq!(row.standard_latest(), "");
        assert_eq!(row.compact_latest(), "checking");
    }

    #[test]
    fn quiet_board_rows_use_subdued_tone() {
        let row = BoardRow {
            selected: false,
            active: false,
            marked: false,
            targeted: false,
            staged: false,
            show_command_in_latest: false,
            attention: String::new(),
            status: String::from("idle"),
            lifecycle: String::from("quiet"),
            mission: String::new(),
            heat: String::new(),
            age: String::new(),
            cpu: String::new(),
            mem: String::new(),
            lane: String::new(),
            pane: String::new(),
            location: String::new(),
            command: String::from("bash"),
            title: String::from("waiting for input"),
        };

        assert_eq!(row.tone(), BoardRowTone::Subdued);
    }

    #[test]
    fn scannable_latest_prefixes_provider_tool_names() {
        let row = BoardRow {
            selected: false,
            active: false,
            marked: false,
            targeted: false,
            staged: false,
            show_command_in_latest: true,
            attention: String::new(),
            status: String::from("running"),
            lifecycle: String::from("working"),
            mission: String::from("write tests"),
            heat: String::new(),
            age: String::new(),
            cpu: String::new(),
            mem: String::new(),
            lane: String::new(),
            pane: String::new(),
            location: String::new(),
            command: String::from("codex"),
            title: String::from("write tests"),
        };

        assert_eq!(row.standard_latest(), "codex: write tests");
        assert_eq!(row.compact_latest(), "codex write tests");
    }

    #[test]
    fn scannable_latest_does_not_repeat_provider_names() {
        let row = BoardRow {
            selected: false,
            active: false,
            marked: false,
            targeted: false,
            staged: false,
            show_command_in_latest: true,
            attention: String::new(),
            status: String::from("running"),
            lifecycle: String::from("working"),
            mission: String::from("codex applying focused polish"),
            heat: String::new(),
            age: String::new(),
            cpu: String::new(),
            mem: String::new(),
            lane: String::new(),
            pane: String::new(),
            location: String::new(),
            command: String::from("codex"),
            title: String::from("codex applying focused polish"),
        };

        assert_eq!(row.standard_latest(), "codex applying focused polish");
        assert_eq!(row.compact_latest(), "codex applying focused polish");
    }

    #[test]
    fn scannable_latest_can_prefix_contextual_identity_labels() {
        let row = BoardRow {
            selected: false,
            active: false,
            marked: false,
            targeted: false,
            staged: false,
            show_command_in_latest: true,
            attention: String::new(),
            status: String::from("running"),
            lifecycle: String::from("working"),
            mission: String::from("building release artifacts"),
            heat: String::new(),
            age: String::new(),
            cpu: String::new(),
            mem: String::new(),
            lane: String::new(),
            pane: String::new(),
            location: String::new(),
            command: String::from("muxboard"),
            title: String::from("building release artifacts"),
        };

        assert_eq!(
            row.standard_latest(),
            "muxboard: building release artifacts"
        );
        assert_eq!(
            row.compact_latest(),
            "working muxboard: building release artifacts"
        );
    }

    #[test]
    fn attention_latest_prioritizes_blocker_over_tool_prefix() {
        let row = BoardRow {
            selected: false,
            active: false,
            marked: false,
            targeted: false,
            staged: false,
            show_command_in_latest: true,
            attention: String::from("!"),
            status: String::from("waiting"),
            lifecycle: String::from("needs you"),
            mission: String::from("network access"),
            heat: String::new(),
            age: String::new(),
            cpu: String::new(),
            mem: String::new(),
            lane: String::new(),
            pane: String::new(),
            location: String::new(),
            command: String::from("claude"),
            title: String::from("network access"),
        };

        assert_eq!(row.standard_latest(), "network access");
        assert_eq!(row.compact_latest(), "needs you: network access");
    }

    #[test]
    fn board_detail_cleaning_removes_protocol_prefixes() {
        assert_eq!(
            clean_board_detail("approval: network access"),
            "network access"
        );
        assert_eq!(
            clean_board_detail("error: command failed"),
            "command failed"
        );
        assert_eq!(clean_board_detail("tool: Bash"), "tool Bash");
    }

    #[test]
    fn board_detail_cleaning_compacts_common_progress_phrases() {
        assert_eq!(
            clean_board_detail("step one preparing release image"),
            "prep release image"
        );
        assert_eq!(
            clean_board_detail("step three validating checksums"),
            "check checksums"
        );
        assert_eq!(
            clean_board_detail("step four notifying staging"),
            "notify staging"
        );
        assert_eq!(
            clean_board_detail("step five completed handoff"),
            "complete handoff"
        );
        assert_eq!(
            clean_board_detail("building release artifacts"),
            "build release artifacts"
        );
        assert_eq!(
            clean_board_detail("preparing the release image for staging deploys"),
            "prep release image"
        );
        assert_eq!(
            clean_board_detail("syncing shell aliases from dotfiles repo"),
            "sync shell aliases"
        );
        assert_eq!(
            clean_board_detail("validating checksums across artifact mirrors"),
            "check checksums"
        );
        assert_eq!(
            clean_board_detail("preparing the internal artifact mirrors"),
            "prep artifact mirrors"
        );
        assert_eq!(
            clean_board_detail("completed the final staging handoff"),
            "complete staging handoff"
        );
        assert_eq!(
            clean_board_detail("preparing the very long release image manifest"),
            "prep release image"
        );
        assert_eq!(clean_board_detail("building..."), "build");
        assert_eq!(
            clean_board_detail("Type your answer..."),
            "Type your answer..."
        );
    }

    #[test]
    fn output_tail_lines_drop_redundant_tool_banners() {
        let tail = output_tail_lines(
            "node",
            &[
                String::from("codex"),
                String::from("STATUS=running | BLOCKER=none | NEXT=write tests"),
            ],
            None,
        );

        assert_eq!(
            tail,
            vec![String::from(
                "STATUS=running | BLOCKER=none | NEXT=write tests"
            )]
        );
    }

    #[test]
    fn output_tail_lines_drop_protocol_when_summary_already_has_signal() {
        let tail = output_tail_lines(
            "node",
            &[
                String::from("codex"),
                String::from("STATUS=running | BLOCKER=none | NEXT=write tests"),
            ],
            Some("write tests"),
        );

        assert!(tail.is_empty());
    }

    #[test]
    fn output_tail_lines_preserve_prompt_text_even_when_summary_is_distilled() {
        let tail = output_tail_lines(
            "opencode",
            &[String::from("Type your answer...")],
            Some("answer"),
        );

        assert_eq!(tail, vec![String::from("Type your answer...")]);
    }
}
