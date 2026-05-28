use std::time::Instant;

use super::{AgentReport, WorkloadKind};

#[derive(Clone, Copy)]
struct ToolAdapter {
    kind: WorkloadKind,
    command_markers: &'static [&'static str],
    strong_markers: &'static [&'static str],
    weak_markers: &'static [&'static str],
    waiting_markers: &'static [&'static str],
    done_markers: &'static [&'static str],
    error_markers: &'static [&'static str],
}

const TOOL_ADAPTERS: &[ToolAdapter] = &[
    ToolAdapter {
        kind: WorkloadKind::Codex,
        command_markers: &["codex"],
        strong_markers: &["codex", "apply_patch", "update_plan", "openai", "gpt-5"],
        weak_markers: &["patch", "tool call", "developer message"],
        waiting_markers: &[
            "approval",
            "waiting for approval",
            "waiting on approval",
            "waitingonapproval",
            "approval required",
            "press enter to continue",
            "pending init",
        ],
        done_markers: &["applied patch", "plan updated", "finished"],
        error_markers: &[
            "tool error",
            "command failed",
            "patch failed",
            "agent spawn failed",
            "agent interaction failed",
            "agent resume failed",
            "agent close failed",
            "not found",
        ],
    },
    ToolAdapter {
        kind: WorkloadKind::ClaudeCode,
        command_markers: &["claude"],
        strong_markers: &["claude code", "claude", "anthropic", "sonnet", "opus"],
        weak_markers: &["thinking...", "assistant response"],
        waiting_markers: &[
            "allow?",
            "continue?",
            "approval",
            "confirm?",
            "approve ",
            "worker request",
            "sandbox request",
            "input needed",
            "network access",
            "answer questions?",
            "choose [",
            "dialog open",
        ],
        done_markers: &["done", "completed", "finished"],
        error_markers: &[
            "anthropic",
            "error:",
            "rate limit",
            "failed",
            "tool permission request failed",
        ],
    },
    ToolAdapter {
        kind: WorkloadKind::Opencode,
        command_markers: &["opencode"],
        strong_markers: &["opencode"],
        weak_markers: &["open code"],
        waiting_markers: &[
            "approval",
            "confirm?",
            "continue?",
            "permission request",
            "pending permission",
            "pending permissions",
            "question request",
            "pending question",
            "reply to question",
            "permission.asked",
            "question.asked",
            "permission required",
            "select one answer",
            "select all answers that apply",
            "type your answer",
        ],
        done_markers: &["done", "completed", "finished", "question.replied"],
        error_markers: &[
            "error:",
            "failed",
            "question.rejected",
            "permission rejected",
        ],
    },
    ToolAdapter {
        kind: WorkloadKind::Aider,
        command_markers: &["aider"],
        strong_markers: &["aider", "/add", "/architect", "aider chat"],
        weak_markers: &["repo map", "tokens:"],
        waiting_markers: &["continue?", "press enter"],
        done_markers: &["done", "applied", "committed"],
        error_markers: &["error:", "git error", "failed"],
    },
    ToolAdapter {
        kind: WorkloadKind::Gemini,
        command_markers: &["gemini"],
        strong_markers: &["gemini", "gemini-cli", "google ai"],
        weak_markers: &["google", "model response"],
        waiting_markers: &["continue?", "approval", "confirm?"],
        done_markers: &["done", "completed", "finished"],
        error_markers: &["error:", "quota", "failed"],
    },
];

const DEFAULT_ADAPTER: ToolAdapter = ToolAdapter {
    kind: WorkloadKind::Job,
    command_markers: &[],
    strong_markers: &[],
    weak_markers: &[],
    waiting_markers: &[],
    done_markers: &[],
    error_markers: &[],
};

pub(crate) fn detect_workload_with_adapters(command: &str, corpus: &str) -> Option<WorkloadKind> {
    let mut best_match = None;
    let mut best_score = 0;

    for adapter in TOOL_ADAPTERS {
        let score = adapter_score(adapter, command, corpus);
        if score > best_score {
            best_score = score;
            best_match = Some(adapter.kind);
        }
    }

    if best_score >= 3 { best_match } else { None }
}

fn adapter_score(adapter: &ToolAdapter, command: &str, corpus: &str) -> u8 {
    let mut score = 0;

    if adapter
        .command_markers
        .iter()
        .any(|marker| command.contains(marker))
    {
        score += 4;
    }
    score += adapter
        .strong_markers
        .iter()
        .filter(|marker| corpus.contains(**marker))
        .count() as u8
        * 3;
    score += adapter
        .weak_markers
        .iter()
        .filter(|marker| corpus.contains(**marker))
        .count() as u8;
    score += source_marker_score(adapter.kind, corpus);

    score
}

