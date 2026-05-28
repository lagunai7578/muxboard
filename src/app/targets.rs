use super::*;

impl App {
    pub(super) fn effective_agent_report_for_pane(&self, pane_id: &str) -> Option<AgentReport> {
        let pane = self.snapshot.panes.iter().find(|pane| pane.id == pane_id)?;
        let insight = self.pane_insight(pane);
        self.effective_agent_report_for_pane_with_insight(pane_id, insight)
    }

    pub(super) fn effective_agent_report_for_pane_with_insight(
        &self,
        pane_id: &str,
        insight: PaneInsight,
    ) -> Option<AgentReport> {
        if let Some(report) = self
            .snapshot
            .panes
            .iter()
            .find(|pane| pane.id == pane_id)
            .and_then(|pane| pane.agent_event.as_ref())
            .and_then(|event| agent_bridge_report(event, insight.status))
        {
            return Some(report);
        }

        effective_agent_report(
            self.pane_runtime.get(pane_id),
            insight,
            self.pane_reports.get(pane_id),
        )
    }

    pub(super) fn selected_lane_workload(&self) -> Option<WorkloadKind> {
        let pane = self.selected_pane()?;
        let workload = self.pane_insight(pane).workload;
        workload.is_agent().then_some(workload)
    }

    pub(super) fn fanout_summary_for_selected(&self) -> String {
        if let Some(name) = &self.active_group_name {
            format!("fleet {name} ({})", self.active_target_count_summary())
        } else if self.using_marked_targets() {
            format!("send list ({})", self.active_target_count_summary())
        } else {
            match self.fanout_mode {
                FanoutMode::Off => String::from("off"),
                FanoutMode::Lane => {
                    format!(
                        "send to {}",
                        pane_count_label(self.active_target_panes().len())
                    )
                }
            }
        }
    }

    pub(super) fn active_target_description(&self) -> String {
        if let Some(name) = &self.active_group_name {
            format!("fleet {name} ({})", self.active_target_count_summary())
        } else if self.using_marked_targets() {
            format!("send list ({})", self.active_target_count_summary())
        } else {
            match self.fanout_mode {
                FanoutMode::Off => self
                    .selected_pane()
                    .map(|pane| self.pane_target_label(pane))
                    .unwrap_or_else(|| String::from("selected pane")),
                FanoutMode::Lane => self
                    .selected_lane_workload()
                    .map(|workload| {
                        format!(
                            "{} lane ({})",
                            workload.display_label(),
                            pane_count_label(self.active_target_panes().len())
                        )
                    })
                    .unwrap_or_else(|| String::from("selected lane")),
            }
        }
    }

    pub(super) fn pane_target_label(&self, pane: &tmux::Pane) -> String {
        let base = format!("{} / {}", pane.session_name, pane.window_name);
        let duplicate_count = self
            .snapshot
            .panes
            .iter()
            .filter(|candidate| {
                candidate.session_name == pane.session_name
                    && candidate.window_name == pane.window_name
            })
            .count();

        if duplicate_count > 1 {
            format!("{base} #{}", pane.pane_index)
        } else {
            base
        }
    }

    pub(super) fn pane_target_label_by_id(&self, pane_id: &str) -> String {
        self.snapshot
            .panes
            .iter()
            .find(|pane| pane.id == pane_id)
            .map(|pane| self.pane_target_label(pane))
            .unwrap_or_else(|| String::from("missing pane"))
    }

    pub(super) fn pane_target_label_by_id_for_current_view(&self, pane_id: &str) -> String {
        self.snapshot
            .panes
            .iter()
            .find(|pane| pane.id == pane_id)
            .map(|pane| {
                let hidden = if self.matches_pane_visibility(pane) {
                    ""
                } else {
                    " (hidden)"
                };
                format!("{}{}", self.pane_target_label(pane), hidden)
            })
            .unwrap_or_else(|| self.pane_target_label_by_id(pane_id))
    }

    pub(super) async fn send_command_text(&mut self, text: &str) -> Result<()> {
        self.dispatch_command_text(text, true, true).await?;
        Ok(())
    }

