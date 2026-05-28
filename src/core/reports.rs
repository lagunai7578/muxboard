use std::time::Instant;

use super::providers::{
    marker_matches, matches_done_hint, matches_error_hint, normalize_summary_line,
    parse_source_status_report, summarize_source_status_line, summarize_tool_progress_line,
    summarize_waiting_line,
};
use super::{
    AgentReport, PaneInsight, PaneRuntime, PaneStatus, WorkloadKind, classify_fallback_summary,
    collect_runtime_live_lines, is_terminal_chatter_noise,
};

pub fn parse_agent_report_line(line: &str) -> Option<AgentReport> {
    let normalized = strip_report_prefix(line);
    if is_status_report_template(normalized) {
        return None;
    }
    parse_status_triplet(strip_protocol_source_prefix(normalized))
        .or_else(|| parse_muxboard_heartbeat(normalized))
        .map(|(status, blocker, next)| AgentReport {
            status,
            blocker,
            next,
            updated_at: Instant::now(),
        })
}

pub(crate) fn parse_agent_report_lines(lines: &[String]) -> Option<AgentReport> {
    if let Some(report) = lines.iter().find_map(|line| parse_agent_report_line(line)) {
        return Some(report);
    }

    let mut status = None;
    let mut blocker = None;
    let mut next = None;

    for line in lines {
        match parse_protocol_field_line(line) {
            Some(("status", value)) if status.is_none() => status = Some(value),
            Some(("blocker", value)) if blocker.is_none() => blocker = Some(value),
            Some(("next", value)) if next.is_none() => next = Some(value),
            _ => {}
        }
    }

    Some(AgentReport {
        status: status?,
        blocker: blocker?,
        next: next?,
        updated_at: Instant::now(),
    })
}

pub fn is_agent_report_protocol_line(line: &str) -> bool {
    parse_agent_report_line(line).is_some() || parse_protocol_field_line(line).is_some()
}

pub fn agent_report_summary(report: &AgentReport) -> String {
    if report.next.is_empty() {
        report.status.clone()
    } else {
        format!("{} -> {}", report.status, report.next)
    }
}

pub fn activity_summary(
    workload: WorkloadKind,
    command: &str,
    report: Option<&AgentReport>,
    recent_lines: &[String],
) -> String {
    report
        .map(agent_report_summary)
        .or_else(|| summarize_recent_lines(workload, recent_lines))
        .unwrap_or_else(|| command.trim().to_owned())
}

pub fn effective_agent_report(
    runtime: Option<&PaneRuntime>,
    insight: PaneInsight,
    report: Option<&AgentReport>,
) -> Option<AgentReport> {
    let synthesized = synthesize_agent_report(runtime, insight);

    match (report, synthesized) {
        (Some(report), Some(synthesized))
            if should_prefer_synthesized_report(report, &synthesized, insight, runtime) =>
        {
            Some(synthesized)
        }
        (Some(report), _) => Some(report.clone()),
        (None, Some(synthesized)) => Some(synthesized),
        (None, None) => None,
    }
}

pub(crate) fn infer_status_from_report(report: &AgentReport) -> Option<PaneStatus> {
    let status = report.status.to_ascii_lowercase();
    let blocker = report.blocker.to_ascii_lowercase();

    if marker_matches(&status, "error")
        || marker_matches(&status, "failed")
        || marker_matches(&status, "panic")
    {
        return Some(PaneStatus::Error);
    }
    if marker_matches(&status, "waiting")
        || marker_matches(&status, "blocked")
        || marker_matches(&status, "needs_input")
        || marker_matches(&status, "needs-input")
        || marker_matches(&status, "approval")
        || marker_matches(&blocker, "approval")
        || marker_matches(&blocker, "input")
        || marker_matches(&blocker, "confirmation")
    {
        return Some(PaneStatus::Waiting);
    }
    if marker_matches(&status, "stuck")
        || marker_matches(&status, "stalled")
        || marker_matches(&status, "hung")
    {
        return Some(PaneStatus::Stuck);
    }
    if marker_matches(&status, "done")
        || marker_matches(&status, "completed")
        || marker_matches(&status, "finished")
        || marker_matches(&status, "success")
    {
        return Some(PaneStatus::Done);
    }
    if marker_matches(&status, "running")
        || marker_matches(&status, "working")
        || marker_matches(&status, "thinking")
        || marker_matches(&status, "active")
    {
        return Some(PaneStatus::Running);
    }
    if marker_matches(&status, "idle") || marker_matches(&status, "quiet") {
        return Some(PaneStatus::Idle);
    }

    None
}