fn source_marker_score(kind: WorkloadKind, corpus: &str) -> u8 {
    let matched = match kind {
        WorkloadKind::Codex => {
            corpus.contains("pending init")
                || corpus.contains("waitingonapproval")
                || corpus.contains("agent spawn failed")
                || corpus.contains("agent interaction failed")
                || corpus.contains("agent resume failed")
                || corpus.contains("agent close failed")
        }
        WorkloadKind::ClaudeCode => {
            corpus.contains("answer questions?")
                || corpus.contains("choose [")
                || corpus.contains("dialog open")
                || corpus.contains("user has answered your questions")
                || corpus.contains("worker request")
                || corpus.contains("sandbox request")
                || looks_like_claude_tool_progress(corpus)
        }
        WorkloadKind::Opencode => {
            corpus.contains("permission.asked")
                || corpus.contains("permission.replied")
                || corpus.contains("question.asked")
                || corpus.contains("question.replied")
                || corpus.contains("question.rejected")
        }
        _ => false,
    };

    if matched { 3 } else { 0 }
}

fn looks_like_claude_tool_progress(corpus: &str) -> bool {
    (corpus.contains("tool ") && corpus.contains(" running for "))
        || (corpus.contains("tool '") && corpus.contains("' still running ("))
}

pub fn matches_waiting_hint(line: &str, workload: WorkloadKind) -> bool {
    let normalized = line.to_ascii_lowercase();
    let generic = [
        "waiting for approval",
        "waiting on approval",
        "waitingonapproval",
        "waiting for confirmation",
        "waiting for input",
        "needs approval",
        "needs confirmation",
        "press enter",
        "press return",
        "[y/n]",
        "(y/n)",
        "hit enter",
        "allow?",
        "deny?",
        "approve ",
        "approval required",
        "input needed",
        "select an option",
        "choose an option",
        "reply to question",
        "permission.asked",
        "question.asked",
        "permission required",
        "select one answer",
        "select all answers that apply",
        "type your answer",
        "answer questions?",
        "choose [",
        "dialog open",
        "worker request",
        "sandbox request",
    ];

    generic.iter().any(|hint| marker_matches(&normalized, hint))
        || adapter_for(workload)
            .waiting_markers
            .iter()
            .any(|hint| marker_matches(&normalized, hint))
}

pub fn matches_enter_hint(line: &str) -> bool {
    let normalized = line.to_ascii_lowercase();
    [
        "press enter",
        "press return",
        "hit enter",
        "press any key",
        "continue by pressing enter",
    ]
    .iter()
    .any(|hint| normalized.contains(hint))
}

pub fn matches_choice_hint(line: &str) -> bool {
    let normalized = line.to_ascii_lowercase();
    ["[y/n]", "(y/n)", "allow?", "deny?", "approve?", "yes/no"]
        .iter()
        .any(|hint| normalized.contains(hint))
}

pub(crate) fn matches_error_hint(line: &str, workload: WorkloadKind) -> bool {
    let normalized = line.to_ascii_lowercase();
    let generic = [
        "error:",
        "failed",
        "panic",
        "traceback",
        "exception",
        "fatal",
        "timed out",
        "permission denied",
        "could not",
        "cannot ",
    ];

    generic.iter().any(|hint| marker_matches(&normalized, hint))
        || adapter_for(workload)
            .error_markers
            .iter()
            .any(|hint| marker_matches(&normalized, hint))
}

pub(crate) fn matches_done_hint(line: &str, workload: WorkloadKind) -> bool {
    let normalized = line.to_ascii_lowercase();
    let generic = [
        "all set",
        "completed successfully",
        "finished successfully",
        "successfully",
        "resolved",
        "task complete",
        "work complete",
    ];

    exact_status_word_matches(&normalized, &["done", "completed", "finished"])
        || generic.iter().any(|hint| marker_matches(&normalized, hint))
        || adapter_for(workload)
            .done_markers
            .iter()
            .any(|hint| marker_matches(&normalized, hint))
}

fn adapter_for(workload: WorkloadKind) -> &'static ToolAdapter {
    TOOL_ADAPTERS
        .iter()
        .find(|adapter| adapter.kind == workload)
        .unwrap_or(&DEFAULT_ADAPTER)
}

