use super::*;

impl App {
    pub(super) fn initialize_pane_status_cache(&mut self) {
        self.pane_last_status = self
            .snapshot
            .panes
            .iter()
            .map(|pane| (pane.id.clone(), self.pane_insight(pane).status))
            .collect();
    }

    pub(super) fn capture_attention_transitions(&mut self) {
        let mut next_statuses = HashMap::new();
        let mut alerts = Vec::new();

        for pane in &self.snapshot.panes {
            let insight = self.pane_insight(pane);
            let current_status = insight.status;
            let previous_status = self
                .pane_last_status
                .get(&pane.id)
                .copied()
                .unwrap_or(PaneStatus::Unknown);

            next_statuses.insert(pane.id.clone(), current_status);

            if previous_status != current_status
                && self.should_alert_for_transition(pane, current_status)
            {
                alerts.push((
                    pane.id.clone(),
                    format!(
                        "{} moved {} -> {}",
                        self.pane_target_label(pane),
                        previous_status.display_label(),
                        current_status.display_label()
                    ),
                ));
            }
        }

        self.pane_last_status = next_statuses;

        if !alerts.is_empty() {
            self.autofocus_initial_attention();
        }

        for (pane_id, alert) in alerts {
            self.raise_alert(pane_id, alert);
        }
    }

    fn autofocus_initial_attention(&mut self) {
        if !self.initial_attention_autofocus {
            return;
        }

        let visible = self.visible_pane_entries();
        let selected_is_attention = self
            .selected_visible_pane_position_in_entries(&visible)
            .is_some_and(|index| {
                let entry = &visible[index];
                let pane = &self.snapshot.panes[entry.index];
                self.pane_requires_attention(pane, entry.insight.status) && !entry.acknowledged
            });
        if selected_is_attention {
            self.initial_attention_autofocus = false;
            return;
        }

        if let Some(entry) = visible.iter().find(|entry| {
            let pane = &self.snapshot.panes[entry.index];
            self.pane_requires_attention(pane, entry.insight.status) && !entry.acknowledged
        }) {
            self.selected_pane_id = Some(self.snapshot.panes[entry.index].id.clone());
            self.details_scroll = 0;
            self.sync_selected_window_from_selection();
            self.initial_attention_autofocus = false;
        }
    }

    pub(super) fn recent_output_lines(&self, pane_id: &str, limit: usize) -> Vec<String> {
        self.pane_runtime
            .get(pane_id)
            .map(|runtime| collect_runtime_recent_lines(runtime, limit))
            .unwrap_or_default()
    }

    pub(super) fn latest_output_lines(&self, pane_id: &str, limit: usize) -> Vec<String> {
        let mut lines = self.recent_output_lines(pane_id, limit);
        lines.reverse();
        lines
    }

    pub(super) fn recent_live_output_lines(&self, pane_id: &str, limit: usize) -> Vec<String> {
        self.pane_runtime
            .get(pane_id)
            .map(|runtime| collect_runtime_live_lines(runtime, limit))
            .unwrap_or_default()
    }

    pub(super) fn latest_live_output_lines(&self, pane_id: &str, limit: usize) -> Vec<String> {
        let mut lines = self.recent_live_output_lines(pane_id, limit);
        lines.reverse();
        lines
    }

    pub(super) fn is_acknowledged(&self, pane: &tmux::Pane, status: PaneStatus) -> bool {
        self.acknowledged_attention
            .get(&AttentionKey::from_pane(pane))
            .copied()
            == Some(status)
    }

    pub(super) fn pane_requires_attention(&self, pane: &tmux::Pane, status: PaneStatus) -> bool {
        if pane
            .agent_event
            .as_ref()
            .is_some_and(tmux::agent_bridge_event_suppresses_attention)
        {
            return false;
        }

        is_attention_status(status)
            || pane
                .agent_event
                .as_ref()
                .is_some_and(tmux::agent_bridge_event_needs_review)
    }

    pub(super) fn is_attention_action_pending(
        &self,
        pane: &tmux::Pane,
        status: PaneStatus,
    ) -> bool {
        self.pending_attention_action(pane, status).is_some()
    }

    pub(super) fn pending_attention_action(
        &self,
        pane: &tmux::Pane,
        status: PaneStatus,
    ) -> Option<PendingAttentionAction> {
        if !self.pane_requires_attention(pane, status) {
            return None;
        }

        let pending = self
            .pending_attention_actions
            .get(&AttentionKey::from_pane(pane))
            .copied()?;
        (pending.status == status
            && pending.output_fingerprint == self.attention_output_fingerprint(&pane.id))
        .then_some(pending)
    }