    pub(super) async fn send_reply_text(&mut self, text: &str) -> Result<()> {
        let command_center_reply_pane = if self.context_pane == ContextPane::Control
            && !self.using_explicit_targets()
            && self.fanout_mode == FanoutMode::Off
        {
            self.selected_pane().cloned().filter(|pane| {
                let insight = self.pane_insight(pane);
                self.command_center_can_reply_to_pane(pane, insight)
            })
        } else {
            None
        };
        let target_description = self.active_target_description();
        match self.dispatch_command_text(text, false, false).await? {
            CommandDispatchStatus::Dispatched(outcome) if outcome.sent_count == 0 => {
                self.status_message =
                    no_target_panes_remain_message("reply", outcome.disappeared_count);
            }
            CommandDispatchStatus::Dispatched(outcome) if outcome.disappeared_count > 0 => {
                self.status_message = format!(
                    "Sent reply to {}; {} disappeared.",
                    send_target_object_phrase(&target_description),
                    pane_count_label(outcome.disappeared_count)
                );
            }
            CommandDispatchStatus::Dispatched(_) => {
                if let Some(pane) = command_center_reply_pane {
                    self.mark_attention_action_pending(&pane, PendingAttentionActionKind::Reply);
                    self.select_next_attention_after_action(&pane);
                    self.status_message =
                        self.attention_action_status_after_send("Sent reply to", &pane);
                } else {
                    self.status_message = format!(
                        "Sent reply to {}.",
                        send_target_object_phrase(&target_description)
                    );
                }
            }
            CommandDispatchStatus::NoTargets | CommandDispatchStatus::Staged => {}
        }
        Ok(())
    }

    pub(super) async fn dispatch_command_text(
        &mut self,
        text: &str,
        remember: bool,
        allow_stage: bool,
    ) -> Result<CommandDispatchStatus> {
        let expanded = {
            let targets = self.active_target_panes();
            if targets.is_empty() {
                self.status_message = self.no_active_targets_message();
                return Ok(CommandDispatchStatus::NoTargets);
            }

            targets
                .iter()
                .map(|pane| {
                    (
                        pane.id.clone(),
                        expand_command_template(text, pane, self.pane_insight(pane).workload),
                    )
                })
                .collect::<Vec<_>>()
        };
        let target_count = expanded.len();

        if allow_stage && target_count > 1 {
            self.search_input_active = false;
            self.command_input_active = false;
            self.macro_assign_active = false;
            self.action_menu_active = false;
            self.group_input_active = false;
            self.launch_input_active = false;
            self.fleet_picker_active = false;
            self.help_overlay_active = false;

            let preview = expanded
                .iter()
                .map(|(pane_id, expanded_text)| (pane_id.clone(), expanded_text.clone()))
                .collect::<Vec<_>>();
            let target_description = self.active_target_description();
            self.pending_dispatch = Some(StagedDispatch {
                text: text.to_owned(),
                expanded: preview,
                remember,
                target_description: target_description.clone(),
            });
            self.status_message = format!(
                "Review send `{}` to {}. Enter sends, Esc cancels.",
                text,
                send_target_object_phrase(&target_description)
            );
            return Ok(CommandDispatchStatus::Staged);
        }

        let outcome = self.send_expanded_text(&expanded).await?;
        if outcome.sent_count == 0 {
            self.status_message =
                no_target_panes_remain_message(&format!("`{text}`"), outcome.disappeared_count);
            return Ok(CommandDispatchStatus::Dispatched(outcome));
        }

        let command_save_failure = if remember {
            self.remember_command(text);
            (!self.save_command_state()).then(|| self.status_message.clone())
        } else {
            None
        };
        let sent_message = if outcome.disappeared_count > 0 {
            format!(
                "Sent command `{}` to {}; {} disappeared.",
                text,
                pane_count_label(outcome.sent_count),
                pane_count_label(outcome.disappeared_count)
            )
        } else {
            format!(
                "Sent command `{}` to {} in {}.",
                text,
                pane_count_label(target_count),
                self.active_target_description()
            )
        };
        self.status_message = if let Some(failure) = command_save_failure {
            format!("{sent_message} {failure}")
        } else {
            sent_message
        };
        Ok(CommandDispatchStatus::Dispatched(outcome))
    }

    pub(super) fn summary_target_scope(&self) -> String {
        if let Some(name) = &self.active_group_name {
            return format!("fleet {name}");
        }
        if self.using_marked_targets() {
            return String::from("the send list");
        }

        match self.fanout_mode {
            FanoutMode::Off => self
                .selected_pane()
                .map(|pane| self.pane_target_label(pane))
                .unwrap_or_else(|| String::from("the selected pane")),
            FanoutMode::Lane => self
                .selected_lane_workload()
                .map(|workload| workload.display_label().to_string())
                .unwrap_or_else(|| String::from("the selected lane")),
        }
    }