fn strip_report_prefix(line: &str) -> &str {
    let trimmed = line.trim();

    trimmed
        .strip_prefix("• ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("- "))
        .unwrap_or(trimmed)
        .trim()
}

fn strip_protocol_source_prefix(line: &str) -> &str {
    let trimmed = line.trim();
    let Some((prefix, rest)) = trimmed.split_once(':') else {
        return trimmed;
    };

    if matches!(
        prefix.trim().to_ascii_lowercase().as_str(),
        "codex" | "claude" | "claude-code" | "opencode" | "aider" | "gemini" | "agent"
    ) {
        rest.trim()
    } else {
        trimmed
    }
}

fn parse_protocol_field_line(line: &str) -> Option<(&'static str, String)> {
    let normalized = strip_protocol_source_prefix(strip_report_prefix(line));
    if is_status_report_template(normalized) || normalized.contains('|') {
        return None;
    }

    let (key, value) = normalized.split_once('=')?;
    let value = value.trim();
    if value.is_empty() || value.contains('<') || value.contains('>') {
        return None;
    }

    match key.trim().to_ascii_lowercase().as_str() {
        "status" => Some(("status", value.to_owned())),
        "blocker" => Some(("blocker", value.to_owned())),
        "next" => Some(("next", value.to_owned())),
        _ => None,
    }
}

fn parse_status_triplet(line: &str) -> Option<(String, String, String)> {
    let normalized = line.trim();
    let parts = normalized.split('|').map(str::trim).collect::<Vec<_>>();
    if parts.len() != 3 {
        return None;
    }

    let status = parts[0]
        .strip_prefix("STATUS=")
        .or_else(|| parts[0].strip_prefix("status="))?
        .trim()
        .to_owned();
    let blocker = parts[1]
        .strip_prefix("BLOCKER=")
        .or_else(|| parts[1].strip_prefix("blocker="))?
        .trim()
        .to_owned();
    let next = parts[2]
        .strip_prefix("NEXT=")
        .or_else(|| parts[2].strip_prefix("next="))?
        .trim()
        .to_owned();

    Some((status, blocker, next))
}

fn parse_muxboard_heartbeat(line: &str) -> Option<(String, String, String)> {
    let normalized = line.trim();
    let payload = normalized
        .strip_prefix("muxboard:")
        .or_else(|| normalized.strip_prefix("MUXBOARD:"))?
        .trim();
    let mut status = None;
    let mut blocker = None;
    let mut next = None;

    for segment in payload.split(';').map(str::trim) {
        if let Some(value) = segment
            .strip_prefix("status=")
            .or_else(|| segment.strip_prefix("STATUS="))
        {
            status = Some(value.trim().to_owned());
        } else if let Some(value) = segment
            .strip_prefix("blocker=")
            .or_else(|| segment.strip_prefix("BLOCKER="))
        {
            blocker = Some(value.trim().to_owned());
        } else if let Some(value) = segment
            .strip_prefix("next=")
            .or_else(|| segment.strip_prefix("NEXT="))
        {
            next = Some(value.trim().to_owned());
        }
    }

    Some((status?, blocker?, next?))
}

fn is_status_report_template(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("reply in exactly one line as:")
        || lower.contains("status=<status>")
        || lower.contains("blocker=<blocker>")
        || lower.contains("next=<next>")
        || lower.contains("status = <status>")
        || lower.contains("blocker = <blocker>")
        || lower.contains("next = <next>")
}

fn synthesize_agent_report(
    runtime: Option<&PaneRuntime>,
    insight: PaneInsight,
) -> Option<AgentReport> {
    let lines = runtime
        .map(|runtime| collect_runtime_live_lines(runtime, 8))
        .unwrap_or_default();

    if let Some(report) = parse_agent_report_lines(&lines) {
        return Some(report);
    }

    if let Some(report) = lines
        .iter()
        .find_map(|line| parse_source_status_report(line, insight.workload))
    {
        return Some(report);
    }

    if let Some(line) = lines
        .iter()
        .find(|line| matches_error_hint(line, insight.workload))
        .cloned()
    {
        return Some(AgentReport {
            status: String::from("error"),
            blocker: normalize_summary_line(&line),
            next: String::from("show output"),
            updated_at: Instant::now(),
        });
    }

    if let Some(summary) = lines
        .iter()
        .find_map(|line| summarize_waiting_line(&normalize_summary_line(line), insight.workload))
    {
        let (blocker, next) = waiting_summary_fields(&summary);
        return Some(AgentReport {
            status: String::from("waiting"),
            blocker,
            next,
            updated_at: Instant::now(),
        });
    }

    if let Some(summary) = lines
        .iter()
        .find_map(|line| summarize_tool_progress_line(&normalize_summary_line(line)))
    {
        return Some(AgentReport {
            status: pane_status_token(insight.status).to_owned(),
            blocker: String::from("none"),
            next: tool_summary_next(&summary),
            updated_at: Instant::now(),
        });
    }

    if let Some(line) = lines
        .iter()
        .find(|line| matches_done_hint(line, insight.workload))
        .cloned()
    {
        return Some(AgentReport {
            status: String::from("done"),
            blocker: String::from("none"),
            next: normalize_summary_line(&line),
            updated_at: Instant::now(),
        });
    }

    let chronological_lines = lines.iter().rev().cloned().collect::<Vec<_>>();

    if let Some(summary) = summarize_recent_lines(insight.workload, &chronological_lines) {
        let (status, blocker, next) = match insight.status {
            PaneStatus::Waiting => {
                let (blocker, next) = waiting_summary_fields(&summary);
                (String::from("waiting"), blocker, next)
            }
            PaneStatus::Error => (String::from("error"), summary, String::from("show output")),
            PaneStatus::Stuck => (String::from("stuck"), summary, String::from("show pane")),
            PaneStatus::Done => (String::from("done"), String::from("none"), summary),
            PaneStatus::Running => (
                String::from("running"),
                String::from("none"),
                running_summary_next(&summary),
            ),
            PaneStatus::Idle => (String::from("idle"), String::from("none"), summary),
            PaneStatus::Unknown => (String::from("checking"), String::from("none"), summary),
        };

        return Some(AgentReport {
            status,
            blocker,
            next,
            updated_at: Instant::now(),
        });
    }

    match insight.status {
        PaneStatus::Waiting => Some(AgentReport {
            status: String::from("waiting"),
            blocker: String::from("input needed"),
            next: String::from("show details"),
            updated_at: Instant::now(),
        }),
        PaneStatus::Error => Some(AgentReport {
            status: String::from("error"),
            blocker: String::from("error detected"),
            next: String::from("show output"),
            updated_at: Instant::now(),
        }),
        PaneStatus::Stuck => Some(AgentReport {
            status: String::from("stuck"),
            blocker: String::from("no recent output"),
            next: String::from("show pane"),
            updated_at: Instant::now(),
        }),
        _ => None,
    }
}

fn should_prefer_synthesized_report(
    stored: &AgentReport,
    synthesized: &AgentReport,
    insight: PaneInsight,
    runtime: Option<&PaneRuntime>,
) -> bool {
    let stored_status = infer_status_from_report(stored);
    let synthesized_status = infer_status_from_report(synthesized);

    if stored_status != Some(insight.status) && synthesized_status == Some(insight.status) {
        return true;
    }

    synthesized_status == Some(insight.status)
        && runtime_has_explicit_report(runtime, insight.workload)
        && synthesized_report_has_fresher_signal(stored, synthesized)
}

fn runtime_has_explicit_report(runtime: Option<&PaneRuntime>, workload: WorkloadKind) -> bool {
    let lines = runtime
        .map(|runtime| collect_runtime_live_lines(runtime, 8))
        .unwrap_or_default();

    parse_agent_report_lines(&lines).is_some()
        || lines
            .iter()
            .any(|line| parse_source_status_report(line, workload).is_some())
}

fn synthesized_report_has_fresher_signal(stored: &AgentReport, synthesized: &AgentReport) -> bool {
    let stored_next = normalized_report_field(&stored.next);
    let synthesized_next = normalized_report_field(&synthesized.next);
    let next_changed = !synthesized_next.is_empty() && synthesized_next != stored_next;

    let stored_blocker = normalized_report_field(&stored.blocker);
    let synthesized_blocker = normalized_report_field(&synthesized.blocker);
    let blocker_changed =
        !is_none_report_field(&synthesized_blocker) && synthesized_blocker != stored_blocker;

    next_changed || blocker_changed
}

fn normalized_report_field(value: &str) -> String {
    normalize_summary_line(value).to_ascii_lowercase()
}

fn is_none_report_field(value: &str) -> bool {
    matches!(value, "" | "none" | "no blocker")
}

fn summarize_recent_lines(workload: WorkloadKind, lines: &[String]) -> Option<String> {
    let mut best = None::<(u8, String)>;

    if let Some(report) = parse_agent_report_lines(lines) {
        return Some(agent_report_summary(&report));
    }

    for line in lines {
        let user_request = summarize_user_request_line(line);
        let normalized = normalize_summary_line(line);
        if normalized.is_empty()
            || is_summary_noise_line(&normalized)
            || is_agent_report_protocol_line(&normalized)
        {
            continue;
        }

        if let Some(summary) = summarize_source_status_line(&normalized, workload) {
            consider_summary_candidate(
                &mut best,
                source_summary_priority(workload, &summary),
                summary,
            );
            continue;
        }

        if let Some(summary) = summarize_waiting_line(&normalized, workload) {
            consider_summary_candidate(&mut best, waiting_summary_priority(&summary), summary);
            continue;
        }

        if let Some(summary) = summarize_tool_progress_line(&normalized) {
            consider_summary_candidate(&mut best, 90, summary);
            continue;
        }

        if let Some(summary) = user_request {
            consider_summary_candidate(&mut best, 88, summary);
            continue;
        }

        if !is_summary_junk_line(&normalized, workload) {
            let fallback = classify_fallback_summary(&normalized);
            consider_summary_candidate(&mut best, fallback.priority(), fallback.into_normalized());
        }
    }

    best.map(|(_, summary)| summary)
}

fn summarize_user_request_line(line: &str) -> Option<String> {
    let request = strip_user_request_marker(line)?;
    if is_status_report_template(request) {
        return None;
    }

    let normalized = normalize_user_request(request);
    let meaningful_words = normalized.split_whitespace().count();
    (meaningful_words >= 2).then_some(normalized)
}

fn strip_user_request_marker(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    trimmed
        .strip_prefix('›')
        .or_else(|| trimmed.strip_prefix("user:"))
        .or_else(|| trimmed.strip_prefix("User:"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn normalize_user_request(request: &str) -> String {
    let mut text = request.trim();
    for prefix in ["please ", "run ", "do "] {
        if text.to_ascii_lowercase().starts_with(prefix)
            && let Some(rest) = text.get(prefix.len()..)
        {
            text = rest.trim();
            break;
        }
    }

    let words = text
        .split_whitespace()
        .filter_map(|word| {
            let cleaned = word
                .trim_matches(|ch: char| ch == '"' || ch == '\'' || ch == '`')
                .trim_start_matches('/');
            let normalized = cleaned
                .trim_matches(|ch: char| matches!(ch, '.' | ',' | ':' | ';' | '?' | '!'))
                .to_ascii_lowercase();
            if normalized.is_empty()
                || matches!(
                    normalized.as_str(),
                    "on" | "my" | "current" | "this" | "the" | "a" | "an" | "your" | "our"
                )
            {
                None
            } else {
                Some(normalized)
            }
        })
        .collect::<Vec<_>>();

    words.join(" ")
}

fn consider_summary_candidate(best: &mut Option<(u8, String)>, priority: u8, summary: String) {
    if best
        .as_ref()
        .is_none_or(|(current_priority, _)| priority >= *current_priority)
    {
        *best = Some((priority, summary));
    }
}

fn source_summary_priority(workload: WorkloadKind, summary: &str) -> u8 {
    if is_generic_running_source_summary(workload, summary) {
        20
    } else if is_generic_waiting_source_summary(summary) {
        45
    } else {
        80
    }
}

fn waiting_summary_priority(summary: &str) -> u8 {
    if summary == "needs approval" || summary == "waiting for input" {
        55
    } else {
        85
    }
}

fn is_generic_running_source_summary(workload: WorkloadKind, summary: &str) -> bool {
    match workload {
        WorkloadKind::Codex => {
            matches!(
                summary,
                "starting agent" | "running" | "completed" | "error"
            )
        }
        WorkloadKind::ClaudeCode | WorkloadKind::Opencode => {
            matches!(summary, "continue from answers" | "conversation compacted")
        }
        _ => false,
    }
}

fn is_generic_waiting_source_summary(summary: &str) -> bool {
    matches!(summary, "needs approval" | "waiting for input")
}

fn is_summary_noise_line(line: &str) -> bool {
    matches!(line, "|" | "||" | "Latest" | "Report" | "❯" | ">" | ">>")
        || is_terminal_chatter_noise(line)
        || looks_like_inline_echo_fragment(line)
}

fn is_summary_junk_line(line: &str, workload: WorkloadKind) -> bool {
    let normalized = line.to_ascii_lowercase();

    normalized.contains("reply in exactly one line as:")
        || normalized.contains("status=<status>")
        || normalized.contains("blocker=<blocker>")
        || normalized.contains("next=<next>")
        || normalized.contains("press enter or type command to continue")
        || normalized.contains("press enter to continue")
        || normalized.contains("config change detected")
        || normalized.starts_with("input:")
        || normalized.starts_with("output:")
        || normalized.starts_with("tool input:")
        || normalized.starts_with("tool output:")
        || looks_like_model_banner(&normalized)
        || (workload == WorkloadKind::Codex
            && (normalized.contains("developer message") || normalized.contains("tool call")))
}

fn looks_like_model_banner(line: &str) -> bool {
    (line.contains("~/") || line.contains("/users/") || line.contains("/home/"))
        && (line.contains("gpt-")
            || line.contains("claude-")
            || line.contains("sonnet")
            || line.contains("opus"))
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

fn waiting_summary_fields(summary: &str) -> (String, String) {
    if let Some(target) = summary.strip_prefix("needs approval: ") {
        return (format!("approval: {target}"), String::from("approve"));
    }

    if summary == "needs approval" {
        return (String::from("approval needed"), String::from("approve"));
    }

    if summary == "waiting for input" {
        return (String::from("input needed"), String::from("answer"));
    }

    (summary.to_owned(), String::from("show details"))
}

fn tool_summary_next(summary: &str) -> String {
    summary
        .strip_prefix("tool: ")
        .map(|tool| format!("wait for {tool}"))
        .unwrap_or_else(|| summary.to_owned())
}

fn running_summary_next(summary: &str) -> String {
    if summary.starts_with("tool: ") {
        tool_summary_next(summary)
    } else {
        summary.to_owned()
    }
}

fn pane_status_token(status: PaneStatus) -> &'static str {
    match status {
        PaneStatus::Running => "running",
        PaneStatus::Waiting => "waiting",
        PaneStatus::Done => "done",
        PaneStatus::Error => "error",
        PaneStatus::Stuck => "stuck",
        PaneStatus::Idle => "idle",
        PaneStatus::Unknown => "checking",
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, time::Instant};

    use super::*;

    fn runtime(lines: &[&str]) -> PaneRuntime {
        PaneRuntime {
            output: lines.iter().map(|line| String::from(*line)).collect(),
            last_output_at: Some(Instant::now()),
            corpus: lines.join(" ").to_ascii_lowercase(),
            partial_line: String::new(),
        }
    }

    fn insight(status: PaneStatus, workload: WorkloadKind) -> PaneInsight {
        PaneInsight {
            workload,
            status,
            last_output_age: None,
        }
    }

    #[test]
    fn parses_heartbeat_and_rejects_incomplete_reports() {
        let heartbeat =
            parse_agent_report_line("MUXBOARD: STATUS=waiting; BLOCKER=approval; NEXT=approve")
                .expect("heartbeat report should parse");
        assert_eq!(heartbeat.status, "waiting");
        assert_eq!(heartbeat.blocker, "approval");
        assert_eq!(heartbeat.next, "approve");

        assert!(parse_agent_report_line("STATUS=running | NEXT=missing blocker").is_none());
        assert!(
            parse_agent_report_line("muxboard: status=running; next=missing blocker").is_none()
        );
    }

    #[test]
    fn report_summary_and_status_cover_visible_tokens() {
        let no_next = AgentReport {
            status: String::from("idle"),
            blocker: String::new(),
            next: String::new(),
            updated_at: Instant::now(),
        };
        assert_eq!(agent_report_summary(&no_next), "idle");

        for (token, expected) in [
            ("failed", Some(PaneStatus::Error)),
            ("needs-input", Some(PaneStatus::Waiting)),
            ("stalled", Some(PaneStatus::Stuck)),
            ("finished", Some(PaneStatus::Done)),
            ("thinking", Some(PaneStatus::Running)),
            ("quiet", Some(PaneStatus::Idle)),
            ("mystery", None),
        ] {
            let report = AgentReport {
                status: String::from(token),
                blocker: String::new(),
                next: String::new(),
                updated_at: Instant::now(),
            };
            assert_eq!(
                infer_status_from_report(&report),
                expected,
                "token: {token}"
            );
        }

        let blocker = AgentReport {
            status: String::from("running"),
            blocker: String::from("confirmation needed"),
            next: String::new(),
            updated_at: Instant::now(),
        };
        assert_eq!(
            infer_status_from_report(&blocker),
            Some(PaneStatus::Waiting)
        );
    }

    #[test]
    fn synthesis_covers_source_errors_done_fallbacks_and_empty_states() {
        let error_runtime = runtime(&["Error: permission denied"]);
        let error = synthesize_agent_report(
            Some(&error_runtime),
            insight(PaneStatus::Error, WorkloadKind::Job),
        )
        .expect("error report should synthesize");
        assert_eq!(error.status, "error");
        assert_eq!(error.next, "show output");

        let done_runtime = runtime(&["Completed"]);
        let done = synthesize_agent_report(
            Some(&done_runtime),
            insight(PaneStatus::Done, WorkloadKind::Codex),
        )
        .expect("done report should synthesize");
        assert_eq!(done.status, "done");

        for (status, expected_status, expected_next) in [
            (PaneStatus::Waiting, "waiting", "show details"),
            (PaneStatus::Error, "error", "show output"),
            (PaneStatus::Stuck, "stuck", "show pane"),
        ] {
            let report = synthesize_agent_report(None, insight(status, WorkloadKind::Agent))
                .expect("attention state should synthesize fallback report");
            assert_eq!(report.status, expected_status);
            assert_eq!(report.next, expected_next);
        }

        assert!(
            synthesize_agent_report(None, insight(PaneStatus::Running, WorkloadKind::Agent))
                .is_none()
        );
    }

    #[test]
    fn synthesis_maps_recent_summary_to_each_status_shape() {
        for (status, expected_status, expected_blocker, expected_next) in [
            (PaneStatus::Waiting, "waiting", "approval needed", "approve"),
            (
                PaneStatus::Error,
                "error",
                "building release artifacts",
                "show output",
            ),
            (
                PaneStatus::Stuck,
                "stuck",
                "building release artifacts",
                "show pane",
            ),
            (
                PaneStatus::Done,
                "done",
                "none",
                "building release artifacts",
            ),
            (
                PaneStatus::Running,
                "running",
                "none",
                "building release artifacts",
            ),
            (
                PaneStatus::Idle,
                "idle",
                "none",
                "building release artifacts",
            ),
            (
                PaneStatus::Unknown,
                "checking",
                "none",
                "building release artifacts",
            ),
        ] {
            let line = if status == PaneStatus::Waiting {
                "Waiting for input"
            } else {
                "building release artifacts"
            };
            let synthesized = synthesize_agent_report(
                Some(&runtime(&[line])),
                insight(status, WorkloadKind::Job),
            )
            .expect("summary report should synthesize");
            assert_eq!(synthesized.status, expected_status, "status: {status:?}");
            assert_eq!(synthesized.blocker, expected_blocker, "status: {status:?}");
            assert_eq!(synthesized.next, expected_next, "status: {status:?}");
        }
    }

    #[test]
    fn summary_selection_filters_noise_and_prioritizes_specific_signals() {
        let summary = summarize_recent_lines(
            WorkloadKind::Codex,
            &[
                String::from("|"),
                String::from("R"),
                String::from("Reply in exactly one line as: STATUS=<status>"),
                String::from("Tool call: Bash"),
                String::from("gpt-5.4 high · ~/Projects"),
                String::from("Running"),
                String::from("building release artifacts"),
            ],
        )
        .expect("specific progress should survive noise");

        assert_eq!(summary, "building release artifacts");

        assert_eq!(
            summarize_recent_lines(
                WorkloadKind::Codex,
                &[String::from("User: please fix renderer tests")]
            )
            .as_deref(),
            Some("fix renderer tests")
        );
        assert_eq!(
            summarize_recent_lines(
                WorkloadKind::Codex,
                &[
                    String::from(
                        "User: Reply in exactly one line as: STATUS=<status> | BLOCKER=<blocker> | NEXT=<next>",
                    ),
                    String::from("Running"),
                ],
            )
            .as_deref(),
            Some("running")
        );
        assert!(
            summarize_recent_lines(
                WorkloadKind::Codex,
                &[String::from(
                    "User: Reply in exactly one line as: STATUS=<status> | BLOCKER=<blocker> | NEXT=<next>",
                )],
            )
            .is_none()
        );
        assert_eq!(
            summarize_recent_lines(
                WorkloadKind::Codex,
                &[String::from("STATUS=done | BLOCKER=none | NEXT=ship")]
            )
            .as_deref(),
            Some("done -> ship")
        );
        assert!(summarize_recent_lines(WorkloadKind::Job, &[String::from("|")]).is_none());
    }

    #[test]
    fn synthesis_uses_user_request_as_waiting_context_without_template_leakage() {
        let report = synthesize_agent_report(
            Some(&runtime(&["User: please review renderer output"])),
            insight(PaneStatus::Waiting, WorkloadKind::Codex),
        )
        .expect("waiting report should synthesize from user intent");

        assert_eq!(report.status, "waiting");
        assert_eq!(report.blocker, "review renderer output");
        assert_eq!(report.next, "show details");

        let template = synthesize_agent_report(
            Some(&runtime(&[
                "User: Reply in exactly one line as: STATUS=<status> | BLOCKER=<blocker> | NEXT=<next>",
            ])),
            insight(PaneStatus::Waiting, WorkloadKind::Codex),
        )
        .expect("waiting fallback should survive template-only noise");

        assert_eq!(template.status, "waiting");
        assert_eq!(template.blocker, "input needed");
        assert_eq!(template.next, "show details");
        assert!(!template.blocker.contains('<'));
        assert!(!template.next.contains("NEXT="));
    }

    #[test]
    fn helper_priorities_and_formatters_cover_edge_values() {
        let mut best = None;
        consider_summary_candidate(&mut best, 10, String::from("weak"));
        consider_summary_candidate(&mut best, 10, String::from("new equal"));
        consider_summary_candidate(&mut best, 9, String::from("older weak"));
        assert_eq!(best, Some((10, String::from("new equal"))));

        assert_eq!(source_summary_priority(WorkloadKind::Codex, "running"), 20);
        assert_eq!(
            source_summary_priority(WorkloadKind::Opencode, "needs approval"),
            45
        );
        assert_eq!(source_summary_priority(WorkloadKind::Job, "useful"), 80);
        assert_eq!(waiting_summary_priority("needs approval"), 55);
        assert_eq!(
            waiting_summary_priority("needs approval: network access"),
            85
        );
        assert!(is_summary_noise_line("ab"));
        assert!(is_summary_noise_line("x/"));
        assert!(looks_like_model_banner(
            "gpt-5.4 high · /users/alice/project"
        ));
        assert!(looks_like_model_banner("claude sonnet · ~/Projects"));
        assert!(looks_like_model_banner("opus high · /home/alice/project"));
        assert_eq!(
            waiting_summary_fields("needs approval"),
            (String::from("approval needed"), String::from("approve"))
        );
        assert_eq!(tool_summary_next("plain progress"), "plain progress");
        assert_eq!(running_summary_next("tool: Bash"), "wait for Bash");

        for status in [
            PaneStatus::Running,
            PaneStatus::Waiting,
            PaneStatus::Done,
            PaneStatus::Error,
            PaneStatus::Stuck,
            PaneStatus::Idle,
            PaneStatus::Unknown,
        ] {
            assert!(!pane_status_token(status).is_empty());
        }
    }

    #[test]
    fn stored_report_preference_requires_status_alignment() {
        let stored = AgentReport {
            status: String::from("waiting"),
            blocker: String::from("approval"),
            next: String::from("approve"),
            updated_at: Instant::now(),
        };
        let synthesized = AgentReport {
            status: String::from("running"),
            blocker: String::from("none"),
            next: String::from("build"),
            updated_at: Instant::now(),
        };

        assert!(should_prefer_synthesized_report(
            &stored,
            &synthesized,
            PaneInsight {
                workload: WorkloadKind::Codex,
                status: PaneStatus::Running,
                last_output_age: None,
            },
            None
        ));
        assert!(!should_prefer_synthesized_report(
            &stored,
            &synthesized,
            PaneInsight {
                workload: WorkloadKind::Codex,
                status: PaneStatus::Waiting,
                last_output_age: None,
            },
            None
        ));
    }

    #[test]
    fn runtime_partial_lines_feed_synthesis() {
        let runtime = PaneRuntime {
            output: VecDeque::new(),
            last_output_at: Some(Instant::now()),
            corpus: String::from("tool bash running"),
            partial_line: String::from("Tool Bash running for 3s..."),
        };

        let report = synthesize_agent_report(
            Some(&runtime),
            insight(PaneStatus::Running, WorkloadKind::ClaudeCode),
        )
        .expect("partial progress should synthesize");

        assert_eq!(report.status, "running");
        assert_eq!(report.next, "wait for Bash");
    }
}
