use std::time::{Duration, Instant};

use super::providers::{
    detect_workload_with_adapters, matches_done_hint, matches_error_hint, matches_waiting_hint,
    parse_source_status_report,
};
use super::{
    ObservedPane, PaneInsight, PaneRuntime, PaneStatus, WorkloadKind, collect_runtime_live_lines,
    infer_status_from_report, is_shell_prompt_noise, pane_corpus, parse_agent_report_line,
    parse_agent_report_lines,
};

pub fn infer_pane_insight(pane: &ObservedPane<'_>, runtime: Option<&PaneRuntime>) -> PaneInsight {
    let workload = infer_workload_kind(pane, runtime);
    let now = Instant::now();
    let last_output_age =
        runtime.and_then(|runtime| runtime.last_output_at.map(|time| now.duration_since(time)));
    let status = infer_pane_status(pane, runtime, workload, last_output_age);

    PaneInsight {
        workload,
        status,
        last_output_age,
    }
}

pub fn pane_heat_score(pane: &ObservedPane<'_>, insight: PaneInsight, acknowledged: bool) -> u16 {
    let status_score = match insight.status {
        PaneStatus::Error => 100,
        PaneStatus::Waiting => 92,
        PaneStatus::Stuck => 84,
        PaneStatus::Running => 66,
        PaneStatus::Idle => 28,
        PaneStatus::Done => 14,
        PaneStatus::Unknown => 8,
    };
    let age_score = match insight.last_output_age {
        Some(age) if age <= Duration::from_secs(5) => 32,
        Some(age) if age <= Duration::from_secs(15) => 24,
        Some(age) if age <= Duration::from_secs(60) => 16,
        Some(age) if age <= Duration::from_secs(180) => 8,
        Some(_) => 0,
        None => 0,
    };
    let workload_score = if insight.workload.is_agent() { 6 } else { 0 };
    let attention_penalty = if acknowledged && is_attention_status(insight.status) {
        18
    } else {
        0
    };
    let active_bonus = if pane.active { 4 } else { 0 };

    status_score + age_score + workload_score + active_bonus - attention_penalty
}

pub(crate) fn infer_workload_kind(
    pane: &ObservedPane<'_>,
    runtime: Option<&PaneRuntime>,
) -> WorkloadKind {
    let command = pane.current_command.to_ascii_lowercase();

    if let Some(workload) = command_workload_hint(&command) {
        return workload;
    }

    if runtime.is_none() && !pane_metadata_has_agent_hint(pane) {
        return match command.as_str() {
            "zsh" | "bash" | "fish" | "sh" => WorkloadKind::Shell,
            _ => WorkloadKind::Job,
        };
    }

    let corpus = pane_corpus(pane, runtime);

    if matches!(command.as_str(), "nvim" | "vim" | "vi" | "less" | "man") {
        return WorkloadKind::Job;
    }

    if let Some(kind) = detect_workload_with_adapters(&command, &corpus) {
        return kind;
    }

    if runtime_has_structured_agent_report(runtime) {
        return WorkloadKind::Agent;
    }

    match command.as_str() {
        "zsh" | "bash" | "fish" | "sh" => {
            if corpus.contains("agent") || corpus.contains("assistant") {
                WorkloadKind::Agent
            } else {
                WorkloadKind::Shell
            }
        }
        "node" | "python" | "python3" | "bun" | "ruby" | "uv" => {
            if corpus.contains("agent") || corpus.contains("assistant") {
                WorkloadKind::Agent
            } else {
                WorkloadKind::Job
            }
        }
        _ => {
            if corpus.contains("agent") || corpus.contains("assistant") {
                WorkloadKind::Agent
            } else {
                WorkloadKind::Job
            }
        }
    }
}

fn command_workload_hint(command: &str) -> Option<WorkloadKind> {
    if command.contains("codex") {
        Some(WorkloadKind::Codex)
    } else if command.contains("claude") {
        Some(WorkloadKind::ClaudeCode)
    } else if command.contains("opencode") {
        Some(WorkloadKind::Opencode)
    } else if command.contains("aider") {
        Some(WorkloadKind::Aider)
    } else if command.contains("gemini") {
        Some(WorkloadKind::Gemini)
    } else {
        None
    }
}