    pub(super) fn command_preview_lines(&self) -> Vec<String> {
        let Some(template) =
            (!self.command_buffer.trim().is_empty()).then_some(self.command_buffer.as_str())
        else {
            return Vec::new();
        };

        let targets = self.active_target_panes();
        if targets.is_empty() {
            return vec![String::from("No panes yet.")];
        }
        let total_targets = targets.len();

        let preview_indices = self
            .preview_indices_with_hidden(&targets, 3, |pane| !self.matches_pane_visibility(pane));
        let mut lines = preview_indices
            .into_iter()
            .map(|index| {
                let pane = targets[index];
                let preview =
                    expand_command_template(template, pane, self.pane_insight(pane).workload);
                let hidden = if self.matches_pane_visibility(pane) {
                    ""
                } else {
                    " (hidden)"
                };
                format!(
                    "{}{} : {}",
                    self.pane_target_label(pane),
                    hidden,
                    truncate_for_panel(&preview)
                )
            })
            .collect::<Vec<_>>();

        if total_targets > lines.len() {
            lines.push(format!(
                "... : {}",
                more_pane_count_label(total_targets - lines.len())
            ));
        }

        lines
    }

    #[must_use]
    pub(super) fn upsert_target_group(&mut self, target_group: TargetGroup) -> bool {
        let target_name = target_group.name.clone();
        if let Some(index) = self
            .target_groups
            .iter()
            .position(|existing| existing.name == target_name)
        {
            self.target_groups[index] = target_group;
            self.selected_group_index = Some(index);
        } else {
            self.target_groups.push(target_group);
            self.target_groups
                .sort_by(|left, right| left.name.cmp(&right.name));
            self.selected_group_index = self
                .target_groups
                .iter()
                .position(|group| group.name == target_name);
        }
        self.active_group_name = Some(target_name);
        self.save_target_groups()
    }

    pub(super) fn apply_target_group(&mut self, index: usize) {
        let Some(group) = self.target_groups.get(index).cloned() else {
            self.status_message = String::from("Saved fleet no longer exists.");
            return;
        };

        let resolved = group
            .members
            .iter()
            .filter_map(|locator| {
                self.snapshot.panes.iter().find(|pane| {
                    pane.session_name == locator.session_name
                        && pane.window_name == locator.window_name
                        && pane.pane_index == locator.pane_index
                })
            })
            .map(|pane| pane.id.clone())
            .collect::<Vec<_>>();

        let first_resolved = resolved.first().cloned();
        self.selected_group_index = Some(index);
        self.active_group_name = Some(group.name.clone());
        self.marked_pane_ids = resolved.into_iter().collect();
        self.fanout_mode = FanoutMode::Off;

        if !self.marked_pane_ids.is_empty() {
            if !self
                .marked_pane_ids
                .contains(self.selected_pane_id.as_deref().unwrap_or_default())
            {
                self.selected_pane_id = first_resolved;
                self.sync_selected_window_from_selection();
            }
            self.status_message = format!(
                "Loaded fleet `{}` with {} live.",
                group.name,
                pane_count_label(self.marked_pane_ids.len())
            );
        } else {
            self.status_message = format!(
                "Fleet `{}` loaded, but none of its panes are live right now.",
                group.name
            );
        }
    }

    #[must_use]
    pub(super) fn save_target_groups(&mut self) -> bool {
        if let Err(error) = self.state_store.save_target_groups(&self.target_groups) {
            self.status_message = format!(
                "Fleet save failed at {}: {error}",
                self.state_store.path().display()
            );
            return false;
        }
        true
    }

    pub(super) fn active_target_report_lines(&self) -> Vec<String> {
        self.active_target_panes()
            .into_iter()
            .filter_map(|pane| {
                self.effective_agent_report_for_pane(&pane.id)
                    .map(|report| {
                        let target = self.pane_target_label_by_id_for_current_view(&pane.id);
                        format!(
                            "{}: {} | {} | {}",
                            truncate_for_panel(&target),
                            truncate_for_panel(&report.status),
                            truncate_for_panel(&report.blocker),
                            truncate_for_panel(&report.next)
                        )
                    })
            })
            .take(3)
            .collect()
    }

    pub(super) fn remember_command(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }

        self.recent_commands.retain(|entry| entry != trimmed);
        self.recent_commands.push_front(trimmed.to_owned());