pub(crate) fn parse_source_status_report(
    line: &str,
    workload: WorkloadKind,
) -> Option<AgentReport> {
    let normalized = normalize_summary_line(line);
    let lower = normalized.to_ascii_lowercase();

    match workload {
        WorkloadKind::Codex => {
            if lower == "pending init" {
                return Some(AgentReport {
                    status: String::from("running"),
                    blocker: String::from("none"),
                    next: String::from("initialize agent"),
                    updated_at: Instant::now(),
                });
            }

            if lower == "interrupted" {
                return Some(AgentReport {
                    status: String::from("waiting"),
                    blocker: String::from("interrupted"),
                    next: String::from("resume"),
                    updated_at: Instant::now(),
                });
            }

            if lower == "running" {
                return Some(AgentReport {
                    status: String::from("running"),
                    blocker: String::from("none"),
                    next: String::from("continue work"),
                    updated_at: Instant::now(),
                });
            }

            if lower == "shutdown" {
                return Some(AgentReport {
                    status: String::from("done"),
                    blocker: String::from("none"),
                    next: String::from("agent shutdown"),
                    updated_at: Instant::now(),
                });
            }

            if lower == "completed" || lower.starts_with("completed ") {
                return Some(AgentReport {
                    status: String::from("done"),
                    blocker: String::from("none"),
                    next: normalized
                        .split_once(' ')
                        .map(|(_, tail)| tail.trim().to_owned())
                        .filter(|tail| !tail.is_empty())
                        .unwrap_or_else(|| String::from("completed")),
                    updated_at: Instant::now(),
                });
            }

            if lower == "error" || lower.starts_with("error ") {
                return Some(AgentReport {
                    status: String::from("error"),
                    blocker: normalized
                        .split_once(' ')
                        .map(|(_, tail)| tail.trim().to_owned())
                        .filter(|tail| !tail.is_empty())
                        .unwrap_or_else(|| String::from("error")),
                    next: String::from("show output"),
                    updated_at: Instant::now(),
                });
            }

            if lower == "not found"
                || lower.contains("agent spawn failed")
                || lower.contains("agent interaction failed")
                || lower.contains("agent resume failed")
                || lower.contains("agent close failed")
            {
                return Some(AgentReport {
                    status: String::from("error"),
                    blocker: lower,
                    next: String::from("show output"),
                    updated_at: Instant::now(),
                });
            }

            if let Some(target) = normalized
                .strip_prefix("Waiting for ")
                .map(str::trim)
                .filter(|target| looks_like_plain_codex_wait_state(target))
            {
                return Some(AgentReport {
                    status: String::from("waiting"),
                    blocker: format!("waiting for {}", target.to_ascii_lowercase()),
                    next: String::from("show agents"),
                    updated_at: Instant::now(),
                });
            }
        }
        WorkloadKind::ClaudeCode => {
            if lower.contains("answer questions?") {
                return Some(AgentReport {
                    status: String::from("waiting"),
                    blocker: String::from("input needed"),
                    next: String::from("answer"),
                    updated_at: Instant::now(),
                });
            }

            if lower.contains("choose [") {
                return Some(AgentReport {
                    status: String::from("waiting"),
                    blocker: String::from("input needed"),
                    next: String::from("answer"),
                    updated_at: Instant::now(),
                });
            }

            if lower.contains("dialog open") {
                return Some(AgentReport {
                    status: String::from("waiting"),
                    blocker: String::from("dialog open"),
                    next: String::from("open dialog"),
                    updated_at: Instant::now(),
                });
            }

            if lower.contains("user has answered your questions") {
                return Some(AgentReport {
                    status: String::from("running"),
                    blocker: String::from("none"),
                    next: String::from("resume"),
                    updated_at: Instant::now(),
                });
            }
        }
        WorkloadKind::Opencode => {
            if lower == "permission required" {
                return Some(AgentReport {
                    status: String::from("waiting"),
                    blocker: String::from("approval needed"),
                    next: String::from("approve"),
                    updated_at: Instant::now(),
                });
            }

            if lower == "question"
                || lower == "select one answer"
                || lower == "select all answers that apply"
                || lower == "type your answer..."
            {
                return Some(AgentReport {
                    status: String::from("waiting"),
                    blocker: String::from("input needed"),
                    next: String::from("answer"),
                    updated_at: Instant::now(),
                });
            }

            if lower.contains("permission.asked") {
                return Some(AgentReport {
                    status: String::from("waiting"),
                    blocker: String::from("approval needed"),
                    next: String::from("approve"),
                    updated_at: Instant::now(),
                });
            }

            if lower.contains("question.asked") {
                return Some(AgentReport {
                    status: String::from("waiting"),
                    blocker: String::from("input needed"),
                    next: String::from("answer"),
                    updated_at: Instant::now(),
                });
            }

            if lower.contains("question.replied") {
                return Some(AgentReport {
                    status: String::from("running"),
                    blocker: String::from("none"),
                    next: String::from("resume"),
                    updated_at: Instant::now(),
                });
            }

            if lower.contains("permission.replied") {
                return Some(AgentReport {
                    status: String::from("running"),
                    blocker: String::from("none"),
                    next: String::from("resume"),
                    updated_at: Instant::now(),
                });
            }

            if lower.contains("question.rejected") {
                return Some(AgentReport {
                    status: String::from("error"),
                    blocker: String::from("question rejected"),
                    next: String::from("show output"),
                    updated_at: Instant::now(),
                });
            }
        }
        _ => {}
    }

    None
}