    pub(super) fn attention_queue(&self) -> Vec<(&tmux::Pane, PaneInsight)> {
        let mut queue = self
            .snapshot
            .panes
            .iter()
            .filter_map(|pane| {
                let insight = self.pane_insight(pane);
                if self.matches_pane_visibility(pane)
                    && self.pane_requires_attention(pane, insight.status)
                    && !self.is_acknowledged(pane, insight.status)
                    && !self.is_attention_action_pending(pane, insight.status)
                {
                    Some((pane, insight))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        queue.sort_by(|(left_pane, left_insight), (right_pane, right_insight)| {
            attention_rank(left_insight.status)
                .cmp(&attention_rank(right_insight.status))
                .then_with(|| {
                    workload_rank(left_insight.workload).cmp(&workload_rank(right_insight.workload))
                })
                .then_with(|| left_pane.session_name.cmp(&right_pane.session_name))
                .then_with(|| left_pane.window_name.cmp(&right_pane.window_name))
                .then_with(|| left_pane.pane_index.cmp(&right_pane.pane_index))
                .then_with(|| left_pane.id.cmp(&right_pane.id))
        });

        queue
    }

    pub(super) fn attention_queue_position(&self, pane_id: &str) -> Option<usize> {
        self.attention_queue()
            .iter()
            .position(|(pane, _)| pane.id == pane_id)
            .map(|index| index + 1)
    }

    pub(super) fn attention_queue_len(&self) -> usize {
        self.snapshot
            .panes
            .iter()
            .filter(|pane| {
                let insight = self.pane_insight(pane);
                self.matches_pane_visibility(pane)
                    && self.pane_requires_attention(pane, insight.status)
                    && !self.is_acknowledged(pane, insight.status)
                    && !self.is_attention_action_pending(pane, insight.status)
            })
            .count()
    }

    pub(super) fn watching_attention_queue(
        &self,
    ) -> Vec<(&tmux::Pane, PaneInsight, PendingAttentionAction)> {
        let mut queue = self
            .snapshot
            .panes
            .iter()
            .filter_map(|pane| {
                let insight = self.pane_insight(pane);
                if self.matches_pane_visibility(pane)
                    && self.pane_requires_attention(pane, insight.status)
                    && !self.is_acknowledged(pane, insight.status)
                {
                    self.pending_attention_action(pane, insight.status)
                        .map(|pending| (pane, insight, pending))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        queue.sort_by(
            |(left_pane, left_insight, _), (right_pane, right_insight, _)| {
                attention_rank(left_insight.status)
                    .cmp(&attention_rank(right_insight.status))
                    .then_with(|| {
                        workload_rank(left_insight.workload)
                            .cmp(&workload_rank(right_insight.workload))
                    })
                    .then_with(|| left_pane.session_name.cmp(&right_pane.session_name))
                    .then_with(|| left_pane.window_name.cmp(&right_pane.window_name))
                    .then_with(|| left_pane.pane_index.cmp(&right_pane.pane_index))
                    .then_with(|| left_pane.id.cmp(&right_pane.id))
            },
        );

        queue
    }

    pub(super) fn reconcile_acknowledgements(&mut self) {
        let stale = self
            .acknowledged_attention
            .iter()
            .filter_map(|(key, status)| {
                let matching = self
                    .snapshot
                    .panes
                    .iter()
                    .find(|pane| AttentionKey::from_pane(pane) == *key);

                match matching {
                    Some(pane)
                        if self.pane_insight(pane).status == *status
                            && self.pane_requires_attention(pane, *status) =>
                    {
                        None
                    }
                    _ => Some(key.clone()),
                }
            })
            .collect::<Vec<_>>();

        let had_stale = !stale.is_empty();
        for key in stale {
            self.acknowledged_attention.remove(&key);
        }
        if had_stale {
            let _saved_or_reported = self.save_persistent_state();
        }
    }

    pub(super) fn reconcile_pending_attention_actions(&mut self) {
        let stale = self
            .pending_attention_actions
            .iter()
            .filter_map(|(key, pending)| {
                let matching = self
                    .snapshot
                    .panes
                    .iter()
                    .find(|pane| AttentionKey::from_pane(pane) == *key);

                match matching {
                    Some(pane)
                        if self.pane_insight(pane).status == pending.status
                            && self.pane_requires_attention(pane, pending.status)
                            && self.attention_output_fingerprint(&pane.id)
                                == pending.output_fingerprint =>
                    {
                        None
                    }
                    _ => Some(key.clone()),
                }
            })
            .collect::<Vec<_>>();

        for key in stale {
            self.pending_attention_actions.remove(&key);
        }
    }

    pub(super) fn mark_attention_action_pending(
        &mut self,
        pane: &tmux::Pane,
        kind: PendingAttentionActionKind,
    ) {
        let status = self.pane_insight(pane).status;
        if !self.pane_requires_attention(pane, status) {
            return;
        }

        self.pending_attention_actions.insert(
            AttentionKey::from_pane(pane),
            PendingAttentionAction {
                status,
                output_fingerprint: self.attention_output_fingerprint(&pane.id),
                kind,
            },
        );
    }

    pub(super) fn select_next_attention_after_action(&mut self, acted_pane: &tmux::Pane) {
        if self.context_pane != ContextPane::Control {
            return;
        }

        let next_pane_id = self
            .attention_queue()
            .into_iter()
            .next()
            .map(|(pane, _)| pane.id.clone())
            .unwrap_or_else(|| acted_pane.id.clone());
        self.selected_pane_id = Some(next_pane_id);
        self.sync_selected_window_from_selection();
        self.details_scroll = 0;
    }

    pub(super) fn attention_action_status_after_send(
        &self,
        prefix: &str,
        pane: &tmux::Pane,
    ) -> String {
        let sent = format!("{prefix} {}.", self.pane_target_label(pane));
        if let Some((next, _)) = self.attention_queue().into_iter().next() {
            format!("{sent} Next: {}.", self.pane_target_label(next))
        } else {
            format!("{sent} Watching for update.")
        }
    }

    fn attention_output_fingerprint(&self, pane_id: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        if let Some(runtime) = self.pane_runtime.get(pane_id) {
            runtime.output.len().hash(&mut hasher);
            for line in runtime.output.iter().rev().take(8) {
                line.hash(&mut hasher);
            }
            runtime.partial_line.hash(&mut hasher);
        } else {
            pane_id.hash(&mut hasher);
        }
        hasher.finish()
    }

    pub(super) fn recommended_smart_action(
        &self,
        pane: &tmux::Pane,
        insight: PaneInsight,
    ) -> SmartAction {
        let recent_lines = self.recent_output_lines(&pane.id, 6);

        if insight.status == PaneStatus::Waiting
            && recent_lines.iter().any(|line| matches_enter_hint(line))
        {
            SmartAction::SendEnter
        } else {
            SmartAction::Focus
        }
    }

    pub(super) fn recommended_action_summary(
        &self,
        pane: &tmux::Pane,
        insight: PaneInsight,
    ) -> String {
        let recent_lines = self.recent_output_lines(&pane.id, 6);

        match insight.status {
            PaneStatus::Waiting if recent_lines.iter().any(|line| matches_enter_hint(line)) => {
                String::from("continue")
            }
            PaneStatus::Waiting if recent_lines.iter().any(|line| matches_choice_hint(line)) => {
                String::from("answer")
            }
            PaneStatus::Error | PaneStatus::Stuck => String::from("show output"),
            PaneStatus::Running => String::from("watch progress"),
            PaneStatus::Done => String::from("review result"),
            PaneStatus::Idle | PaneStatus::Unknown => String::from("show in tmux"),
            PaneStatus::Waiting => String::from("show prompt"),
        }
    }

    pub(super) async fn send_keys_to_selected(&mut self, keys: &[&str], label: &str) -> Result<()> {
        if self.selected_pane_hidden_by_current_view()
            && (!self.using_explicit_targets() || self.visible_pane_indices().is_empty())
        {
            let action = label.strip_prefix("Sent ").unwrap_or(label);
            self.status_message = format!("Show all panes before sending {action}.");
            return Ok(());
        }

        let targets = self.active_target_panes();
        if targets.is_empty() {
            self.status_message = self.no_active_targets_message();
            return Ok(());
        }

        let target_count = targets.len();
        let selected_label = (!self.using_explicit_targets()
            && self.fanout_mode == FanoutMode::Off)
            .then(|| self.pane_target_label(targets[0]));
        let workload = if self.fanout_mode == FanoutMode::Lane {
            self.selected_lane_workload()
                .map(WorkloadKind::display_label)
                .unwrap_or("selected lane")
        } else {
            "selected lane"
        };
        let target_ids = targets
            .iter()
            .map(|pane| pane.id.clone())
            .collect::<Vec<_>>();
        let outcome = self.send_keys_to_pane_ids(&target_ids, keys).await?;

        if outcome.sent_count == 0 {
            let action = label.strip_prefix("Sent ").unwrap_or(label);
            self.status_message = no_target_panes_remain_message(action, outcome.disappeared_count);
            return Ok(());
        }

        self.status_message = if outcome.disappeared_count > 0 && self.using_marked_targets() {
            format!(
                "{label} to {} in the send list; {} disappeared.",
                pane_count_label(outcome.sent_count),
                pane_count_label(outcome.disappeared_count)
            )
        } else if outcome.disappeared_count > 0 && self.fanout_mode == FanoutMode::Lane {
            format!(
                "{label} to {} in {workload}; {} disappeared.",
                pane_count_label(outcome.sent_count),
                pane_count_label(outcome.disappeared_count)
            )
        } else if self.using_marked_targets() {
            format!(
                "{label} to {} in the send list.",
                pane_count_label(target_count)
            )
        } else if self.fanout_mode == FanoutMode::Lane {
            format!(
                "{label} to {} in {workload}.",
                pane_count_label(target_count)
            )
        } else {
            format!(
                "{label} to {}.",
                selected_label.unwrap_or_else(|| pane_count_label(outcome.sent_count))
            )
        };
        Ok(())
    }

    pub(super) fn bulk_enter_targets(&self) -> Vec<String> {
        self.attention_queue()
            .into_iter()
            .filter_map(|(pane, insight)| {
                if self.recommended_smart_action(pane, insight) == SmartAction::SendEnter {
                    Some(pane.id.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub(super) fn push_alert(&mut self, line: String) {
        self.recent_alerts.push_front(line);
        while self.recent_alerts.len() > MAX_RECENT_ALERTS {
            self.recent_alerts.pop_back();
        }
    }

    pub(super) fn raise_alert(&mut self, pane_id: String, line: String) {
        self.alert_count += 1;
        self.last_alerted_at.insert(pane_id, Instant::now());
        if self.notification_settings.bell_enabled {
            self.pending_bell = true;
        }
        if self.notification_settings.desktop_enabled {
            self.notifier.notify_alert("muxboard alert", &line);
        }
        self.status_message = format!("Alert: {line}");
        self.push_alert(line);
    }

    pub(super) fn should_alert_for_transition(
        &self,
        pane: &tmux::Pane,
        status: PaneStatus,
    ) -> bool {
        self.pane_requires_attention(pane, status)
            && self.notification_settings.alert_policy.matches(status)
            && !self.is_acknowledged(pane, status)
            && !self.is_within_alert_debounce(&pane.id)
    }

    pub(super) fn is_within_alert_debounce(&self, pane_id: &str) -> bool {
        let debounce = self.notification_settings.debounce_seconds;
        if debounce == 0 {
            return false;
        }

        self.last_alerted_at
            .get(pane_id)
            .is_some_and(|time| time.elapsed() < Duration::from_secs(debounce))
    }
}

#[cfg(test)]
pub(super) fn attention_label(status: PaneStatus) -> &'static str {
    match status {
        PaneStatus::Error => "needs attention",
        PaneStatus::Waiting => "awaiting input",
        PaneStatus::Stuck => "possibly stalled",
        PaneStatus::Running => "on track",
        PaneStatus::Done => "complete",
        PaneStatus::Idle => "quiet",
        PaneStatus::Unknown => "checking",
    }
}

pub(super) fn is_attention_status(status: PaneStatus) -> bool {
    matches!(
        status,
        PaneStatus::Waiting | PaneStatus::Error | PaneStatus::Stuck
    )
}

pub(super) fn attention_rank(status: PaneStatus) -> u8 {
    match status {
        PaneStatus::Error => 0,
        PaneStatus::Waiting => 1,
        PaneStatus::Stuck => 2,
        PaneStatus::Running => 3,
        PaneStatus::Idle => 4,
        PaneStatus::Done => 5,
        PaneStatus::Unknown => 6,
    }
}