        while self.recent_commands.len() > MAX_RECENT_COMMANDS {
            self.recent_commands.pop_back();
        }
    }

    pub(super) fn active_target_panes(&self) -> Vec<&tmux::Pane> {
        if self.using_explicit_targets() {
            self.marked_target_panes()
        } else {
            match self.fanout_mode {
                FanoutMode::Off => self.selected_pane().into_iter().collect(),
                FanoutMode::Lane => self.selected_lane_targets(),
            }
        }
    }

    pub(super) fn using_marked_targets(&self) -> bool {
        !self.marked_pane_ids.is_empty()
    }

    pub(super) fn using_explicit_targets(&self) -> bool {
        self.active_group_name.is_some() || self.using_marked_targets()
    }

    pub(super) fn no_active_targets_message(&self) -> String {
        if let Some(name) = &self.active_group_name {
            format!("Fleet `{name}` has no live panes.")
        } else if self.using_marked_targets() {
            String::from("Add a pane before sending.")
        } else {
            String::from("Select a pane first.")
        }
    }

    pub(super) fn no_live_target_list_line(&self) -> String {
        self.active_group_name
            .as_ref()
            .map(|name| format!("fleet {} has no live panes", truncate_for_panel(name)))
            .unwrap_or_else(|| String::from("send list has no live panes"))
    }

    pub(super) fn active_hidden_target_count(&self) -> usize {
        self.active_target_panes()
            .into_iter()
            .filter(|pane| !self.matches_pane_visibility(pane))
            .count()
    }

    pub(super) fn active_target_count_summary(&self) -> String {
        let total = self.active_target_panes().len();
        let hidden = self.active_hidden_target_count();
        if hidden == 0 {
            pane_count_label(total)
        } else {
            format!("{}, {hidden} hidden", pane_count_label(total))
        }
    }

    pub(super) fn hidden_target_note(&self) -> Option<String> {
        let hidden = self.active_hidden_target_count();
        (hidden > 0).then(|| {
            format!(
                "{} by current view",
                count_label(hidden, "pane hidden", "panes hidden")
            )
        })
    }

    pub(super) fn hidden_pending_target_note(
        &self,
        targets: &[(String, String)],
    ) -> Option<String> {
        let hidden = targets
            .iter()
            .filter_map(|(pane_id, _)| self.snapshot.panes.iter().find(|pane| pane.id == *pane_id))
            .filter(|pane| !self.matches_pane_visibility(pane))
            .count();
        (hidden > 0).then(|| {
            format!(
                "{} by current view",
                count_label(hidden, "pane hidden", "panes hidden")
            )
        })
    }

    pub(super) fn preview_indices_with_hidden<T>(
        &self,
        items: &[T],
        limit: usize,
        is_hidden: impl Fn(&T) -> bool,
    ) -> Vec<usize> {
        if items.is_empty() || limit == 0 {
            return Vec::new();
        }

        let preview_len = limit.min(items.len());
        let mut indices = (0..preview_len).collect::<Vec<_>>();
        let already_shows_hidden = indices.iter().any(|index| is_hidden(&items[*index]));
        if already_shows_hidden {
            return indices;
        }

        if let Some(hidden_index) =
            (preview_len..items.len()).find(|index| is_hidden(&items[*index]))
            && let Some(last) = indices.last_mut()
        {
            *last = hidden_index;
        }
        indices
    }

    #[cfg(test)]
    pub(super) fn is_in_active_target_set(&self, pane_id: &str) -> bool {
        self.active_target_panes()
            .into_iter()
            .any(|pane| pane.id == pane_id)
    }

    pub(super) fn marked_target_panes(&self) -> Vec<&tmux::Pane> {
        let mut targets = self
            .snapshot
            .panes
            .iter()
            .filter(|pane| self.marked_pane_ids.contains(&pane.id))
            .collect::<Vec<_>>();

        targets.sort_by(|left, right| {
            left.session_name
                .cmp(&right.session_name)
                .then_with(|| left.window_name.cmp(&right.window_name))
                .then_with(|| left.pane_index.cmp(&right.pane_index))
                .then_with(|| left.id.cmp(&right.id))
        });
        targets
    }

    pub(super) fn selected_lane_targets(&self) -> Vec<&tmux::Pane> {
        let Some(workload) = self.selected_lane_workload() else {
            return Vec::new();
        };

        self.snapshot
            .panes
            .iter()
            .filter(|pane| self.matches_base_filter(pane))
            .filter(|pane| self.pane_insight(pane).workload == workload)
            .collect()
    }
}