pub(crate) fn summarize_source_status_line(line: &str, workload: WorkloadKind) -> Option<String> {
    let lower = line.to_ascii_lowercase();

    match workload {
        WorkloadKind::Codex => {
            if lower == "pending init" {
                return Some(String::from("starting agent"));
            }
            if lower == "running" {
                return Some(String::from("running"));
            }
            if lower == "interrupted" {
                return Some(String::from("interrupted"));
            }
            if lower == "shutdown" {
                return Some(String::from("agent shutdown"));
            }
            if lower == "completed" || lower.starts_with("completed ") {
                return Some(String::from("completed"));
            }
            if lower == "error" || lower.starts_with("error ") {
                return Some(String::from("error"));
            }
            if lower == "not found" {
                return Some(String::from("agent not found"));
            }
            if lower.contains("agent spawn failed")
                || lower.contains("agent interaction failed")
                || lower.contains("agent resume failed")
                || lower.contains("agent close failed")
            {
                return Some(line.to_owned());
            }
            if let Some(target) = line
                .strip_prefix("Waiting for ")
                .map(str::trim)
                .filter(|target| looks_like_plain_codex_wait_state(target))
            {
                return Some(format!("waiting for {target}"));
            }
        }
        WorkloadKind::ClaudeCode => {
            if lower.contains("answer questions?") || lower.contains("choose [") {
                return Some(String::from("waiting for input"));
            }
            if lower.contains("dialog open") {
                return Some(String::from("waiting for dialog"));
            }
            if lower.contains("user has answered your questions") {
                return Some(String::from("continue from answers"));
            }
            if lower == "conversation compacted" {
                return Some(String::from("conversation compacted"));
            }
        }
        WorkloadKind::Opencode => {
            if lower == "permission required" {
                return Some(String::from("needs approval"));
            }
            if lower == "question"
                || lower == "select one answer"
                || lower == "select all answers that apply"
                || lower == "type your answer..."
            {
                return Some(String::from("waiting for input"));
            }
            if lower.contains("permission.asked") {
                return Some(String::from("needs approval"));
            }
            if lower.contains("question.asked") {
                return Some(String::from("waiting for input"));
            }
            if lower.contains("question.replied") {
                return Some(String::from("continue from answers"));
            }
            if lower.contains("permission.replied") {
                return Some(String::from("continue after permission"));
            }
            if lower.contains("question.rejected") {
                return Some(String::from("question rejected"));
            }
        }
        _ => {}
    }

    None
}

pub(crate) fn summarize_waiting_line(line: &str, workload: WorkloadKind) -> Option<String> {
    if !matches_waiting_hint(line, workload) {
        return None;
    }

    let normalized = line.to_ascii_lowercase();

    if normalized.contains("worker request") {
        return Some(String::from("needs approval: worker request"));
    }

    if normalized.contains("sandbox request") {
        return Some(String::from("needs approval: sandbox request"));
    }

    if normalized.contains("network access") {
        return Some(String::from("needs approval: network access"));
    }

    if normalized.contains("dialog open") {
        return Some(String::from("waiting for dialog"));
    }

    if normalized.contains("reply to question")
        || normalized.contains("answer questions?")
        || normalized.contains("choose [")
        || normalized.contains("choose an option")
        || normalized.contains("select an option")
        || normalized.contains("[y/n]")
        || normalized.contains("(y/n)")
        || normalized.contains("yes/no")
    {
        return Some(String::from("waiting for input"));
    }

    if let Some(target) = extract_after_word(line, "approve ") {
        return Some(format!("needs approval: {target}"));
    }

    if normalized.contains("input needed") {
        return Some(String::from("waiting for input"));
    }

    Some(String::from("needs approval"))
}

