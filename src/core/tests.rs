use super::*;
use serde::Deserialize;
use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

fn sample_pane<'a>(command: &'a str) -> ObservedPane<'a> {
    ObservedPane {
        current_command: command,
        title: "workspace",
        window_name: "agents",
        current_path: "/workspace",
        active: true,
    }
}

fn runtime(lines: &[&str]) -> PaneRuntime {
    let output = lines.iter().map(|line| String::from(*line)).collect();
    PaneRuntime {
        output,
        last_output_at: Some(Instant::now()),
        corpus: lines.join(" ").to_ascii_lowercase(),
        partial_line: String::new(),
    }
}

fn fixture_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[derive(Debug, Deserialize)]
struct ProviderFixture {
    name: String,
    command: String,
    #[serde(default)]
    lines: Vec<String>,
    #[serde(default)]
    partial_line: String,
    expected_workload: String,
    expected_status: String,
    expected_summary: String,
    expected_report: ExpectedReport,
}

#[derive(Debug, Deserialize)]
struct ExpectedReport {
    status: String,
    blocker: String,
    next: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeStreamFixture {
    name: String,
    chunks: Vec<String>,
    expected_output: Vec<String>,
    expected_partial: String,
    expected_visible_partial: Option<String>,
}

fn parse_workload_kind(value: &str) -> WorkloadKind {
    match value {
        "Codex" => WorkloadKind::Codex,
        "ClaudeCode" => WorkloadKind::ClaudeCode,
        "Opencode" => WorkloadKind::Opencode,
        "Aider" => WorkloadKind::Aider,
        "Gemini" => WorkloadKind::Gemini,
        "Agent" => WorkloadKind::Agent,
        "Shell" => WorkloadKind::Shell,
        "Job" => WorkloadKind::Job,
        other => panic!("unknown workload fixture value: {other}"),
    }
}

fn parse_pane_status(value: &str) -> PaneStatus {
    match value {
        "Running" => PaneStatus::Running,
        "Waiting" => PaneStatus::Waiting,
        "Done" => PaneStatus::Done,
        "Error" => PaneStatus::Error,
        "Stuck" => PaneStatus::Stuck,
        "Idle" => PaneStatus::Idle,
        "Unknown" => PaneStatus::Unknown,
        other => panic!("unknown status fixture value: {other}"),
    }
}

#[test]
fn detects_known_agent_command() {
    let pane = sample_pane("codex");
    assert_eq!(infer_workload_kind(&pane, None), WorkloadKind::Codex);
}

#[test]
fn detects_provider_from_runtime_corpus() {
    let pane = sample_pane("node");
    let runtime = PaneRuntime {
        output: VecDeque::from([String::from("claude is planning the next step")]),
        last_output_at: Some(Instant::now()),
        corpus: String::from("claude is planning the next step"),
        partial_line: String::new(),
    };

    assert_eq!(
        infer_workload_kind(&pane, Some(&runtime)),
        WorkloadKind::ClaudeCode
    );
}

#[test]
fn workload_inference_covers_generic_shell_job_and_agent_fallbacks() {
    let editor = sample_pane("nvim");
    assert_eq!(infer_workload_kind(&editor, None), WorkloadKind::Job);

    let active_job = ObservedPane {
        current_command: "python",
        title: "server",
        window_name: "jobs",
        current_path: "/workspace",
        active: true,
    };
    assert_eq!(infer_workload_kind(&active_job, None), WorkloadKind::Job);

    let shell = ObservedPane {
        current_command: "zsh",
        title: "",
        window_name: "shell",
        current_path: "/workspace",
        active: true,
    };
    assert_eq!(infer_workload_kind(&shell, None), WorkloadKind::Shell);

    let agent_shell_runtime = runtime(&["assistant agent is running inside this shell"]);
    assert_eq!(
        infer_workload_kind(&shell, Some(&agent_shell_runtime)),
        WorkloadKind::Agent
    );

    let node_agent = sample_pane("node");
    let agent_runtime = runtime(&["assistant agent is working"]);
    assert_eq!(
        infer_workload_kind(&node_agent, Some(&agent_runtime)),
        WorkloadKind::Agent
    );

    let wrapper = sample_pane("runner");
    let structured_runtime = runtime(&["STATUS=waiting | BLOCKER=approval | NEXT=approve"]);
    assert_eq!(
        infer_workload_kind(&wrapper, Some(&structured_runtime)),
        WorkloadKind::Agent
    );
    assert_eq!(
        infer_workload_kind(&wrapper, Some(&runtime(&["assistant is working"]))),
        WorkloadKind::Agent
    );
    assert_eq!(
        infer_workload_kind(&wrapper, Some(&runtime(&["compiling assets"]))),
        WorkloadKind::Job
    );

    let quiet_runtime = PaneRuntime {
        output: VecDeque::new(),
        last_output_at: Some(Instant::now() - Duration::from_secs(30)),
        corpus: String::new(),
        partial_line: String::new(),
    };
    assert_eq!(
        infer_pane_insight(&sample_pane("python"), Some(&quiet_runtime)).status,
        PaneStatus::Running
    );
    let old_runtime = PaneRuntime {
        last_output_at: Some(Instant::now() - Duration::from_secs(120)),
        ..quiet_runtime.clone()
    };
    assert_eq!(
        infer_pane_insight(&sample_pane("python"), Some(&old_runtime)).status,
        PaneStatus::Idle
    );
    let inactive = ObservedPane {
        active: false,
        ..sample_pane("python")
    };
    assert_eq!(
        infer_pane_insight(&inactive, None).status,
        PaneStatus::Unknown
    );
    assert_eq!(
        infer_pane_insight(&sample_pane("python"), None).status,
        PaneStatus::Idle
    );

    let never_updated_runtime = PaneRuntime {
        output: VecDeque::new(),
        last_output_at: None,
        corpus: String::new(),
        partial_line: String::new(),
    };
    assert_eq!(
        infer_pane_insight(&sample_pane("runner"), Some(&never_updated_runtime)).status,
        PaneStatus::Idle
    );
    let inactive_runner = ObservedPane {
        active: false,
        ..sample_pane("runner")
    };
    assert_eq!(
        infer_pane_insight(&inactive_runner, Some(&never_updated_runtime)).status,
        PaneStatus::Unknown
    );
}

#[test]
fn heat_score_makes_attention_recent_agent_work_sort_first() {
    let active_agent = sample_pane("codex");
    let inactive_shell = ObservedPane {
        active: false,
        ..sample_pane("zsh")
    };
    let waiting_recent_agent = PaneInsight {
        workload: WorkloadKind::Codex,
        status: PaneStatus::Waiting,
        last_output_age: Some(Duration::from_secs(4)),
    };
    let idle_shell = PaneInsight {
        workload: WorkloadKind::Shell,
        status: PaneStatus::Idle,
        last_output_age: None,
    };

    assert!(
        pane_heat_score(&active_agent, waiting_recent_agent, false)
            > pane_heat_score(&inactive_shell, idle_shell, false)
    );
}

#[test]
fn heat_score_dampens_acknowledged_attention_without_penalizing_done_work() {
    let pane = sample_pane("codex");
    let waiting = PaneInsight {
        workload: WorkloadKind::Codex,
        status: PaneStatus::Waiting,
        last_output_age: Some(Duration::from_secs(10)),
    };
    let done = PaneInsight {
        workload: WorkloadKind::Codex,
        status: PaneStatus::Done,
        last_output_age: Some(Duration::from_secs(10)),
    };

    assert_eq!(
        pane_heat_score(&pane, waiting, false) - pane_heat_score(&pane, waiting, true),
        18
    );
    assert_eq!(
        pane_heat_score(&pane, done, false),
        pane_heat_score(&pane, done, true)
    );
}

#[test]
fn structured_report_status_beats_generic_tail_text() {
    let pane = sample_pane("codex");
    let runtime = runtime(&["done", "STATUS=running | BLOCKER=none | NEXT=write tests"]);

    let insight = infer_pane_insight(&pane, Some(&runtime));
    assert_eq!(insight.status, PaneStatus::Running);
}

#[test]
fn split_protocol_report_status_beats_older_prompt_text() {
    let pane = sample_pane("codex");
    let runtime = runtime(&[
        "Waiting for approval. Continue?",
        "STATUS=running",
        "BLOCKER=none",
        "NEXT=continue build",
    ]);

    let insight = infer_pane_insight(&pane, Some(&runtime));
    let report = effective_agent_report(Some(&runtime), insight, None)
        .expect("split protocol report should synthesize");

    assert_eq!(insight.status, PaneStatus::Running);
    assert_eq!(report.status, "running");
    assert_eq!(report.blocker, "none");
    assert_eq!(report.next, "continue build");
}

#[test]
fn waiting_hint_wins_over_running() {
    let pane = sample_pane("codex");
    let runtime = runtime(&["Waiting for approval. Press Enter to continue."]);

    let insight = infer_pane_insight(&pane, Some(&runtime));
    assert_eq!(insight.status, PaneStatus::Waiting);
}

#[test]
fn newer_error_beats_older_waiting_hint() {
    let pane = sample_pane("bash");
    let runtime = runtime(&["Waiting for approval. Continue?", "error: command failed"]);

    let insight = infer_pane_insight(&pane, Some(&runtime));
    assert_eq!(insight.status, PaneStatus::Error);
}

#[test]
fn newer_done_beats_older_waiting_hint() {
    let pane = sample_pane("bash");
    let runtime = runtime(&["Waiting for approval. Continue?", "done"]);

    let insight = infer_pane_insight(&pane, Some(&runtime));
    assert_eq!(insight.status, PaneStatus::Done);
}

#[test]
fn stale_agent_becomes_stuck() {
    let pane = sample_pane("claude");
    let runtime = PaneRuntime {
        output: VecDeque::from([String::from("thinking")]),
        last_output_at: Some(Instant::now() - Duration::from_secs(240)),
        corpus: String::from("thinking"),
        partial_line: String::new(),
    };

    let insight = infer_pane_insight(&pane, Some(&runtime));
    assert_eq!(insight.status, PaneStatus::Stuck);
}

#[test]
fn visible_agent_thinking_state_is_running_not_idle() {
    let pane = sample_pane("bash");
    let runtime = PaneRuntime {
        output: VecDeque::from([String::from("Codex v0.99"), String::from("Thinking...")]),
        last_output_at: None,
        corpus: String::from("codex thinking"),
        partial_line: String::new(),
    };

    let insight = infer_pane_insight(&pane, Some(&runtime));
    assert_eq!(insight.workload, WorkloadKind::Codex);
    assert_eq!(insight.status, PaneStatus::Running);
}

#[test]
fn prompt_only_shell_runtime_is_idle_not_running() {
    for prompt in ["$", "%", "#", "muxboard: user@host:~/workspace/muxboard$"] {
        let pane = sample_pane("zsh");
        let runtime = runtime(&[prompt]);

        let insight = infer_pane_insight(&pane, Some(&runtime));

        assert_eq!(insight.workload, WorkloadKind::Shell);
        assert_eq!(insight.status, PaneStatus::Idle, "{prompt}");
    }
}

#[test]
fn shell_prompt_after_agent_activity_makes_active_hint_stale() {
    let pane = sample_pane("bash");
    let runtime = PaneRuntime {
        output: VecDeque::from([
            String::from("Codex v0.99"),
            String::from("Thinking..."),
            String::from("ready"),
            String::from("muxboard: ali@tau:~/Projects/muxboard$"),
        ]),
        last_output_at: None,
        corpus: String::from("codex thinking ready muxboard"),
        partial_line: String::new(),
    };

    let insight = infer_pane_insight(&pane, Some(&runtime));
    assert_eq!(insight.workload, WorkloadKind::Codex);
    assert_eq!(insight.status, PaneStatus::Idle);
}

#[test]
fn shell_prompt_after_waiting_hint_makes_waiting_stale() {
    let pane = sample_pane("bash");
    let runtime = PaneRuntime {
        output: VecDeque::from([
            String::from("Codex v0.99"),
            String::from("Waiting for approval. Press Enter to continue."),
            String::from("ready"),
            String::from("muxboard: ali@tau:~/Projects/muxboard$"),
        ]),
        last_output_at: None,
        corpus: String::from("codex waiting approval ready muxboard"),
        partial_line: String::new(),
    };

    let insight = infer_pane_insight(&pane, Some(&runtime));
    assert_eq!(insight.workload, WorkloadKind::Codex);
    assert_eq!(insight.status, PaneStatus::Idle);
}

#[test]
fn shell_prompt_after_agent_error_keeps_error_visible() {
    let pane = sample_pane("bash");
    let runtime = PaneRuntime {
        output: VecDeque::from([
            String::from("Codex v0.99"),
            String::from("error: command failed"),
            String::from("muxboard: ali@tau:~/Projects/muxboard$"),
        ]),
        last_output_at: None,
        corpus: String::from("codex error command failed muxboard"),
        partial_line: String::new(),
    };

    let insight = infer_pane_insight(&pane, Some(&runtime));
    assert_eq!(insight.workload, WorkloadKind::Codex);
    assert_eq!(insight.status, PaneStatus::Error);
}

#[test]
fn waiting_hint_parser_matches_approval_prompt() {
    assert!(matches_waiting_hint(
        "Waiting for approval. Continue?",
        WorkloadKind::Codex
    ));
}

#[test]
fn waiting_hint_parser_ignores_non_prompt_confirmation_words() {
    assert!(!matches_waiting_hint(
        "Confirming repository structure before continuing the task",
        WorkloadKind::Codex
    ));
}

#[test]
fn parses_structured_report_lines() {
    let report = parse_agent_report_line("STATUS=running | BLOCKER=none | NEXT=write tests")
        .expect("structured report should parse");

    assert_eq!(report.status, "running");
    assert_eq!(report.blocker, "none");
    assert_eq!(report.next, "write tests");
}

#[test]
fn parses_bulleted_structured_report_lines() {
    let report = parse_agent_report_line("• STATUS=waiting | BLOCKER=user input | NEXT=confirm")
        .expect("structured report should parse");

    assert_eq!(report.status, "waiting");
    assert_eq!(report.blocker, "user input");
    assert_eq!(report.next, "confirm");
}

#[test]
fn ignores_summary_request_templates_as_agent_reports() {
    assert!(
        parse_agent_report_line(
            "Reply in exactly one line as: STATUS=<status> | BLOCKER=<blocker> | NEXT=<next>."
        )
        .is_none()
    );
    assert!(parse_agent_report_line("STATUS=<status> | BLOCKER=<blocker> | NEXT=<next>").is_none());
}

#[test]
fn effective_agent_report_prefers_fresh_runtime_signal_over_stale_waiting_report() {
    let pane = sample_pane("bash");
    let runtime = runtime(&["Waiting for approval. Continue?", "error: command failed"]);
    let insight = infer_pane_insight(&pane, Some(&runtime));
    let stale = AgentReport {
        status: String::from("waiting"),
        blocker: String::from("approval"),
        next: String::from("press enter"),
        updated_at: Instant::now(),
    };

    let report =
        effective_agent_report(Some(&runtime), insight, Some(&stale)).expect("report should exist");

    assert_eq!(report.status, "error");
    assert_eq!(report.blocker, "error: command failed");
    assert_eq!(report.next, "show output");
}

#[test]
fn effective_agent_report_prefers_fresh_runtime_signal_when_status_matches() {
    let pane = sample_pane("codex");
    let runtime = runtime(&[
        "STATUS=running | BLOCKER=none | NEXT=write renderer regression tests",
        "building renderer observability checks",
    ]);
    let insight = infer_pane_insight(&pane, Some(&runtime));
    let stale = AgentReport {
        status: String::from("running"),
        blocker: String::from("none"),
        next: String::from("read docs"),
        updated_at: Instant::now(),
    };

    let report =
        effective_agent_report(Some(&runtime), insight, Some(&stale)).expect("report should exist");

    assert_eq!(report.status, "running");
    assert_eq!(report.blocker, "none");
    assert_eq!(report.next, "write renderer regression tests");
}

#[test]
fn effective_agent_report_same_status_explicit_runtime_always_beats_stale_stored_report() {
    for (line, expected_status, expected_blocker, expected_next) in [
        (
            "STATUS=running | BLOCKER=none | NEXT=ship fix",
            "running",
            "none",
            "ship fix",
        ),
        (
            "muxboard: status=running; blocker=none; next=ship fix",
            "running",
            "none",
            "ship fix",
        ),
        (
            "STATUS=waiting | BLOCKER=user input | NEXT=choose option",
            "waiting",
            "user input",
            "choose option",
        ),
        (
            "STATUS=done | BLOCKER=none | NEXT=review result",
            "done",
            "none",
            "review result",
        ),
    ] {
        let pane = sample_pane("codex");
        let runtime = runtime(&[line]);
        let insight = infer_pane_insight(&pane, Some(&runtime));
        let stale = AgentReport {
            status: expected_status.to_owned(),
            blocker: String::from("none"),
            next: String::from("old stale action"),
            updated_at: Instant::now(),
        };

        let report = effective_agent_report(Some(&runtime), insight, Some(&stale))
            .unwrap_or_else(|| panic!("report should exist for line: {line}"));

        assert_eq!(report.status, expected_status, "line: {line}");
        assert_eq!(report.blocker, expected_blocker, "line: {line}");
        assert_eq!(report.next, expected_next, "line: {line}");
    }
}

#[test]
fn effective_agent_report_does_not_replace_rich_stored_report_with_generic_inference() {
    let pane = sample_pane("claude");
    let runtime = runtime(&["Waiting for approval. Continue?"]);
    let insight = infer_pane_insight(&pane, Some(&runtime));
    let stored = AgentReport {
        status: String::from("waiting"),
        blocker: String::from("approval: staging network"),
        next: String::from("approve staging network request"),
        updated_at: Instant::now(),
    };

    let report = effective_agent_report(Some(&runtime), insight, Some(&stored))
        .expect("report should exist");

    assert_eq!(report.status, "waiting");
    assert_eq!(report.blocker, "approval: staging network");
    assert_eq!(report.next, "approve staging network request");
}

#[test]
fn detects_codex_from_captured_output() {
    let pane = sample_pane("node");
    let runtime = runtime(&["apply_patch and update_plan are available"]);

    assert_eq!(
        infer_workload_kind(&pane, Some(&runtime)),
        WorkloadKind::Codex
    );
}

#[test]
fn generic_node_with_structured_status_is_labeled_as_agent_not_plain_job() {
    let pane = sample_pane("node");
    let runtime = runtime(&["STATUS=running | BLOCKER=none | NEXT=write tests"]);

    assert_eq!(
        infer_workload_kind(&pane, Some(&runtime)),
        WorkloadKind::Agent
    );
}

#[test]
fn captured_inactive_pane_with_output_is_idle_not_unknown() {
    let mut pane = sample_pane("bash");
    pane.active = false;
    let runtime = PaneRuntime {
        output: VecDeque::from([String::from("ali@host:~/project$")]),
        last_output_at: None,
        corpus: String::from("bash shell ali@host:~/project$"),
        partial_line: String::new(),
    };

    assert_eq!(
        infer_pane_insight(&pane, Some(&runtime)).status,
        PaneStatus::Idle
    );
}

#[test]
fn activity_summary_prefers_tool_progress_over_prompt_scaffolding() {
    let summary = activity_summary(
        WorkloadKind::ClaudeCode,
        "claude",
        None,
        &[
            String::from("Reply in exactly one line as: STATUS=running"),
            String::from("Tool Bash running for 3s..."),
        ],
    );

    assert_eq!(summary, "tool: Bash");
}

#[test]
fn activity_summary_prefers_user_intent_over_prompt_templates_and_banners() {
    let summary = activity_summary(
        WorkloadKind::Codex,
        "node",
        None,
        &[
            String::from("Reply in exactly one line as: STATUS=<status> | BLOCKER=<blocker> |"),
            String::from("NEXT=<next>."),
            String::from("gpt-5.4 high · ~/Projects/muxboard"),
            String::from("› Run /review on my current changes"),
        ],
    );

    assert_eq!(summary, "review changes");
}

#[test]
fn activity_summary_ignores_shell_prompt_noise() {
    let summary = activity_summary(
        WorkloadKind::Shell,
        "bash",
        None,
        &[
            String::from("ready"),
            String::from("muxboard: ali@tau:~/Projects/muxboard$"),
        ],
    );

    assert_eq!(summary, "ready");
    assert!(!is_meaningful_live_fragment(
        "muxboard: ali@tau:~/Projects/muxboard$"
    ));
}

#[test]
fn activity_summary_ignores_shell_startup_banner_noise() {
    let summary = activity_summary(
        WorkloadKind::Shell,
        "bash",
        None,
        &[
            String::from("ready"),
            String::from("The default interactive shell is now zsh."),
            String::from("To update your account to use zsh, please run `chsh -s /bin/zsh`."),
            String::from("For more details, please visit https://support.apple.com/kb/HT208050."),
        ],
    );

    assert_eq!(summary, "ready");
    assert!(!is_meaningful_live_fragment(
        "For more details, please visit https://support.apple.com/kb/HT208050."
    ));
}

#[test]
fn activity_summary_prefers_specific_progress_over_generic_running_marker() {
    let summary = activity_summary(
        WorkloadKind::Codex,
        "codex",
        None,
        &[
            String::from("Running"),
            String::from("building release artifacts"),
        ],
    );

    assert_eq!(summary, "building release artifacts");
}

#[test]
fn activity_summary_prefers_tool_progress_over_generic_running_marker() {
    let summary = activity_summary(
        WorkloadKind::Codex,
        "codex",
        None,
        &[
            String::from("Running"),
            String::from("Tool Bash running for 3s..."),
        ],
    );

    assert_eq!(summary, "tool: Bash");
}

#[test]
fn activity_summary_prefers_specific_progress_over_resume_event() {
    let summary = activity_summary(
        WorkloadKind::Opencode,
        "opencode",
        None,
        &[
            String::from("question.replied"),
            String::from("building release artifacts"),
        ],
    );

    assert_eq!(summary, "building release artifacts");
}

#[test]
fn activity_summary_prefers_specific_progress_over_pending_init_marker() {
    let summary = activity_summary(
        WorkloadKind::Codex,
        "codex",
        None,
        &[
            String::from("Pending init"),
            String::from("loading workspace"),
        ],
    );

    assert_eq!(summary, "loading workspace");
}

#[test]
fn activity_summary_trims_visual_ellipsis_from_progress_lines() {
    let summary = activity_summary(
        WorkloadKind::Job,
        "bash",
        None,
        &[String::from("building...")],
    );

    assert_eq!(summary, "building");
}

#[test]
fn activity_summary_prefers_newer_specific_progress_when_priorities_tie() {
    let summary = activity_summary(
        WorkloadKind::Job,
        "bash",
        None,
        &[
            String::from("indexing repository"),
            String::from("writing integration tests"),
        ],
    );

    assert_eq!(summary, "writing integration tests");
}

#[test]
fn activity_summary_prefers_signal_noun_phrase_over_newer_weaker_progress() {
    let summary = activity_summary(
        WorkloadKind::Job,
        "bash",
        None,
        &[
            String::from("building release artifacts"),
            String::from("writing logs"),
        ],
    );

    assert_eq!(summary, "building release artifacts");
}

#[test]
fn activity_summary_prefers_stronger_signal_noun_over_newer_weaker_signal() {
    let summary = activity_summary(
        WorkloadKind::Job,
        "bash",
        None,
        &[
            String::from("syncing shell aliases"),
            String::from("updating staging"),
        ],
    );

    assert_eq!(summary, "syncing shell aliases");
}

#[test]
fn activity_summary_prefers_later_phase_over_newer_earlier_phase() {
    let summary = activity_summary(
        WorkloadKind::Job,
        "bash",
        None,
        &[
            String::from("completed staging handoff"),
            String::from("preparing release image"),
        ],
    );

    assert_eq!(summary, "completed staging handoff");
}

#[test]
fn activity_summary_prefers_validation_phase_over_newer_prep_phase() {
    let summary = activity_summary(
        WorkloadKind::Job,
        "bash",
        None,
        &[
            String::from("validating checksums"),
            String::from("preparing release image"),
        ],
    );

    assert_eq!(summary, "validating checksums");
}

#[test]
fn activity_summary_normalizes_claude_network_approval() {
    let summary = activity_summary(
        WorkloadKind::ClaudeCode,
        "claude",
        None,
        &[String::from(
            "Waiting for leader to approve network access to api.example.com",
        )],
    );

    assert_eq!(summary, "needs approval: network access");
}

#[test]
fn activity_summary_covers_source_backed_claude_surfaces() {
    let cases = [
        ("worker request", "needs approval: worker request"),
        ("sandbox request", "needs approval: sandbox request"),
        ("dialog open", "waiting for dialog"),
        ("input needed", "waiting for input"),
        ("Answer questions?", "waiting for input"),
        ("Choose [1/2] (default: 1):", "waiting for input"),
        (
            "User has answered your questions: \"A\"=\"B\"",
            "continue from answers",
        ),
        ("Conversation compacted", "conversation compacted"),
        (
            "Tool 'WebFetch' still running (12 elapsed)",
            "tool: WebFetch",
        ),
    ];

    for (line, expected) in cases {
        let summary = activity_summary(
            WorkloadKind::ClaudeCode,
            "claude",
            None,
            &[String::from(line)],
        );
        assert_eq!(summary, expected, "line: {line}");
    }
}

#[test]
fn activity_summary_covers_source_backed_codex_surfaces() {
    let cases = [
        ("Pending init", "starting agent"),
        ("Running", "running"),
        ("Interrupted", "interrupted"),
        ("Completed", "completed"),
        ("Completed 39916800", "completed"),
        ("Error tool timeout", "error"),
        ("Shutdown", "agent shutdown"),
        ("Not found", "agent not found"),
        ("Waiting for agents", "waiting for agents"),
        ("Agent spawn failed", "Agent spawn failed"),
    ];

    for (line, expected) in cases {
        let summary = activity_summary(WorkloadKind::Codex, "codex", None, &[String::from(line)]);
        assert_eq!(summary, expected, "line: {line}");
    }
}

#[test]
fn activity_summary_covers_source_backed_opencode_surfaces() {
    let cases = [
        ("permission.asked", "needs approval"),
        ("question.asked", "waiting for input"),
        ("question.replied", "continue from answers"),
        ("question.rejected", "question rejected"),
        ("Permission required", "needs approval"),
        ("Question", "waiting for input"),
        ("Select one answer", "waiting for input"),
        ("Select all answers that apply", "waiting for input"),
        ("Type your answer...", "waiting for input"),
    ];

    for (line, expected) in cases {
        let summary = activity_summary(
            WorkloadKind::Opencode,
            "opencode",
            None,
            &[String::from(line)],
        );
        assert_eq!(summary, expected, "line: {line}");
    }
}

#[test]
fn claude_approve_prompt_maps_to_waiting_status() {
    let pane = sample_pane("claude");
    let runtime = runtime(&["approve Bash"]);

    let insight = infer_pane_insight(&pane, Some(&runtime));
    assert_eq!(insight.status, PaneStatus::Waiting);
}

#[test]
fn codex_waiting_on_approval_maps_to_waiting_status() {
    let pane = sample_pane("codex");
    let runtime = runtime(&["state: waitingOnApproval"]);

    let insight = infer_pane_insight(&pane, Some(&runtime));
    assert_eq!(insight.status, PaneStatus::Waiting);
}

#[test]
fn codex_pending_init_maps_to_running_status() {
    let pane = sample_pane("codex");
    let runtime = runtime(&["Pending init"]);

    let insight = infer_pane_insight(&pane, Some(&runtime));
    assert_eq!(insight.status, PaneStatus::Running);
}

#[test]
fn codex_running_and_error_source_states_map_to_status() {
    let pane = sample_pane("codex");

    let running = runtime(&["Running"]);
    assert_eq!(
        infer_pane_insight(&pane, Some(&running)).status,
        PaneStatus::Running
    );

    let errored = runtime(&["Error tool timeout"]);
    assert_eq!(
        infer_pane_insight(&pane, Some(&errored)).status,
        PaneStatus::Error
    );
}

#[test]
fn opencode_question_asked_maps_to_waiting_status() {
    let pane = sample_pane("opencode");
    let runtime = runtime(&["question.asked"]);

    let insight = infer_pane_insight(&pane, Some(&runtime));
    assert_eq!(insight.status, PaneStatus::Waiting);
}

#[test]
fn opencode_permission_and_question_ui_prompts_map_to_waiting_status() {
    let pane = sample_pane("opencode");

    for line in [
        "Permission required",
        "Question",
        "Select one answer",
        "Select all answers that apply",
        "Type your answer...",
    ] {
        let runtime = runtime(&[line]);
        assert_eq!(
            infer_pane_insight(&pane, Some(&runtime)).status,
            PaneStatus::Waiting,
            "line: {line}"
        );
    }
}

#[test]
fn effective_agent_report_synthesizes_wait_reason() {
    let pane = sample_pane("claude");
    let runtime = runtime(&["Waiting for leader to approve network access to api.example.com"]);
    let insight = infer_pane_insight(&pane, Some(&runtime));

    let report =
        effective_agent_report(Some(&runtime), insight, None).expect("report should synthesize");

    assert_eq!(report.status, "waiting");
    assert_eq!(report.blocker, "approval: network access");
    assert_eq!(report.next, "approve");
}

#[test]
fn effective_agent_report_synthesizes_tool_progress_from_partial_line() {
    let pane = sample_pane("claude");
    let runtime = PaneRuntime {
        output: VecDeque::new(),
        last_output_at: Some(Instant::now()),
        corpus: String::from("tool bash running"),
        partial_line: String::from("Tool Bash running for 3s..."),
    };
    let insight = infer_pane_insight(&pane, Some(&runtime));

    let report =
        effective_agent_report(Some(&runtime), insight, None).expect("report should synthesize");

    assert_eq!(report.status, "running");
    assert_eq!(report.blocker, "none");
    assert_eq!(report.next, "wait for Bash");
}

#[test]
fn append_output_chunk_tracks_partial_and_complete_lines() {
    let mut runtime = PaneRuntime::default();

    assert_eq!(append_output_chunk(&mut runtime, "hello"), None);
    assert!(runtime.output.is_empty());
    assert_eq!(runtime.partial_line, "hello");

    let latest = append_output_chunk(&mut runtime, " world\nsecond line\n");
    assert_eq!(latest.as_deref(), Some("second line"));
    assert_eq!(
        runtime.output.into_iter().collect::<Vec<_>>(),
        vec![String::from("hello world"), String::from("second line")]
    );
}

#[test]
fn runtime_corpus_and_fragments_filter_visual_noise() {
    let lines = VecDeque::from([
        String::from(""),
        String::from("first"),
        String::from("second"),
        String::from(""),
    ]);
    let runtime = PaneRuntime {
        output: lines,
        last_output_at: None,
        corpus: String::new(),
        partial_line: String::from("partial progress"),
    };
    assert_eq!(
        collect_runtime_live_lines(&runtime, 2),
        vec![String::from("partial progress"), String::from("")]
    );
    assert!(!is_meaningful_live_fragment(""));
    assert!(!is_meaningful_live_fragment("ab"));
    assert!(is_meaningful_live_fragment("building release"));
}

#[test]
fn live_fragment_filter_hides_prompt_glyphs_and_short_echoes() {
    for fragment in ["❯", ">", ">>", "a", "ab", "x/"] {
        assert!(
            !is_meaningful_live_fragment(fragment),
            "{fragment:?} should stay hidden"
        );
    }

    assert!(is_meaningful_live_fragment("go build ./..."));
    assert!(is_meaningful_live_fragment("OK done"));
}

#[test]
fn runtime_corpus_uses_meaningful_partial_lines_without_prompt_noise() {
    let pane = sample_pane("codex");
    let mut runtime = PaneRuntime {
        output: VecDeque::from([String::from("checking tests")]),
        last_output_at: Some(Instant::now()),
        corpus: String::new(),
        partial_line: String::from("❯"),
    };

    let corpus = build_runtime_corpus(&pane, &runtime);
    assert!(corpus.contains("checking tests"));
    assert!(!corpus.contains("❯"));

    runtime.partial_line = String::from("Reply in exactly one line as: STATUS=running");
    let corpus = build_runtime_corpus(&pane, &runtime);
    assert!(corpus.contains("reply in exactly one line as: status=running"));
}

#[test]
fn append_output_chunk_handles_multiple_carriage_return_rewrites() {
    let mut runtime = PaneRuntime::default();

    append_output_chunk(&mut runtime, "loading");
    append_output_chunk(&mut runtime, "\rthinking");
    let latest = append_output_chunk(&mut runtime, "\rready\n");

    assert_eq!(latest.as_deref(), Some("ready"));
    assert_eq!(
        runtime.output.into_iter().collect::<Vec<_>>(),
        vec![String::from("ready")]
    );
}

#[test]
fn append_output_chunk_handles_backspace_corrections() {
    let mut runtime = PaneRuntime::default();

    let latest = append_output_chunk(&mut runtime, "erroor\u{8}\u{8}r\n");

    assert_eq!(latest.as_deref(), Some("error"));
    assert_eq!(
        runtime.output.into_iter().collect::<Vec<_>>(),
        vec![String::from("error")]
    );
}

#[test]
fn visible_partial_line_keeps_long_meaningful_progress_text() {
    let runtime = PaneRuntime {
        output: VecDeque::new(),
        last_output_at: Some(Instant::now()),
        corpus: String::from("tool bash running long progress"),
        partial_line: String::from(
            "Tool Bash running for 300s while compiling a very long dependency graph",
        ),
    };

    assert_eq!(
        visible_partial_line(&runtime),
        Some("Tool Bash running for 300s while compiling a very long dependency graph")
    );
}

#[test]
fn structured_status_line_beats_older_waiting_prompt() {
    let pane = sample_pane("node");
    let runtime = runtime(&[
        "Waiting for approval. Continue?",
        "STATUS=running | BLOCKER=none | NEXT=write tests",
    ]);

    let insight = infer_pane_insight(&pane, Some(&runtime));
    let report =
        effective_agent_report(Some(&runtime), insight, None).expect("report should exist");

    assert_eq!(insight.status, PaneStatus::Running);
    assert_eq!(report.status, "running");
    assert_eq!(report.next, "write tests");
}

#[test]
fn provider_contract_fixtures_hold() {
    let fixtures = fs::read_to_string(fixture_path("tests/fixtures/core/provider_contracts.json"))
        .expect("provider fixtures should read");
    let fixtures: Vec<ProviderFixture> =
        serde_json::from_str(&fixtures).expect("provider fixtures should parse");

    for fixture in fixtures {
        let pane = sample_pane(&fixture.command);
        let mut runtime = PaneRuntime {
            output: fixture.lines.clone().into(),
            last_output_at: Some(Instant::now()),
            corpus: String::new(),
            partial_line: fixture.partial_line.clone(),
        };
        runtime.corpus = build_runtime_corpus(&pane, &runtime);

        let workload = infer_workload_kind(&pane, Some(&runtime));
        assert_eq!(
            workload,
            parse_workload_kind(&fixture.expected_workload),
            "fixture: {} workload",
            fixture.name
        );

        let insight = infer_pane_insight(&pane, Some(&runtime));
        assert_eq!(
            insight.status,
            parse_pane_status(&fixture.expected_status),
            "fixture: {} status",
            fixture.name
        );

        let live_lines = collect_runtime_live_lines(&runtime, 8);
        let summary = activity_summary(workload, pane.current_command, None, &live_lines);
        assert_eq!(
            summary, fixture.expected_summary,
            "fixture: {} summary",
            fixture.name
        );

        let report =
            effective_agent_report(Some(&runtime), insight, None).expect("report should exist");
        assert_eq!(
            report.status, fixture.expected_report.status,
            "fixture: {} report status",
            fixture.name
        );
        assert_eq!(
            report.blocker, fixture.expected_report.blocker,
            "fixture: {} report blocker",
            fixture.name
        );
        assert_eq!(
            report.next, fixture.expected_report.next,
            "fixture: {} report next",
            fixture.name
        );
    }
}

#[test]
fn runtime_stream_fixtures_hold() {
    let fixtures = fs::read_to_string(fixture_path("tests/fixtures/core/runtime_streams.json"))
        .expect("runtime stream fixtures should read");
    let fixtures: Vec<RuntimeStreamFixture> =
        serde_json::from_str(&fixtures).expect("runtime stream fixtures should parse");

    for fixture in fixtures {
        let mut runtime = PaneRuntime::default();
        for chunk in &fixture.chunks {
            append_output_chunk(&mut runtime, chunk);
        }

        assert_eq!(
            runtime.output.iter().cloned().collect::<Vec<_>>(),
            fixture.expected_output,
            "fixture: {} output",
            fixture.name
        );
        assert_eq!(
            runtime.partial_line, fixture.expected_partial,
            "fixture: {} partial",
            fixture.name
        );
        assert_eq!(
            visible_partial_line(&runtime).map(str::to_owned),
            fixture.expected_visible_partial,
            "fixture: {} visible partial",
            fixture.name
        );
    }
}