fn pane_metadata_has_agent_hint(pane: &ObservedPane<'_>) -> bool {
    [
        pane.current_command,
        pane.title,
        pane.window_name,
        pane.current_path,
    ]
    .into_iter()
    .any(|value| {
        contains_ascii_case_insensitive(value, "agent")
            || contains_ascii_case_insensitive(value, "assistant")
            || contains_ascii_case_insensitive(value, "codex")
            || contains_ascii_case_insensitive(value, "claude")
            || contains_ascii_case_insensitive(value, "opencode")
            || contains_ascii_case_insensitive(value, "aider")
            || contains_ascii_case_insensitive(value, "gemini")
    })
}

fn contains_ascii_case_insensitive(value: &str, needle: &str) -> bool {
    value
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn runtime_has_structured_agent_report(runtime: Option<&PaneRuntime>) -> bool {
    let lines = runtime
        .map(|runtime| collect_runtime_live_lines(runtime, 8))
        .unwrap_or_default();

    parse_agent_report_lines(&lines).is_some()
}

fn infer_pane_status(
    pane: &ObservedPane<'_>,
    runtime: Option<&PaneRuntime>,
    workload: WorkloadKind,
    last_output_age: Option<Duration>,
) -> PaneStatus {
    let Some(runtime) = runtime else {
        return if pane.active {
            PaneStatus::Idle
        } else {
            PaneStatus::Unknown
        };
    };

    let recent_lines = collect_runtime_live_lines(runtime, 6);
    if !recent_lines.is_empty() && recent_lines.iter().all(|line| is_shell_prompt_noise(line)) {
        return PaneStatus::Idle;
    }

    let current_report_lines = recent_lines
        .iter()
        .take_while(|line| !is_shell_prompt_noise(line))
        .cloned()
        .collect::<Vec<_>>();
    if let Some(status) = parse_agent_report_lines(&current_report_lines)
        .and_then(|report| infer_status_from_report(&report))
    {
        return status;
    }

    let mut saw_newer_shell_prompt = false;
    for line in &recent_lines {
        if is_shell_prompt_noise(line) {
            saw_newer_shell_prompt = true;
            continue;
        }

        if let Some(status) =
            parse_agent_report_line(line).and_then(|report| infer_status_from_report(&report))
        {
            if prompt_return_makes_status_stale(status, saw_newer_shell_prompt) {
                continue;
            }
            return status;
        }
        if let Some(status) = parse_source_status_report(line, workload)
            .and_then(|report| infer_status_from_report(&report))
        {
            if prompt_return_makes_status_stale(status, saw_newer_shell_prompt) {
                continue;
            }
            return status;
        }
        if matches_error_hint(line, workload) {
            return PaneStatus::Error;
        }
        if !saw_newer_shell_prompt && matches_waiting_hint(line, workload) {
            return PaneStatus::Waiting;
        }
        if matches_done_hint(line, workload) {
            return PaneStatus::Done;
        }
        if !saw_newer_shell_prompt && matches_running_hint(line, workload, last_output_age) {
            return PaneStatus::Running;
        }
    }

    match last_output_age {
        Some(age) if age <= Duration::from_secs(8) => PaneStatus::Running,
        Some(age) if workload.is_agent() && age >= Duration::from_secs(180) => PaneStatus::Stuck,
        Some(age) if age <= Duration::from_secs(90) => PaneStatus::Running,
        Some(_) => PaneStatus::Idle,
        None if !recent_lines.is_empty() => PaneStatus::Idle,
        None if pane.active => PaneStatus::Idle,
        None => PaneStatus::Unknown,
    }
}

fn prompt_return_makes_status_stale(status: PaneStatus, saw_newer_shell_prompt: bool) -> bool {
    saw_newer_shell_prompt && matches!(status, PaneStatus::Running | PaneStatus::Waiting)
}

fn matches_running_hint(
    line: &str,
    workload: WorkloadKind,
    last_output_age: Option<Duration>,
) -> bool {
    if !workload.is_agent() || !running_hint_is_current_enough(last_output_age) {
        return false;
    }

    let normalized = line.trim().to_ascii_lowercase();
    let collapsed = normalized.trim_end_matches('.').trim();
    matches!(collapsed, "thinking" | "working" | "running")
        || collapsed.starts_with("thinking ")
        || collapsed.starts_with("working ")
}

fn running_hint_is_current_enough(last_output_age: Option<Duration>) -> bool {
    match last_output_age {
        Some(age) => age < Duration::from_secs(180),
        None => true,
    }
}

fn is_attention_status(status: PaneStatus) -> bool {
    matches!(
        status,
        PaneStatus::Waiting | PaneStatus::Error | PaneStatus::Stuck
    )
}