pub(crate) fn summarize_tool_progress_line(line: &str) -> Option<String> {
    if let Some(name) = extract_between(line, "Tool ", " running for ") {
        return Some(format!("tool: {name}"));
    }

    if let Some(name) = extract_between(line, "Tool '", "' still running (") {
        return Some(format!("tool: {name}"));
    }

    if let Some(name) = line.strip_prefix("Tool: ").map(str::trim) {
        let name = name.split(" Input:").next().unwrap_or(name).trim();
        if name.to_ascii_lowercase().starts_with("input:") {
            return None;
        }
        if !name.is_empty() {
            return Some(format!("tool: {name}"));
        }
    }

    for tool in [
        "apply_patch",
        "update_plan",
        "exec_command",
        "search_query",
        "image_query",
        "open",
        "click",
        "find",
        "spawn_agent",
        "send_input",
        "wait_agent",
    ] {
        if line
            .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'))
            .any(|token| token == tool)
        {
            return Some(format!("tool: {tool}"));
        }
    }

    None
}

pub(crate) fn marker_matches(normalized: &str, marker: &str) -> bool {
    if marker
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        normalized
            .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'))
            .any(|token| token == marker)
    } else {
        normalized.contains(marker)
    }
}

pub(crate) fn exact_status_word_matches(normalized: &str, words: &[&str]) -> bool {
    let trimmed = normalized.trim();

    words.iter().any(|word| {
        trimmed == *word
            || trimmed == format!("{word}.")
            || trimmed == format!("{word}!")
            || trimmed == format!("{word}:")
    })
}

pub(crate) fn normalize_summary_line(line: &str) -> String {
    line.trim()
        .trim_start_matches(['•', '*', '-', '›', '>', '□', '☑', '✓', '✔', ' '])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn extract_between<'a>(line: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(prefix)?;
    let end = rest.find(suffix)?;
    Some(rest[..end].trim())
}

pub(crate) fn extract_after_word<'a>(line: &'a str, word: &str) -> Option<&'a str> {
    let lower = line.to_ascii_lowercase();
    let start = lower.find(word)?;
    let rest = line.get(start + word.len()..)?.trim();

    (!rest.is_empty()).then_some(rest)
}

fn looks_like_plain_codex_wait_state(target: &str) -> bool {
    !target.is_empty()
        && !target.contains(['?', '!', '.', ':', ';', '[', ']', '(', ')'])
        && target.chars().all(|ch| {
            ch.is_ascii_alphanumeric() || ch.is_ascii_whitespace() || ch == '-' || ch == '_'
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_reports_cover_provider_specific_edge_states() {
        let cases = [
            (
                WorkloadKind::Codex,
                "Shutdown",
                ("done", "none", "agent shutdown"),
            ),
            (
                WorkloadKind::Codex,
                "Agent interaction failed",
                ("error", "agent interaction failed", "show output"),
            ),
            (
                WorkloadKind::Codex,
                "Waiting for worker pool",
                ("waiting", "waiting for worker pool", "show agents"),
            ),
            (
                WorkloadKind::ClaudeCode,
                "Answer questions?",
                ("waiting", "input needed", "answer"),
            ),
            (
                WorkloadKind::Opencode,
                "permission.asked",
                ("waiting", "approval needed", "approve"),
            ),
        ];

        for (workload, line, (status, blocker, next)) in cases {
            let report =
                parse_source_status_report(line, workload).expect("source status should parse");
            assert_eq!(report.status, status, "line: {line}");
            assert_eq!(report.blocker, blocker, "line: {line}");
            assert_eq!(report.next, next, "line: {line}");
        }
    }

    #[test]
    fn waiting_and_tool_summaries_cover_source_variants() {
        assert_eq!(
            summarize_waiting_line("Dialog open", WorkloadKind::ClaudeCode).as_deref(),
            Some("waiting for dialog")
        );
        assert_eq!(
            summarize_waiting_line("Approve network access", WorkloadKind::ClaudeCode).as_deref(),
            Some("needs approval: network access")
        );
        assert_eq!(
            summarize_waiting_line("Approve Bash", WorkloadKind::ClaudeCode).as_deref(),
            Some("needs approval: Bash")
        );
        assert_eq!(
            summarize_tool_progress_line("Tool: Bash Input: cargo test").as_deref(),
            Some("tool: Bash")
        );
        assert_eq!(
            summarize_tool_progress_line("Tool: Input: cargo test").as_deref(),
            None
        );
        assert_eq!(summarize_tool_progress_line("Tool: ").as_deref(), None);
        assert_eq!(
            summarize_tool_progress_line("running apply_patch now").as_deref(),
            Some("tool: apply_patch")
        );
        assert_eq!(
            extract_after_word("please approve network access", "approve "),
            Some("network access")
        );
        assert_eq!(extract_after_word("approve ", "approve "), None);
    }
}
