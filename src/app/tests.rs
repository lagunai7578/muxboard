use super::{
    AlertPolicy, App, FanoutMode, FilterMode, LayoutPreset, MetricsMode, NotificationSettings,
    PaneRuntime, PaneStatus, SmartAction, SortMode, ThemePreset, UiSettings, WorkloadKind,
    attention_label, attention_rank, default_macro_slots, expand_command_template,
    launch_window_name,
};
use crate::{
    cli::Cli,
    core::{
        AgentSourceEvent, AgentSourceProvider, AgentSourceScanner, build_pane_corpus,
        build_runtime_corpus,
    },
    metrics, notifications, state,
    tmux::{AgentBridgeEvent, Pane, Probe, Snapshot, Target},
};
use serde::Deserialize;
use std::{
    collections::{HashMap, VecDeque},
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static TEST_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn sample_pane(command: &str) -> Pane {
    Pane {
        id: String::from("%1"),
        session_id: String::from("$0"),
        session_name: String::from("demo"),
        window_id: String::from("@0"),
        window_name: String::from("agents"),
        pane_index: 0,
        pane_pid: 4242,
        title: String::from("workspace"),
        current_command: String::from(command),
        current_path: String::from("/workspace"),
        active: true,
        alternate_on: false,
        agent_event: None,
    }
}

pub(crate) fn mark_pane_done_for_review(
    pane: &mut Pane,
    agent: &str,
    summary: &str,
    thread_name: &str,
    progress: &str,
) {
    pane.agent_event = Some(AgentBridgeEvent {
        agent: agent.to_owned(),
        state: String::from("done"),
        summary: summary.to_owned(),
        thread_name: Some(thread_name.to_owned()),
        progress: Some(progress.to_owned()),
        unseen: Some(true),
        ..AgentBridgeEvent::default()
    });
}

pub(crate) fn mark_pane_running_agent(
    pane: &mut Pane,
    agent: &str,
    summary: &str,
    thread_name: &str,
    progress: &str,
) {
    pane.agent_event = Some(AgentBridgeEvent {
        agent: agent.to_owned(),
        state: String::from("running"),
        summary: summary.to_owned(),
        thread_name: Some(thread_name.to_owned()),
        progress: Some(progress.to_owned()),
        log: Some(String::from("native transcript is active")),
        ..AgentBridgeEvent::default()
    });
}

fn target_group(name: &str, window_name: &str, pane_index: u32) -> super::TargetGroup {
    super::TargetGroup {
        name: String::from(name),
        members: vec![super::PaneLocator {
            session_name: String::from("demo"),
            window_name: String::from(window_name),
            pane_index,
        }],
    }
}

pub(crate) fn app_with_panes(panes: Vec<Pane>, runtimes: Vec<(&str, Vec<&str>)>) -> App {
    let mut pane_runtime = std::collections::HashMap::new();

    for (pane_id, output) in runtimes {
        pane_runtime.insert(
            pane_id.to_string(),
            PaneRuntime {
                output: output
                    .into_iter()
                    .map(String::from)
                    .collect::<VecDeque<_>>(),
                last_output_at: Some(Instant::now()),
                corpus: String::new(),
                partial_line: String::new(),
            },
        );
    }

    let temp_root = unique_test_path("app-test", "");
    let state_path = temp_root.join("state.json");
    let config_path = temp_root.join("config.json");
    let mut sessions = Vec::new();
    let mut windows = Vec::new();

    for pane in &panes {
        if !sessions
            .iter()
            .any(|session: &crate::tmux::Session| session.id == pane.session_id)
        {
            sessions.push(crate::tmux::Session {
                id: pane.session_id.clone(),
                name: pane.session_name.clone(),
            });
        }
        if !windows
            .iter()
            .any(|window: &crate::tmux::Window| window.id == pane.window_id)
        {
            windows.push(crate::tmux::Window {
                id: pane.window_id.clone(),
                session_id: pane.session_id.clone(),
                session_name: pane.session_name.clone(),
                name: pane.window_name.clone(),
            });
        }
    }

    for pane in &panes {
        if let Some(runtime) = pane_runtime.get_mut(&pane.id) {
            runtime.corpus =
                build_pane_corpus(&crate::core::ObservedPane::from(pane), &runtime.output);
        }
    }

    App {
        cli: Cli {
            tmux_bin: String::from("tmux"),
            socket: None,
            session: None,
            dump_probe_json: false,
            print_config_example: false,
            print_default_keybindings: false,
            theme: None,
            theme_picker: false,
            command: None,
        },
        probe: Probe {
            version: String::from("tmux 3.5a"),
            target: Target {
                binary: String::from("tmux"),
                socket: None,
                session: None,
            },
        },
        runtime_context: crate::tmux::RuntimeContext::default(),
        snapshot: Snapshot {
            sessions,
            windows,
            panes,
        },
        control: None,
        selected_pane_id: Some(String::from("%1")),
        selected_window_id: Some(String::from("@0")),
        initial_attention_autofocus: false,
        pane_runtime,
        dirty_pane_ids: std::collections::HashSet::new(),
        native_agent_scanner: AgentSourceScanner::disabled(),
        native_agent_assignments: std::collections::HashMap::new(),
        native_agent_versions: std::collections::HashMap::new(),
        last_native_agent_scan: None,
        pane_metrics: std::collections::HashMap::new(),
        pane_last_status: std::collections::HashMap::new(),
        last_alerted_at: std::collections::HashMap::new(),
        acknowledged_attention: std::collections::HashMap::new(),
        pending_attention_actions: std::collections::HashMap::new(),
        state_store: state::Store::new_at(state_path),
        config_store: crate::config::Store::new_at(config_path),
        notifier: notifications::Notifier::with_mode_for_test(
            notifications::NotificationMode::LocalDesktop,
        ),
        notification_settings: NotificationSettings::default(),
        search_query: String::new(),
        search_query_before_input: None,
        search_input_active: false,
        command_buffer: String::new(),
        command_input_active: false,
        launch_buffer: String::new(),
        launch_input_active: false,
        recent_commands: VecDeque::new(),
        macro_slots: default_macro_slots(),
        macro_assign_active: false,
        action_menu_active: false,
        help_overlay_active: false,
        group_name_buffer: String::new(),
        group_input_active: false,
        fleet_picker_active: false,
        fleet_picker_index: 0,
        theme_picker_active: false,
        theme_picker_page: super::ThemePickerPage::Top,
        theme_picker_index: 0,
        theme_picker_first_run: false,
        target_groups: Vec::new(),
        selected_group_index: None,
        active_group_name: None,
        marked_pane_ids: std::collections::HashSet::new(),
        ui_settings: UiSettings::default(),
        context_pane: super::ContextPane::Inspect,
        panel_focus: super::PanelFocus::Fleet,
        details_scroll: 0,
        rendered_scroll_context: std::cell::Cell::new(super::ContextPane::Inspect),
        rendered_scroll_viewport_lines: std::cell::Cell::new(super::DETAILS_OUTPUT_VIEWPORT_LINES),
        rendered_scroll_content_lines: std::cell::Cell::new(0),
        view_scope: super::ViewScope::All,
        fanout_mode: FanoutMode::Off,
        metrics_mode: super::MetricsMode::Off,
        sort_mode: SortMode::Attention,
        filter_mode: FilterMode::All,
        refresh_count: 1,
        notification_count: 0,
        alert_count: 0,
        pending_bell: false,
        last_metrics_refresh: None,
        pending_dispatch: None,
        pane_reports: std::collections::HashMap::new(),
        control_state: String::from("connected"),
        close_after_jump: false,
        should_quit: false,
        status_message: String::new(),
        recent_alerts: VecDeque::new(),
        recent_events: VecDeque::new(),
    }
}

pub(crate) fn apply_rebound_keybindings(app: &mut App) {
    app.ui_settings.keybindings.move_down = vec![String::from("n")];
    app.ui_settings.keybindings.move_up = vec![String::from("p")];
    app.ui_settings.keybindings.focus = vec![String::from("o")];
    app.ui_settings.keybindings.jump = vec![String::from("l")];
    app.ui_settings.keybindings.mark = vec![String::from("v")];
    app.ui_settings.keybindings.command = vec![String::from("c")];
    app.ui_settings.keybindings.search = vec![String::from("f")];
    app.ui_settings.keybindings.actions = vec![String::from("m")];
    app.ui_settings.keybindings.smart_action = vec![String::from("u")];
    app.ui_settings.keybindings.quit = vec![String::from("z")];
    app.ui_settings.keybindings.action_enter_queue = vec![String::from(";")];
    app.ui_settings.keybindings.action_zoom = vec![String::from("0")];
    app.ui_settings.keybindings.action_layout = vec![String::from("8")];
}

pub(crate) fn set_pane_runtime(app: &mut App, pane_id: &str, runtime: PaneRuntime) {
    app.pane_runtime.insert(String::from(pane_id), runtime);
}

pub(crate) fn set_runtime_lines_without_age(
    app: &mut App,
    pane_id: &str,
    lines: &[&str],
    corpus: &str,
) {
    set_pane_runtime(
        app,
        pane_id,
        PaneRuntime {
            output: lines.iter().map(|line| String::from(*line)).collect(),
            last_output_at: None,
            corpus: String::from(corpus),
            partial_line: String::new(),
        },
    );
}

pub(crate) fn mark_pane_runtime_stale(app: &mut App, pane_id: &str, age: Duration) {
    let runtime = app
        .pane_runtime
        .get_mut(pane_id)
        .expect("test pane runtime should exist");

    runtime.last_output_at = Some(Instant::now() - age);
}

pub(crate) fn set_live_partial_runtime(
    app: &mut App,
    pane_id: &str,
    corpus: &str,
    partial_line: &str,
) {
    set_pane_runtime(
        app,
        pane_id,
        PaneRuntime {
            output: VecDeque::new(),
            last_output_at: Some(Instant::now()),
            corpus: String::from(corpus),
            partial_line: String::from(partial_line),
        },
    );
}

pub(crate) fn set_pane_report(app: &mut App, pane_id: &str, report: crate::core::AgentReport) {
    app.pane_reports.insert(String::from(pane_id), report);
}

pub(crate) fn use_notification_mode_for_test(app: &mut App, mode: notifications::NotificationMode) {
    app.notifier = notifications::Notifier::with_mode_for_test(mode);
}

fn coverage_instrumented() -> bool {
    std::env::var_os("LLVM_PROFILE_FILE").is_some() || std::env::var_os("CARGO_LLVM_COV").is_some()
}

#[test]
fn close_after_jump_env_flag_parses_plain_truthy_values() {
    for value in [
        Some("1"),
        Some("true"),
        Some("yes"),
        Some("on"),
        Some(" ON "),
    ] {
        assert!(
            super::env_flag_enabled(value),
            "{value:?} should enable close-after-jump"
        );
    }

    for value in [
        None,
        Some(""),
        Some("0"),
        Some("false"),
        Some("off"),
        Some("no"),
    ] {
        assert!(
            !super::env_flag_enabled(value),
            "{value:?} should leave close-after-jump disabled"
        );
    }
}

pub(crate) fn set_pane_report_fields(
    app: &mut App,
    pane_id: &str,
    status: &str,
    blocker: &str,
    next: &str,
) {
    set_pane_report(
        app,
        pane_id,
        crate::core::AgentReport {
            status: String::from(status),
            blocker: String::from(blocker),
            next: String::from(next),
            updated_at: Instant::now(),
        },
    );
}

pub(crate) fn remember_command_for_test(app: &mut App, text: &str) {
    app.remember_command(text);
}

pub(crate) fn fixture_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

pub(crate) fn unique_test_path(label: &str, suffix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should work")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "muxboard-{label}-{}-{}-{unique}{suffix}",
        std::process::id(),
        TEST_PATH_COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

pub(crate) fn fake_tmux_script(name: &str, body: &str) -> String {
    let path = unique_test_path(&format!("app-{name}"), ".sh");
    let body = if body.starts_with("#!") {
        body.to_owned()
    } else {
        format!("#!/usr/bin/env sh\n{body}")
    };
    fs::write(&path, body).expect("fake tmux script should be writable");
    let mut permissions = fs::metadata(&path)
        .expect("fake tmux script metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("fake tmux script should be executable");
    path.display().to_string()
}

pub(crate) fn use_fake_tmux_for_test(app: &mut App, tmux_bin: String) {
    app.probe.target.binary = tmux_bin;
    app.runtime_context = crate::tmux::RuntimeContext {
        socket_name: Some(String::from("default")),
        pane_id: Some(String::from("%muxboard")),
    };
}

fn live_control_monitor_for_test() -> crate::tmux::control::Monitor {
    let (_tx, rx) = tokio::sync::mpsc::channel(1);
    crate::tmux::control::Monitor::for_test(
        rx,
        tokio::spawn(async {
            std::future::pending::<()>().await;
        }),
    )
}

#[derive(Debug, Deserialize)]
pub(crate) struct ViewModelFixture {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) panes: Vec<ViewModelPaneFixture>,
    #[serde(default)]
    pub(crate) runtimes: Vec<ViewModelRuntimeFixture>,
    #[serde(default)]
    pub(crate) search_query: String,
    #[serde(default)]
    pub(crate) marked_pane_ids: Vec<String>,
    #[serde(default)]
    pub(crate) metrics_mode: String,
    #[serde(default)]
    pub(crate) command_input: String,
    pub(crate) pending_dispatch: Option<ViewModelDispatchFixture>,
    pub(crate) board_title_limit: Option<usize>,
    pub(crate) board_title_width: Option<u16>,
    pub(crate) header_width: Option<u16>,
    pub(crate) footer_width: Option<u16>,
    pub(crate) board_row_limit: Option<usize>,
    pub(crate) expected_board_title: Option<String>,
    pub(crate) expected_header_context: Option<String>,
    pub(crate) expected_footer: Option<String>,
    pub(crate) expected_first_row: Option<ViewModelRowExpectation>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ViewModelPaneFixture {
    pub(crate) id: String,
    pub(crate) command: String,
    pub(crate) window_name: String,
    pub(crate) active: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ViewModelRuntimeFixture {
    pub(crate) pane_id: String,
    pub(crate) lines: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ViewModelDispatchFixture {
    pub(crate) text: String,
    pub(crate) expanded: Vec<(String, String)>,
    pub(crate) target_description: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ViewModelRowExpectation {
    pub(crate) status: String,
    pub(crate) location: String,
    pub(crate) title: String,
    pub(crate) selected: bool,
    pub(crate) attention: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PanelFixture {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) panes: Vec<ViewModelPaneFixture>,
    #[serde(default)]
    pub(crate) runtimes: Vec<ViewModelRuntimeFixture>,
    #[serde(default)]
    pub(crate) search_query: String,
    #[serde(default)]
    pub(crate) command_input: String,
    pub(crate) pending_dispatch: Option<ViewModelDispatchFixture>,
    #[serde(default)]
    pub(crate) action_menu_active: bool,
    pub(crate) context_pane: Option<String>,
    pub(crate) panel: String,
    pub(crate) expected_context_title: Option<String>,
    pub(crate) expected_overlay_title: Option<String>,
    #[serde(default)]
    pub(crate) expect_overlay_absent: bool,
    #[serde(default)]
    pub(crate) expected_contains: Vec<String>,
    #[serde(default)]
    pub(crate) expected_exact: Vec<String>,
    #[serde(default)]
    pub(crate) expected_exact_absent: Vec<String>,
    #[serde(default)]
    pub(crate) expected_absent: Vec<String>,
}

pub(crate) fn app_from_view_model_fixture(fixture: &ViewModelFixture) -> App {
    let panes = if fixture.panes.is_empty() {
        vec![sample_pane("codex")]
    } else {
        fixture
            .panes
            .iter()
            .enumerate()
            .map(|(index, pane)| {
                let mut tmux_pane = sample_pane(&pane.command);
                tmux_pane.id = pane.id.clone();
                tmux_pane.window_name = pane.window_name.clone();
                tmux_pane.active = pane.active;
                tmux_pane.pane_index = index as u32;
                tmux_pane
            })
            .collect::<Vec<_>>()
    };

    let runtimes = fixture
        .runtimes
        .iter()
        .map(|runtime| {
            (
                runtime.pane_id.as_str(),
                runtime.lines.iter().map(String::as_str).collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();

    let mut app = app_with_panes(panes, runtimes);
    app.search_query = fixture.search_query.clone();
    app.ensure_selection();
    app.marked_pane_ids = fixture.marked_pane_ids.iter().cloned().collect();

    if fixture.metrics_mode == "local" {
        app.metrics_mode = super::MetricsMode::Local;
    }

    if !fixture.command_input.is_empty() {
        app.begin_command_input();
        for ch in fixture.command_input.chars() {
            app.push_command_char(ch);
        }
    }

    if let Some(staged) = &fixture.pending_dispatch {
        app.pending_dispatch = Some(super::StagedDispatch {
            text: staged.text.clone(),
            expanded: staged.expanded.clone(),
            remember: true,
            target_description: staged.target_description.clone(),
        });
    }

    app
}

pub(crate) fn app_from_panel_fixture(fixture: &PanelFixture) -> App {
    let shim = ViewModelFixture {
        name: fixture.name.clone(),
        panes: fixture.panes.clone(),
        runtimes: fixture.runtimes.clone(),
        search_query: fixture.search_query.clone(),
        marked_pane_ids: Vec::new(),
        metrics_mode: String::new(),
        command_input: fixture.command_input.clone(),
        pending_dispatch: fixture.pending_dispatch.clone(),
        board_title_limit: None,
        board_title_width: None,
        header_width: None,
        footer_width: None,
        board_row_limit: None,
        expected_board_title: None,
        expected_header_context: None,
        expected_footer: None,
        expected_first_row: None,
    };

    let mut app = app_from_view_model_fixture(&shim);
    app.action_menu_active = fixture.action_menu_active;
    if let Some(context_pane) = &fixture.context_pane {
        app.context_pane = match context_pane.as_str() {
            "inspect" => super::ContextPane::Inspect,
            "tail" => super::ContextPane::Tail,
            "targets" => super::ContextPane::Targets,
            "navigator" => super::ContextPane::Navigator,
            "control" => super::ContextPane::Control,
            other => panic!("unknown panel fixture context pane: {other}"),
        };
    }
    app
}

pub(crate) fn load_view_model_fixtures() -> Vec<ViewModelFixture> {
    let fixtures = fs::read_to_string(fixture_path("tests/fixtures/app/view_models.json"))
        .expect("view model fixtures should read");
    serde_json::from_str(&fixtures).expect("view model fixtures should parse")
}

pub(crate) fn load_panel_fixtures() -> Vec<PanelFixture> {
    let fixtures = fs::read_to_string(fixture_path("tests/fixtures/app/panels.json"))
        .expect("panel fixtures should read");
    serde_json::from_str(&fixtures).expect("panel fixtures should parse")
}

fn assert_no_retired_user_terms(lines: &[String]) {
    let joined = lines.join("\n");
    for term in [
        "Ready to send",
        "staged send",
        "staged command",
        "Enter inspect",
        "Space target",
        "target panes",
        "target set",
        "target group",
        "Target group",
        "send target",
        "target hidden by current view",
        "targets hidden by current view",
        "No target panes remain",
        "Start target disappeared",
        "Selected pane",
        "inspects here",
        "Smart action",
        "smart action",
        "smart move",
        "G tmux",
        "G jump",
        ". actions",
        "Space send",
        "T send list",
        "Flags:",
        "quick jump",
        "target-set",
        "ack selected attention",
        "clear selected acknowledgement",
        "Send mode for",
        "Search panes. Type",
        "Editing pane search",
        "More open",
        "Help open",
        "actions-menu",
        "Local pane metrics",
        "local metrics",
        "M metrics",
        "metrics: pid",
        "Bell notifications",
        "SSH-safe: bell only",
        "V bell",
        "toggle bell",
        "desktop alerts (ssh-safe)",
        "desktop alerts (terminal)",
        "no desktop notifier found",
        "Desktop alerts on, SSH-safe",
        "poll one-line summaries",
        "Requested one-line summaries",
    ] {
        assert!(
            !joined.contains(term),
            "retired user-facing term `{term}` appeared in:\n{joined}"
        );
    }
}

#[test]
fn primary_view_model_copy_avoids_retired_user_terms() {
    let mut app = app_with_panes(vec![sample_pane("codex")], vec![]);
    assert_no_retired_user_terms(&app.help_lines());
    assert_no_retired_user_terms(&app.command_lines());
    assert_no_retired_user_terms(&app.control_lines());
    assert_no_retired_user_terms(&app.selected_pane_lines());
    assert_no_retired_user_terms(&[app.header_context_line(), app.status_hint_line()]);

    app.open_action_menu();
    assert_no_retired_user_terms(&app.command_lines());
    assert_no_retired_user_terms(&[
        app.status_message().to_owned(),
        app.footer_line_for_width(120),
    ]);

    app.close_action_menu();
    app.begin_command_input();
    assert_no_retired_user_terms(&app.command_lines());
    assert_no_retired_user_terms(&[app.header_context_line(), app.status_hint_line()]);
}

#[test]
fn help_lines_include_board_state_legend() {
    let app = app_with_panes(vec![sample_pane("codex")], vec![]);

    assert!(
        app.help_lines()
            .iter()
            .any(|line| line.contains("Legend: > selected, * active, + listed"))
    );
}

#[test]
fn usability_help_lines_are_task_oriented_instead_of_footer_repetition() {
    let app = app_with_panes(vec![sample_pane("codex")], vec![]);
    let help = app.help_lines().join("\n");

    for term in [
        "Now:", "Send:", "Find:", "Move:", "Views:", "More:", "Legend:", "Close:",
    ] {
        assert!(help.contains(term), "{help}");
    }
    for useful_recovery in [
        "backspace show all",
        "Fleet/Details",
        "[ browse",
        "] command center",
        "add/remove pane",
        "+ start agent",
    ] {
        assert!(help.contains(useful_recovery), "{help}");
    }
    for stale_footer_dump in [
        "moves, Enter shows output",
        "opens more actions",
        "opens Send",
    ] {
        assert!(!help.contains(stale_footer_dump), "{help}");
    }
}

#[test]
fn help_lines_empty_states_are_recovery_not_inert_pane_actions() {
    let empty = app_with_panes(Vec::new(), vec![]);
    let empty_help = empty.help_lines().join("\n");

    assert!(
        empty_help.contains("Now: start tmux panes, then R refresh."),
        "{empty_help}"
    );
    assert!(
        empty_help.contains("More: . layout and settings."),
        "{empty_help}"
    );
    for inert in [
        "Enter output",
        "G show in tmux",
        ": command pane",
        "Space add/remove pane",
        "+ start agent",
        "Z zoom pane",
    ] {
        assert!(
            !empty_help.contains(inert),
            "empty Help advertised inert `{inert}`:\n{empty_help}"
        );
    }

    let mut no_match = app_with_panes(vec![sample_pane("codex")], vec![]);
    no_match.search_query = String::from("zz-no-match");
    no_match.ensure_selection();
    let no_match_help = no_match.help_lines().join("\n");

    assert!(
        no_match_help.contains("Now: backspace show all panes."),
        "{no_match_help}"
    );
    assert!(
        no_match_help.contains("Find: / filter, R refresh."),
        "{no_match_help}"
    );
    for inert in [
        "Enter output",
        "G show in tmux",
        ": command pane",
        "Space add/remove pane",
        "+ start agent",
        "Z zoom pane",
    ] {
        assert!(
            !no_match_help.contains(inert),
            "no-match Help advertised inert `{inert}`:\n{no_match_help}"
        );
    }
}

#[test]
fn help_lines_match_secondary_surface_actions() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.show_browse_view();
    let browse_help = app.help_lines().join("\n");
    assert!(
        browse_help.contains("Now: Enter opens window, Esc back, G show in tmux."),
        "{browse_help}"
    );
    assert!(
        browse_help.contains("Move: J/K browse windows."),
        "{browse_help}"
    );
    assert!(!browse_help.contains("Fleet/Details"), "{browse_help}");

    app.show_command_center();
    let command_center_help = app.help_lines().join("\n");
    assert!(
        command_center_help.contains("Now: Enter output, Esc back, G show in tmux."),
        "{command_center_help}"
    );
    assert!(
        command_center_help.contains("Move: J/K choose action."),
        "{command_center_help}"
    );
    assert!(
        !command_center_help.contains("Fleet/Details"),
        "{command_center_help}"
    );

    app.context_pane = super::ContextPane::Tail;
    let output_help = app.help_lines().join("\n");
    assert!(
        output_help.contains("Now: Esc back to Fleet, G show in tmux."),
        "{output_help}"
    );
    assert!(!output_help.contains("Enter keeps output"), "{output_help}");

    let mut waiting_browse = app_with_panes(
        vec![sample_pane("codex")],
        vec![("%1", vec!["Press Enter to continue."])],
    );
    waiting_browse.show_browse_view();
    let waiting_browse_help = waiting_browse.help_lines().join("\n");
    assert!(
        waiting_browse_help
            .contains("Now: Enter opens window, Esc back, G show in tmux, A continue waiting."),
        "{waiting_browse_help}"
    );

    waiting_browse.context_pane = super::ContextPane::Tail;
    let waiting_output_help = waiting_browse.help_lines().join("\n");
    assert!(
        waiting_output_help.contains("Now: Esc back to Fleet, G show in tmux, A continue waiting."),
        "{waiting_output_help}"
    );
    assert!(
        !waiting_output_help.contains("Enter keeps output"),
        "{waiting_output_help}"
    );
}

#[test]
fn help_lines_only_advertise_continue_when_enter_is_safe() {
    let idle = app_with_panes(vec![sample_pane("codex")], vec![]);
    let idle_help = idle.help_lines().join("\n");

    assert!(
        idle_help.contains("Now: Enter output, G show in tmux."),
        "{idle_help}"
    );
    assert!(!idle_help.contains("A continue waiting"), "{idle_help}");

    let waiting = app_with_panes(
        vec![sample_pane("claude")],
        vec![("%1", vec!["Press Enter to continue."])],
    );
    let waiting_help = waiting.help_lines().join("\n");

    assert!(
        waiting_help.contains("Now: Enter output, G show in tmux, A continue waiting."),
        "{waiting_help}"
    );

    let mut selected = sample_pane("codex");
    selected.id = String::from("%1");
    let mut waiting_pane = sample_pane("claude");
    waiting_pane.id = String::from("%2");
    waiting_pane.active = false;
    waiting_pane.pane_index = 1;
    let fleet_waiting = app_with_panes(
        vec![selected, waiting_pane],
        vec![("%2", vec!["Press Enter to continue."])],
    );
    let fleet_waiting_help = fleet_waiting.help_lines().join("\n");

    assert!(
        fleet_waiting_help.contains("A continue waiting"),
        "{fleet_waiting_help}"
    );
}

#[test]
fn help_lines_surface_reply_for_selected_free_form_prompts() {
    let mut app = app_with_panes(
        vec![sample_pane("codex")],
        vec![("%1", vec!["Type your answer to continue."])],
    );
    let help = app.help_lines().join("\n");

    assert!(
        help.contains("Now: : reply, Enter output, G show in tmux."),
        "{help}"
    );
    assert!(
        help.contains("Send: Space add/remove pane for a send list."),
        "{help}"
    );
    assert!(!help.contains(": send reply"), "{help}");
    assert!(!help.contains("Send: : send text"), "{help}");

    app.show_command_center();
    let command_center_help = app.help_lines().join("\n");
    assert!(
        command_center_help.contains("Now: : reply, Esc back, G show in tmux."),
        "{command_center_help}"
    );
    assert!(
        !command_center_help.contains("Enter output"),
        "{command_center_help}"
    );

    app.context_pane = super::ContextPane::Tail;
    let output_help = app.help_lines().join("\n");
    assert!(
        output_help.contains("Now: : reply, Esc back, G show in tmux."),
        "{output_help}"
    );
}

#[test]
fn attention_sort_puts_waiting_before_running() {
    let mut first = sample_pane("node");
    first.id = String::from("%1");
    first.window_name = String::from("first");

    let mut second = sample_pane("node");
    second.id = String::from("%2");
    second.window_name = String::from("second");

    let app = app_with_panes(
        vec![first, second],
        vec![
            ("%1", vec!["building..."]),
            ("%2", vec!["Waiting for approval. Continue?"]),
        ],
    );

    let visible = app.visible_pane_indices();
    assert_eq!(app.snapshot.panes[visible[0]].id, "%2");
}

#[test]
fn heat_sort_prefers_recent_running_pane() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_name = String::from("warm");

    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.window_name = String::from("cold");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.sort_mode = SortMode::Heat;
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([String::from("building...")]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("building..."),
            partial_line: String::new(),
        },
    );
    app.pane_runtime.insert(
        String::from("%2"),
        PaneRuntime {
            output: VecDeque::from([String::from("idle")]),
            last_output_at: Some(Instant::now() - Duration::from_secs(600)),
            corpus: String::from("idle"),
            partial_line: String::new(),
        },
    );

    let visible = app.visible_pane_indices();
    assert_eq!(app.snapshot.panes[visible[0]].id, "%1");
}

#[test]
fn attention_filter_keeps_only_attention_panes() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");

    let mut second = sample_pane("codex");
    second.id = String::from("%2");

    let mut app = app_with_panes(
        vec![first, second],
        vec![("%1", vec!["done"]), ("%2", vec!["error: command failed"])],
    );
    app.filter_mode = FilterMode::Attention;

    let visible = app.visible_pane_indices();
    assert_eq!(visible.len(), 1);
    assert_eq!(app.snapshot.panes[visible[0]].id, "%2");
}

#[test]
fn error_has_higher_attention_than_waiting() {
    assert!(attention_rank(PaneStatus::Error) < attention_rank(PaneStatus::Waiting));
}

#[test]
fn attention_labels_are_plain_for_all_states() {
    let labels = [
        (PaneStatus::Error, "needs attention"),
        (PaneStatus::Waiting, "awaiting input"),
        (PaneStatus::Stuck, "possibly stalled"),
        (PaneStatus::Running, "on track"),
        (PaneStatus::Done, "complete"),
        (PaneStatus::Idle, "quiet"),
        (PaneStatus::Unknown, "checking"),
    ];

    for (status, expected) in labels {
        assert_eq!(attention_label(status), expected);
    }
}

#[test]
fn acknowledged_attention_drops_from_attention_filter() {
    let mut pane = sample_pane("bash");
    pane.window_name = String::from("codex");
    pane.id = String::from("%1");

    let mut app = app_with_panes(
        vec![pane.clone()],
        vec![("%1", vec!["error: command failed"])],
    );
    app.filter_mode = FilterMode::Attention;
    app.acknowledge_selected_attention();

    assert!(app.visible_pane_indices().is_empty());
}

#[test]
fn acknowledgement_clears_when_status_changes() {
    let mut pane = sample_pane("codex");
    pane.id = String::from("%1");

    let mut app = app_with_panes(
        vec![pane.clone()],
        vec![("%1", vec!["error: command failed"])],
    );
    app.acknowledge_selected_attention();

    app.reconcile_acknowledgements();
    assert_eq!(app.acknowledged_attention.len(), 1);
    assert_eq!(app.attention_queue_len(), 0);

    let runtime = app
        .pane_runtime
        .get_mut("%1")
        .expect("runtime should exist");
    runtime.output = VecDeque::from([String::from("done")]);
    runtime.last_output_at = Some(Instant::now());

    app.reconcile_acknowledgements();
    assert!(app.acknowledged_attention.is_empty());
}

#[test]
fn recent_alerts_keep_newest_items_without_unbounded_growth() {
    let mut app = app_with_panes(Vec::new(), vec![]);

    for index in 0..(super::MAX_RECENT_ALERTS + 2) {
        app.push_alert(format!("alert {index}"));
    }

    let newest = format!("alert {}", super::MAX_RECENT_ALERTS + 1);
    let oldest_kept = String::from("alert 2");
    assert_eq!(app.recent_alerts.len(), super::MAX_RECENT_ALERTS);
    assert_eq!(app.recent_alerts.front(), Some(&newest));
    assert_eq!(app.recent_alerts.back(), Some(&oldest_kept));
    assert!(!app.recent_alerts.iter().any(|line| line == "alert 0"));
}

#[test]
fn smart_action_prefers_enter_for_enter_prompt() {
    let pane = sample_pane("codex");
    let runtime = PaneRuntime {
        output: VecDeque::from([String::from("Press Enter to continue.")]),
        last_output_at: Some(Instant::now()),
        corpus: String::from("press enter to continue."),
        partial_line: String::new(),
    };

    let mut app = app_with_panes(vec![pane.clone()], vec![]);
    app.pane_runtime.insert(pane.id.clone(), runtime);

    let insight = app.pane_insight(&pane);
    assert_eq!(
        app.recommended_smart_action(&pane, insight),
        SmartAction::SendEnter
    );
}

#[test]
fn smart_action_prefers_focus_for_yes_no_prompt() {
    let pane = sample_pane("codex");
    let runtime = PaneRuntime {
        output: VecDeque::from([String::from("Approve execution? [y/n]")]),
        last_output_at: Some(Instant::now()),
        corpus: String::from("approve execution? [y/n]"),
        partial_line: String::new(),
    };

    let mut app = app_with_panes(vec![pane.clone()], vec![]);
    app.pane_runtime.insert(pane.id.clone(), runtime);

    let insight = app.pane_insight(&pane);
    assert_eq!(
        app.recommended_smart_action(&pane, insight),
        SmartAction::Focus
    );
}

#[tokio::test]
async fn lane_smart_action_reports_when_no_lane_panes_are_ready() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut app = app_with_panes(
        vec![first, second],
        vec![("%1", vec!["thinking"]), ("%2", vec!["working"])],
    );

    app.toggle_fanout_mode();
    app.perform_smart_action()
        .await
        .expect("empty lane smart action should not call tmux");

    assert_eq!(app.fanout_mode, FanoutMode::Lane);
    assert_eq!(app.status_message(), "No lane panes are ready for Enter.");
}

#[tokio::test]
async fn send_list_smart_action_reports_when_no_send_list_panes_are_ready() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut app = app_with_panes(
        vec![first, second],
        vec![("%1", vec!["thinking"]), ("%2", vec!["working"])],
    );
    app.marked_pane_ids
        .extend([String::from("%1"), String::from("%2")]);

    app.perform_smart_action()
        .await
        .expect("empty send-list smart action should not call tmux");

    assert_eq!(
        app.status_message(),
        "No send-list panes are ready for Enter."
    );
}

#[tokio::test]
async fn lane_smart_action_reports_skipped_and_disappeared_panes() {
    let fake_tmux = fake_tmux_script(
        "lane-smart-action-disappears",
        r#"#!/bin/sh
if [ "$1" = "send-keys" ]; then
  case "$*" in
    *"%2"*) echo "can't find pane: %2" >&2; exit 1 ;;
    *) exit 0 ;;
  esac
fi

if [ "$1" = "list-panes" ]; then
  cat <<'EOF'
$0	demo	@0	agents	%1	0	100	workspace	codex	/workspace	1	0
$0	demo	@0	agents	%3	2	300	workspace	codex	/workspace	0	0
EOF
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut third = sample_pane("codex");
    third.id = String::from("%3");
    third.active = false;
    third.pane_index = 2;
    let mut app = app_with_panes(
        vec![first, second, third],
        vec![
            ("%1", vec!["Press Enter to continue."]),
            ("%2", vec!["Press Enter to continue."]),
            ("%3", vec!["still working"]),
        ],
    );
    app.probe.target.binary = fake_tmux;

    app.toggle_fanout_mode();
    app.perform_smart_action()
        .await
        .expect("lane smart action should recover from a vanished pane");

    assert_eq!(
        app.status_message(),
        "Lane: sent Enter to 1 pane, skipped 1 pane, 1 pane disappeared."
    );
    assert_eq!(app.snapshot().pane_count(), 2);
    assert!(app.snapshot().panes.iter().all(|pane| pane.id != "%2"));
}

#[tokio::test]
async fn lane_smart_action_reports_when_all_ready_panes_disappear() {
    let fake_tmux = fake_tmux_script(
        "lane-smart-action-all-disappear",
        r#"#!/bin/sh
if [ "$1" = "send-keys" ]; then
  echo "can't find pane: $4" >&2
  exit 1
fi

if [ "$1" = "list-panes" ]; then
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut app = app_with_panes(
        vec![first, second],
        vec![
            ("%1", vec!["Press Enter to continue."]),
            ("%2", vec!["Press Enter to continue."]),
        ],
    );
    app.probe.target.binary = fake_tmux;

    app.toggle_fanout_mode();
    app.perform_smart_action()
        .await
        .expect("lane smart action should explain vanished panes");

    assert_eq!(
        app.status_message(),
        "Lane: no panes remain for Enter, 2 panes disappeared."
    );
    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(app.selected_pane_id.is_none());
}

#[tokio::test]
async fn selected_smart_action_reports_when_ready_pane_disappears() {
    let fake_tmux = fake_tmux_script(
        "selected-smart-action-disappears",
        r#"#!/bin/sh
if [ "$1" = "send-keys" ]; then
  echo "can't find pane: $4" >&2
  exit 1
fi

if [ "$1" = "list-panes" ]; then
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["Press Enter to continue."])]);
    app.probe.target.binary = fake_tmux;

    app.perform_smart_action()
        .await
        .expect("selected smart action should explain vanished pane");

    assert_eq!(
        app.status_message(),
        "No panes remain for Enter; 1 pane disappeared."
    );
    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(app.selected_pane_id.is_none());
}

#[tokio::test]
async fn send_list_smart_action_reports_when_all_ready_panes_disappear() {
    let fake_tmux = fake_tmux_script(
        "send-list-smart-action-all-disappear",
        r#"#!/bin/sh
if [ "$1" = "send-keys" ]; then
  echo "can't find pane: $4" >&2
  exit 1
fi

if [ "$1" = "list-panes" ]; then
  cat <<'EOF'
$0	demo	@0	agents	%3	2	300	workspace	codex	/workspace	0	0
EOF
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut third = sample_pane("codex");
    third.id = String::from("%3");
    third.active = false;
    third.pane_index = 2;
    let mut app = app_with_panes(
        vec![first, second, third],
        vec![
            ("%1", vec!["Press Enter to continue."]),
            ("%2", vec!["Press Enter to continue."]),
            ("%3", vec!["still working"]),
        ],
    );
    app.probe.target.binary = fake_tmux;
    app.marked_pane_ids
        .extend([String::from("%1"), String::from("%2"), String::from("%3")]);

    app.perform_smart_action()
        .await
        .expect("send-list smart action should explain vanished panes");

    assert_eq!(
        app.status_message(),
        "Send list: no panes remain for Enter, skipped 1 pane, 2 panes disappeared."
    );
    assert_eq!(app.snapshot().pane_count(), 1);
    assert_eq!(app.selected_pane_id.as_deref(), Some("%3"));
}

#[tokio::test]
async fn fleet_smart_action_reports_named_fleet_instead_of_generic_send_list() {
    let fake_tmux = fake_tmux_script(
        "fleet-smart-action-success",
        r#"#!/bin/sh
if [ "$1" = "send-keys" ]; then
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut app = app_with_panes(
        vec![first, second],
        vec![
            ("%1", vec!["Press Enter to continue."]),
            ("%2", vec!["still working"]),
        ],
    );
    app.probe.target.binary = fake_tmux;
    app.target_groups = vec![super::TargetGroup {
        name: String::from("triage"),
        members: vec![
            super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("agents"),
                pane_index: 0,
            },
            super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("agents"),
                pane_index: 1,
            },
        ],
    }];
    app.apply_target_group(0);

    app.perform_smart_action()
        .await
        .expect("fleet smart action should use the named fleet label");

    assert_eq!(
        app.status_message(),
        "Fleet `triage`: sent Enter to 1 pane, skipped 1 pane."
    );
}

#[tokio::test]
async fn fleet_smart_action_reports_named_fleet_when_all_ready_panes_disappear() {
    let fake_tmux = fake_tmux_script(
        "fleet-smart-action-all-disappear",
        r#"#!/bin/sh
if [ "$1" = "send-keys" ]; then
  echo "can't find pane: $4" >&2
  exit 1
fi

if [ "$1" = "list-panes" ]; then
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut app = app_with_panes(
        vec![first, second],
        vec![
            ("%1", vec!["Press Enter to continue."]),
            ("%2", vec!["Press Enter to continue."]),
        ],
    );
    app.probe.target.binary = fake_tmux;
    app.target_groups = vec![super::TargetGroup {
        name: String::from("triage"),
        members: vec![
            super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("agents"),
                pane_index: 0,
            },
            super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("agents"),
                pane_index: 1,
            },
        ],
    }];
    app.apply_target_group(0);

    app.perform_smart_action()
        .await
        .expect("fleet smart action should explain vanished panes with the fleet label");

    assert_eq!(
        app.status_message(),
        "Fleet `triage`: no panes remain for Enter, 2 panes disappeared."
    );
    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(app.selected_pane_id.is_none());
}

#[test]
fn attention_queue_omits_acknowledged_panes() {
    let mut pane = sample_pane("codex");
    pane.id = String::from("%1");

    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["error: command failed"])]);
    app.acknowledge_selected_attention();

    assert_eq!(
        app.attention_queue_lines(),
        vec![String::from("All clear.")]
    );
}

#[test]
fn agent_lanes_group_by_tool_and_attention() {
    let mut codex = sample_pane("codex");
    codex.id = String::from("%1");
    codex.window_name = String::from("codex");

    let mut claude = sample_pane("claude");
    claude.id = String::from("%2");
    claude.window_name = String::from("claude");
    claude.active = false;

    let app = app_with_panes(
        vec![codex, claude],
        vec![
            ("%1", vec!["Waiting for approval. Continue?"]),
            ("%2", vec!["thinking..."]),
        ],
    );

    let lanes = app.agent_lane_lines();
    assert!(lanes[0].contains("codex"));
    assert!(lanes[0].contains("1 waiting"));
    assert!(lanes[1].contains("claude"));
    assert!(lanes[1].contains("1 running"));
}

#[test]
fn control_panel_lines_keep_selected_lane_visible_when_more_than_five_lanes_exist() {
    let mut codex = sample_pane("codex");
    codex.id = String::from("%1");
    codex.window_name = String::from("codex");

    let mut claude = sample_pane("claude");
    claude.id = String::from("%2");
    claude.window_name = String::from("claude");
    claude.active = false;

    let mut opencode = sample_pane("opencode");
    opencode.id = String::from("%3");
    opencode.window_name = String::from("opencode");
    opencode.active = false;

    let mut aider = sample_pane("aider");
    aider.id = String::from("%4");
    aider.window_name = String::from("aider");
    aider.active = false;

    let mut gemini = sample_pane("gemini");
    gemini.id = String::from("%5");
    gemini.window_name = String::from("gemini");
    gemini.active = false;

    let mut agent = sample_pane("node");
    agent.id = String::from("%6");
    agent.window_name = String::from("agent");
    agent.active = false;

    let mut app = app_with_panes(
        vec![codex, claude, opencode, aider, gemini, agent],
        vec![
            ("%1", vec!["error: command failed"]),
            ("%2", vec!["Waiting for approval. Continue?"]),
            ("%3", vec!["Waiting for approval. Continue?"]),
            ("%4", vec!["thinking..."]),
            ("%5", vec!["thinking..."]),
            ("%6", vec!["agent", "done"]),
        ],
    );
    app.selected_pane_id = Some(String::from("%6"));
    app.context_pane = super::ContextPane::Control;

    let lines = app.context_panel_lines();
    let lane_lines = lines
        .iter()
        .skip_while(|line| line.as_str() != "Lanes")
        .skip(1)
        .take_while(|line| !line.is_empty())
        .cloned()
        .collect::<Vec<_>>();

    assert_eq!(lane_lines.len(), 5);
    assert!(lane_lines.iter().any(|line| line.starts_with("> agent:")));
    assert!(lane_lines.iter().any(|line| line.contains("codex")));
    assert!(lane_lines.iter().any(|line| line.contains("claude")));
    assert!(lane_lines.iter().any(|line| line.contains("opencode")));
    assert!(lane_lines.iter().any(|line| line.contains("aider")));
    assert!(!lane_lines.iter().any(|line| line.contains("gemini")));
}

#[test]
fn command_center_selection_markers_keep_a_visual_gutter() {
    let pane = sample_pane("codex");
    let app = app_with_panes(vec![pane], vec![("%1", vec!["Press Enter to continue."])]);

    let attention = app.attention_queue_lines().join("\n");
    let lanes = app.agent_lane_lines().join("\n");

    assert!(
        attention.contains("> continue demo / agents"),
        "{attention}"
    );
    assert!(lanes.contains("> codex:"), "{lanes}");
    assert!(!attention.contains(">continue"), "{attention}");
    assert!(!lanes.contains(">codex"), "{lanes}");
}

#[test]
fn live_tail_shows_recent_lines_in_order() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([
                String::from("download dependencies"),
                String::from("compile crate"),
                String::from("run unit tests"),
            ]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("download dependencies compile crate run unit tests"),
            partial_line: String::new(),
        },
    );

    let lines = app.live_tail_lines();
    assert!(lines.iter().any(|line| line.contains("demo / agents")));
    assert!(lines.iter().any(|line| line.contains("Running")));
    assert!(lines.iter().any(|line| line == "Summary"));
    assert!(lines.iter().any(|line| line == "Latest"));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("download dependencies"))
    );
    assert!(lines.iter().any(|line| line.contains("run unit tests")));
}

#[test]
fn live_tail_leads_with_distilled_summary_before_raw_tail() {
    let pane = sample_pane("node");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec!["codex", "STATUS=running | BLOCKER=none | NEXT=write tests"],
        )],
    );

    let lines = app.live_tail_lines();

    let summary_index = lines
        .iter()
        .position(|line| line == "Summary")
        .expect("summary heading should exist");

    assert_eq!(lines[summary_index + 1], "  write tests");
    assert!(!lines.iter().any(|line| line == "Latest"), "{lines:?}");
    assert!(!lines.iter().any(|line| line.contains("STATUS=running")));
    assert!(!lines.iter().any(|line| line == "  codex"));
}

#[test]
fn live_tail_falls_back_to_raw_output_when_cleaning_would_blank_it() {
    let pane = sample_pane("none");
    let app = app_with_panes(vec![pane], vec![("%1", vec!["none"])]);

    let lines = app.live_tail_lines();

    assert!(!lines.iter().any(|line| line == "Summary"), "{lines:?}");
    assert!(lines.iter().any(|line| line == "Latest"), "{lines:?}");
    assert!(lines.iter().any(|line| line == "  none"), "{lines:?}");
}

#[test]
fn acknowledge_all_attention_marks_every_attention_pane() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");

    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(
        vec![first.clone(), second.clone()],
        vec![
            ("%1", vec!["Waiting for approval. Continue?"]),
            ("%2", vec!["error: command failed"]),
        ],
    );

    app.acknowledge_all_attention();

    assert!(app.is_acknowledged(&first, PaneStatus::Waiting));
    assert!(app.is_acknowledged(&second, PaneStatus::Error));
}

#[test]
fn bulk_enter_targets_only_waiting_enter_prompts() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");

    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;

    let app = app_with_panes(
        vec![first, second],
        vec![
            ("%1", vec!["Press Enter to continue."]),
            ("%2", vec!["Approve execution? [y/n]"]),
        ],
    );

    assert_eq!(app.bulk_enter_targets(), vec![String::from("%1")]);
}

#[tokio::test]
async fn bulk_enter_reports_panes_that_disappear_during_send() {
    let fake_tmux = fake_tmux_script(
        "bulk-enter-disappears",
        r#"#!/bin/sh
if [ "$1" = "send-keys" ]; then
  case "$*" in
    *"%2"*) echo "can't find pane: %2" >&2; exit 1 ;;
    *) exit 0 ;;
  esac
fi

if [ "$1" = "list-panes" ]; then
  cat <<'EOF'
$0	demo	@0	agents	%1	0	100	workspace	codex	/workspace	1	0
EOF
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut app = app_with_panes(
        vec![first, second],
        vec![
            ("%1", vec!["Press Enter to continue."]),
            ("%2", vec!["Press Enter to continue."]),
        ],
    );
    app.probe.target.binary = fake_tmux;

    app.send_enter_to_attention_queue()
        .await
        .expect("bulk Enter should recover from a vanished pane");

    assert_eq!(
        app.status_message(),
        "Sent Enter to 1 waiting pane; 1 pane disappeared."
    );
    assert_eq!(app.snapshot().pane_count(), 1);
    assert_eq!(app.selected_pane_id.as_deref(), Some("%1"));
}

#[tokio::test]
async fn bulk_enter_reports_when_all_waiting_panes_disappear() {
    let fake_tmux = fake_tmux_script(
        "bulk-enter-all-disappear",
        r#"#!/bin/sh
if [ "$1" = "send-keys" ]; then
  echo "can't find pane: $4" >&2
  exit 1
fi

if [ "$1" = "list-panes" ]; then
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut app = app_with_panes(
        vec![first, second],
        vec![
            ("%1", vec!["Press Enter to continue."]),
            ("%2", vec!["Press Enter to continue."]),
        ],
    );
    app.probe.target.binary = fake_tmux;

    app.send_enter_to_attention_queue()
        .await
        .expect("bulk Enter should explain when every target vanished");

    assert_eq!(
        app.status_message(),
        "No waiting panes remain for Enter; 2 panes disappeared."
    );
    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(app.selected_pane_id.is_none());
}

#[tokio::test]
async fn direct_enter_reports_partial_disappearance_for_send_list_and_lane() {
    let fake_tmux = fake_tmux_script(
        "direct-enter-partial-disappear",
        r#"#!/bin/sh
if [ "$1" = "send-keys" ]; then
  case "$*" in
    *"%2"*) echo "can't find pane: %2" >&2; exit 1 ;;
    *) exit 0 ;;
  esac
fi

if [ "$1" = "list-panes" ]; then
  cat <<'EOF'
$0	demo	@0	agents	%1	0	100	workspace	codex	/workspace	1	0
$0	demo	@0	agents	%3	2	300	workspace	codex	/workspace	0	0
EOF
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut third = sample_pane("codex");
    third.id = String::from("%3");
    third.active = false;
    third.pane_index = 2;

    let mut send_list = app_with_panes(vec![first.clone(), second.clone(), third.clone()], vec![]);
    send_list.probe.target.binary = fake_tmux.clone();
    send_list
        .marked_pane_ids
        .extend([String::from("%1"), String::from("%2")]);
    send_list
        .send_enter_to_selected()
        .await
        .expect("send-list Enter should recover from partial disappearance");
    assert_eq!(
        send_list.status_message(),
        "Sent Enter to 1 pane in the send list; 1 pane disappeared."
    );
    assert_eq!(send_list.snapshot().pane_count(), 2);
    assert!(send_list.marked_pane_ids.contains("%1"));
    assert!(!send_list.marked_pane_ids.contains("%2"));

    let mut lane = app_with_panes(vec![first, second, third], vec![]);
    lane.probe.target.binary = fake_tmux;
    lane.toggle_fanout_mode();
    lane.send_enter_to_selected()
        .await
        .expect("lane Enter should recover from partial disappearance");
    assert_eq!(
        lane.status_message(),
        "Sent Enter to 2 panes in Codex; 1 pane disappeared."
    );
    assert_eq!(lane.snapshot().pane_count(), 2);
}

#[tokio::test]
async fn direct_enter_reports_lane_success_without_staging() {
    let log_path = unique_test_path("lane-enter-success", ".log");
    let fake_tmux = fake_tmux_script(
        "lane-enter-success",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
            log_path.display().to_string().replace('\'', "'\\''")
        ),
    );
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut third = sample_pane("codex");
    third.id = String::from("%3");
    third.active = false;
    third.pane_index = 2;
    let mut app = app_with_panes(vec![first, second, third], vec![]);
    app.probe.target.binary = fake_tmux;

    app.toggle_fanout_mode();
    app.send_enter_to_selected()
        .await
        .expect("lane Enter should send directly to every lane pane");

    assert_eq!(app.status_message(), "Sent Enter to 3 panes in Codex.");
    assert!(!app.has_pending_dispatch());
    let recorded = fs::read_to_string(&log_path).expect("lane Enter sends should be recorded");
    assert_eq!(recorded.matches("send-keys -t %").count(), 3, "{recorded}");
}

#[test]
fn search_filters_visible_panes() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_name = String::from("beta");
    second.active = false;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.begin_search();
    for ch in "beta".chars() {
        app.push_search_char(ch);
    }
    app.finish_search();

    let visible = app.visible_pane_indices();
    assert_eq!(visible.len(), 1);
    assert_eq!(app.snapshot.panes[visible[0]].id, "%2");
}

#[test]
fn cancel_search_restores_the_previous_filter() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_name = String::from("beta");
    second.active = false;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.search_query = String::from("alpha");
    app.begin_search();
    app.pop_search_char();
    app.pop_search_char();
    app.push_search_char('e');
    app.push_search_char('t');
    app.push_search_char('a');
    app.cancel_search();

    assert_eq!(app.search_query, "alpha");
    let visible = app.visible_pane_indices();
    assert_eq!(visible.len(), 1);
    assert_eq!(app.snapshot.panes[visible[0]].id, "%1");
}

#[test]
fn header_hint_line_uses_configured_focus_and_jump_bindings() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.ui_settings.keybindings.focus = vec![String::from("o")];
    app.ui_settings.keybindings.jump = vec![String::from("enter")];

    let hint = app.header_hint_line();
    assert_eq!(hint, "");
}

#[test]
fn header_hint_line_switches_to_navigator_specific_shortcuts() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.context_pane = super::ContextPane::Navigator;

    let hint = app.header_hint_line();
    assert_eq!(hint, "");
}

#[test]
fn header_hint_line_switches_to_search_mode_shortcuts() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.begin_search();

    let hint = app.header_hint_line();
    assert_eq!(
        hint,
        "type to filter  Enter apply  Esc cancel  backspace delete"
    );
}

#[test]
fn width_aware_header_lines_condense_for_tight_spaces() {
    let pane = sample_pane("codex");
    let app = app_with_panes(vec![pane], vec![]);

    let context = app.header_context_line_for_width(60);
    let hint = app.header_hint_line_for_width(70);

    assert!(context.contains("agents"));
    assert_eq!(hint, "");
}

#[test]
fn board_title_shows_visible_window_range_for_large_views() {
    let panes = (0..8)
        .map(|index| {
            let mut pane = sample_pane("codex");
            pane.id = format!("%{}", index + 1);
            pane.window_id = String::from("@1");
            pane.pane_index = index;
            pane.active = index == 0;
            pane
        })
        .collect::<Vec<_>>();
    let mut app = app_with_panes(panes, vec![]);
    app.selected_pane_id = Some(String::from("%7"));

    let title = app.board_title(4);

    assert!(title.contains("5-8 / 8"));
    assert!(title.contains("all quiet"));
}

#[test]
fn board_title_shows_attention_count_when_queue_is_not_empty() {
    let pane = sample_pane("codex");
    let app = app_with_panes(vec![pane], vec![("%1", vec!["Press Enter to continue."])]);

    let title = app.board_title(8);

    assert!(title.contains("1 needs you"));
}

#[test]
fn board_title_keeps_working_count_visible_when_attention_exists() {
    let mut waiting = sample_pane("codex");
    waiting.id = String::from("%1");
    let mut running = sample_pane("claude");
    running.id = String::from("%2");
    running.active = false;
    running.pane_index = 1;
    let app = app_with_panes(
        vec![waiting, running],
        vec![
            ("%1", vec!["Press Enter to continue."]),
            (
                "%2",
                vec!["STATUS=running | BLOCKER=none | NEXT=write tests"],
            ),
        ],
    );

    let title = app.board_title(8);
    let narrow = app.board_title_for_width(8, 72);

    assert!(title.contains("1 needs you, 1 working"), "{title}");
    assert!(narrow.contains("1 needs you, 1 working"), "{narrow}");
}

#[test]
fn width_aware_board_title_stays_compact_on_tight_widths() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.search_query = String::from("approval");
    app.marked_pane_ids.insert(String::from("%1"));
    app.metrics_mode = super::MetricsMode::Local;

    let title = app.board_title_for_width(8, 68);

    assert_eq!(title, "Fleet | no matches");
}

#[test]
fn width_aware_board_title_expands_when_space_allows() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.search_query = String::from("approval");
    app.marked_pane_ids.insert(String::from("%1"));
    app.metrics_mode = super::MetricsMode::Local;

    let title = app.board_title_for_width(8, 110);

    assert!(title.contains("Fleet"));
    assert!(title.contains("no matches"));
    assert!(title.contains("send list 1 pane"));
    assert!(title.contains("CPU/mem"));
}

#[test]
fn board_title_surfaces_active_filter_and_sort_state() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.cycle_filter_mode();
    app.cycle_sort_mode();

    assert!(app.board_title(8).contains("agents"));
    assert!(app.board_title(8).contains("activity sort"));

    let narrow = app.board_title_for_width(8, 68);
    assert!(!narrow.contains("agents"), "{narrow}");
    assert!(!narrow.contains("activity sort"), "{narrow}");

    let wide = app.board_title_for_width(8, 120);
    assert!(wide.contains("agents"), "{wide}");
    assert!(wide.contains("activity sort"), "{wide}");

    app.cycle_filter_mode();
    app.cycle_sort_mode();
    let wide = app.board_title_for_width(8, 120);
    assert!(wide.contains("needs you"), "{wide}");
    assert!(wide.contains("tmux order"), "{wide}");
}

#[test]
fn view_model_fixtures_hold() {
    let fixtures = fs::read_to_string(fixture_path("tests/fixtures/app/view_models.json"))
        .expect("view model fixtures should read");
    let fixtures: Vec<ViewModelFixture> =
        serde_json::from_str(&fixtures).expect("view model fixtures should parse");

    for fixture in fixtures {
        let app = app_from_view_model_fixture(&fixture);

        if let (Some(limit), Some(width), Some(expected)) = (
            fixture.board_title_limit,
            fixture.board_title_width,
            fixture.expected_board_title.as_ref(),
        ) {
            assert_eq!(
                app.board_title_for_width(limit, width),
                *expected,
                "fixture: {} board title",
                fixture.name
            );
        }

        if let (Some(width), Some(expected)) = (
            fixture.header_width,
            fixture.expected_header_context.as_ref(),
        ) {
            assert_eq!(
                app.header_context_line_for_width(width),
                *expected,
                "fixture: {} header context",
                fixture.name
            );
        }

        if let (Some(width), Some(expected)) =
            (fixture.footer_width, fixture.expected_footer.as_ref())
        {
            assert_eq!(
                app.footer_line_for_width(width),
                *expected,
                "fixture: {} footer",
                fixture.name
            );
        }

        if let Some(expected) = fixture.expected_first_row.as_ref() {
            let rows = app.board_rows(fixture.board_row_limit.unwrap_or(8));
            let first = rows
                .first()
                .expect("fixture should produce at least one row");
            assert_eq!(
                first.status, expected.status,
                "fixture: {} row status",
                fixture.name
            );
            assert_eq!(
                first.location, expected.location,
                "fixture: {} row location",
                fixture.name
            );
            assert_eq!(
                first.title, expected.title,
                "fixture: {} row title",
                fixture.name
            );
            assert_eq!(
                first.selected, expected.selected,
                "fixture: {} row selected",
                fixture.name
            );
            assert_eq!(
                first.attention, expected.attention,
                "fixture: {} row attention",
                fixture.name
            );
        }
    }
}

#[test]
fn panel_fixtures_hold() {
    let fixtures = fs::read_to_string(fixture_path("tests/fixtures/app/panels.json"))
        .expect("panel fixtures should read");
    let fixtures: Vec<PanelFixture> =
        serde_json::from_str(&fixtures).expect("panel fixtures should parse");

    for fixture in fixtures {
        let app = app_from_panel_fixture(&fixture);
        if let Some(expected_title) = fixture.expected_context_title.as_ref() {
            assert_eq!(
                app.context_panel_title(),
                *expected_title,
                "fixture: {} context title",
                fixture.name
            );
        }

        let overlay = app.overlay_panel();
        if fixture.expect_overlay_absent {
            assert!(
                overlay.is_none(),
                "fixture: {} expected no overlay panel",
                fixture.name
            );
        }
        if let Some(expected_title) = fixture.expected_overlay_title.as_ref() {
            let (title, _) = overlay.expect("overlay panel should exist");
            assert_eq!(
                title, *expected_title,
                "fixture: {} overlay title",
                fixture.name
            );
        }

        let lines = match fixture.panel.as_str() {
            "selected" => app.selected_pane_lines(),
            "live_tail" => app.live_tail_lines(),
            "navigator" => app.navigator_lines(),
            "command" => app.command_lines(),
            "context" => app.context_panel_lines(),
            "overlay" => app
                .overlay_panel()
                .map(|(_, lines)| lines)
                .expect("overlay panel should exist"),
            other => panic!("unknown panel fixture: {other}"),
        };

        if !fixture.expected_exact.is_empty() {
            assert_eq!(
                lines, fixture.expected_exact,
                "fixture: {} exact",
                fixture.name
            );
        }

        for needle in &fixture.expected_contains {
            assert!(
                lines.iter().any(|line| line.contains(needle)),
                "fixture: {} missing `{needle}` in {:?}",
                fixture.name,
                lines
            );
        }

        for needle in &fixture.expected_exact_absent {
            assert!(
                !lines.iter().any(|line| line == needle),
                "fixture: {} unexpectedly contained exact `{needle}` in {:?}",
                fixture.name,
                lines
            );
        }

        for needle in &fixture.expected_absent {
            assert!(
                !lines.iter().any(|line| line.contains(needle)),
                "fixture: {} unexpectedly contained `{needle}` in {:?}",
                fixture.name,
                lines
            );
        }
    }
}

#[test]
fn control_title_shows_attention_count() {
    let pane = sample_pane("codex");
    let app = app_with_panes(vec![pane], vec![("%1", vec!["Press Enter to continue."])]);

    assert_eq!(app.control_title(), "Command Center");
}

#[test]
fn control_lines_make_all_clear_running_agents_observable_before_sending() {
    let pane = sample_pane("codex");
    let app = app_with_panes(vec![pane], vec![("%1", vec!["working"])]);

    let lines = app.control_lines();

    assert_eq!(lines[0], "All clear: 1 agent working");
    assert_eq!(lines[1], "Action: Enter output demo / agents");
    assert_eq!(lines[2], "Target: demo / agents");
    assert!(
        !lines.iter().any(|line| line.contains(": send")),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line == "Working: 1 agent"),
        "{lines:?}"
    );
}

#[test]
fn control_lines_lead_with_attention_and_target_summary() {
    let pane = sample_pane("codex");
    let app = app_with_panes(
        vec![pane],
        vec![("%1", vec!["Waiting for approval. Continue?"])],
    );

    let lines = app.control_lines();

    assert_eq!(lines[0], "Action: : reply to demo / agents");
    assert_eq!(lines[1], "Needs you: 1 waiting");
    assert_eq!(lines[2], "Target: demo / agents");
    assert_eq!(lines[3], "Start: + agent in workspace");
    assert_eq!(
        app.attention_queue_lines()[0],
        "> reply to demo / agents: approval needed"
    );
    assert!(!lines.iter().any(|line| line == "Working: none"));
}

#[test]
fn control_lines_offer_reply_for_the_next_prompt_without_attaching() {
    let mut waiting = sample_pane("codex");
    waiting.id = String::from("%1");
    waiting.window_name = String::from("blocked");
    let mut selected = sample_pane("node");
    selected.id = String::from("%2");
    selected.window_name = String::from("active");
    selected.pane_index = 1;

    let mut app = app_with_panes(
        vec![waiting, selected],
        vec![("%1", vec!["Type your answer to continue."])],
    );
    app.select_next_pane();

    let lines = app.control_lines();

    assert_eq!(lines[0], "Action: : reply to demo / blocked");
    assert_eq!(
        app.attention_queue_lines()[0],
        "  reply to demo / blocked: approval needed"
    );
}

#[test]
fn control_lines_make_selected_choice_prompts_answerable_from_command_center() {
    let app = app_with_panes(
        vec![sample_pane("claude")],
        vec![("%1", vec!["Allow command? [y/n]"])],
    );

    let lines = app.control_lines();

    assert_eq!(lines[0], "Action: . answer demo / agents");
    assert_eq!(
        app.attention_queue_lines()[0],
        "> answer demo / agents: yes/no choice"
    );
}

#[test]
fn command_center_queue_counts_multiple_attention_items() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(
        vec![first, second],
        vec![
            ("%1", vec!["Waiting for approval. Continue?"]),
            ("%2", vec!["Allow command? [y/n]"]),
        ],
    );
    app.show_command_center();

    let lines = app.context_panel_lines();

    assert!(lines.iter().any(|line| line == "Queue (2)"), "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|line| line == "> reply to demo / alpha: approval needed")
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "  answer demo / beta: yes/no choice")
    );
    assert!(!lines.iter().any(|line| line == "Queue"), "{lines:?}");
}

#[test]
fn attention_queue_rows_explain_why_attention_is_needed() {
    let mut waiting = sample_pane("claude");
    waiting.id = String::from("%1");
    waiting.window_name = String::from("approval");

    let mut failed = sample_pane("codex");
    failed.id = String::from("%2");
    failed.window_name = String::from("failed");
    failed.active = false;
    failed.pane_index = 1;

    let app = app_with_panes(
        vec![waiting, failed],
        vec![
            ("%1", vec!["Waiting for leader to approve network access."]),
            ("%2", vec!["error: build failed"]),
        ],
    );

    let queue = app.attention_queue_lines();

    assert_eq!(queue[0], "  output demo / failed: build failed");
    assert_eq!(queue[1], "> reply to demo / approval: network access");
    assert!(
        !queue.iter().any(|line| line.contains("STATUS=")),
        "{queue:?}"
    );
    assert!(
        !queue.iter().any(|line| line.contains("NEXT=")),
        "{queue:?}"
    );
}

#[tokio::test]
async fn command_center_continue_promotes_next_item_and_keeps_first_watching() {
    let fake_tmux = fake_tmux_script("command-center-watch-next", "#!/bin/sh\nexit 0\n");
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_name = String::from("alpha");
    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;
    let mut app = app_with_panes(
        vec![first, second],
        vec![
            ("%1", vec!["Press Enter to continue."]),
            ("%2", vec!["Press Enter to continue."]),
        ],
    );
    use_fake_tmux_for_test(&mut app, fake_tmux);
    app.show_command_center();

    assert!(
        app.perform_command_center_primary_action(super::CommandCenterPrimaryTrigger::Smart)
            .await
            .expect("Command Center continue should send Enter")
    );

    assert_eq!(
        app.status_message(),
        "Sent Enter to demo / alpha. Next: demo / beta."
    );
    assert_eq!(app.selected_pane_id.as_deref(), Some("%2"));
    assert_eq!(app.attention_queue_len(), 1);
    assert_eq!(
        app.attention_queue_lines(),
        vec![String::from("> continue demo / beta: needs Enter")]
    );

    let panel = app.context_panel_lines();
    assert!(
        panel
            .iter()
            .any(|line| line == "Action: A continue demo / beta"),
        "{panel:?}"
    );
    assert!(panel.iter().any(|line| line == "Watching"), "{panel:?}");
    assert!(
        panel
            .iter()
            .any(|line| line == "  demo / alpha: sent Enter"),
        "{panel:?}"
    );
    let rows = app.board_rows(2);
    assert_eq!(rows[0].lifecycle, "watching");
    assert_eq!(rows[0].attention, "~");
    assert_eq!(rows[1].lifecycle, "needs you");
    assert_eq!(rows[1].attention, "!");
}

#[tokio::test]
async fn command_center_watching_item_returns_to_queue_when_output_changes() {
    let fake_tmux = fake_tmux_script("command-center-watch-clears", "#!/bin/sh\nexit 0\n");
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["Press Enter to continue."])]);
    use_fake_tmux_for_test(&mut app, fake_tmux);
    app.show_command_center();

    app.perform_command_center_primary_action(super::CommandCenterPrimaryTrigger::Smart)
        .await
        .expect("Command Center continue should send Enter");

    assert_eq!(app.attention_queue_len(), 0);
    assert!(
        app.context_panel_lines()
            .iter()
            .any(|line| line == "> demo / agents: sent Enter")
    );
    assert!(
        app.context_panel_lines()
            .iter()
            .any(|line| line == "Watching: demo / agents")
    );
    assert!(
        app.context_panel_lines()
            .iter()
            .any(|line| line == "Action: G show in tmux")
    );
    assert!(
        !app.context_panel_lines()
            .iter()
            .any(|line| line == "Action: : send this pane")
    );

    app.append_output("%1", String::from("Press Enter to continue again.\n"), None);

    assert_eq!(app.attention_queue_len(), 1);
    assert_eq!(
        app.attention_queue_lines(),
        vec![String::from("> continue demo / agents: needs Enter")]
    );
    assert!(
        !app.context_panel_lines()
            .iter()
            .any(|line| line.contains("sent Enter"))
    );
}

#[test]
fn attention_queue_shows_when_attention_items_are_hidden() {
    let mut panes = Vec::new();
    let mut pane_ids = Vec::new();

    for index in 0..8 {
        let mut pane = sample_pane("codex");
        pane.id = format!("%{}", index + 1);
        pane.window_id = format!("@{index}");
        pane.window_name = format!("agent-{index}");
        pane.active = index == 0;
        pane.pane_index = index;
        pane_ids.push(pane.id.clone());
        panes.push(pane);
    }

    let runtimes = pane_ids
        .iter()
        .map(|id| (id.as_str(), vec!["Press Enter to continue."]))
        .collect::<Vec<_>>();
    let app = app_with_panes(panes, runtimes);

    let queue = app.attention_queue_lines();

    assert_eq!(queue.len(), 7, "{queue:?}");
    assert_eq!(queue[0], "> continue demo / agent-0: needs Enter");
    assert_eq!(queue[5], "  continue demo / agent-5: needs Enter");
    assert_eq!(queue[6], "+ 2 more need you: continue");
    assert!(
        !queue.iter().any(|line| line.contains("agent-6")),
        "{queue:?}"
    );
}

#[test]
fn attention_queue_overflow_summarizes_hidden_action_types() {
    let mut panes = Vec::new();
    let mut runtimes = Vec::new();

    for index in 0..8 {
        let mut pane = sample_pane("codex");
        pane.id = format!("%{}", index + 1);
        pane.window_id = format!("@{index}");
        pane.window_name = format!("agent-{index}");
        pane.active = index == 0;
        pane.pane_index = index;
        let output = match index {
            6 => "Allow command? [y/n]",
            7 => "Waiting for leader to approve network access.",
            _ => "Press Enter to continue.",
        };
        runtimes.push((pane.id.clone(), vec![output.to_owned()]));
        panes.push(pane);
    }

    let runtime_refs = runtimes
        .iter()
        .map(|(id, lines)| {
            (
                id.as_str(),
                lines.iter().map(String::as_str).collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    let runtime_refs = runtime_refs
        .iter()
        .map(|(id, lines)| (*id, lines.clone()))
        .collect::<Vec<_>>();
    let app = app_with_panes(panes, runtime_refs);

    let queue = app.attention_queue_lines();

    assert_eq!(queue.len(), 7, "{queue:?}");
    assert_eq!(queue[0], "> continue demo / agent-0: needs Enter");
    assert_eq!(queue[6], "+ 2 more need you: answer, reply");
    assert!(
        !queue.iter().any(|line| line.contains("agent-6")),
        "{queue:?}"
    );
    assert!(
        !queue.iter().any(|line| line.contains("agent-7")),
        "{queue:?}"
    );
}

#[test]
fn control_lines_working_count_excludes_waiting_agents() {
    let mut running_agent = sample_pane("claude");
    running_agent.id = String::from("%1");
    running_agent.window_name = String::from("alpha");

    let mut waiting_agent = sample_pane("codex");
    waiting_agent.id = String::from("%2");
    waiting_agent.window_name = String::from("blocked");
    waiting_agent.active = false;
    waiting_agent.pane_index = 1;

    let mut waiting_shell = sample_pane("zsh");
    waiting_shell.id = String::from("%3");
    waiting_shell.window_name = String::from("approval");
    waiting_shell.active = false;
    waiting_shell.pane_index = 2;

    let app = app_with_panes(
        vec![running_agent, waiting_agent, waiting_shell],
        vec![
            (
                "%1",
                vec!["STATUS=running | BLOCKER=none | NEXT=continue build"],
            ),
            (
                "%2",
                vec!["STATUS=waiting | BLOCKER=missing fixture | NEXT=need input"],
            ),
            ("%3", vec!["Proceed? [y/N]"]),
        ],
    );

    let lines = app.control_lines();

    assert_eq!(lines[1], "Needs you: 2 waiting");
    assert_eq!(lines[2], "Working: 1 agent");
}

#[test]
fn control_lines_omit_zero_attention_noise() {
    let mut error = sample_pane("bash");
    error.id = String::from("%1");
    error.window_name = String::from("deploy");

    let mut waiting = sample_pane("codex");
    waiting.id = String::from("%2");
    waiting.window_name = String::from("review");
    waiting.active = false;
    waiting.pane_index = 1;

    let app = app_with_panes(
        vec![error, waiting],
        vec![
            ("%1", vec!["error: command failed"]),
            ("%2", vec!["Waiting for approval. Continue?"]),
        ],
    );

    let lines = app.control_lines();

    assert_eq!(lines[1], "Needs you: 1 error, 1 waiting");
    assert!(!lines[1].contains('0'), "{lines:?}");
}

#[test]
fn control_lines_count_stale_agents_as_stuck_needs_you() {
    let pane = sample_pane("claude");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([String::from("thinking")]),
            last_output_at: Some(Instant::now() - Duration::from_secs(240)),
            corpus: String::from("thinking"),
            partial_line: String::new(),
        },
    );

    let lines = app.control_lines();

    assert_eq!(lines[0], "Action: Enter output demo / agents");
    assert_eq!(lines[1], "Needs you: 1 stuck");
    assert_eq!(lines[2], "Target: demo / agents");
    assert!(!lines.iter().any(|line| line == "Working: none"));
}

#[test]
fn control_lines_count_all_supported_agent_families() {
    let mut codex = sample_pane("codex");
    codex.id = String::from("%1");

    let mut claude = sample_pane("claude");
    claude.id = String::from("%2");
    claude.active = false;
    claude.pane_index = 1;

    let mut opencode = sample_pane("opencode");
    opencode.id = String::from("%3");
    opencode.active = false;
    opencode.pane_index = 2;

    let mut aider = sample_pane("aider");
    aider.id = String::from("%4");
    aider.active = false;
    aider.pane_index = 3;

    let mut gemini = sample_pane("gemini");
    gemini.id = String::from("%5");
    gemini.active = false;
    gemini.pane_index = 4;

    let mut generic = sample_pane("python");
    generic.id = String::from("%6");
    generic.active = false;
    generic.pane_index = 5;

    let app = app_with_panes(
        vec![codex, claude, opencode, aider, gemini, generic],
        vec![("%6", vec!["agent status: planning next step"])],
    );

    let lines = app.control_lines();

    assert_eq!(lines[0], "Action: : send this pane");
    assert_eq!(lines[1], "Working: 1 agent");
    assert_eq!(lines[2], "Target: demo / agents #0");
    assert_eq!(lines[3], "Start: + agent in workspace");
    assert!(!lines.iter().any(|line| line == "Needs you: none"));
}

#[test]
fn selected_pane_title_reflects_status_and_workload() {
    let pane = sample_pane("codex");
    let app = app_with_panes(vec![pane], vec![("%1", vec!["building..."])]);

    assert_eq!(app.selected_pane_title(), "Details");
}

#[test]
fn navigator_title_shows_window_count_and_scope() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.context_pane = super::ContextPane::Navigator;

    assert_eq!(app.navigator_title(), "Browse");
}

#[test]
fn navigator_title_mentions_scope_when_board_is_scoped() {
    let mut first = sample_pane("codex");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.view_scope = super::ViewScope::Window {
        id: String::from("@2"),
        name: String::from("demo/beta"),
    };

    assert_eq!(app.navigator_title(), "Browse");
}

#[test]
fn recent_events_and_live_tail_titles_reflect_current_state() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["building..."])]);
    app.recent_events.push_front(String::from("window renamed"));

    assert_eq!(app.recent_events_title(), "Recent events | 1");
    assert_eq!(app.live_tail_title(), "Output");
}

#[test]
fn context_panel_title_includes_active_target_description() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.context_pane = super::ContextPane::Targets;

    assert!(app.context_panel_title().contains("Send"));
}

#[test]
fn header_context_line_reflects_search_and_command_modes() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.begin_search();
    app.push_search_char('o');
    app.push_search_char('p');
    assert_eq!(app.header_context_line(), "Searching for `op`.");

    app.cancel_search();
    app.begin_command_input();
    assert!(app.header_context_line().contains("Send to demo / agents."));
}

#[test]
fn action_menu_lines_use_configured_action_bindings() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["Press Enter to continue."])]);
    app.ui_settings.keybindings.action_ack_selected = vec![String::from("c")];
    app.ui_settings.keybindings.action_ack_clear_selected = vec![String::from("w")];
    app.ui_settings.keybindings.action_send_yes = vec![String::from("1")];
    app.ui_settings.keybindings.action_send_no = vec![String::from("2")];
    app.ui_settings.keybindings.action_zoom = vec![String::from("0")];
    app.ui_settings.keybindings.action_enter_queue = vec![String::from(";")];
    app.action_menu_active = true;

    let lines = app.command_lines();

    assert!(lines.iter().any(|line| line == "  C mute alert"));
    assert!(!lines.iter().any(|line| line == "  W unmute alert"));
    assert!(!lines.iter().any(|line| line == "  1 answer yes"));
    assert!(!lines.iter().any(|line| line == "  2 answer no"));
    assert!(lines.iter().any(|line| line == "  0 zoom pane"));
    assert!(lines.iter().any(|line| line.contains("; continue waiting")));

    app.acknowledge_selected_attention();
    app.action_menu_active = true;
    let lines = app.command_lines();
    assert!(lines.iter().any(|line| line == "  W unmute alert"));
    assert!(!lines.iter().any(|line| line == "  C mute alert"));
}

#[test]
fn action_menu_labels_the_send_destination_like_a_form() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.open_action_menu();

    let lines = app.command_lines();

    assert!(
        lines.iter().any(|line| line == "To: demo / agents"),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line == "send to demo / agents"),
        "{lines:?}"
    );
}

#[test]
fn action_menu_surfaces_reply_for_selected_free_form_prompts() {
    let mut app = app_with_panes(
        vec![sample_pane("codex")],
        vec![("%1", vec!["Type your answer to continue."])],
    );
    app.open_action_menu();

    let lines = app.command_lines();

    assert_eq!(lines[0], "Action: : reply");
    assert!(lines.iter().any(|line| line == "  : reply"), "{lines:?}");
    assert!(
        !lines.iter().any(|line| line == "  : send text"),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line == "Action: C mute alert"),
        "{lines:?}"
    );
}

#[test]
fn action_menu_row_predicates_match_command_model() {
    let has_row =
        |app: &App, needle: &str| app.command_lines().iter().any(|line| line.contains(needle));

    let mut empty = app_with_panes(Vec::new(), vec![]);
    empty.action_menu_active = true;
    assert!(!empty.action_menu_has_visible_selection());
    assert!(!has_row(&empty, "+ start agent"));
    assert!(!empty.action_menu_has_sortable_panes());
    assert!(!has_row(&empty, "T sort by"));

    let mut visible = app_with_panes(vec![sample_pane("codex")], vec![]);
    visible.action_menu_active = true;
    assert!(visible.action_menu_has_visible_selection());
    assert!(has_row(&visible, "+ start agent"));
    assert!(visible.action_menu_has_sortable_panes());
    assert!(has_row(&visible, "T sort by"));
    assert!(visible.action_menu_can_target_lane());
    assert!(has_row(&visible, "B send lane"));

    let mut marked = app_with_panes(vec![sample_pane("codex")], vec![]);
    marked.toggle_selected_mark();
    marked.action_menu_active = true;
    assert!(marked.action_menu_can_clear_marks());
    assert!(has_row(&marked, "X clear send list"));
    assert!(marked.action_menu_can_save_group());
    assert!(has_row(&marked, "G save fleet"));

    let mut choice = app_with_panes(
        vec![sample_pane("claude")],
        vec![("%1", vec!["Approve network access? [y/n]"])],
    );
    choice.action_menu_active = true;
    assert!(choice.action_menu_can_answer_choice());
    assert!(has_row(&choice, "Y answer yes"));
    assert!(has_row(&choice, "N answer no"));

    let mut acknowledged = app_with_panes(
        vec![sample_pane("codex")],
        vec![("%1", vec!["Waiting for approval."])],
    );
    acknowledged.acknowledge_selected_attention();
    acknowledged.action_menu_active = true;
    assert!(acknowledged.action_menu_can_clear_selected_ack());
    assert!(has_row(&acknowledged, "W unmute alert"));
    assert!(acknowledged.action_menu_can_clear_all_acks());
    assert!(has_row(&acknowledged, "U unmute all"));

    acknowledged.set_search_query_for_test("zz-no-match");
    acknowledged.action_menu_active = true;
    assert!(!acknowledged.action_menu_has_visible_selection());
    assert!(!acknowledged.action_menu_can_clear_selected_ack());
    assert!(!acknowledged.action_menu_can_clear_all_acks());
    assert!(!has_row(&acknowledged, "W unmute alert"));
    assert!(!has_row(&acknowledged, "U unmute all"));
}

#[test]
fn action_menu_labels_saved_fleets_by_the_next_safe_action() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.target_groups.push(super::TargetGroup {
        name: String::from("triage"),
        members: vec![super::PaneLocator {
            session_name: String::from("demo"),
            window_name: String::from("agents"),
            pane_index: 0,
        }],
    });
    app.action_menu_active = true;

    let lines = app.command_lines();

    assert!(lines.iter().any(|line| line == "saved 1 fleet"));
    assert!(lines.iter().any(|line| line == "  L choose fleet"));
    assert!(!lines.iter().any(|line| line.contains("delete fleet")));
    assert!(!lines.iter().any(|line| line.contains("delete triage")));

    app.apply_target_group(0);
    app.action_menu_active = true;
    let lines = app.command_lines();

    assert!(lines.iter().any(|line| line == "fleet triage"));
    assert!(lines.iter().any(|line| line == "  D delete triage"));
}

#[test]
fn action_menu_only_shows_yes_no_for_choice_prompts() {
    let mut plain = app_with_panes(vec![sample_pane("codex")], vec![]);
    plain.action_menu_active = true;
    let plain_lines = plain.command_lines();
    assert!(!plain_lines.iter().any(|line| line.contains("answer yes")));
    assert!(!plain_lines.iter().any(|line| line.contains("answer no")));
    assert!(plain_lines.iter().any(|line| line == "  Z zoom pane"));

    let mut choice = app_with_panes(
        vec![sample_pane("claude")],
        vec![("%1", vec!["Approve network access? [y/n]"])],
    );
    choice.action_menu_active = true;
    let choice_lines = choice.command_lines();
    assert!(choice_lines.iter().any(|line| line == "  Y answer yes"));
    assert!(choice_lines.iter().any(|line| line == "  N answer no"));
    assert!(
        choice_lines
            .iter()
            .position(|line| line == "  Y answer yes")
            < choice_lines.iter().position(|line| line == "  Z zoom pane")
    );
}

#[test]
fn action_menu_sort_and_filter_labels_show_the_result_not_jargon() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.action_menu_active = true;

    let lines = app.command_lines();
    assert!(lines.iter().any(|line| line == "  T sort by activity"));
    assert!(lines.iter().any(|line| line == "  F show agents"));
    assert!(!lines.iter().any(|line| line == "  T sort"));
    assert!(!lines.iter().any(|line| line == "  F filter"));

    app.action_menu_active = false;
    app.cycle_sort_mode();
    assert_eq!(app.status_message(), "Sorted by activity.");
    app.open_action_menu();
    let lines = app.command_lines();
    assert!(lines.iter().any(|line| line == "  T sort by tmux order"));

    app.close_action_menu();
    app.cycle_filter_mode();
    assert_eq!(app.status_message(), "Showing agents.");
    app.open_action_menu();
    let lines = app.command_lines();
    assert!(lines.iter().any(|line| line == "  F show needs you"));
}

#[test]
fn action_menu_empty_states_are_recovery_not_inert_send_actions() {
    let mut empty = app_with_panes(Vec::new(), vec![]);
    empty.action_menu_active = true;
    let lines = empty.command_lines();

    assert!(
        lines
            .iter()
            .any(|line| line == "Action: R refresh after starting tmux panes")
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "start tmux panes, then refresh")
    );
    for inert in [
        "send to selected pane",
        "+ start agent",
        "Enter show output",
    ] {
        assert!(!lines.iter().any(|line| line.contains(inert)), "{lines:?}");
    }

    let mut hidden = app_with_panes(vec![sample_pane("codex")], vec![]);
    hidden.search_query = String::from("no-match");
    hidden.action_menu_active = true;
    let lines = hidden.command_lines();

    assert!(
        lines
            .iter()
            .any(|line| line == "Action: backspace show all panes")
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.as_str() == "  backspace show all panes")
            .count(),
        1,
        "More should list the show-all recovery action once, not repeat it: {lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.as_str() == "backspace show all panes"),
        "More should indent the show-all recovery action under view: {lines:?}"
    );
    for inert in [
        "send to demo / agents",
        "+ start agent",
        "Enter show output",
    ] {
        assert!(!lines.iter().any(|line| line.contains(inert)), "{lines:?}");
    }

    let mut narrowed = app_with_panes(vec![sample_pane("codex")], vec![]);
    narrowed.search_query = String::from("codex");
    narrowed.action_menu_active = true;
    let lines = narrowed.command_lines();

    assert_eq!(
        lines
            .iter()
            .filter(|line| line.as_str() == "  backspace show all panes")
            .count(),
        1,
        "More should keep show-all discoverable while a narrowed view still has matches: {lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.as_str() == "backspace show all panes"),
        "More should indent the show-all recovery action under view: {lines:?}"
    );
}

#[test]
fn action_menu_makes_secondary_views_reachable() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.action_menu_active = true;

    let lines = app.command_lines();
    assert!(lines.iter().any(|line| line == "View"));
    assert!(lines.iter().any(|line| line == "  Enter show output"));
    assert!(lines.iter().any(|line| line == "  [ browse windows"));
    assert!(lines.iter().any(|line| line == "  ] command center"));

    app.context_pane = super::ContextPane::Navigator;
    app.action_menu_active = true;
    let lines = app.command_lines();
    assert!(lines.iter().any(|line| line == "  Enter open window"));
    assert!(!lines.iter().any(|line| line == "  Enter show output"));

    app.context_pane = super::ContextPane::Tail;
    app.action_menu_active = true;
    let lines = app.command_lines();
    assert!(lines.iter().any(|line| line == "  Enter show details"));
}

#[test]
fn command_lines_recommend_reply_for_selected_free_form_attention() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(
        vec![pane],
        vec![("%1", vec!["Waiting for approval. Continue?"])],
    );
    app.ui_settings.keybindings.action_ack_selected = vec![String::from("c")];

    let lines = app.command_lines();

    assert_eq!(lines[1], "Action: : reply");
}

#[test]
fn command_lines_recommend_selected_ack_for_selected_non_reply_attention() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["command failed"])]);
    app.ui_settings.keybindings.action_ack_selected = vec![String::from("c")];
    set_pane_report_fields(&mut app, "%1", "error", "command failed", "show output");

    let lines = app.command_lines();

    assert_eq!(lines[1], "Action: C mute alert");
}

#[test]
fn command_lines_recommend_summaries_for_reported_agents_without_alerts() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["working"])]);
    set_pane_report_fields(&mut app, "%1", "running", "none", "write tests");

    let lines = app.command_lines();

    assert_eq!(lines[1], "Action: S summarize panes");
}

#[test]
fn command_lines_recommend_bulk_mute_when_attention_is_off_selection() {
    let mut selected = sample_pane("codex");
    selected.id = String::from("%1");
    let mut waiting = sample_pane("claude");
    waiting.id = String::from("%2");
    waiting.pane_index = 1;
    waiting.active = false;
    let app = app_with_panes(
        vec![selected, waiting],
        vec![("%2", vec!["Waiting for approval. Continue?"])],
    );

    let lines = app.command_lines();

    assert_eq!(lines[1], "Action: A mute alerts");
}

#[test]
fn command_lines_recommend_recovery_when_there_are_no_visible_panes() {
    let empty = app_with_panes(vec![], vec![]);
    assert_eq!(
        empty.command_lines()[1],
        "Action: R refresh after starting tmux panes"
    );

    let mut no_match = app_with_panes(vec![sample_pane("codex")], vec![]);
    no_match.set_search_query_for_test("no-match");
    assert_eq!(
        no_match.command_lines()[1],
        "Action: backspace show all panes"
    );
}

#[test]
fn command_lines_recommend_live_send_list_before_default_send_actions() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.toggle_selected_mark();

    let lines = app.command_lines();

    assert!(lines.iter().any(|line| line == "Action: : send list"));
}

#[test]
fn command_lines_recommend_selection_when_send_list_targets_disappear() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.marked_pane_ids.insert(String::from("%missing"));

    let lines = app.command_lines();

    assert!(
        lines
            .iter()
            .any(|line| line == "send list has no live panes")
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "Action: Space add a visible pane")
    );
    assert!(
        !lines.iter().any(|line| line == "send list (0 panes)"),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line == "send list 1 pane"),
        "{lines:?}"
    );

    let footer = app.footer_line_for_width(100);
    assert!(footer.contains("send list empty"), "{footer}");
    assert!(footer.contains("Space add"), "{footer}");
    assert!(footer.contains("X clear"), "{footer}");
    assert!(!footer.contains(": send"), "{footer}");

    app.open_action_menu();
    let more = app.command_lines();
    assert!(
        more.iter()
            .any(|line| line == "send list has no live panes"),
        "{more:?}"
    );
    assert!(
        more.iter().any(|line| line == "  X clear send list"),
        "{more:?}"
    );
    assert!(!more.iter().any(|line| line == "  : send text"), "{more:?}");
    assert!(
        !more.iter().any(|line| line == "  G save fleet"),
        "{more:?}"
    );
}

#[test]
fn command_lines_recommend_continue_for_enter_safe_waiting_panes() {
    let pane = sample_pane("claude");
    let app = app_with_panes(vec![pane], vec![("%1", vec!["Press Enter to continue."])]);

    let lines = app.command_lines();

    assert_eq!(lines[1], "Action: I continue waiting panes");
}

#[test]
fn command_lines_recommend_default_send_or_add_for_single_pane() {
    let pane = sample_pane("codex");
    let app = app_with_panes(vec![pane], vec![]);

    let lines = app.command_lines();

    assert_eq!(lines[1], "Action: : send this pane");
}

#[test]
fn command_lines_show_first_steps_when_history_is_empty() {
    let pane = sample_pane("codex");
    let app = app_with_panes(vec![pane], vec![]);

    let lines = app.command_lines();

    assert_eq!(lines[0], "send to demo / agents");
    assert!(lines[1].starts_with("Action: "));
    assert!(!lines.iter().any(|line| line.starts_with("start ")));
    assert!(!lines.iter().any(|line| line == "Macros"));
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("vars {session} {window}"))
    );
}

#[test]
fn command_lines_expand_macro_section_when_a_slot_is_present() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.macro_slots[0] = Some(String::from("cargo test"));

    let lines = app.command_lines();

    assert!(lines.iter().any(|line| line == "Macros"));
    assert!(lines.iter().any(|line| line == "1: cargo test"));
}

#[test]
fn macro_slot_lookup_uses_rebound_keys() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.ui_settings.keybindings.macro_slot_1 = vec![String::from("u")];

    assert_eq!(app.macro_slot_for_key_token("u"), Some(0));
    assert_eq!(app.macro_slot_for_key_token("1"), None);
}

#[test]
fn command_lines_show_rebound_macro_labels() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.ui_settings.keybindings.macro_slot_1 = vec![String::from("u")];
    app.ui_settings.keybindings.repeat_last = vec![String::from("R")];
    app.remember_command("cargo test");

    let lines = app.command_lines();

    assert!(!lines.iter().any(|line| line == "Macros"));
    assert!(lines.iter().any(|line| line == "R repeat cargo test"));
}

#[test]
fn navigator_groups_windows_and_marks_selected_window() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.selected_window_id = Some(String::from("@2"));
    app.context_pane = super::ContextPane::Navigator;

    let lines = app.navigator_lines();
    assert!(lines.iter().any(|line| line == "demo:"));
    assert!(lines.iter().any(|line| line.contains(">  beta")));
}

#[test]
fn ensure_selection_defaults_to_the_top_visible_attention_pane() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(
        vec![first, second],
        vec![("%2", vec!["Waiting for approval. Continue?"])],
    );
    app.selected_pane_id = None;
    app.selected_window_id = None;

    app.ensure_selection();

    assert_eq!(app.selected_pane_id.as_deref(), Some("%2"));
}

#[test]
fn reset_selection_to_top_visible_reselects_after_runtime_changes() {
    let mut first = sample_pane("bash");
    first.id = String::from("%1");
    first.window_name = String::from("prompt");

    let mut second = sample_pane("bash");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("wait");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(
        vec![first, second],
        vec![("%2", vec!["Waiting for approval. Continue?"])],
    );

    app.selected_pane_id = Some(String::from("%1"));
    app.selected_window_id = Some(String::from("@0"));
    app.initialize_pane_status_cache();
    app.reset_selection_to_top_visible();

    assert_eq!(app.selected_pane_id.as_deref(), Some("%2"));
    assert_eq!(app.selected_window_id.as_deref(), Some("@2"));
}

#[test]
fn selection_changes_from_filtering_reset_details_scroll() {
    let mut first = sample_pane("bash");
    first.id = String::from("%1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(
        vec![first, second],
        vec![
            (
                "%1",
                vec![
                    "step 01 plan",
                    "step 02 fetch",
                    "step 03 build",
                    "step 04 test",
                    "step 05 verify",
                    "step 06 lint",
                    "step 07 render",
                    "step 08 inspect",
                    "step 09 patch",
                    "step 10 unit",
                    "step 11 ux",
                    "step 12 live",
                    "step 13 perf",
                    "step 14 ci",
                    "step 15 review",
                    "step 16 package",
                    "step 17 smoke",
                    "step 18 bless",
                    "step 19 audit",
                    "step 20 done",
                ],
            ),
            ("%2", vec!["Working on beta task"]),
        ],
    );
    app.context_pane = super::ContextPane::Tail;
    app.panel_focus = super::PanelFocus::Details;
    app.select_previous_pane();
    app.select_previous_pane();

    assert_eq!(app.selected_pane_id.as_deref(), Some("%1"));
    assert_eq!(app.details_scroll, 2);

    app.begin_search();
    for ch in "beta".chars() {
        app.push_search_char(ch);
    }
    app.finish_search();

    assert_eq!(app.selected_pane_id.as_deref(), Some("%2"));
    assert_eq!(app.selected_window_id.as_deref(), Some("@2"));
    assert_eq!(app.details_scroll, 0);
}

#[test]
fn focus_in_navigator_scopes_board_and_backspace_clears_it() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.context_pane = super::ContextPane::Navigator;
    app.selected_window_id = Some(String::from("@2"));

    tokio::runtime::Runtime::new()
        .expect("runtime should build")
        .block_on(app.focus_selected_pane())
        .expect("focus should succeed");

    assert_eq!(
        app.view_scope,
        super::ViewScope::Window {
            id: String::from("@2"),
            name: String::from("demo/beta"),
        }
    );
    assert_eq!(
        app.status_hint_line(),
        "? help  backspace shows all panes  J/K browse  Enter window  G show  / filter  L layout  . more  Esc back  Q quit"
    );

    app.clear_view_scope();
    assert_eq!(app.view_scope, super::ViewScope::All);
}

#[test]
fn navigator_selection_retargets_when_search_hides_the_browsed_window() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.context_pane = super::ContextPane::Navigator;
    app.selected_pane_id = Some(String::from("%1"));
    app.selected_window_id = Some(String::from("@2"));

    app.begin_search();
    for ch in "alpha".chars() {
        app.push_search_char(ch);
    }
    app.finish_search();

    assert_eq!(app.selected_pane_id.as_deref(), Some("%1"));
    assert_eq!(app.selected_window_id.as_deref(), Some("@1"));
    assert!(
        app.navigator_lines()
            .iter()
            .any(|line| line.contains(">  alpha"))
    );

    tokio::runtime::Runtime::new()
        .expect("runtime should build")
        .block_on(app.focus_selected_pane())
        .expect("focus should scope the visible window");
    assert_eq!(
        app.view_scope,
        super::ViewScope::Window {
            id: String::from("@1"),
            name: String::from("demo/alpha"),
        }
    );
}

#[test]
fn navigator_actions_self_heal_hidden_window_selection() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.context_pane = super::ContextPane::Navigator;
    app.selected_pane_id = Some(String::from("%1"));
    app.selected_window_id = Some(String::from("@2"));
    app.search_query = String::from("alpha");

    assert!(
        app.navigator_lines()
            .iter()
            .any(|line| line.contains(">  alpha"))
    );
    assert!(
        !app.navigator_lines()
            .iter()
            .any(|line| line.contains(">  beta"))
    );

    tokio::runtime::Runtime::new()
        .expect("runtime should build")
        .block_on(app.focus_selected_pane())
        .expect("focus should scope the visible window");
    assert_eq!(app.selected_window_id.as_deref(), Some("@1"));
    assert_eq!(
        app.view_scope,
        super::ViewScope::Window {
            id: String::from("@1"),
            name: String::from("demo/alpha"),
        }
    );
}

#[test]
fn navigator_movement_from_hidden_window_selection_starts_at_first_visible_window() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("visible-one");

    let mut hidden = sample_pane("bash");
    hidden.id = String::from("%2");
    hidden.window_id = String::from("@2");
    hidden.window_name = String::from("hidden");
    hidden.active = false;
    hidden.pane_index = 1;

    let mut third = sample_pane("claude");
    third.id = String::from("%3");
    third.window_id = String::from("@3");
    third.window_name = String::from("visible-two");
    third.active = false;
    third.pane_index = 2;

    let mut next = app_with_panes(vec![first.clone(), hidden.clone(), third.clone()], vec![]);
    next.context_pane = super::ContextPane::Navigator;
    next.panel_focus = super::PanelFocus::Details;
    next.selected_pane_id = Some(String::from("%1"));
    next.selected_window_id = Some(String::from("@2"));
    next.search_query = String::from("visible");

    next.select_next_pane();
    assert_eq!(next.selected_window_id.as_deref(), Some("@1"));

    let mut previous = app_with_panes(vec![first, hidden, third], vec![]);
    previous.context_pane = super::ContextPane::Navigator;
    previous.panel_focus = super::PanelFocus::Details;
    previous.selected_pane_id = Some(String::from("%1"));
    previous.selected_window_id = Some(String::from("@2"));
    previous.search_query = String::from("visible");

    previous.select_previous_pane();
    assert_eq!(previous.selected_window_id.as_deref(), Some("@1"));
}

#[test]
fn backspace_clears_search_filter_and_scope_in_one_obvious_recovery_step() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("bash");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("shell");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.search_query = String::from("no-match");
    app.filter_mode = FilterMode::Agents;
    app.view_scope = super::ViewScope::Window {
        id: String::from("@1"),
        name: String::from("demo/alpha"),
    };

    let footer = app.footer_line_for_width(100);
    assert!(footer.contains("backspace show all"), "{footer}");
    assert!(app.visible_pane_indices().is_empty());

    app.clear_view_scope();

    assert_eq!(app.search_query, "");
    assert_eq!(app.filter_mode, FilterMode::All);
    assert_eq!(app.view_scope, super::ViewScope::All);
    assert_eq!(app.status_message(), "Showing all panes.");
    assert_eq!(app.visible_pane_indices().len(), 2);
}

#[test]
fn fanout_targets_selected_lane_members() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");

    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;

    let mut third = sample_pane("claude");
    third.id = String::from("%3");
    third.active = false;
    third.window_name = String::from("other");

    let mut app = app_with_panes(vec![first, second, third], vec![]);
    app.toggle_fanout_mode();

    assert_eq!(
        app.active_target_panes()
            .into_iter()
            .map(|pane| pane.id.clone())
            .collect::<Vec<_>>(),
        vec![String::from("%1"), String::from("%2")]
    );
}

#[test]
fn target_scope_copy_stays_plain_for_empty_and_marked_states() {
    let mut empty = app_with_panes(Vec::new(), vec![]);

    assert_eq!(empty.fanout_summary_for_selected(), "off");
    assert_eq!(empty.active_target_description(), "selected pane");
    assert_eq!(empty.summary_target_scope(), "the selected pane");
    assert_eq!(empty.command_preview_lines(), Vec::<String>::new());

    empty.fanout_mode = FanoutMode::Lane;
    assert_eq!(empty.fanout_summary_for_selected(), "send to 0 panes");
    assert_eq!(empty.active_target_description(), "selected lane");
    assert_eq!(empty.summary_target_scope(), "the selected lane");
    assert!(empty.active_target_panes().is_empty());

    let mut pane = sample_pane("codex");
    pane.id = String::from("%1");
    let mut marked = app_with_panes(vec![pane], vec![]);
    marked.toggle_selected_mark();

    assert_eq!(marked.fanout_summary_for_selected(), "send list (1 pane)");
    assert_eq!(marked.active_target_description(), "send list (1 pane)");
    assert_eq!(marked.summary_target_scope(), "the send list");
}

#[test]
fn command_mode_tracks_typed_buffer() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.begin_command_input();
    for ch in "cargo test".chars() {
        app.push_command_char(ch);
    }

    assert!(app.is_command_input_active());
    assert_eq!(app.command_buffer, "cargo test");
    assert!(app.panes_title().contains("cmd: cargo test"));
}

#[test]
fn command_mode_uses_lane_targets_when_fanout_is_on() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");

    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;

    let mut third = sample_pane("claude");
    third.id = String::from("%3");
    third.active = false;

    let mut app = app_with_panes(vec![first, second, third], vec![]);
    app.toggle_fanout_mode();
    app.begin_command_input();

    assert!(app.is_command_input_active());
    assert_eq!(
        app.active_target_panes()
            .into_iter()
            .map(|pane| pane.id.clone())
            .collect::<Vec<_>>(),
        vec![String::from("%1"), String::from("%2")]
    );
    assert_eq!(app.active_target_description(), "Codex lane (2 panes)");
}

#[test]
fn marked_panes_automatically_become_the_target_set() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;

    let mut third = sample_pane("opencode");
    third.id = String::from("%3");
    third.active = false;
    third.pane_index = 2;

    let mut app = app_with_panes(vec![first, second, third], vec![]);
    app.toggle_selected_mark();
    app.select_next_pane();
    app.toggle_selected_mark();

    assert_eq!(
        app.active_target_panes()
            .into_iter()
            .map(|pane| pane.id.clone())
            .collect::<Vec<_>>(),
        vec![String::from("%1"), String::from("%2")]
    );
}

#[test]
fn remembering_commands_keeps_recent_unique_order() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.remember_command("cargo test");
    app.remember_command("git status");
    app.remember_command("cargo test");

    assert_eq!(
        app.recent_commands.iter().cloned().collect::<Vec<_>>(),
        vec![String::from("cargo test"), String::from("git status")]
    );
}

#[test]
fn remembering_commands_ignores_blank_input_and_caps_history() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.remember_command("   ");
    assert!(app.recent_commands.is_empty());

    for index in 0..(super::MAX_RECENT_COMMANDS + 2) {
        app.remember_command(&format!("cmd {index}"));
    }

    let newest = format!("cmd {}", super::MAX_RECENT_COMMANDS + 1);
    let oldest_kept = String::from("cmd 2");
    assert_eq!(app.recent_commands.len(), super::MAX_RECENT_COMMANDS);
    assert_eq!(app.recent_commands.front(), Some(&newest));
    assert_eq!(app.recent_commands.back(), Some(&oldest_kept));
    assert!(!app.recent_commands.iter().any(|command| command == "cmd 0"));
}

#[test]
fn pinning_recent_command_fills_macro_slot() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.remember_command("cargo test");
    app.begin_macro_assign();
    app.assign_recent_command_to_slot(1);

    assert_eq!(app.macro_slots[1], Some(String::from("cargo test")));
    assert!(!app.is_macro_assign_active());
    assert_eq!(app.context_panel_title(), "Send");
    assert!(app.command_shortcuts_are_visible());
}

#[test]
fn command_panel_lists_macro_slots_and_recent_commands() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.macro_slots[0] = Some(String::from("continue"));
    app.remember_command("cargo test");

    let lines = app.command_lines();
    assert!(lines.iter().any(|line| line.contains("1: continue")));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("] repeat cargo test"))
    );
    assert!(!lines.iter().any(|line| line.contains("vars {session}")));
}

#[test]
fn command_panel_only_labels_the_replayable_recent_command() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.remember_command("older command");
    app.remember_command("latest command");

    let lines = app.command_lines();
    let repeat_rows = lines
        .iter()
        .filter(|line| line.contains("] repeat"))
        .map(String::as_str)
        .collect::<Vec<_>>();

    assert_eq!(repeat_rows, vec!["] repeat latest command"]);
    assert!(
        !lines.iter().any(|line| line.contains("older command")),
        "{lines:?}"
    );
}

#[test]
fn command_input_only_shows_recent_shortcut_when_it_is_usable() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.macro_slots[0] = Some(String::from("continue"));
    app.remember_command("cargo test");

    app.begin_command_input();
    let empty_lines = app.command_lines();

    assert!(app.command_input_can_repeat_recent());
    assert!(
        empty_lines
            .iter()
            .any(|line| line.contains("] repeat cargo test"))
    );
    assert!(!empty_lines.iter().any(|line| line == "Macros"));
    assert!(
        app.status_hint_line_for_width(120)
            .contains("] repeat latest")
    );

    app.push_command_char('x');
    let typed_lines = app.command_lines();

    assert!(!app.command_input_can_repeat_recent());
    assert!(!typed_lines.iter().any(|line| line == "Recent"));
    assert!(!typed_lines.iter().any(|line| line == "Macros"));
    assert!(
        !app.status_hint_line_for_width(120)
            .contains("repeat latest")
    );
}

#[test]
fn command_shortcuts_only_count_as_visible_on_the_send_surface() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.macro_slots[0] = Some(String::from("continue"));
    app.remember_command("cargo test");

    assert!(!app.command_shortcuts_are_visible());

    app.cycle_context_pane();
    assert_eq!(app.context_pane, super::ContextPane::Tail);
    assert!(!app.command_shortcuts_are_visible());

    app.cycle_context_pane();
    assert_eq!(app.context_pane, super::ContextPane::Targets);
    assert!(app.command_shortcuts_are_visible());

    app.begin_command_input();
    assert!(!app.command_shortcuts_are_visible());
    assert!(app.command_input_can_repeat_recent());
}

#[tokio::test]
async fn pending_send_review_owns_the_visible_surface() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut app = app_with_panes(vec![first, second], vec![]);
    set_pane_report_fields(&mut app, "%1", "running", "none", "background work");
    app.toggle_selected_mark();
    app.select_next_pane();
    app.toggle_selected_mark();
    app.remember_command("cargo test");
    app.begin_macro_assign();
    app.assign_recent_command_to_slot(0);
    app.open_action_menu();
    assert!(app.is_action_menu_active());

    app.send_command_text("echo hi")
        .await
        .expect("multi-target send should stage");
    let lines = app.command_lines();

    assert!(app.has_pending_dispatch());
    assert!(!app.is_action_menu_active());
    assert_eq!(app.context_panel_title(), "Send");
    assert!(
        lines
            .iter()
            .any(|line| line == "To: the send list (2 panes)")
    );
    assert!(lines.iter().any(|line| line == "Text: echo hi"));
    assert!(lines.iter().any(|line| line == "Targets"));
    assert!(!lines.iter().any(|line| line == "review"));
    assert!(!lines.iter().any(|line| line == "Reports"));
    assert!(!lines.iter().any(|line| line.contains("background work")));
    assert!(!lines.iter().any(|line| line == "send echo hi"));
    assert!(!lines.iter().any(|line| line.starts_with("try ")));
    assert!(!lines.iter().any(|line| line == "View"));
    assert!(!lines.iter().any(|line| line == "Pane"));
    assert!(!lines.iter().any(|line| line == "Recent"));
    assert!(!lines.iter().any(|line| line == "Macros"));

    app.action_menu_active = true;
    assert_eq!(app.context_panel_title(), "Send");
    let review_lines = app.context_panel_lines();
    assert_eq!(
        review_lines.first().map(String::as_str),
        Some("To: the send list (2 panes)")
    );
    app.action_menu_active = false;

    app.open_action_menu();
    assert!(!app.is_action_menu_active());
    assert_eq!(app.context_panel_title(), "Send");
}

#[tokio::test]
async fn pending_send_review_recovers_when_all_targets_disappear() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.pending_dispatch = Some(super::StagedDispatch {
        text: String::from("echo gone"),
        expanded: vec![(String::from("%missing"), String::from("echo gone"))],
        remember: true,
        target_description: String::from("send list (1 pane)"),
    });

    app.confirm_pending_dispatch()
        .await
        .expect("missing staged panes should not call tmux");

    assert!(!app.has_pending_dispatch());
    assert_eq!(
        app.status_message(),
        "No panes remain for `echo gone`; 1 pane disappeared."
    );
    assert!(app.recent_commands.is_empty());
}

#[tokio::test]
async fn pending_send_review_reports_partial_disappearance_without_remembering() {
    let log_path = unique_test_path("partial-dispatch", ".log");
    let fake_tmux = fake_tmux_script(
        "partial-dispatch",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
            log_path.display().to_string().replace('\'', "'\\''")
        ),
    );
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.probe.target.binary = fake_tmux;
    app.pending_dispatch = Some(super::StagedDispatch {
        text: String::from("echo hi"),
        expanded: vec![
            (String::from("%1"), String::from("echo hi")),
            (String::from("%missing"), String::from("echo hi")),
        ],
        remember: false,
        target_description: String::from("send list (2 panes)"),
    });

    app.confirm_pending_dispatch()
        .await
        .expect("partial target disappearance should still send to live panes");

    assert!(!app.has_pending_dispatch());
    assert_eq!(
        app.status_message(),
        "Sent `echo hi` to 1 pane; 1 pane disappeared."
    );
    assert!(app.recent_commands.is_empty());
    let recorded = fs::read_to_string(&log_path).expect("live pane send should be recorded");
    assert!(
        recorded.contains("send-keys -t %1 -l -- echo hi"),
        "{recorded}"
    );
    assert!(!recorded.contains("%missing"), "{recorded}");
}

#[tokio::test]
async fn summary_polling_reports_disappeared_targets_without_exposing_prompt() {
    let log_path = unique_test_path("summary-partial-dispatch", ".log");
    let fake_tmux = fake_tmux_script(
        "summary-partial-dispatch",
        &format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> '{}'

if [ "$1" = "send-keys" ]; then
  case "$*" in
    *"%2"*) echo "can't find pane: %2" >&2; exit 1 ;;
    *) exit 0 ;;
  esac
fi

if [ "$1" = "list-panes" ]; then
  cat <<'EOF'
$0	demo	@0	agents	%1	0	100	workspace	codex	/workspace	1	0
EOF
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
            log_path.display().to_string().replace('\'', "'\\''")
        ),
    );
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut app = app_with_panes(vec![first, second], vec![]);
    app.probe.target.binary = fake_tmux;
    app.marked_pane_ids
        .extend([String::from("%1"), String::from("%2")]);

    app.request_target_summaries()
        .await
        .expect("summary polling should recover from a vanished target");

    assert_eq!(
        app.status_message(),
        "Asked 1 pane for a one-line summary: the send list; 1 pane disappeared."
    );
    assert!(!app.status_message().contains("STATUS="));
    assert!(!app.status_message().contains("NEXT=<next>"));
    assert_eq!(app.snapshot().pane_count(), 1);
    assert!(app.marked_pane_ids.contains("%1"));
    assert!(!app.marked_pane_ids.contains("%2"));
    assert!(app.recent_commands.is_empty());
    let recorded = fs::read_to_string(&log_path).expect("summary prompt should be recorded");
    assert!(
        recorded.contains("send-keys -t %1 -l -- Reply in exactly one line"),
        "{recorded}"
    );
}

#[tokio::test]
async fn summary_request_without_targets_uses_plain_recovery_copy() {
    let mut app = app_with_panes(Vec::new(), vec![]);

    app.request_target_summaries()
        .await
        .expect("empty summary request should stay recoverable");

    assert_eq!(app.status_message(), "No panes available to summarize.");
}

#[tokio::test]
async fn summary_request_for_stale_loaded_fleet_names_the_fleet() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.target_groups = vec![super::TargetGroup {
        name: String::from("triage"),
        members: vec![super::PaneLocator {
            session_name: String::from("demo"),
            window_name: String::from("missing"),
            pane_index: 0,
        }],
    }];
    app.apply_target_group(0);

    app.request_target_summaries()
        .await
        .expect("stale fleet summary request should stay local");

    assert_eq!(
        app.status_message(),
        "Fleet `triage` has no live panes to summarize."
    );
}

#[tokio::test]
async fn summary_polling_reports_when_every_target_disappears() {
    let fake_tmux = fake_tmux_script(
        "summary-all-disappear",
        r#"#!/bin/sh
if [ "$1" = "send-keys" ]; then
  echo "can't find pane" >&2
  exit 1
fi

if [ "$1" = "list-panes" ]; then
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut app = app_with_panes(vec![first, second], vec![]);
    app.probe.target.binary = fake_tmux;
    app.marked_pane_ids
        .extend([String::from("%1"), String::from("%2")]);

    app.request_target_summaries()
        .await
        .expect("summary polling should explain when every target vanished");

    assert_eq!(
        app.status_message(),
        "No panes remain for summaries; 2 panes disappeared."
    );
    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(app.marked_pane_ids.is_empty());
    assert!(app.recent_commands.is_empty());
}

#[tokio::test]
async fn summary_polling_reports_named_fleet_when_every_target_disappears() {
    let fake_tmux = fake_tmux_script(
        "summary-fleet-all-disappear",
        r#"#!/bin/sh
if [ "$1" = "send-keys" ]; then
  echo "can't find pane" >&2
  exit 1
fi

if [ "$1" = "list-panes" ]; then
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut app = app_with_panes(vec![first, second], vec![]);
    app.probe.target.binary = fake_tmux;
    app.target_groups = vec![super::TargetGroup {
        name: String::from("triage"),
        members: vec![
            super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("agents"),
                pane_index: 0,
            },
            super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("agents"),
                pane_index: 1,
            },
        ],
    }];
    app.apply_target_group(0);

    app.request_target_summaries()
        .await
        .expect("summary polling should preserve the fleet name when targets vanish");

    assert_eq!(
        app.status_message(),
        "Fleet `triage`: no panes remain for summaries; 2 panes disappeared."
    );
    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(app.marked_pane_ids.is_empty());
}

#[tokio::test]
async fn immediate_send_and_enter_recover_when_selected_target_disappears() {
    let fake_tmux = fake_tmux_script(
        "missing-pane-send",
        "#!/bin/sh\necho \"can't find pane: %1\" >&2\nexit 1\n",
    );
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["Press Enter to continue"])]);
    app.probe.target.binary = fake_tmux.clone();

    app.send_command_text("echo gone")
        .await
        .expect("missing pane should be recoverable for immediate sends");
    assert_eq!(
        app.status_message(),
        "No panes remain for `echo gone`; 1 pane disappeared."
    );
    assert!(app.recent_commands.is_empty());
    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(app.selected_pane_id.is_none());

    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["Press Enter to continue"])]);
    app.probe.target.binary = fake_tmux;
    app.send_enter_to_selected()
        .await
        .expect("missing pane should be recoverable for direct Enter");
    assert_eq!(
        app.status_message(),
        "No panes remain for Enter; 1 pane disappeared."
    );
    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(app.selected_pane_id.is_none());
}

#[tokio::test]
async fn jump_zoom_and_smart_focus_recover_when_selected_target_disappears() {
    let fake_tmux = fake_tmux_script(
        "missing-pane-navigation",
        "#!/bin/sh\necho \"can't find pane: %1\" >&2\nexit 1\n",
    );
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["idle"])]);
    app.probe.target.binary = fake_tmux.clone();

    app.toggle_selected_zoom()
        .await
        .expect("missing pane should be recoverable for zoom");
    assert!(
        app.status_message()
            .contains("disappeared. Refreshed panes.")
    );
    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(app.selected_pane_id.is_none());

    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["idle"])]);
    app.probe.target.binary = fake_tmux.clone();
    app.jump_to_selected_pane()
        .await
        .expect("missing pane should be recoverable for jump");
    assert!(
        app.status_message()
            .contains("disappeared. Refreshed panes.")
    );
    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(app.selected_pane_id.is_none());

    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["idle"])]);
    app.probe.target.binary = fake_tmux;
    app.perform_smart_action()
        .await
        .expect("missing pane should be recoverable for smart focus");
    assert!(
        app.status_message()
            .contains("disappeared. Refreshed panes.")
    );
    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(app.selected_pane_id.is_none());
}

#[tokio::test]
async fn runtime_capture_prunes_panes_that_disappear_before_capture() {
    let fake_tmux = fake_tmux_script(
        "missing-pane-capture",
        "#!/bin/sh\necho \"can't find pane: %1\" >&2\nexit 1\n",
    );
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.probe.target.binary = fake_tmux;

    app.capture_runtime_from_snapshot(true)
        .await
        .expect("missing pane capture should stay recoverable");

    assert!(app.snapshot.panes.is_empty());
    assert!(app.selected_pane_id.is_none());
    assert!(
        app.status_message()
            .contains("disappeared. Refreshed panes.")
    );
    assert!(!app.status_message().contains("Pane capture failed"));
}

#[tokio::test]
async fn dirty_capture_prunes_panes_that_disappear_before_capture() {
    let fake_tmux = fake_tmux_script(
        "missing-dirty-capture",
        "#!/bin/sh\necho \"can't find pane: %1\" >&2\nexit 1\n",
    );
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.probe.target.binary = fake_tmux;
    app.dirty_pane_ids.insert(String::from("%1"));

    app.capture_runtime_for_dirty_panes()
        .await
        .expect("missing dirty pane capture should stay recoverable");

    assert!(app.snapshot.panes.is_empty());
    assert!(app.dirty_pane_ids.is_empty());
    assert!(
        app.status_message()
            .contains("disappeared. Refreshed panes.")
    );
    assert!(!app.status_message().contains("Pane capture failed"));

    let mut app = app_with_panes(Vec::new(), vec![]);
    app.status_message = String::from("steady");
    app.dirty_pane_ids.insert(String::from("%already-gone"));

    app.capture_runtime_for_dirty_panes()
        .await
        .expect("stale dirty ids should be skipped without tmux work");

    assert!(app.snapshot.panes.is_empty());
    assert!(app.dirty_pane_ids.is_empty());
    assert_eq!(app.status_message(), "steady");
}

#[tokio::test]
async fn runtime_capture_failures_stay_jargon_free_without_pruning_panes() {
    let fake_tmux = fake_tmux_script(
        "capture-permission-denied",
        r#"#!/bin/sh
if [ "$1" = "capture-pane" ]; then
  echo "permission denied by tmux hook" >&2
  exit 1
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.probe.target.binary = fake_tmux;

    app.capture_runtime_from_snapshot(true)
        .await
        .expect("capture failure should stay recoverable");
    assert_eq!(
        app.status_message(),
        "Could not read output for demo / agents: permission denied by tmux hook."
    );
    assert!(!app.status_message().contains("tmux command failed"));
    assert!(!app.status_message().contains("Pane capture failed"));
    assert_eq!(app.snapshot().pane_count(), 1);

    app.status_message.clear();
    app.dirty_pane_ids.insert(String::from("%1"));
    app.capture_runtime_for_dirty_panes()
        .await
        .expect("dirty capture failure should stay recoverable");
    assert_eq!(
        app.status_message(),
        "Could not read output for demo / agents: permission denied by tmux hook."
    );
    assert!(!app.status_message().contains("tmux command failed"));
    assert!(!app.status_message().contains("Pane capture failed"));
    assert_eq!(app.snapshot().pane_count(), 1);
}

#[tokio::test]
async fn runtime_capture_clears_snapshot_when_tmux_server_disappears() {
    let fake_tmux = fake_tmux_script(
        "server-gone-capture",
        "#!/bin/sh\necho 'no server running on /tmp/tmux-501/default' >&2\nexit 1\n",
    );
    let first = sample_pane("codex");
    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("agents2");
    let mut app = app_with_panes(vec![first, second], vec![]);
    app.probe.target.binary = fake_tmux;

    app.capture_runtime_from_snapshot(true)
        .await
        .expect("server loss during capture should stay recoverable");

    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(app.selected_pane_id.is_none());
    assert!(
        app.status_message().starts_with("No tmux server found"),
        "{}",
        app.status_message()
    );
    assert!(!app.status_message().contains("Pane capture failed"));
}

#[tokio::test]
async fn dirty_capture_clears_snapshot_when_tmux_server_disappears() {
    let fake_tmux = fake_tmux_script(
        "server-gone-dirty-capture",
        "#!/bin/sh\necho 'no server running on /tmp/tmux-501/default' >&2\nexit 1\n",
    );
    let first = sample_pane("codex");
    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("agents2");
    let mut app = app_with_panes(vec![first, second], vec![]);
    app.probe.target.binary = fake_tmux;
    app.dirty_pane_ids.insert(String::from("%1"));

    app.capture_runtime_for_dirty_panes()
        .await
        .expect("server loss during dirty capture should stay recoverable");

    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(app.selected_pane_id.is_none());
    assert!(
        app.status_message().starts_with("No tmux server found"),
        "{}",
        app.status_message()
    );
    assert!(!app.status_message().contains("Pane capture failed"));
}

#[test]
fn send_recovery_feedback_stays_visible_when_search_is_narrowed() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.set_search_query_for_test("prompt");
    app.set_status_message_for_test("No panes remain for `echo gone`.");

    let footer = app.footer_line_for_width(120);

    assert!(footer.contains("No panes remain"), "{footer}");
    assert!(footer.contains("? help"), "{footer}");

    app.set_status_message_for_test("ops/prompt disappeared. Refreshed panes.");
    let footer = app.footer_line_for_width(120);
    assert!(footer.contains("disappeared. Refreshed panes."), "{footer}");
}

#[test]
fn status_hint_line_collapses_to_mode_labels_for_active_inputs() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.begin_search();
    assert_eq!(
        app.status_hint_line(),
        "type to filter  Enter apply  Esc cancel  backspace delete"
    );

    app.cancel_search();
    app.begin_command_input();
    assert_eq!(
        app.status_hint_line(),
        "type text  Enter send  Esc cancel  backspace delete"
    );

    app.cancel_command_input();
    app.open_action_menu();
    assert_eq!(
        app.status_hint_line(),
        "? help  press a listed key  Esc close"
    );

    app.toggle_help_overlay();
    assert_eq!(app.status_hint_line(), "Esc close  Q quit");
}

#[test]
fn width_aware_status_hint_compacts_on_small_widths() {
    let pane = sample_pane("codex");
    let app = app_with_panes(vec![pane], vec![]);

    assert_eq!(
        app.status_hint_line_for_width(60),
        "? help  J/K move  Enter output  : send  / filter  Q quit"
    );
    assert_eq!(
        app.status_hint_line_for_width(88),
        "? help  J/K move  Enter output  G show  Space add  : send  / filter  . more  Q quit"
    );
}

#[test]
fn footer_drops_whole_actions_instead_of_cutting_the_last_key_hint() {
    let mut pane = sample_pane("claude");
    pane.id = String::from("%1");
    let output = vec![
        "Claude Code",
        "Dialog open: Allow command? [y/n]",
        "Worker request: run cargo test usability_",
        "line 04",
        "line 05",
        "line 06",
        "line 07",
        "line 08",
        "line 09",
        "line 10",
        "line 11",
        "line 12",
        "line 13",
        "line 14",
        "line 15",
        "line 16",
        "line 17",
        "line 18",
        "line 19",
        "line 20",
    ];
    let mut app = app_with_panes(vec![pane], vec![("%1", output)]);

    app.set_search_query_for_test("claude");
    app.context_pane = super::ContextPane::Tail;
    app.panel_focus = super::PanelFocus::Details;

    let footer = app.footer_line_for_width(120);
    assert!(footer.contains("backspace show all"), "{footer}");
    assert!(footer.contains("K older/J newer"), "{footer}");
    assert!(footer.contains(". more"), "{footer}");
    assert!(!footer.contains(". mor..."), "{footer}");
    assert!(!footer.contains("..."), "{footer}");
    assert!(footer.chars().count() <= 120, "{footer}");
}

#[test]
fn status_hint_shows_continue_only_when_enter_is_safe() {
    let pane = sample_pane("claude");
    let waiting = app_with_panes(vec![pane], vec![("%1", vec!["Press Enter to continue."])]);
    assert!(
        waiting
            .status_hint_line_for_width(120)
            .contains("A continue")
    );
    assert!(
        !waiting
            .status_hint_line_for_width(88)
            .contains("A continue")
    );

    let pane = sample_pane("codex");
    let running = app_with_panes(vec![pane], vec![("%1", vec!["Working..."])]);
    assert!(
        !running
            .status_hint_line_for_width(120)
            .contains("A continue")
    );

    let mut lane_running =
        app_with_panes(vec![sample_pane("codex")], vec![("%1", vec!["Working..."])]);
    lane_running.toggle_fanout_mode();
    assert_eq!(lane_running.fanout_mode, FanoutMode::Lane);
    assert!(
        !lane_running
            .status_hint_line_for_width(120)
            .contains("A continue")
    );

    let mut active = sample_pane("codex");
    active.id = String::from("%1");
    let mut waiting_peer = sample_pane("codex");
    waiting_peer.id = String::from("%2");
    waiting_peer.active = false;
    waiting_peer.pane_index = 1;
    let mut lane_waiting = app_with_panes(
        vec![active, waiting_peer],
        vec![
            ("%1", vec!["Working..."]),
            ("%2", vec!["Press Enter to continue."]),
        ],
    );
    lane_waiting.toggle_fanout_mode();
    assert_eq!(lane_waiting.fanout_mode, FanoutMode::Lane);
    assert!(
        lane_waiting
            .status_hint_line_for_width(120)
            .contains("A continue")
    );
}

#[test]
fn footer_line_prefers_compact_hints_for_tight_search_and_command_modes() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.begin_search();
    app.push_search_char('o');
    app.push_search_char('p');
    assert_eq!(
        app.footer_line_for_width(60),
        "type to filter  Enter apply  Esc cancel  backspace delete"
    );

    app.cancel_search();
    app.begin_command_input();
    assert_eq!(
        app.footer_line_for_width(88),
        "type text  Enter send  Esc cancel  backspace delete"
    );
}

#[test]
fn footer_status_feedback_never_buries_recovery_or_keymap() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.set_search_query_for_test("codex");
    app.set_status_message_for_test("Saved fleet `triage`.");
    let narrowed_footer = app.footer_line_for_width(100);
    assert!(
        narrowed_footer.contains("backspace show all"),
        "{narrowed_footer}"
    );
    assert!(
        !narrowed_footer.contains("Saved fleet"),
        "{narrowed_footer}"
    );

    app.set_status_message_for_test("No panes remain for `echo gone`.");
    let important_narrowed_footer = app.footer_line_for_width(100);
    assert!(
        important_narrowed_footer.contains("No panes remain"),
        "{important_narrowed_footer}"
    );
    assert!(
        important_narrowed_footer.contains("? help"),
        "{important_narrowed_footer}"
    );

    app.set_search_query_for_test("");
    app.set_status_message_for_test("Saved fleet `triage`.");
    let tight_footer = app.footer_line_for_width(48);
    assert_eq!(tight_footer, "Saved fleet `triage`.");

    let medium_footer = app.footer_line_for_width(96);
    assert!(medium_footer.contains("? help"), "{medium_footer}");
    assert!(medium_footer.contains("Saved fleet"), "{medium_footer}");

    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut marked = app_with_panes(vec![first, second], vec![]);
    marked.toggle_selected_mark();
    marked.select_next_pane();
    marked.toggle_selected_mark();
    marked.context_pane = super::ContextPane::Tail;
    marked.set_status_message_for_test("Saved fleet `triage`.");
    let dense_footer = marked.footer_line_for_width(104);
    assert!(dense_footer.contains("send list 2 panes"), "{dense_footer}");
    assert!(!dense_footer.contains("Saved fleet"), "{dense_footer}");

    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut narrow_marked = app_with_panes(vec![first.clone(), second.clone()], vec![]);
    narrow_marked.toggle_selected_mark();
    narrow_marked.context_pane = super::ContextPane::Tail;
    let narrow_marked_footer = narrow_marked.footer_line_for_width(88);
    assert!(
        narrow_marked_footer.contains("send list 1 pane"),
        "{narrow_marked_footer}"
    );
    assert!(
        narrow_marked_footer.contains("Esc back"),
        "{narrow_marked_footer}"
    );

    let mut add_marked = app_with_panes(vec![first.clone(), second.clone()], vec![]);
    add_marked.toggle_selected_mark();
    add_marked.select_next_pane();
    add_marked.context_pane = super::ContextPane::Tail;
    add_marked.set_status_message_for_test("Saved fleet `triage`.");
    let add_footer = add_marked.footer_line_for_width(104);
    assert!(add_footer.contains("send list 1 pane"), "{add_footer}");
    assert!(add_footer.contains("Space add"), "{add_footer}");
    assert!(add_footer.contains("Esc back"), "{add_footer}");

    let mut hidden = app_with_panes(vec![first.clone(), second.clone()], vec![]);
    hidden.toggle_selected_mark();
    hidden.select_next_pane();
    hidden.toggle_selected_mark();
    hidden.set_search_query_for_test("codex");
    let hidden_footer = hidden.footer_line_for_width(68);
    assert!(
        hidden_footer.contains("send list 2 panes, 1 hidden"),
        "{hidden_footer}"
    );
    assert!(hidden_footer.contains(": send"), "{hidden_footer}");
    assert!(!hidden_footer.contains("Space"), "{hidden_footer}");

    let mut no_match = app_with_panes(vec![first], vec![]);
    no_match.toggle_selected_mark();
    no_match.set_search_query_for_test("zz-no-match");
    no_match.ensure_selection();
    let no_match_footer = no_match.footer_line_for_width(68);
    assert_eq!(
        no_match_footer,
        "? help  1 pane hidden  : send  X clear  backspace show all  Q quit"
    );
    no_match.context_pane = super::ContextPane::Tail;
    let wide_no_match_footer = no_match.status_hint_line_for_width(104);
    for term in [
        "send list 1 pane, 1 hidden",
        ": send",
        "X clear",
        "backspace show all",
        "/ filter",
        "Esc back",
        ". more",
    ] {
        assert!(
            wide_no_match_footer.contains(term),
            "missing {term}: {wide_no_match_footer}"
        );
    }
    let roomy_no_match_footer = no_match.status_hint_line_for_width(120);
    for term in [
        "send list 1 pane, 1 hidden",
        ": send",
        "X clear",
        "backspace show all",
        "/ filter",
        "Esc back",
        ". more",
        "Q quit",
    ] {
        assert!(
            roomy_no_match_footer.contains(term),
            "missing {term}: {roomy_no_match_footer}"
        );
    }
    assert!(
        !roomy_no_match_footer.contains("1 pane hidden"),
        "{roomy_no_match_footer}"
    );
    no_match.set_status_message_for_test("No panes remain for `echo gone`.");
    let feedback_no_match_footer = no_match.footer_line_for_width(104);
    assert!(
        feedback_no_match_footer.contains("backspace show all"),
        "{feedback_no_match_footer}"
    );
    assert!(
        !feedback_no_match_footer.contains("J/K move"),
        "{feedback_no_match_footer}"
    );
    let roomy_feedback_no_match_footer = no_match.footer_line_for_width(116);
    for term in [
        "send list 1 pane, 1 hidden",
        ": send",
        "X clear",
        "backspace show all",
        "/ filter",
        "Esc back",
        ". more",
        "Q quit",
    ] {
        assert!(
            roomy_feedback_no_match_footer.contains(term),
            "missing {term}: {roomy_feedback_no_match_footer}"
        );
    }
    no_match.set_status_message_for_test("Saved fleet `triage`.");
    let saved_feedback_no_match_footer = no_match.footer_line_for_width(104);
    assert!(
        saved_feedback_no_match_footer.contains("backspace show all"),
        "{saved_feedback_no_match_footer}"
    );
    assert!(
        !saved_feedback_no_match_footer.contains("J/K move"),
        "{saved_feedback_no_match_footer}"
    );

    no_match.set_status_message_for_test("No panes remain for `echo gone`.");
    let important_feedback_footer = no_match.footer_line_for_width(104);
    for term in [
        "No panes remain",
        "1 pane hidden",
        ": send",
        "X clear",
        "backspace show all",
    ] {
        assert!(
            important_feedback_footer.contains(term),
            "missing {term}: {important_feedback_footer}"
        );
    }
    for inert in ["J/K move", "Enter output", "Space add"] {
        assert!(
            !important_feedback_footer.contains(inert),
            "hidden send-list status feedback advertised {inert}: {important_feedback_footer}"
        );
    }
}

#[test]
fn footer_and_header_hints_stay_truthful_for_input_picker_and_narrowed_states() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.remember_command("cargo test");
    app.begin_command_input();
    assert_eq!(
        app.header_hint_line_for_width(60),
        "type  ] repeat  Enter send  Esc cancel"
    );
    assert_eq!(
        app.status_hint_line_for_width(60),
        "type  ] repeat  Enter send  Esc cancel"
    );
    assert_eq!(
        app.footer_line_for_width(60),
        "type  ] repeat  Enter send  Esc cancel"
    );
    assert_eq!(
        app.header_hint_line_for_width(88),
        "type text  ] repeat latest  Enter send  Esc cancel  backspace delete"
    );
    assert_eq!(
        app.status_hint_line_for_width(88),
        "type text  ] repeat latest  Enter send  Esc cancel  backspace delete"
    );

    app.cancel_command_input();
    app.toggle_selected_mark();
    app.begin_group_save_input();
    assert_eq!(
        app.header_hint_line_for_width(60),
        "type  Enter save  Esc cancel  backspace delete"
    );
    assert_eq!(
        app.header_hint_line_for_width(88),
        "type name  Enter save  Esc cancel  backspace delete"
    );
    assert_eq!(
        app.status_hint_line_for_width(88),
        "type name  Enter save  Esc cancel  backspace delete"
    );

    app.cancel_group_input();
    app.target_groups = vec![target_group("triage", "agents", 0)];
    app.open_fleet_picker();
    assert_eq!(
        app.header_hint_line_for_width(60),
        "J/K choose  Enter load  D delete  Esc close"
    );
    assert_eq!(
        app.status_hint_line_for_width(60),
        "? help  J/K choose  Enter load  D delete  Esc close"
    );
    assert_eq!(
        app.footer_line_for_width(60),
        "? help  J/K choose  Enter load  D delete  Esc close"
    );
    app.set_status_message_for_test("Deleted fleet `triage`.");
    let wide_picker_footer = app.footer_line_for_width(120);
    assert!(
        wide_picker_footer.contains("J/K choose"),
        "{wide_picker_footer}"
    );
    assert!(
        wide_picker_footer.contains("Enter load"),
        "{wide_picker_footer}"
    );
    assert!(
        wide_picker_footer.contains("D delete"),
        "{wide_picker_footer}"
    );
    assert!(
        !wide_picker_footer.contains("J/K move"),
        "{wide_picker_footer}"
    );
}

#[test]
fn narrowed_and_browse_footers_keep_recovery_and_browse_actions_visible() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_name = String::from("agents");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("review");
    second.active = false;
    second.pane_index = 1;

    let mut marked = app_with_panes(vec![first.clone(), second.clone()], vec![]);
    marked.toggle_selected_mark();
    marked.select_next_pane();
    marked.toggle_selected_mark();
    marked.set_search_query_for_test("review");
    marked.cycle_panel_focus();
    assert_eq!(marked.active_hidden_target_count(), 1);
    assert_eq!(marked.active_target_count_summary(), "2 panes, 1 hidden");
    let hidden_targets = marked.status_hint_line_for_width(95);
    for term in [
        "? help",
        "J/K move",
        "send list 2 panes, 1 hidden",
        ": send",
        "Space remove",
        "Esc back",
        "X clear",
        "Q quit",
    ] {
        assert!(
            hidden_targets.contains(term),
            "hidden-target footer should keep `{term}` visible:\n{hidden_targets}"
        );
    }

    let mut no_match = app_with_panes(vec![first.clone()], vec![]);
    no_match.set_search_query_for_test("zznomatch");
    assert!(no_match.visible_pane_indices().is_empty());
    let compact_recovery = no_match.status_hint_line_for_width(72);
    assert!(compact_recovery.contains("? help"), "{compact_recovery}");
    assert!(compact_recovery.contains("/ filter"), "{compact_recovery}");
    assert!(compact_recovery.contains(". more"), "{compact_recovery}");
    assert!(compact_recovery.contains("Q quit"), "{compact_recovery}");
    assert!(
        !compact_recovery.contains("backspace show all"),
        "{compact_recovery}"
    );

    let medium_recovery = no_match.status_hint_line_for_width(100);
    assert!(
        medium_recovery.contains("backspace show all"),
        "{medium_recovery}"
    );
    assert!(
        !medium_recovery.contains("backspace shows all panes"),
        "{medium_recovery}"
    );

    let wide_recovery = no_match.status_hint_line_for_width(140);
    assert!(
        wide_recovery.contains("backspace shows all panes"),
        "{wide_recovery}"
    );

    let mut browse = app_with_panes(vec![first, second], vec![]);
    browse.show_browse_view();
    browse.set_status_message_for_test("Showing demo / review #1 in tmux.");
    let browse_footer = browse.footer_line_for_width(104);
    for term in [
        "Showing demo",
        "J/K browse",
        "Enter window",
        "G show",
        "/ filter",
        ". more",
        "Esc back",
        "Q quit",
    ] {
        assert!(
            browse_footer.contains(term),
            "Browse footer should keep `{term}` visible:\n{browse_footer}"
        );
    }
    for inert in ["Space add", ": send"] {
        assert!(
            !browse_footer.contains(inert),
            "Browse footer advertised inert action `{inert}`:\n{browse_footer}"
        );
    }
}

#[test]
fn command_templates_expand_selected_pane_placeholders() {
    let mut pane = sample_pane("codex");
    pane.id = String::from("%9");
    pane.window_name = String::from("ops");
    pane.current_path = String::from("/tmp/demo");

    let expanded = expand_command_template(
        "echo {session} {window} {id} {path} {lane}",
        &pane,
        WorkloadKind::Codex,
    );

    assert_eq!(expanded, "echo demo ops %9 /tmp/demo codex");
}

#[test]
fn selected_pane_lines_show_template_preview_during_command_input() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.begin_command_input();
    for ch in "echo {window}".chars() {
        app.push_command_char(ch);
    }

    assert!(
        app.selected_pane_lines()
            .iter()
            .any(|line| line.contains("demo / agents : echo agents"))
    );
}

#[test]
fn selected_pane_lines_hide_tmux_pane_id_by_default() {
    let pane = sample_pane("codex");
    let app = app_with_panes(vec![pane], vec![]);

    let lines = app.selected_pane_lines();

    assert_eq!(lines[0], "demo/agents");
    assert!(!lines[0].contains("%1"));
}

#[test]
fn inactive_panes_without_runtime_use_checking_copy_not_unknown() {
    let mut pane = sample_pane("codex");
    pane.active = false;
    let app = app_with_panes(vec![pane], vec![]);

    let details = app.selected_pane_lines().join("\n");
    let row = app.board_rows(1).remove(0);
    let lanes = app.agent_lane_lines().join("\n");
    let combined = format!("{details}\n{row:?}\n{}\n{lanes}", row.compact_latest());

    assert!(details.contains("State: Checking"), "{details}");
    assert!(details.contains("Action: G show in tmux"), "{details}");
    assert!(!details.contains("Output"), "{details}");
    assert!(!details.contains("open in tmux"), "{details}");
    assert_eq!(row.status, "checking");
    assert_eq!(row.lifecycle, "checking");
    assert_eq!(row.title, "checking");
    assert_eq!(row.standard_latest(), "codex: checking");
    assert_eq!(row.compact_latest(), "codex checking");
    assert!(lanes.contains("1 checking"), "{lanes}");
    assert!(!combined.contains("codex: codex"), "{combined}");
    assert!(!combined.contains("codex codex"), "{combined}");
    assert!(!combined.contains("Unknown"), "{combined}");
    assert!(!combined.contains("unknown"), "{combined}");
    assert!(!combined.contains("unk"), "{combined}");
}

#[test]
fn selected_pane_lines_include_recent_tail_excerpt() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([
                String::from("first line"),
                String::from("second line"),
                String::from("third line"),
                String::from("fourth line"),
            ]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("first line second line third package binaryth line"),
            partial_line: String::new(),
        },
    );

    let lines = app.selected_pane_lines();

    assert!(lines.iter().any(|line| line == "Output"));
    assert!(lines.iter().any(|line| line.contains("second line")));
    assert!(lines.iter().any(|line| line.contains("fourth line")));
}

#[test]
fn selected_pane_lines_keep_a_useful_recent_output_slice() {
    let pane = sample_pane("bash");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([
                String::from("download dependencies"),
                String::from("compile crate"),
                String::from("run unit tests"),
                String::from("package binary"),
                String::from("upload artifact"),
                String::from("notify operator"),
                String::from("done release"),
            ]),
            last_output_at: Some(Instant::now()),
            corpus: String::from(
                "download dependencies compile crate run unit tests package binary upload artifact notify operator done release",
            ),
            partial_line: String::new(),
        },
    );

    let lines = app.selected_pane_lines();

    assert!(lines.iter().any(|line| line == "Output"));
    assert!(lines.iter().any(|line| line.contains("package binary")));
    assert!(lines.iter().any(|line| line.contains("upload artifact")));
    assert!(lines.iter().any(|line| line.contains("notify operator")));
    assert!(lines.iter().any(|line| line.contains("done release")));
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("Updated: ") && line.len() > "Updated: ".len())
    );
}

#[test]
fn selected_pane_lines_keep_a_larger_recent_output_slice() {
    let pane = sample_pane("bash");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([
                String::from("step 01 plan"),
                String::from("step 02 fetch"),
                String::from("step 03 build"),
                String::from("step 04 test"),
                String::from("step 05 lint"),
                String::from("step 06 package"),
                String::from("step 07 upload"),
                String::from("step 08 notify"),
                String::from("step 09 verify"),
                String::from("step 10 done"),
            ]),
            last_output_at: Some(Instant::now()),
            corpus: String::from(
                "step 01 plan step 02 fetch step 03 build step 04 test step 05 lint step 06 package step 07 upload step 08 notify step 09 verify step 10 done",
            ),
            partial_line: String::new(),
        },
    );

    let lines = app.selected_pane_lines();
    let output_index = lines
        .iter()
        .position(|line| line == "Output")
        .expect("Output section should exist");
    let output_lines = lines[output_index + 1..]
        .iter()
        .take_while(|line| line.starts_with("  "))
        .collect::<Vec<_>>();

    assert!(
        output_lines.len() >= 6,
        "expected a larger output slice: {lines:?}"
    );
    assert!(lines.iter().any(|line| line.contains("lint")));
    assert!(lines.iter().any(|line| line.contains("done")));
}

#[test]
fn details_focus_scrolls_output_instead_of_moving_fleet_selection() {
    let mut first = sample_pane("bash");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.pane_index = 1;
    second.active = false;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([
                String::from("step 01 plan"),
                String::from("step 02 fetch"),
                String::from("step 03 build"),
                String::from("step 04 test"),
                String::from("step 05 lint"),
                String::from("step 06 package"),
                String::from("step 07 upload"),
                String::from("step 08 notify"),
                String::from("step 09 verify"),
                String::from("step 10 done"),
            ]),
            last_output_at: Some(Instant::now()),
            corpus: String::from(
                "step 01 plan step 02 fetch step 03 build step 04 test step 05 lint step 06 package step 07 upload step 08 notify step 09 verify step 10 done",
            ),
            partial_line: String::new(),
        },
    );

    assert!(app.is_fleet_panel_focused());
    app.cycle_panel_focus();
    assert!(app.is_details_panel_focused());
    app.observe_details_scroll_viewport(Some(crate::tui::ScrollMetrics {
        content_len: app.details_scrollable_output_line_count(),
        viewport_len: 4,
    }));
    app.select_previous_pane();

    assert_eq!(app.selected_pane_id.as_deref(), Some("%1"));
    assert_eq!(app.details_scroll, 1);

    app.select_next_pane();
    assert_eq!(app.selected_pane_id.as_deref(), Some("%1"));
    assert_eq!(app.details_scroll, 0);

    app.cycle_panel_focus();
    assert!(app.is_fleet_panel_focused());
    app.select_next_pane();
    assert_eq!(app.selected_pane_id.as_deref(), Some("%2"));
}

#[test]
fn browse_and_command_center_keep_focus_on_the_visible_surface() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.show_browse_view();
    assert!(app.is_details_panel_focused());
    assert!(app.footer_line_for_width(120).contains("J/K browse"));
    assert!(!app.footer_line_for_width(120).contains("Tab focus"));

    app.cycle_panel_focus();
    assert!(app.is_details_panel_focused());
    assert!(app.footer_line_for_width(120).contains("J/K browse"));
    assert!(!app.footer_line_for_width(120).contains("Tab focus"));

    app.show_command_center();
    assert!(app.is_details_panel_focused());
    assert!(app.footer_line_for_width(120).contains("J/K move"));
    assert!(!app.footer_line_for_width(120).contains("Tab focus"));

    app.cycle_panel_focus();
    assert!(app.is_details_panel_focused());
    assert!(app.footer_line_for_width(120).contains("J/K move"));
    assert!(!app.footer_line_for_width(120).contains("Tab focus"));
}

#[test]
fn output_and_details_still_expose_panel_focus_when_it_changes_journey_control() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    assert!(app.footer_line_for_width(120).contains("Tab focus"));
    app.context_pane = super::ContextPane::Tail;
    assert!(app.footer_line_for_width(120).contains("Tab focus"));
}

#[test]
fn details_focus_scrolls_live_tail_when_output_view_is_open() {
    let pane = sample_pane("bash");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.context_pane = super::ContextPane::Tail;
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([
                String::from("tail 01"),
                String::from("tail 02"),
                String::from("tail 03"),
                String::from("tail 04"),
                String::from("tail 05"),
                String::from("tail 06"),
                String::from("tail 07"),
                String::from("tail 08"),
                String::from("tail 09"),
                String::from("tail 10"),
                String::from("tail 11"),
                String::from("tail 12"),
                String::from("tail 13"),
                String::from("tail 14"),
                String::from("tail 15"),
                String::from("tail 16"),
                String::from("tail 17"),
                String::from("tail 18"),
                String::from("tail 19"),
                String::from("tail 20"),
                String::from("tail 21"),
                String::from("tail 22"),
                String::from("tail 23"),
                String::from("tail 24"),
            ]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("tail 01 tail 02 tail 03 tail 04 tail 05 tail 06 tail 07 tail 08"),
            partial_line: String::new(),
        },
    );

    app.cycle_panel_focus();
    app.observe_details_scroll_viewport(Some(crate::tui::ScrollMetrics {
        content_len: app.details_scrollable_output_line_count(),
        viewport_len: 8,
    }));
    assert!(
        app.status_hint_line_for_width(120)
            .contains("K older/J newer")
    );
    app.select_previous_pane();

    assert_eq!(app.selected_pane_id.as_deref(), Some("%1"));
    assert_eq!(app.details_scroll, 1);
}

#[test]
fn details_scroll_clamps_to_real_output_length_without_overscroll_debt() {
    let pane = sample_pane("bash");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.context_pane = super::ContextPane::Tail;
    app.panel_focus = super::PanelFocus::Details;
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([
                String::from("tail 01"),
                String::from("tail 02"),
                String::from("tail 03"),
                String::from("tail 04"),
                String::from("tail 05"),
                String::from("tail 06"),
                String::from("tail 07"),
                String::from("tail 08"),
                String::from("tail 09"),
                String::from("tail 10"),
                String::from("tail 11"),
                String::from("tail 12"),
                String::from("tail 13"),
                String::from("tail 14"),
                String::from("tail 15"),
                String::from("tail 16"),
                String::from("tail 17"),
                String::from("tail 18"),
                String::from("tail 19"),
                String::from("tail 20"),
                String::from("tail 21"),
                String::from("tail 22"),
                String::from("tail 23"),
                String::from("tail 24"),
            ]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("tail 01 tail 02 tail 03 tail 04 tail 05 tail 06"),
            partial_line: String::new(),
        },
    );

    app.observe_details_scroll_viewport(Some(crate::tui::ScrollMetrics {
        content_len: app.details_scrollable_output_line_count(),
        viewport_len: 8,
    }));
    let max_offset = app.details_scroll_max_offset();
    assert!(max_offset > 1);

    for _ in 0..20 {
        app.select_previous_pane();
    }
    assert_eq!(app.details_scroll, max_offset);

    app.select_next_pane();
    assert_eq!(app.details_scroll, max_offset - 1);
}

#[test]
fn details_scroll_uses_rendered_viewport_metrics_not_raw_fallbacks() {
    let mut app = app_with_panes(
        vec![sample_pane("bash")],
        vec![("%1", vec!["short 01", "short 02", "short 03", "short 04"])],
    );
    app.context_pane = super::ContextPane::Tail;
    app.panel_focus = super::PanelFocus::Details;

    app.observe_details_scroll_viewport(Some(crate::tui::ScrollMetrics {
        content_len: 18,
        viewport_len: 6,
    }));
    assert_eq!(
        app.details_scroll_max_offset(),
        12,
        "scroll state should follow the rendered wrapped viewport, not raw output line count"
    );

    for _ in 0..20 {
        app.select_previous_pane();
    }
    assert_eq!(app.details_scroll, 12);

    app.observe_details_scroll_viewport(Some(crate::tui::ScrollMetrics {
        content_len: 18,
        viewport_len: 18,
    }));
    app.clamp_details_scroll_to_content();
    assert_eq!(
        app.details_scroll, 0,
        "a roomy rendered viewport should remove stale scroll debt"
    );
    assert_eq!(app.details_scroll_max_offset(), 0);
}

#[test]
fn details_scroll_page_home_and_end_are_deterministic() {
    let mut app = app_with_panes(
        vec![sample_pane("bash")],
        vec![(
            "%1",
            vec![
                "tail 01", "tail 02", "tail 03", "tail 04", "tail 05", "tail 06", "tail 07",
                "tail 08", "tail 09", "tail 10", "tail 11", "tail 12", "tail 13", "tail 14",
                "tail 15", "tail 16", "tail 17", "tail 18", "tail 19", "tail 20", "tail 21",
                "tail 22", "tail 23", "tail 24", "tail 25", "tail 26", "tail 27", "tail 28",
                "tail 29", "tail 30",
            ],
        )],
    );
    app.context_pane = super::ContextPane::Tail;
    app.panel_focus = super::PanelFocus::Details;
    app.observe_details_scroll_viewport(Some(crate::tui::ScrollMetrics {
        content_len: 30,
        viewport_len: 8,
    }));

    assert!(app.scroll_details_page_older());
    assert_eq!(app.details_scroll, 8);
    assert!(app.scroll_details_page_older());
    assert_eq!(app.details_scroll, 16);
    assert!(app.scroll_details_page_newer());
    assert_eq!(app.details_scroll, 8);
    assert!(app.scroll_details_to_oldest());
    assert_eq!(app.details_scroll, 22);
    assert!(app.scroll_details_to_newest());
    assert_eq!(app.details_scroll, 0);
}

#[test]
fn details_scroll_recovers_from_stale_overscroll_without_dead_zone() {
    let mut app = app_with_panes(
        vec![sample_pane("bash")],
        vec![(
            "%1",
            vec![
                "tail 01", "tail 02", "tail 03", "tail 04", "tail 05", "tail 06", "tail 07",
                "tail 08", "tail 09", "tail 10", "tail 11", "tail 12", "tail 13", "tail 14",
                "tail 15", "tail 16", "tail 17", "tail 18", "tail 19", "tail 20", "tail 21",
                "tail 22", "tail 23", "tail 24",
            ],
        )],
    );

    for context in [super::ContextPane::Tail, super::ContextPane::Inspect] {
        app.context_pane = context;
        app.panel_focus = super::PanelFocus::Details;
        app.details_scroll = 0;
        let fallback_viewport = if context == super::ContextPane::Tail {
            8
        } else {
            4
        };
        app.observe_details_scroll_viewport(Some(crate::tui::ScrollMetrics {
            content_len: app.details_scrollable_output_line_count(),
            viewport_len: fallback_viewport,
        }));
        let max_offset = app.details_scroll_max_offset();
        assert!(max_offset > 1);

        app.details_scroll = max_offset + 50;

        app.select_next_pane();
        assert_eq!(
            app.details_scroll,
            max_offset - 1,
            "one newer scroll should escape stale overscroll immediately in {context:?}"
        );

        for _ in 0..max_offset + 5 {
            app.select_next_pane();
        }
        assert_eq!(app.details_scroll, 0);
    }
}

#[test]
fn details_scroll_survives_layout_toggle_and_still_recovers_bottom() {
    let mut app = app_with_panes(
        vec![sample_pane("bash")],
        vec![(
            "%1",
            vec![
                "tail 01", "tail 02", "tail 03", "tail 04", "tail 05", "tail 06", "tail 07",
                "tail 08", "tail 09", "tail 10", "tail 11", "tail 12", "tail 13", "tail 14",
                "tail 15", "tail 16", "tail 17", "tail 18", "tail 19", "tail 20", "tail 21",
                "tail 22", "tail 23", "tail 24",
            ],
        )],
    );
    app.context_pane = super::ContextPane::Tail;
    app.panel_focus = super::PanelFocus::Details;
    app.observe_details_scroll_viewport(Some(crate::tui::ScrollMetrics {
        content_len: app.details_scrollable_output_line_count(),
        viewport_len: 8,
    }));

    for _ in 0..20 {
        app.select_previous_pane();
    }
    let oldest_offset = app.details_scroll;
    assert!(oldest_offset > 0);

    app.cycle_layout_preset();
    assert_eq!(
        app.details_scroll, oldest_offset,
        "layout toggles must not reset the user's scroll position"
    );

    for _ in 0..20 {
        app.select_next_pane();
    }
    assert_eq!(app.details_scroll, 0);
}

#[test]
fn focus_selected_pane_opens_a_scrollable_details_surface() {
    let pane = sample_pane("bash");
    let mut app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec![
                "step 01 plan",
                "step 02 fetch",
                "step 03 build",
                "step 04 test",
                "step 05 lint",
                "step 06 package",
                "step 07 upload",
                "step 08 notify",
                "step 09 verify",
                "step 10 snapshot",
                "step 11 render",
                "step 12 inspect",
                "step 13 patch",
                "step 14 live",
                "step 15 perf",
                "step 16 ci",
                "step 17 audit",
                "step 18 done",
            ],
        )],
    );
    app.panel_focus = super::PanelFocus::Fleet;
    app.details_scroll = 3;

    tokio::runtime::Runtime::new()
        .expect("runtime should build")
        .block_on(app.focus_selected_pane())
        .expect("focus should open output without tmux");

    assert_eq!(app.context_pane, super::ContextPane::Tail);
    assert!(app.is_details_panel_focused());
    assert_eq!(app.details_scroll, 0);
    assert!(
        app.status_hint_line_for_width(120)
            .contains("K older/J newer")
    );

    app.details_scroll = 2;
    tokio::runtime::Runtime::new()
        .expect("runtime should build")
        .block_on(app.focus_selected_pane())
        .expect("enter should not move backward from output");

    assert_eq!(app.context_pane, super::ContextPane::Tail);
    assert!(app.is_details_panel_focused());
    assert_eq!(app.details_scroll, 2);
    assert_eq!(app.status_message(), "");

    assert!(app.go_back());
    assert_eq!(app.context_pane, super::ContextPane::Inspect);
    assert!(app.is_fleet_panel_focused());
    assert_eq!(app.details_scroll, 0);
    assert!(!app.go_back());
}

#[test]
fn focus_selected_pane_does_not_advertise_empty_output_as_scrollable() {
    let mut first = sample_pane("bash");
    first.id = String::from("%1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("zsh");
    second.id = String::from("%2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    tokio::runtime::Runtime::new()
        .expect("runtime should build")
        .block_on(app.focus_selected_pane())
        .expect("focus should open output without tmux");

    assert_eq!(app.context_pane, super::ContextPane::Tail);
    assert!(app.is_fleet_panel_focused());
    assert!(app.status_hint_line_for_width(120).contains("Esc back"));
    assert!(!app.status_hint_line_for_width(120).contains("J/K move"));
    assert!(
        !app.status_hint_line_for_width(120)
            .contains("K older/J newer")
    );
    assert!(
        app.live_tail_lines()
            .contains(&String::from("No output yet."))
    );

    app.select_next_pane();
    assert_eq!(app.selected_pane_id.as_deref(), Some("%2"));
}

#[tokio::test]
async fn focus_selected_pane_marks_unseen_agent_bridge_review_seen() {
    let log_path = unique_test_path("bridge-seen-output", ".log");
    let fake_tmux = fake_tmux_script(
        "bridge-seen-output",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
            log_path.display().to_string().replace('\'', "'\\''")
        ),
    );

    let mut pane = sample_pane("codex");
    pane.id = String::from("%1");
    mark_pane_done_for_review(&mut pane, "codex", "release ready", "Ship V1", "10/10");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["all checks passed"])]);
    app.probe.target.binary = fake_tmux;

    app.focus_selected_pane()
        .await
        .expect("opening output should mark explicit review events seen");

    assert_eq!(app.context_pane, super::ContextPane::Tail);
    assert_eq!(
        app.snapshot.panes[0]
            .agent_event
            .as_ref()
            .expect("event should remain visible")
            .unseen,
        Some(false)
    );
    assert!(
        app.attention_queue().is_empty(),
        "seen bridge review events should leave the attention queue"
    );
    let recorded = fs::read_to_string(&log_path).expect("seen update should be logged");
    assert!(
        recorded.contains("set-environment -g MUXBOARD_AGENT_PANE__1_UNSEEN 0"),
        "{recorded}"
    );
    assert!(
        recorded.contains("set-environment -g TMUX_AGENT_PANE_%1_UNSEEN 0"),
        "{recorded}"
    );
    assert!(recorded.contains("refresh-client -S"), "{recorded}");
}

#[test]
fn summary_only_output_does_not_consume_movement_as_inert_scroll() {
    let mut first = sample_pane("node");
    first.id = String::from("%1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("zsh");
    second.id = String::from("%2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(
        vec![first, second],
        vec![(
            "%1",
            vec!["codex", "STATUS=running | BLOCKER=none | NEXT=write tests"],
        )],
    );
    tokio::runtime::Runtime::new()
        .expect("runtime should build")
        .block_on(app.focus_selected_pane())
        .expect("focus should open output without tmux");

    assert_eq!(app.context_pane, super::ContextPane::Tail);
    assert!(app.is_fleet_panel_focused());
    assert!(app.status_hint_line_for_width(120).contains("J/K move"));
    assert!(
        !app.status_hint_line_for_width(120)
            .contains("K older/J newer")
    );

    app.cycle_panel_focus();
    assert!(app.is_details_panel_focused());
    assert!(app.status_hint_line_for_width(120).contains("J/K move"));
    app.select_next_pane();
    assert_eq!(app.selected_pane_id.as_deref(), Some("%2"));
}

#[test]
fn movement_footer_labels_match_j_k_behavior_across_panels() {
    fn two_pane_app(runtimes: Vec<(&str, Vec<&str>)>) -> App {
        let mut first = sample_pane("bash");
        first.id = String::from("%1");
        first.window_id = String::from("@1");
        first.window_name = String::from("alpha");

        let mut second = sample_pane("zsh");
        second.id = String::from("%2");
        second.window_id = String::from("@2");
        second.window_name = String::from("beta");
        second.active = false;
        second.pane_index = 1;

        app_with_panes(vec![first, second], runtimes)
    }

    let mut fleet = two_pane_app(vec![]);
    assert!(fleet.status_hint_line_for_width(120).contains("J/K move"));
    fleet.select_next_pane();
    assert_eq!(fleet.selected_pane_id.as_deref(), Some("%2"));

    let mut empty_output = two_pane_app(vec![]);
    empty_output.context_pane = super::ContextPane::Tail;
    empty_output.panel_focus = super::PanelFocus::Details;
    assert!(
        empty_output
            .status_hint_line_for_width(120)
            .contains("Esc back")
    );
    assert!(
        !empty_output
            .status_hint_line_for_width(120)
            .contains("J/K move")
    );
    empty_output.set_status_message_for_test("No output yet.");
    let empty_output_feedback_footer = empty_output.footer_line_for_width(104);
    assert!(empty_output_feedback_footer.contains("Esc back"));
    assert!(!empty_output_feedback_footer.contains("J/K move"));
    assert!(!empty_output_feedback_footer.contains("K older/J newer"));
    empty_output.select_next_pane();
    assert_eq!(empty_output.selected_pane_id.as_deref(), Some("%2"));

    let mut control = two_pane_app(vec![]);
    control.context_pane = super::ContextPane::Control;
    control.panel_focus = super::PanelFocus::Details;
    assert!(control.status_hint_line_for_width(120).contains("J/K move"));
    control.select_next_pane();
    assert_eq!(control.selected_pane_id.as_deref(), Some("%2"));

    let mut scrollable_details = two_pane_app(vec![(
        "%1",
        vec![
            "step 01 plan",
            "step 02 fetch",
            "step 03 build",
            "step 04 test",
            "step 05 lint",
            "step 06 package",
            "step 07 upload",
            "step 08 notify",
            "step 09 verify",
            "step 10 snapshot",
            "step 11 render",
            "step 12 inspect",
            "step 13 patch",
            "step 14 live",
            "step 15 perf",
            "step 16 ci",
            "step 17 audit",
            "step 18 done",
        ],
    )]);
    scrollable_details.panel_focus = super::PanelFocus::Details;
    scrollable_details.observe_details_scroll_viewport(Some(crate::tui::ScrollMetrics {
        content_len: 18,
        viewport_len: 8,
    }));
    assert!(
        scrollable_details
            .status_hint_line_for_width(120)
            .contains("K older/J newer")
    );
    scrollable_details.set_status_message_for_test("Desktop alerts enabled.");
    let scroll_feedback_footer = scrollable_details.footer_line_for_width(104);
    assert!(scroll_feedback_footer.contains("K older/J newer"));
    assert!(!scroll_feedback_footer.contains("J/K move"));
    scrollable_details.select_previous_pane();
    assert_eq!(scrollable_details.selected_pane_id.as_deref(), Some("%1"));
    assert_eq!(scrollable_details.details_scroll, 1);

    let mut browse = two_pane_app(vec![]);
    browse.context_pane = super::ContextPane::Navigator;
    browse.panel_focus = super::PanelFocus::Details;
    browse.selected_window_id = Some(String::from("@1"));
    assert!(
        browse
            .status_hint_line_for_width(120)
            .contains("J/K browse")
    );
    assert!(
        browse
            .status_hint_line_for_width(120)
            .contains("Enter window")
    );
    browse.set_status_message_for_test("Saved fleet `triage`.");
    let roomy_browse_footer = browse.footer_line_for_width(120);
    assert!(roomy_browse_footer.contains("J/K browse"));
    assert!(roomy_browse_footer.contains("Enter window"));
    assert!(!roomy_browse_footer.contains("J/K move"));
    let feedback_browse_footer = browse.footer_line_for_width(96);
    assert!(feedback_browse_footer.contains("J/K browse"));
    assert!(feedback_browse_footer.contains("Enter window"));
    assert!(!feedback_browse_footer.contains("J/K move"));
    browse.select_next_pane();
    assert_eq!(browse.selected_window_id.as_deref(), Some("@2"));
    assert_eq!(browse.selected_pane_id.as_deref(), Some("%1"));
}

#[test]
fn selected_pane_lines_hide_empty_updated_metadata() {
    let pane = sample_pane("bash");
    let app = app_with_panes(vec![pane], vec![]);

    let lines = app.selected_pane_lines();

    assert!(!lines.iter().any(|line| line.starts_with("Updated:")));
}

#[test]
fn selected_pane_lines_hide_none_blockers_from_structured_reports() {
    for (status, expected_state, expected_action) in [
        ("waiting", "State: Waiting   Tool: Codex", "Action: : reply"),
        (
            "idle",
            "State: Idle   Tool: Codex",
            "Action: G show in tmux",
        ),
        (
            "running",
            "State: Running   Tool: Codex",
            "Now: keep working",
        ),
    ] {
        let pane = sample_pane("codex");
        let app = app_with_panes(
            vec![pane],
            vec![(
                "%1",
                vec![match status {
                    "waiting" => "STATUS=waiting | BLOCKER=none | NEXT=continue",
                    "idle" => "STATUS=idle | BLOCKER=none | NEXT=wait",
                    _ => "STATUS=running | BLOCKER=none | NEXT=keep working",
                }],
            )],
        );

        let lines = app.selected_pane_lines();
        let details = lines.join("\n");

        assert!(details.contains(expected_state), "{status}: {details}");
        assert!(details.contains(expected_action), "{status}: {details}");
        assert!(!details.contains("Blocked: none"), "{status}: {details}");
        assert!(!details.contains("Problem: none"), "{status}: {details}");
        assert!(!details.contains("BLOCKER=none"), "{status}: {details}");
    }
}

#[test]
fn selected_pane_lines_do_not_call_attention_state_an_update() {
    let pane = sample_pane("claude");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([
                String::from("Dialog open: Allow command? [y/n]"),
                String::from("Worker request: run cargo test usability_"),
            ]),
            last_output_at: None,
            corpus: String::from(
                "claude dialog open allow command worker request run cargo test usability",
            ),
            partial_line: String::new(),
        },
    );

    let lines = app.selected_pane_lines();

    assert!(lines.iter().any(|line| line.contains("State: Waiting")));
    assert!(lines.iter().any(|line| line == "Action: . answer yes/no"));
    assert!(!lines.iter().any(|line| line == "Updated: awaiting input"));
    assert!(!lines.iter().any(|line| line.starts_with("Updated:")));
}

#[test]
fn selected_pane_lines_expose_reply_without_hiding_the_action() {
    let continue_app = app_with_panes(
        vec![sample_pane("codex")],
        vec![("%1", vec!["Press Enter to continue."])],
    );
    let continue_lines = continue_app.selected_pane_lines();

    assert!(
        continue_lines
            .iter()
            .any(|line| line == "Action: A continue"),
        "{continue_lines:?}"
    );
    assert!(
        continue_lines.iter().any(|line| line == "Also: : send"),
        "{continue_lines:?}"
    );

    let answer_app = app_with_panes(
        vec![sample_pane("claude")],
        vec![("%1", vec!["Allow command? [y/n]"])],
    );
    let answer_lines = answer_app.selected_pane_lines();

    assert!(
        answer_lines
            .iter()
            .any(|line| line == "Action: . answer yes/no"),
        "{answer_lines:?}"
    );
    assert!(
        answer_lines
            .iter()
            .any(|line| line == "Also: : send, G show"),
        "{answer_lines:?}"
    );

    let running_app = app_with_panes(
        vec![sample_pane("codex")],
        vec![(
            "%1",
            vec!["STATUS=running | BLOCKER=none | NEXT=write tests"],
        )],
    );
    assert!(
        !running_app
            .selected_pane_lines()
            .iter()
            .any(|line| line.starts_with("Also:")),
        "secondary actions should only appear when they help a waiting pane"
    );
}

#[test]
fn selected_pane_lines_keep_updated_metadata_to_time_not_state() {
    let mut app = app_with_panes(
        vec![sample_pane("claude")],
        vec![("%1", vec!["Allow command? [y/n]"])],
    );
    app.pane_runtime
        .get_mut("%1")
        .expect("runtime should be present")
        .last_output_at = Some(Instant::now());

    let lines = app.selected_pane_lines();
    let updated = lines
        .iter()
        .find(|line| line.starts_with("Updated:"))
        .expect("recent output should show an update age");

    assert!(updated.starts_with("Updated: "), "{lines:?}");
    assert!(
        !updated.contains("awaiting input"),
        "state belongs in State, not timestamp metadata: {lines:?}"
    );
}

#[test]
fn selected_pane_reply_lines_honor_rebound_keys() {
    let mut continue_app = app_with_panes(
        vec![sample_pane("codex")],
        vec![("%1", vec!["Press Enter to continue."])],
    );
    continue_app.ui_settings.keybindings.smart_action = vec![String::from("enter")];
    continue_app.ui_settings.keybindings.command = vec![String::from(";")];
    assert!(
        continue_app
            .selected_pane_lines()
            .iter()
            .any(|line| line == "Action: Enter continue"),
        "{:?}",
        continue_app.selected_pane_lines()
    );
    assert!(
        continue_app
            .selected_pane_lines()
            .iter()
            .any(|line| line == "Also: ; send"),
        "{:?}",
        continue_app.selected_pane_lines()
    );

    let mut answer_app = app_with_panes(
        vec![sample_pane("claude")],
        vec![("%1", vec!["Allow command? [y/n]"])],
    );
    answer_app.ui_settings.keybindings.actions = vec![String::from("m")];
    answer_app.ui_settings.keybindings.command = vec![String::from(";")];
    answer_app.ui_settings.keybindings.jump = vec![String::from("o")];
    answer_app.ui_settings.keybindings.action_send_yes = vec![String::from("1")];
    answer_app.ui_settings.keybindings.action_send_no = vec![String::from("2")];

    assert!(
        answer_app
            .selected_pane_lines()
            .iter()
            .any(|line| line == "Action: M answer yes/no"),
        "{:?}",
        answer_app.selected_pane_lines()
    );
    assert!(
        answer_app
            .selected_pane_lines()
            .iter()
            .any(|line| line == "Also: ; send, O show"),
        "{:?}",
        answer_app.selected_pane_lines()
    );
    assert!(
        answer_app
            .command_lines()
            .iter()
            .any(|line| line == "Action: M answer yes/no"),
        "{:?}",
        answer_app.command_lines()
    );
}

#[test]
fn selected_pane_lines_show_attention_order_as_queue_not_alert_jargon() {
    let lines = app_with_panes(
        vec![sample_pane("claude")],
        vec![("%1", vec!["Waiting for approval. Continue?"])],
    )
    .selected_pane_lines();

    assert!(lines.iter().any(|line| line == "Queue: #1"), "{lines:?}");
    assert!(
        !lines.iter().any(|line| line.starts_with("Alert:")),
        "{lines:?}"
    );
}

#[test]
fn control_lines_use_visible_scope_when_search_has_no_matches() {
    let mut waiting = sample_pane("codex");
    waiting.id = String::from("%1");

    let mut running = sample_pane("claude");
    running.id = String::from("%2");
    running.window_id = String::from("@1");
    running.window_name = String::from("agents-2");
    running.active = false;
    running.pane_index = 1;

    let mut app = app_with_panes(
        vec![waiting, running],
        vec![
            ("%1", vec!["Waiting for approval. Continue?"]),
            ("%2", vec!["thinking..."]),
        ],
    );
    app.search_query = String::from("zzz-no-match");
    app.ensure_selection();

    let lines = app.control_lines();

    assert_eq!(
        lines,
        vec![
            String::from("No matching panes."),
            String::from("Action: backspace show all panes"),
        ]
    );
    assert!(!lines.iter().any(|line| line == "Needs you: none"));
    assert!(!lines.iter().any(|line| line == "Working: none"));
}

#[test]
fn selected_pane_lines_do_not_show_a_hidden_pane_when_search_has_no_matches() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.search_query = String::from("zzz-no-match");
    app.ensure_selection();

    let lines = app.selected_pane_lines();

    assert_eq!(
        lines,
        vec![
            String::from("No matching panes."),
            String::from("Action: backspace show all panes"),
        ]
    );
}

#[test]
fn live_tail_lines_do_not_show_a_hidden_pane_when_search_has_no_matches() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["private hidden output"])]);
    app.search_query = String::from("zzz-no-match");
    app.ensure_selection();

    let lines = app.live_tail_lines();

    assert_eq!(
        lines,
        vec![
            String::from("No matching panes."),
            String::from("Action: backspace show all panes"),
        ]
    );
}

#[test]
fn command_panel_shows_marked_target_preview() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.toggle_selected_mark();
    app.select_next_pane();
    app.toggle_selected_mark();
    app.begin_command_input();
    for ch in "echo {id}".chars() {
        app.push_command_char(ch);
    }

    let lines = app.command_lines();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("send list (2 panes)"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("demo / agents #0 : echo %1"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("demo / agents #1 : echo %2"))
    );
}

#[test]
fn send_list_surfaces_targets_hidden_by_current_view() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.toggle_selected_mark();
    app.select_next_pane();
    app.toggle_selected_mark();
    app.search_query = String::from("alpha");
    app.ensure_selection();

    assert_eq!(
        app.active_target_description(),
        "send list (2 panes, 1 hidden)"
    );
    assert_eq!(app.active_hidden_target_count(), 1);
    assert!(
        app.status_hint_line_for_width(120)
            .contains("send list 2 panes, 1 hidden")
    );
    app.set_status_message_for_test("");
    let tight_footer = app.footer_line_for_width(68);
    assert_eq!(
        tight_footer,
        "? help  J/K move  send list 2 panes, 1 hidden  : send  Space remove"
    );
    assert!(!tight_footer.contains(":..."), "{tight_footer}");

    app.begin_command_input();
    for ch in "echo {id}".chars() {
        app.push_command_char(ch);
    }

    let lines = app.command_lines();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("send list (2 panes, 1 hidden)")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "1 pane hidden by current view"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("demo / beta (hidden) : echo %2")),
        "{lines:?}"
    );
}

#[test]
fn command_input_hides_agent_reports_so_compose_stays_focused() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["working"])]);
    set_pane_report_fields(&mut app, "%1", "running", "none", "write tests");

    let idle_lines = app.command_lines();
    assert!(idle_lines.iter().any(|line| line == "Reports"));

    app.begin_command_input();
    for ch in "echo hi".chars() {
        app.push_command_char(ch);
    }
    let compose_lines = app.command_lines();

    assert!(compose_lines.iter().any(|line| line == "To: demo / agents"));
    assert!(compose_lines.iter().any(|line| line == "Text: echo hi"));
    assert!(compose_lines.iter().any(|line| line == "Preview"));
    assert!(!compose_lines.iter().any(|line| line == "Reports"));
    assert!(
        !compose_lines
            .iter()
            .any(|line| line.contains("running | none | write tests")),
        "{compose_lines:?}"
    );
}

#[test]
fn review_send_surfaces_targets_hidden_by_current_view() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.toggle_selected_mark();
    app.select_next_pane();
    app.toggle_selected_mark();
    app.search_query = String::from("alpha");
    app.begin_command_input();
    for ch in "echo {id}".chars() {
        app.push_command_char(ch);
    }

    tokio::runtime::Runtime::new()
        .expect("runtime should build")
        .block_on(app.submit_command_input())
        .expect("submit should stage");

    assert!(app.has_pending_dispatch());
    assert!(
        app.header_context_line_for_width(120)
            .contains("send to the send list (2 panes, 1 hidden)")
    );

    let lines = app.command_lines();
    assert!(
        lines
            .iter()
            .any(|line| line == "To: the send list (2 panes, 1 hidden)"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "1 pane hidden by current view"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("demo / beta (hidden) echo %2")),
        "{lines:?}"
    );
}

#[test]
fn send_previews_include_hidden_target_example_when_overflowed() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut third = sample_pane("codex");
    third.id = String::from("%3");
    third.window_id = String::from("@3");
    third.window_name = String::from("gamma");
    third.active = false;
    third.pane_index = 2;

    let mut hidden = sample_pane("claude");
    hidden.id = String::from("%4");
    hidden.window_id = String::from("@4");
    hidden.window_name = String::from("zeta");
    hidden.active = false;
    hidden.pane_index = 3;

    let mut app = app_with_panes(vec![first, second, third, hidden], vec![]);
    for pane_id in ["%1", "%2", "%3", "%4"] {
        app.selected_pane_id = Some(String::from(pane_id));
        app.toggle_selected_mark();
    }
    app.search_query = String::from("codex");
    app.begin_command_input();
    for ch in "echo {window}".chars() {
        app.push_command_char(ch);
    }

    let preview = app.command_preview_lines();
    assert!(
        preview
            .iter()
            .any(|line| line.contains("demo / zeta (hidden) : echo zeta")),
        "{preview:?}"
    );
    assert!(
        preview.iter().any(|line| line == "... : 1 more pane"),
        "{preview:?}"
    );

    tokio::runtime::Runtime::new()
        .expect("runtime should build")
        .block_on(app.submit_command_input())
        .expect("submit should stage");

    let review = app.command_lines();
    assert!(review.iter().any(|line| line == "Text: echo {window}"));
    assert!(!review.iter().any(|line| line == "send echo {window}"));
    assert!(
        review
            .iter()
            .any(|line| line == "1 pane hidden by current view"),
        "{review:?}"
    );
    assert!(
        review
            .iter()
            .any(|line| line.contains("demo / zeta (hidden) echo zeta")),
        "{review:?}"
    );
    assert!(
        review.iter().any(|line| line == "  ... 2 more"),
        "{review:?}"
    );
}

#[tokio::test]
async fn command_input_recovers_when_target_disappears_before_submit() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.begin_command_input();
    for ch in "echo hi".chars() {
        app.push_command_char(ch);
    }

    app.snapshot.panes.clear();
    app.selected_pane_id = Some(String::from("%1"));

    assert_eq!(app.command_preview_lines(), vec!["No panes yet."]);

    app.submit_command_input()
        .await
        .expect("missing target should not call tmux");

    assert!(!app.is_command_input_active());
    assert_eq!(app.status_message(), "Select a pane first.");
    assert!(app.recent_commands.is_empty());
}

#[test]
fn send_preview_edges_keep_hidden_and_empty_states_obvious() {
    let app = app_with_panes(Vec::new(), vec![]);

    assert_eq!(
        app.preview_indices_with_hidden::<usize>(&[], 3, |_| false),
        Vec::<usize>::new()
    );
    assert_eq!(
        app.preview_indices_with_hidden(&[1, 2, 3], 0, |_| false),
        Vec::<usize>::new()
    );
    assert_eq!(
        app.preview_indices_with_hidden(&[1, 2, 3, 4], 3, |item| *item == 2),
        vec![0, 1, 2]
    );
    assert_eq!(
        app.preview_indices_with_hidden(&[1, 2, 3, 4], 3, |item| *item == 4),
        vec![0, 1, 3]
    );
}

#[test]
fn begin_group_save_input_closes_the_action_menu() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.toggle_selected_mark();
    app.select_next_pane();
    app.toggle_selected_mark();
    app.open_action_menu();

    app.begin_group_save_input();

    assert!(app.is_group_input_active());
    assert!(!app.is_action_menu_active());
}

#[test]
fn multi_target_command_stages_before_send() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.toggle_selected_mark();
    app.select_next_pane();
    app.toggle_selected_mark();
    app.begin_command_input();
    for ch in "continue".chars() {
        app.push_command_char(ch);
    }

    tokio::runtime::Runtime::new()
        .expect("runtime should build")
        .block_on(app.submit_command_input())
        .expect("submit should succeed");

    assert!(app.has_pending_dispatch());
    assert!(app.status_message().contains("Review send `continue`"));
}

#[test]
fn command_input_labels_enter_send_for_one_target_and_review_for_multiple_targets() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);

    app.begin_command_input();
    assert!(app.header_hint_line_for_width(80).contains("Enter send"));
    assert!(app.status_hint_line_for_width(100).contains("Enter send"));
    assert!(!app.footer_line_for_width(100).contains("Enter review"));

    app.cancel_command_input();
    app.toggle_selected_mark();
    app.select_next_pane();
    app.toggle_selected_mark();
    app.begin_command_input();

    assert!(app.header_hint_line_for_width(80).contains("Enter review"));
    assert!(app.status_hint_line_for_width(100).contains("Enter review"));
    assert!(!app.footer_line_for_width(100).contains("Enter send"));
}

#[test]
fn command_input_labels_free_form_waiting_panes_as_reply_not_generic_send() {
    let pane = sample_pane("claude");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["approval needed"])]);
    remember_command_for_test(&mut app, "cargo test");

    app.begin_command_input();

    assert!(app.command_input_is_reply_context());
    assert_eq!(app.context_panel_title(), "Reply");
    assert_eq!(app.header_context_line(), "Reply to demo / agents.");
    assert!(app.header_hint_line_for_width(80).contains("Enter reply"));
    assert!(app.status_hint_line_for_width(100).contains("Enter reply"));
    assert!(!app.footer_line_for_width(100).contains("Enter send"));
    assert!(!app.command_input_can_repeat_recent());

    let lines = app.command_lines();
    assert!(lines.contains(&String::from("Reply to: demo / agents")));
    assert!(lines.contains(&String::from("Text: _")));
    assert!(!lines.iter().any(|line| line == "Recent"), "{lines:?}");
    assert!(
        !lines.iter().any(|line| line.contains("repeat cargo test")),
        "{lines:?}"
    );

    assert!(app.cancel_command_input());
    assert_eq!(app.status_message(), "Closed Reply.");
}

#[tokio::test]
async fn command_input_reply_submission_stays_reply_and_not_recent_command() {
    let log_path = unique_test_path("reply-submit", ".log");
    let escaped_log_path = log_path.display().to_string().replace('\'', "'\\''");
    let fake_tmux = fake_tmux_script(
        "reply-submit",
        &format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{escaped_log_path}'\nexit 0\n"),
    );
    let pane = sample_pane("claude");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["approval needed"])]);
    remember_command_for_test(&mut app, "cargo test");
    use_fake_tmux_for_test(&mut app, fake_tmux);

    app.begin_command_input();
    assert!(app.command_input_is_reply_context());
    for ch in "ship it".chars() {
        app.push_command_char(ch);
    }

    app.submit_command_input()
        .await
        .expect("reply submit should send through tmux");

    assert!(!app.is_command_input_active());
    assert_eq!(app.status_message(), "Sent reply to demo / agents.");
    assert_eq!(
        app.recent_commands.front().map(String::as_str),
        Some("cargo test")
    );
    assert!(
        !app.recent_commands
            .iter()
            .any(|command| command == "ship it")
    );

    let log = fs::read_to_string(&log_path).expect("fake tmux log should be written");
    assert!(log.contains("send-keys -t %1 -l -- ship it"), "{log}");
    assert!(log.contains("send-keys -t %1 Enter"), "{log}");
    assert!(!log.contains("select-pane"), "{log}");
    let _ = fs::remove_file(log_path);
}

#[test]
fn saving_existing_named_fleet_replaces_it_in_place() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.target_groups = vec![
        super::TargetGroup {
            name: String::from("alpha"),
            members: vec![super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("alpha"),
                pane_index: 0,
            }],
        },
        super::TargetGroup {
            name: String::from("triage"),
            members: vec![super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("old"),
                pane_index: 0,
            }],
        },
        super::TargetGroup {
            name: String::from("zeta"),
            members: vec![super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("zeta"),
                pane_index: 0,
            }],
        },
    ];

    let saved = app.upsert_target_group(super::TargetGroup {
        name: String::from("triage"),
        members: vec![super::PaneLocator {
            session_name: String::from("demo"),
            window_name: String::from("agents"),
            pane_index: 0,
        }],
    });

    assert!(saved);
    assert_eq!(app.target_groups.len(), 3);
    assert_eq!(app.selected_group_index, Some(1));
    assert_eq!(app.active_group_name.as_deref(), Some("triage"));
    assert_eq!(
        app.target_groups
            .iter()
            .filter(|group| group.name == "triage")
            .count(),
        1
    );
    assert_eq!(app.target_groups[1].members[0].window_name, "agents");
}

#[test]
fn saving_and_loading_named_target_group_restores_marked_set() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.toggle_selected_mark();
    app.select_next_pane();
    app.toggle_selected_mark();
    app.begin_group_save_input();
    for ch in "triage".chars() {
        app.push_group_name_char(ch);
    }
    app.submit_group_input();
    app.clear_marked_panes();
    app.load_next_target_group();

    assert_eq!(app.active_group_name.as_deref(), Some("triage"));
    assert_eq!(app.marked_pane_ids.len(), 2);
}

#[test]
fn saved_fleet_picker_chooses_named_fleets_without_cycling_blindly() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.active = false;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.target_groups = vec![
        super::TargetGroup {
            name: String::from("alpha"),
            members: vec![super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("alpha"),
                pane_index: 0,
            }],
        },
        super::TargetGroup {
            name: String::from("beta"),
            members: vec![super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("beta"),
                pane_index: 0,
            }],
        },
    ];

    app.open_fleet_picker();
    assert!(app.is_fleet_picker_active());
    assert_eq!(
        app.fleet_picker_lines(),
        vec![
            String::from("> alpha  1/1 live"),
            String::from("  beta  1/1 live")
        ]
    );

    app.select_next_fleet();
    assert_eq!(
        app.fleet_picker_lines(),
        vec![
            String::from("  alpha  1/1 live"),
            String::from("> beta  1/1 live")
        ]
    );
    app.submit_fleet_picker();

    assert!(!app.is_fleet_picker_active());
    assert_eq!(app.active_group_name.as_deref(), Some("beta"));
    assert!(app.marked_pane_ids.contains("%2"));
    assert_eq!(
        app.status_message(),
        "Loaded fleet `beta` with 1 pane live."
    );
}

#[test]
fn saved_fleet_picker_recovery_edges_stay_obvious() {
    let mut alpha = sample_pane("codex");
    alpha.id = String::from("%1");
    alpha.window_name = String::from("alpha");

    let mut beta = sample_pane("claude");
    beta.id = String::from("%2");
    beta.window_id = String::from("@2");
    beta.window_name = String::from("beta");
    beta.active = false;

    let groups = || {
        vec![
            super::TargetGroup {
                name: String::from("alpha"),
                members: vec![super::PaneLocator {
                    session_name: String::from("demo"),
                    window_name: String::from("alpha"),
                    pane_index: 0,
                }],
            },
            super::TargetGroup {
                name: String::from("beta"),
                members: vec![super::PaneLocator {
                    session_name: String::from("demo"),
                    window_name: String::from("beta"),
                    pane_index: 0,
                }],
            },
        ]
    };

    let mut empty = app_with_panes(vec![alpha.clone()], vec![]);
    empty.open_fleet_picker();
    assert!(!empty.is_fleet_picker_active());
    assert_eq!(empty.status_message(), "No saved fleets.");

    let mut app = app_with_panes(vec![alpha.clone(), beta.clone()], vec![]);
    app.target_groups = groups();
    app.open_fleet_picker();
    app.select_previous_fleet();
    assert_eq!(
        app.fleet_picker_lines(),
        vec![
            String::from("  alpha  1/1 live"),
            String::from("> beta  1/1 live")
        ]
    );
    assert!(app.close_fleet_picker());
    assert!(!app.is_fleet_picker_active());
    assert_eq!(app.status_message(), "Closed Fleets.");
    assert!(!app.close_fleet_picker());

    for exercise_stale_empty in [
        App::select_next_fleet as fn(&mut App),
        App::select_previous_fleet as fn(&mut App),
        App::submit_fleet_picker as fn(&mut App),
        App::delete_fleet_picker_selection as fn(&mut App),
    ] {
        let mut stale = app_with_panes(vec![alpha.clone(), beta.clone()], vec![]);
        stale.target_groups = groups();
        stale.open_fleet_picker();
        assert!(stale.is_fleet_picker_active());
        stale.target_groups.clear();

        exercise_stale_empty(&mut stale);

        assert!(!stale.is_fleet_picker_active());
        assert_eq!(stale.status_message(), "No saved fleets.");
    }
}

#[test]
fn saved_fleet_surfaces_targets_hidden_by_current_view_without_duplicate_name() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.target_groups = vec![super::TargetGroup {
        name: String::from("triage"),
        members: vec![
            super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("alpha"),
                pane_index: 0,
            },
            super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("beta"),
                pane_index: 1,
            },
        ],
    }];

    app.apply_target_group(0);
    app.search_query = String::from("alpha");

    assert_eq!(app.active_group_name.as_deref(), Some("triage"));
    assert_eq!(
        app.active_target_description(),
        "fleet triage (2 panes, 1 hidden)"
    );
    assert_eq!(
        app.hidden_target_note().as_deref(),
        Some("1 pane hidden by current view")
    );

    app.begin_command_input();
    for ch in "echo {window}".chars() {
        app.push_command_char(ch);
    }

    let lines = app.command_lines();
    assert!(
        lines
            .iter()
            .any(|line| line == "To: fleet triage (2 panes, 1 hidden)"),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line == "Text: echo {window}"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "1 pane hidden by current view"),
        "{lines:?}"
    );
    assert_eq!(
        lines.iter().filter(|line| line.contains("triage")).count(),
        1,
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("  demo / beta (hidden) : echo beta")),
        "{lines:?}"
    );
}

#[test]
fn usability_command_panel_keeps_stale_loaded_fleet_visible() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.target_groups = vec![super::TargetGroup {
        name: String::from("triage"),
        members: vec![super::PaneLocator {
            session_name: String::from("demo"),
            window_name: String::from("missing"),
            pane_index: 0,
        }],
    }];

    app.apply_target_group(0);

    assert_eq!(
        app.status_message(),
        "Fleet `triage` loaded, but none of its panes are live right now."
    );
    assert!(!app.using_marked_targets());
    assert_eq!(app.fanout_summary_for_selected(), "fleet triage (0 panes)");
    assert_eq!(app.active_target_description(), "fleet triage (0 panes)");
    assert_eq!(app.summary_target_scope(), "fleet triage");
    assert!(
        app.command_lines()
            .iter()
            .any(|line| line == "fleet triage has no live panes"),
        "{:?}",
        app.command_lines()
    );
    assert!(
        app.command_lines()
            .iter()
            .any(|line| line == "Action: . then L choose fleet"),
        "{:?}",
        app.command_lines()
    );
    assert!(
        !app.command_lines()
            .iter()
            .any(|line| line == "send to demo / agents"),
        "{:?}",
        app.command_lines()
    );

    app.begin_command_input();
    assert!(!app.is_command_input_active());
    assert_eq!(app.status_message(), "Fleet `triage` has no live panes.");

    tokio::runtime::Runtime::new()
        .expect("runtime should build")
        .block_on(app.send_command_text("continue"))
        .expect("stale fleet send should stay local");
    assert_eq!(app.status_message(), "Fleet `triage` has no live panes.");

    tokio::runtime::Runtime::new()
        .expect("runtime should build")
        .block_on(app.perform_smart_action())
        .expect("stale fleet smart action should stay local");
    assert_eq!(
        app.status_message(),
        "Fleet `triage` has no panes ready for Enter."
    );
}

#[test]
fn command_panel_saved_fleets_use_self_explanatory_pane_counts() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.target_groups.push(super::TargetGroup {
        name: String::from("triage"),
        members: vec![
            super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("agents"),
                pane_index: 0,
            },
            super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("review"),
                pane_index: 0,
            },
        ],
    });
    app.target_groups.push(target_group("solo", "agents", 0));
    app.selected_group_index = Some(0);

    let lines = app.command_lines();

    assert!(lines.iter().any(|line| line == "> triage (2 panes)"));
    assert!(lines.iter().any(|line| line == "  solo (1 pane)"));
    assert!(!lines.iter().any(|line| line.contains("(2)")));
    assert!(!lines.iter().any(|line| line.contains("(1)")));
}

#[test]
fn saved_fleet_picker_delete_is_local_and_keeps_the_picker_recoverable() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.target_groups = vec![
        super::TargetGroup {
            name: String::from("alpha"),
            members: vec![super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("agents"),
                pane_index: 0,
            }],
        },
        super::TargetGroup {
            name: String::from("beta"),
            members: vec![super::PaneLocator {
                session_name: String::from("demo"),
                window_name: String::from("missing"),
                pane_index: 0,
            }],
        },
    ];
    app.active_group_name = Some(String::from("alpha"));
    app.selected_group_index = Some(0);

    app.open_fleet_picker();
    app.select_next_fleet();
    app.delete_fleet_picker_selection();

    assert!(app.is_fleet_picker_active());
    assert_eq!(app.status_message(), "Deleted fleet `beta`.");
    assert_eq!(app.active_group_name.as_deref(), Some("alpha"));
    assert_eq!(
        app.fleet_picker_lines(),
        vec![String::from("> alpha  1/1 live current")]
    );

    app.delete_fleet_picker_selection();
    assert!(!app.is_fleet_picker_active());
    assert!(app.target_groups.is_empty());
    assert!(app.active_group_name.is_none());
    assert_eq!(app.status_message(), "Deleted fleet `alpha`.");
}

#[test]
fn inactive_fleet_picker_actions_do_not_mutate_state() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.target_groups = vec![target_group("alpha", "agents", 0)];
    app.active_group_name = Some(String::from("alpha"));
    app.selected_group_index = Some(0);
    app.fleet_picker_index = 7;
    app.status_message = String::from("steady");

    let groups_before = app.target_groups.clone();

    for inactive_action in [
        App::select_next_fleet as fn(&mut App),
        App::select_previous_fleet as fn(&mut App),
        App::submit_fleet_picker as fn(&mut App),
        App::delete_fleet_picker_selection as fn(&mut App),
    ] {
        inactive_action(&mut app);
    }

    assert!(!app.is_fleet_picker_active());
    assert_eq!(app.target_groups, groups_before);
    assert_eq!(app.active_group_name.as_deref(), Some("alpha"));
    assert_eq!(app.selected_group_index, Some(0));
    assert_eq!(app.fleet_picker_index, 7);
    assert_eq!(app.status_message(), "steady");
}

#[test]
fn saved_fleet_picker_previous_from_middle_moves_one_row() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.target_groups = vec![
        target_group("alpha", "agents", 0),
        target_group("beta", "missing", 0),
        target_group("gamma", "ghost", 0),
    ];

    app.open_fleet_picker();
    app.fleet_picker_index = 2;
    app.select_previous_fleet();

    assert_eq!(app.fleet_picker_index, 1);
    assert_eq!(
        app.fleet_picker_lines(),
        vec![
            String::from("  alpha  1/1 live"),
            String::from("> beta  0/1 live"),
            String::from("  gamma  0/1 live")
        ]
    );
    let footer = app.status_hint_line_for_width(100);
    assert!(footer.contains("D delete stale"), "{footer}");
    assert!(
        !footer.contains("Enter load"),
        "stale fleet picker footer must not advertise loading a fleet with no live panes: {footer}"
    );

    app.submit_fleet_picker();

    assert!(app.is_fleet_picker_active());
    assert_eq!(app.status_message(), "Fleet `beta` has no live panes.");
    assert!(app.marked_pane_ids.is_empty());
}

#[test]
fn saved_fleet_picker_delete_preserves_active_fleet_and_surfaces_save_failure() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.target_groups = vec![
        target_group("alpha", "agents", 0),
        target_group("beta", "missing", 0),
        target_group("gamma", "ghost", 0),
    ];
    app.active_group_name = Some(String::from("gamma"));
    app.selected_group_index = Some(2);
    let root = unique_test_path("state-blocked-picker-delete", "");
    fs::write(&root, "not a directory").expect("blocking file should be writable");
    app.state_store = state::Store::new_at(root.join("state.json"));

    app.open_fleet_picker();
    app.fleet_picker_index = 0;
    app.delete_fleet_picker_selection();

    assert!(app.is_fleet_picker_active());
    assert_eq!(
        app.target_groups
            .iter()
            .map(|group| group.name.as_str())
            .collect::<Vec<_>>(),
        vec!["beta", "gamma"]
    );
    assert_eq!(app.active_group_name.as_deref(), Some("gamma"));
    assert_eq!(app.selected_group_index, Some(1));
    assert_eq!(app.fleet_picker_index, 0);
    assert!(
        app.status_message().starts_with("Deleted fleet `alpha`."),
        "{}",
        app.status_message()
    );
    assert!(
        app.status_message().contains("Fleet save failed at "),
        "{}",
        app.status_message()
    );
    let _ = fs::remove_file(root);
}

#[test]
fn saved_fleet_picker_delete_without_active_fleet_stays_local() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.target_groups = vec![
        target_group("alpha", "agents", 0),
        target_group("beta", "missing", 0),
    ];
    app.active_group_name = None;
    app.selected_group_index = None;

    app.open_fleet_picker();
    app.delete_fleet_picker_selection();

    assert!(app.is_fleet_picker_active());
    assert_eq!(
        app.target_groups
            .iter()
            .map(|group| group.name.as_str())
            .collect::<Vec<_>>(),
        vec!["beta"]
    );
    assert!(app.active_group_name.is_none());
    assert!(app.selected_group_index.is_none());
    assert_eq!(app.fleet_picker_index, 0);
    assert_eq!(app.status_message(), "Deleted fleet `alpha`.");
}

#[test]
fn delete_selected_fleet_keeps_selection_on_remaining_fleet() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.target_groups = vec![
        target_group("alpha", "agents", 0),
        target_group("beta", "missing", 0),
        target_group("gamma", "ghost", 0),
    ];
    app.active_group_name = Some(String::from("alpha"));
    app.selected_group_index = Some(1);

    app.delete_selected_target_group();

    assert_eq!(
        app.target_groups
            .iter()
            .map(|group| group.name.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "gamma"]
    );
    assert_eq!(app.active_group_name.as_deref(), Some("alpha"));
    assert_eq!(app.selected_group_index, Some(1));
    assert_eq!(app.status_message(), "Deleted fleet `beta`.");
}

#[test]
fn inactive_input_cancellations_are_noops() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    assert!(!app.cancel_search());
    assert!(!app.cancel_command_input());
    assert!(!app.cancel_group_input());
    assert!(!app.cancel_macro_assign());
    assert!(!app.cancel_pending_dispatch());
    assert!(!app.dismiss_action_menu());

    app.push_search_char('x');
    app.push_command_char('x');
    app.push_group_name_char('x');

    assert!(app.search_query.is_empty());
    assert!(app.command_buffer.is_empty());
    assert!(app.group_name_buffer.is_empty());
}

#[tokio::test]
async fn inactive_text_and_menu_actions_do_not_mutate_hidden_state() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.search_query = String::from("keep-search");
    app.command_buffer = String::from("keep-command");
    app.group_name_buffer = String::from("keep-fleet");

    app.pop_search_char();
    app.pop_command_char();
    app.pop_group_name_char();
    app.submit_group_input();
    app.submit_command_input()
        .await
        .expect("inactive submit should be a no-op");

    assert_eq!(app.search_query, "keep-search");
    assert_eq!(app.command_buffer, "keep-command");
    assert_eq!(app.group_name_buffer, "keep-fleet");
    assert!(!app.close_action_menu());
    assert!(!app.is_action_menu_active());
    assert_eq!(app.status_message(), "");
}

#[test]
fn top_level_presentation_metadata_tracks_modes_and_targets() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    assert_eq!(app.title(), "muxboard");
    assert_eq!(app.help_overlay_title(), "Help");
    assert_eq!(app.tmux_version(), "tmux 3.5a");
    assert_eq!(app.tmux_bin(), "tmux");
    assert_eq!(app.snapshot().pane_count(), 1);
    assert_eq!(
        app.target().display_target(),
        "default socket, default session"
    );
    assert_eq!(app.refresh_count(), 1);

    app.search_query = String::from("agent");
    app.begin_search();
    app.toggle_selected_mark();
    app.begin_group_save_input();
    app.push_group_name_char('x');
    app.cancel_group_input();
    app.remember_command("cargo test");
    app.begin_macro_assign();
    app.toggle_fanout_mode();

    let title = app.panes_title();
    assert!(title.contains("search: agent"));
    assert!(title.contains("pin slot"));
    assert!(title.contains("send list 1 pane"));
    assert!(title.contains("send list"));
    assert!(title.contains("lane send"));
}

#[test]
fn overview_lines_expose_tmux_target_and_snapshot_counts() {
    let pane = sample_pane("codex");
    let app = app_with_panes(vec![pane], vec![]);

    let lines = app.overview_lines();

    assert!(lines.iter().any(|line| line == "tmux version : tmux 3.5a"));
    assert!(lines.iter().any(|line| line == "tmux binary  : tmux"));
    assert!(
        lines
            .iter()
            .any(|line| line == "target       : default socket, default session")
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "attach cmd   : tmux attach-session")
    );
    assert!(lines.iter().any(|line| line == "sessions     : 1"));
    assert!(lines.iter().any(|line| line == "windows      : 1"));
    assert!(lines.iter().any(|line| line == "panes        : 1"));
}

#[test]
fn command_center_action_lines_stay_plain_for_empty_filtered_and_targeted_states() {
    let empty = app_with_panes(Vec::new(), vec![]);
    let lines = empty.control_lines();
    assert_eq!(
        lines,
        vec![
            String::from("No panes yet."),
            String::from("Action: start tmux panes, then R refresh"),
        ]
    );

    let mut no_folder = sample_pane("codex");
    no_folder.current_path.clear();
    let lines = app_with_panes(vec![no_folder], vec![]).control_lines();
    assert_eq!(
        lines.last().map(String::as_str),
        Some("Start: + agent in selected folder")
    );

    let mut root_folder = sample_pane("codex");
    root_folder.current_path = String::from("/");
    let lines = app_with_panes(vec![root_folder], vec![]).control_lines();
    assert_eq!(
        lines.last().map(String::as_str),
        Some("Start: + agent in /")
    );

    let mut filtered = app_with_panes(vec![sample_pane("codex")], vec![]);
    filtered.search_query = String::from("no matches");
    let lines = filtered.control_lines();
    assert_eq!(
        lines,
        vec![
            String::from("No matching panes."),
            String::from("Action: backspace show all panes"),
        ]
    );

    let mut targeted = app_with_panes(vec![sample_pane("codex")], vec![]);
    targeted.toggle_selected_mark();
    let lines = targeted.control_lines();
    assert_eq!(lines[0], "Action: : send to the send list (1 pane)");
    assert!(!lines[0].contains("send send list"));
}

#[test]
fn header_and_footer_cover_all_input_modes_without_extra_explanation() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.pending_dispatch = Some(super::StagedDispatch {
        text: String::from("continue"),
        expanded: vec![(String::from("%1"), String::from("continue"))],
        remember: true,
        target_description: String::from("demo / agents"),
    });
    assert_eq!(
        app.header_hint_line_for_width(120),
        "Enter send  Esc cancel"
    );
    assert_eq!(
        app.header_context_line_for_width(60),
        "Review send to 1 pane."
    );
    app.pending_dispatch = None;

    app.begin_command_input();
    assert_eq!(
        app.header_hint_line_for_width(40),
        "type  Enter send  Esc cancel  backspace delete"
    );
    app.cancel_command_input();

    app.begin_launch_input();
    assert_eq!(
        app.header_hint_line_for_width(40),
        "type  Tab preset  Enter start  Esc cancel  backspace delete"
    );
    assert_eq!(app.header_context_line_for_width(120), "Start agent.");
    app.cancel_launch_input();

    app.toggle_selected_mark();
    app.begin_group_save_input();
    assert_eq!(
        app.header_hint_line_for_width(40),
        "type  Enter save  Esc cancel  backspace delete"
    );
    assert_eq!(
        app.header_context_line_for_width(120),
        "Save this send list as a reusable fleet."
    );
    app.cancel_group_input();

    app.remember_command("cargo test");
    app.begin_macro_assign();
    assert!(
        app.header_hint_line_for_width(120)
            .contains("pin latest command")
    );
    assert_eq!(
        app.footer_line_for_width(80),
        "? help  1/2/3/4/5 pin latest  Esc cancel"
    );
    assert_eq!(
        app.header_context_line_for_width(120),
        "Choose a slot for the latest command."
    );
    app.cancel_macro_assign();

    app.open_action_menu();
    assert_eq!(
        app.header_hint_line_for_width(120),
        "press a listed key  Esc close"
    );
    assert_eq!(app.header_context_line_for_width(120), "More");
    assert_eq!(
        app.footer_line_for_width(60),
        "? help  press a listed key  Esc close"
    );
}

#[test]
fn footer_and_help_copy_honor_rebound_keys() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.ui_settings.keybindings.move_down = vec![String::from("n")];
    app.ui_settings.keybindings.move_up = vec![String::from("p")];
    app.ui_settings.keybindings.command = vec![String::from("c")];
    app.ui_settings.keybindings.search = vec![String::from("f")];
    app.ui_settings.keybindings.actions = vec![String::from("m")];
    app.ui_settings.keybindings.clear_marks = vec![String::from("d")];
    app.ui_settings.keybindings.quit = vec![String::from("z")];

    let footer = app.footer_line_for_width(120);

    for term in ["N/P move", "C send", "F filter", "M more", "Z quit"] {
        assert!(footer.contains(term), "{footer}");
    }
    for stale in ["J/K move", ": send", "/ filter", ". more", "Q quit"] {
        assert!(!footer.contains(stale), "{footer}");
    }

    let help = app.help_lines().join("\n");
    for term in [
        "N/P select panes",
        "C send text",
        "F filter",
        "Views: M then [ browse, ] command center; L layout.",
        "M then + start agent, Z zoom pane.",
        "Z quit muxboard",
    ] {
        assert!(help.contains(term), "{help}");
    }
    for stale in ["J/K select", ": command", "/ filter", ". sort", "Q quit"] {
        assert!(!help.contains(stale), "{help}");
    }

    app.toggle_selected_mark();
    let send_list_footer = app.footer_line_for_width(120);
    for term in [
        "N/P move",
        "Space remove",
        "D clear",
        "C send",
        "M more",
        "Z quit",
    ] {
        assert!(send_list_footer.contains(term), "{send_list_footer}");
    }
    assert!(!send_list_footer.contains("X clear"), "{send_list_footer}");

    app.remember_command("cargo test");
    app.ui_settings.keybindings.macro_slot_1 = vec![String::from("u")];
    app.begin_macro_assign();
    assert_eq!(
        app.footer_line_for_width(80),
        "? help  U/2/3/4/5 pin latest  Esc cancel"
    );
    assert!(!app.footer_line_for_width(80).contains("1-5"));
}

#[test]
fn board_and_pane_titles_surface_scope_targets_review_and_metrics() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![("%2", vec!["Approve? [y/n]"])]);
    app.toggle_selected_mark();
    app.pending_dispatch = Some(super::StagedDispatch {
        text: String::from("continue"),
        expanded: vec![(String::from("%1"), String::from("continue"))],
        remember: true,
        target_description: String::from("send list (1 pane)"),
    });
    app.metrics_mode = MetricsMode::Local;
    app.view_scope = super::ViewScope::Window {
        id: String::from("@1"),
        name: String::from("demo/alpha"),
    };

    let title = app.board_title(8);
    assert!(title.contains("window demo/alpha"));
    assert!(title.contains("send list 1 pane"));
    assert!(title.contains("review 1 pane"));
    assert!(title.contains("send list"));
    assert!(title.contains("pane CPU/mem"));

    let wide = app.board_title_for_width(8, 120);
    assert!(wide.contains("window demo/alpha"));

    let lines = app.pane_lines();
    assert!(lines.iter().any(|line| line.contains("codex demo / alpha")));
}

#[test]
fn command_input_requires_a_visible_target_and_rejects_empty_text() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.search_query = String::from("zzz-no-match");
    app.ensure_selection();

    app.begin_command_input();
    assert!(!app.is_command_input_active());
    assert_eq!(app.status_message(), "Show all panes before sending.");

    let mut filtered = app_with_panes(vec![sample_pane("codex")], vec![]);
    filtered.filter_mode = FilterMode::Attention;
    filtered.ensure_selection();
    assert!(filtered.visible_pane_indices().is_empty());
    filtered.begin_command_input();
    assert!(!filtered.is_command_input_active());
    assert_eq!(filtered.status_message(), "Show all panes before sending.");

    app.search_query.clear();
    app.ensure_selection();
    app.begin_command_input();
    assert!(app.is_command_input_active());

    tokio::runtime::Runtime::new()
        .expect("runtime should build")
        .block_on(app.submit_command_input())
        .expect("empty submit should not call tmux");

    assert!(app.is_command_input_active());
    assert_eq!(app.status_message(), "Send text is empty.");

    let mut stale_marked = app_with_panes(vec![sample_pane("codex")], vec![]);
    stale_marked
        .marked_pane_ids
        .insert(String::from("%missing"));
    stale_marked.begin_command_input();
    assert!(!stale_marked.is_command_input_active());
    assert_eq!(stale_marked.status_message(), "Add a pane before sending.");
}

#[tokio::test]
async fn hidden_selected_pane_actions_recover_instead_of_mutating_hidden_state() {
    let mut app = app_with_panes(
        vec![sample_pane("codex")],
        vec![("%1", vec!["Press Enter to continue"])],
    );
    app.toggle_selected_mark();
    assert!(app.marked_pane_ids.contains("%1"));
    app.set_search_query_for_test("zz-no-match");
    assert!(app.selected_pane_hidden_by_current_view());

    app.focus_selected_pane()
        .await
        .expect("hidden focus should recover in app state");
    assert_eq!(app.context_pane, super::ContextPane::Inspect);
    assert_eq!(
        app.status_message(),
        "Show all panes before opening output."
    );

    app.toggle_selected_mark();
    assert!(app.marked_pane_ids.contains("%1"));
    assert_eq!(
        app.status_message(),
        "Show all panes before changing the send list."
    );

    app.jump_to_selected_pane()
        .await
        .expect("hidden jump should recover in app state");
    assert_eq!(
        app.status_message(),
        "Show all panes before showing a pane in tmux."
    );

    app.toggle_selected_zoom()
        .await
        .expect("hidden zoom should recover in app state");
    assert_eq!(
        app.status_message(),
        "Show all panes before zooming a pane."
    );

    app.begin_launch_input();
    assert!(!app.is_launch_input_active());
    assert_eq!(
        app.status_message(),
        "Show all panes before starting an agent."
    );

    app.toggle_fanout_mode();
    assert_eq!(app.fanout_mode, FanoutMode::Off);
    assert_eq!(
        app.status_message(),
        "Show all panes before sending a lane."
    );

    app.perform_smart_action()
        .await
        .expect("hidden Smart Action should recover in app state");
    assert_eq!(
        app.status_message(),
        "Show all panes before using Smart Action."
    );

    app.send_enter_to_selected()
        .await
        .expect("hidden direct Enter should recover in app state");
    assert_eq!(app.status_message(), "Show all panes before sending Enter.");
}

#[test]
fn macro_assignment_handles_empty_invalid_cancel_and_success_paths() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.begin_macro_assign();
    assert!(!app.is_macro_assign_active());
    assert_eq!(app.status_message(), "No recent command to pin.");

    app.remember_command("cargo test");
    app.begin_macro_assign();
    assert!(app.is_macro_assign_active());
    assert!(app.status_message().contains("Pin `cargo test`"));

    app.assign_recent_command_to_slot(99);
    assert!(!app.is_macro_assign_active());
    assert_eq!(app.status_message(), "Invalid macro slot.");

    app.begin_macro_assign();
    assert!(app.cancel_macro_assign());
    assert!(!app.is_macro_assign_active());
    assert_eq!(app.status_message(), "Closed macro pin mode.");

    app.begin_macro_assign();
    app.assign_recent_command_to_slot(0);
    assert_eq!(app.macro_slots[0], Some(String::from("cargo test")));
    assert_eq!(app.status_message(), "Pinned `cargo test` to slot 1.");
}

#[test]
fn group_save_and_delete_empty_paths_are_explicit() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.begin_group_save_input();
    assert!(!app.is_group_input_active());
    assert_eq!(
        app.status_message(),
        "Build a send list first before saving a fleet."
    );

    app.toggle_selected_mark();
    app.begin_group_save_input();
    assert!(app.is_group_input_active());
    app.submit_group_input();
    assert!(app.is_group_input_active());
    assert_eq!(app.status_message(), "Fleet name is empty.");

    app.push_group_name_char('x');
    app.pop_group_name_char();
    assert!(app.group_name_buffer.is_empty());
    assert!(app.cancel_group_input());
    assert!(!app.is_group_input_active());

    app.delete_selected_target_group();
    assert_eq!(app.status_message(), "No saved fleets.");
}

#[test]
fn launch_input_is_scan_ready_and_prefills_known_agent_tools() {
    let mut codex = sample_pane("node");
    codex.current_path = String::from("/workspace/muxboard");
    let mut app = app_with_panes(vec![codex], vec![("%1", vec!["codex is working"])]);

    app.begin_launch_input();
    assert!(app.is_launch_input_active());
    assert_eq!(app.context_panel_title(), "Start");
    assert_eq!(app.launch_lines()[0], "In: demo / agents");
    assert_eq!(app.launch_lines()[1], "Folder: /workspace/muxboard");
    assert_eq!(app.launch_lines()[2], "Window: codex");
    assert_eq!(app.launch_lines()[3], "Command: codex");
    assert_eq!(
        app.launch_lines()[4],
        "Presets: Tab codex, claude, opencode, bash"
    );
    assert_eq!(
        app.header_hint_line_for_width(120),
        "type command  Tab preset  Enter start  Esc cancel  backspace delete"
    );
    assert_eq!(app.header_context_line_for_width(120), "Start agent.");

    app.cycle_launch_preset(false);
    assert_eq!(app.launch_lines()[3], "Command: bash");
    app.cycle_launch_preset(true);
    assert_eq!(app.launch_lines()[3], "Command: codex");
    app.cycle_launch_preset(true);
    assert_eq!(app.launch_lines()[3], "Command: claude");
    app.cycle_launch_preset(false);
    assert_eq!(app.launch_lines()[3], "Command: codex");

    app.pop_launch_char();
    app.pop_launch_char();
    app.pop_launch_char();
    app.pop_launch_char();
    app.pop_launch_char();
    assert_eq!(app.launch_lines()[2], "Window: agent");
    assert_eq!(app.launch_lines()[3], "Command: _");
    app.cycle_launch_preset(true);
    assert_eq!(app.launch_lines()[3], "Command: codex");
    assert!(app.cancel_launch_input());
    assert!(!app.is_launch_input_active());
    assert_eq!(app.status_message(), "Closed Start.");

    for (command, expected) in [
        ("claude", "claude"),
        ("opencode", "opencode"),
        ("aider", "aider"),
        ("gemini", "gemini"),
        ("zsh", "_"),
    ] {
        let mut pane = sample_pane(command);
        pane.current_path = String::from("/workspace/muxboard");
        let mut app = app_with_panes(vec![pane], vec![]);

        app.begin_launch_input();

        assert_eq!(
            app.launch_lines()[3],
            format!("Command: {expected}"),
            "{command}"
        );
    }
}

#[tokio::test]
async fn launch_input_recovery_paths_stay_obvious_without_tmux() {
    let mut empty = app_with_panes(Vec::new(), vec![]);
    empty.begin_launch_input();
    assert!(!empty.is_launch_input_active());
    assert_eq!(empty.status_message(), "Select a pane first.");

    empty.push_launch_char('x');
    empty.push_launch_char('\n');
    empty.pop_launch_char();
    empty.cycle_launch_preset(true);
    assert!(!empty.cancel_launch_input());
    empty
        .submit_launch_input()
        .await
        .expect("inactive Start should be a safe no-op");
    assert_eq!(empty.status_message(), "Select a pane first.");

    let pane = sample_pane("bash");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.begin_launch_input();
    app.launch_buffer.clear();
    app.submit_launch_input()
        .await
        .expect("empty Start command should stay recoverable");
    assert!(app.is_launch_input_active());
    assert_eq!(
        app.status_message(),
        "Type a command or press Tab for a preset."
    );

    app.launch_buffer = String::from("custom harness");
    app.cycle_launch_preset(false);
    assert_eq!(app.launch_buffer, "bash");

    app.launch_buffer = String::from("codex");
    app.selected_pane_id = None;
    app.submit_launch_input()
        .await
        .expect("missing Start target should stay recoverable");
    assert!(app.is_launch_input_active());
    assert_eq!(app.status_message(), "Select a pane first.");
    assert!(
        app.launch_lines()
            .contains(&String::from("Action: Esc cancel, then choose a pane"))
    );
    assert_eq!(
        app.header_hint_line_for_width(80),
        "Esc cancel, then choose a pane"
    );
    assert_eq!(
        app.status_hint_line_for_width(80),
        "Esc cancel, then choose a pane"
    );
}

#[tokio::test]
async fn launch_agent_window_uses_selected_pane_session_path_and_command() {
    let log_path = unique_test_path("app-launch", ".log");
    let fake_tmux = fake_tmux_script(
        "launch",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
            log_path.display().to_string().replace('\'', "'\\''")
        ),
    );
    let mut pane = sample_pane("bash");
    pane.session_name = String::from("ops");
    pane.window_name = String::from("agents");
    pane.current_path = String::from("/workspace/muxboard");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.probe.target.binary = fake_tmux;

    app.begin_launch_input();
    assert_eq!(app.launch_lines()[3], "Command: _");
    app.push_launch_char('c');
    app.push_launch_char('o');
    app.push_launch_char('d');
    app.push_launch_char('e');
    app.push_launch_char('x');
    app.push_launch_char(' ');
    app.push_launch_char('-');
    app.push_launch_char('-');
    app.push_launch_char('f');
    app.push_launch_char('a');
    app.push_launch_char('s');
    app.push_launch_char('t');
    app.submit_launch_input()
        .await
        .expect("launch should run through tmux");

    assert!(!app.is_launch_input_active());
    assert!(app.status_message().contains("Started `codex --fast`"));
    let recorded = fs::read_to_string(&log_path).expect("launch should be recorded");
    assert!(
        recorded.contains("new-window -d -t ops -n codex -c /workspace/muxboard codex --fast"),
        "{recorded}"
    );
    assert!(
        recorded.contains("list-panes -a -F"),
        "launch should refresh the fleet after creating the window:\n{recorded}"
    );
}

#[tokio::test]
async fn failed_agent_start_stays_open_and_explains_recovery() {
    let fake_tmux = fake_tmux_script(
        "launch-fails",
        "#!/bin/sh\necho 'new-window refused' >&2\nexit 2\n",
    );
    let mut pane = sample_pane("bash");
    pane.current_path = String::from("/workspace/muxboard");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.probe.target.binary = fake_tmux;

    app.begin_launch_input();
    for ch in "codex".chars() {
        app.push_launch_char(ch);
    }
    app.submit_launch_input()
        .await
        .expect("launch failures should stay recoverable in the UI");

    assert!(app.is_launch_input_active());
    assert_eq!(app.launch_lines()[3], "Command: codex");
    assert!(
        app.status_message().starts_with("Start failed:"),
        "{}",
        app.status_message()
    );
    assert!(
        app.launch_lines()
            .iter()
            .any(|line| line.contains("Error:")),
        "{:?}",
        app.launch_lines()
    );
}

#[tokio::test]
async fn start_agent_recovers_when_target_server_disappears() {
    let fake_tmux = fake_tmux_script(
        "launch-no-server",
        "#!/bin/sh\necho 'no server running on /tmp/tmux-501/default' >&2\nexit 1\n",
    );
    let mut pane = sample_pane("bash");
    pane.current_path = String::from("/workspace/muxboard");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.probe.target.binary = fake_tmux;

    app.begin_launch_input();
    for ch in "codex".chars() {
        app.push_launch_char(ch);
    }
    app.submit_launch_input()
        .await
        .expect("target disappearance during Start should stay in the app");

    assert!(!app.is_launch_input_active());
    assert!(
        app.status_message().starts_with("No tmux server found"),
        "{}",
        app.status_message()
    );
    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(app.selected_pane_id.is_none());
}

#[tokio::test]
async fn start_agent_refreshes_when_selected_session_disappears() {
    let fake_tmux = fake_tmux_script(
        "launch-missing-session",
        "#!/bin/sh\nif [ \"$1\" = \"new-window\" ]; then echo \"can't find session: demo\" >&2; exit 1; fi\nif [ \"$1\" = \"list-panes\" ]; then printf '$1\\tdemo\\t@1\\tother\\t%%2\\t0\\t4243\\tother\\tbash\\t/workspace\\t1\\t0\\n'; exit 0; fi\necho \"unexpected $*\" >&2\nexit 1\n",
    );
    let mut pane = sample_pane("bash");
    pane.current_path = String::from("/workspace/muxboard");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.probe.target.binary = fake_tmux;

    app.begin_launch_input();
    for ch in "codex".chars() {
        app.push_launch_char(ch);
    }
    app.submit_launch_input()
        .await
        .expect("missing launch target should refresh the board");

    assert!(!app.is_launch_input_active());
    assert_eq!(
        app.status_message(),
        "Start canceled; pane disappeared. Refreshed panes."
    );
    assert_eq!(app.snapshot().pane_count(), 1);
    assert_eq!(app.selected_pane_id.as_deref(), Some("%2"));
    assert_eq!(app.launch_lines()[0], "In: demo / other");
}

#[test]
fn launch_window_names_are_short_human_and_safe() {
    assert_eq!(launch_window_name("codex --fast"), "codex");
    assert_eq!(launch_window_name("/usr/local/bin/claude --plan"), "claude");
    assert_eq!(launch_window_name("weird!!agent"), "weirdagent");
    assert_eq!(launch_window_name("!!! --flag"), "agent");
    assert_eq!(launch_window_name(""), "agent");
}

#[test]
fn user_visible_edge_copy_helpers_stay_plain() {
    assert_eq!(
        super::no_target_panes_remain_message("Enter", 0),
        "No panes remain for Enter."
    );
    assert_eq!(
        super::no_waiting_panes_remain_message(0),
        "No waiting panes remain ready for Enter."
    );
    assert_eq!(
        super::action_error_status_message(&anyhow::anyhow!("")),
        "Action failed: unknown error."
    );
    assert_eq!(
        super::append_startup_status(String::from("Ready."), String::new()),
        "Ready."
    );

    let lane = |stuck, idle, done| super::AgentLane {
        workload: WorkloadKind::Codex,
        total: stuck + idle + done,
        waiting: 0,
        error: 0,
        stuck,
        running: 0,
        done,
        idle,
        unknown: 0,
        selected: false,
    };

    assert_eq!(super::lane_attention_rank(lane(1, 0, 0)), (2, 0));
    assert_eq!(super::lane_attention_rank(lane(0, 1, 0)), (4, 0));
    assert_eq!(super::lane_attention_rank(lane(0, 0, 0)), (6, 0));
}

#[test]
fn deleting_loaded_group_clears_active_group_when_needed() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.toggle_selected_mark();
    app.begin_group_save_input();
    for ch in "solo".chars() {
        app.push_group_name_char(ch);
    }
    app.submit_group_input();
    app.clear_marked_panes();
    app.load_next_target_group();

    assert_eq!(app.active_group_name.as_deref(), Some("solo"));
    app.delete_selected_target_group();

    assert!(app.target_groups.is_empty());
    assert!(app.active_group_name.is_none());
    assert_eq!(app.status_message(), "Deleted fleet `solo`.");
}

#[test]
fn local_settings_toggles_cycle_through_user_visible_states() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    assert_eq!(app.ui_settings.layout_preset, LayoutPreset::Auto);
    app.cycle_layout_preset();
    assert_eq!(app.ui_settings.layout_preset, LayoutPreset::Horizontal);
    assert_eq!(app.status_message(), "Layout: side by side.");
    assert_eq!(
        app.config_store
            .load_ui_settings()
            .expect("layout should save")
            .layout_preset,
        LayoutPreset::Horizontal
    );
    app.cycle_layout_preset();
    assert_eq!(app.ui_settings.layout_preset, LayoutPreset::Vertical);
    assert_eq!(app.status_message(), "Layout: stacked.");
    app.cycle_layout_preset();
    assert_eq!(app.ui_settings.layout_preset, LayoutPreset::Auto);
    assert_eq!(app.status_message(), "Layout: auto.");

    app.toggle_metrics_mode();
    assert_eq!(app.metrics_mode, MetricsMode::Local);
    assert_eq!(
        app.status_message(),
        "Pane CPU/memory shown for local tmux pane PIDs."
    );
    app.toggle_metrics_mode();
    assert_eq!(app.metrics_mode, MetricsMode::Off);
    assert_eq!(app.status_message(), "Pane CPU/memory hidden.");

    let initial_bell = app.notification_settings.bell_enabled;
    app.toggle_bell_notifications();
    assert_eq!(app.notification_settings.bell_enabled, !initial_bell);
    assert_eq!(app.status_message(), "Terminal bell off.");
    app.toggle_bell_notifications();
    assert_eq!(app.status_message(), "Terminal bell on.");

    let initial_desktop = app.notification_settings.desktop_enabled;
    app.toggle_desktop_notifications();
    assert_eq!(app.notification_settings.desktop_enabled, !initial_desktop);
    assert_eq!(app.status_message(), "Desktop alerts off.");
    app.toggle_desktop_notifications();
    assert_eq!(app.status_message(), "Desktop alerts on.");

    app.open_action_menu();
    assert!(
        app.command_lines()
            .iter()
            .any(|line| line == "  O desktop alerts")
    );
    app.close_action_menu();

    app.notifier =
        notifications::Notifier::with_mode_for_test(notifications::NotificationMode::TerminalOnly);
    app.notification_settings.desktop_enabled = false;
    app.toggle_desktop_notifications();
    assert_eq!(
        app.status_message(),
        "Desktop alerts unavailable here; terminal bell still works."
    );
    app.open_action_menu();
    assert!(
        app.command_lines()
            .iter()
            .any(|line| line == "  O desktop alerts unavailable here")
    );
    app.close_action_menu();

    app.notification_settings.alert_policy = AlertPolicy::AllAttention;
    app.cycle_alert_policy();
    assert_eq!(
        app.notification_settings.alert_policy,
        AlertPolicy::ErrorAndWaiting
    );
    app.cycle_alert_policy();
    assert_eq!(
        app.notification_settings.alert_policy,
        AlertPolicy::ErrorsOnly
    );
    app.cycle_alert_policy();
    assert_eq!(
        app.notification_settings.alert_policy,
        AlertPolicy::AllAttention
    );

    app.notification_settings.debounce_seconds = 0;
    app.cycle_alert_debounce();
    assert_eq!(app.notification_settings.debounce_seconds, 15);
    app.cycle_alert_debounce();
    assert_eq!(app.notification_settings.debounce_seconds, 30);
}

#[test]
fn usability_notification_setting_save_failures_stay_visible() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    let root = unique_test_path("config-blocked", "");
    fs::write(&root, "not a directory").expect("blocking file should be writable");
    app.config_store = crate::config::Store::new_at(root.join("config.json"));

    app.toggle_desktop_notifications();

    assert!(
        app.status_message()
            .starts_with("Notification settings save failed at "),
        "{}",
        app.status_message()
    );
    assert!(!app.status_message().contains("Desktop alerts on"));

    app.toggle_bell_notifications();

    assert!(
        app.status_message()
            .starts_with("Notification settings save failed at "),
        "{}",
        app.status_message()
    );
    assert!(!app.status_message().contains("Terminal bell"));
    let _ = fs::remove_file(root);
}

#[test]
fn usability_layout_setting_save_failures_stay_visible() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    let root = unique_test_path("ui-config-blocked", "");
    fs::write(&root, "not a directory").expect("blocking file should be writable");
    app.config_store = crate::config::Store::new_at(root.join("config.json"));

    app.cycle_layout_preset();

    assert_eq!(app.ui_settings.layout_preset, LayoutPreset::Horizontal);
    assert!(
        app.status_message()
            .starts_with("UI settings save failed at "),
        "{}",
        app.status_message()
    );
    assert!(!app.status_message().contains("Layout:"));
    let _ = fs::remove_file(root);
}

#[test]
fn usability_command_state_save_failures_stay_visible_after_macro_pin() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    let root = unique_test_path("state-blocked-command", "");
    fs::write(&root, "not a directory").expect("blocking file should be writable");
    app.state_store = state::Store::new_at(root.join("state.json"));

    app.remember_command("cargo test");
    app.begin_macro_assign();
    app.assign_recent_command_to_slot(0);

    assert!(
        app.status_message().starts_with("Pinned `cargo test`"),
        "{}",
        app.status_message()
    );
    assert!(
        app.status_message()
            .contains("Command state save failed at "),
        "{}",
        app.status_message()
    );
    assert_eq!(app.macro_slots[0], Some(String::from("cargo test")));
    let _ = fs::remove_file(root);
}

#[tokio::test]
async fn usability_command_state_save_failures_stay_visible_after_send() {
    let fake_tmux = fake_tmux_script("send-save-fails", "#!/bin/sh\nexit 0\n");
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.probe.target.binary = fake_tmux;
    let root = unique_test_path("state-blocked-send", "");
    fs::write(&root, "not a directory").expect("blocking file should be writable");
    app.state_store = state::Store::new_at(root.join("state.json"));

    app.send_command_text("echo hi")
        .await
        .expect("send should still succeed when command history cannot be saved");

    assert!(
        app.status_message().starts_with("Sent command `echo hi`"),
        "{}",
        app.status_message()
    );
    assert!(
        app.status_message()
            .contains("Command state save failed at "),
        "{}",
        app.status_message()
    );
    assert_eq!(
        app.recent_commands.front().map(String::as_str),
        Some("echo hi")
    );
    let _ = fs::remove_file(root);
}

#[tokio::test]
async fn usability_command_state_save_failures_stay_visible_after_review_send() {
    let fake_tmux = fake_tmux_script("review-send-save-fails", "#!/bin/sh\nexit 0\n");
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;
    let mut app = app_with_panes(vec![first, second], vec![]);
    app.probe.target.binary = fake_tmux;
    app.marked_pane_ids
        .extend([String::from("%1"), String::from("%2")]);
    let root = unique_test_path("state-blocked-review-send", "");
    fs::write(&root, "not a directory").expect("blocking file should be writable");
    app.state_store = state::Store::new_at(root.join("state.json"));

    app.send_command_text("echo hi")
        .await
        .expect("multi-target send should stage before saving history");
    assert!(app.has_pending_dispatch());

    app.confirm_pending_dispatch()
        .await
        .expect("confirmed send should still succeed when command history cannot be saved");

    assert!(
        app.status_message()
            .starts_with("Sent `echo hi` to the send list (2 panes)."),
        "{}",
        app.status_message()
    );
    assert!(
        app.status_message()
            .contains("Command state save failed at "),
        "{}",
        app.status_message()
    );
    assert!(
        !app.status_message().contains("send to list"),
        "{}",
        app.status_message()
    );
    assert_eq!(
        app.recent_commands.front().map(String::as_str),
        Some("echo hi")
    );
    let _ = fs::remove_file(root);
}

#[test]
fn usability_fleet_save_failures_stay_visible_after_save_and_delete() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    let root = unique_test_path("state-blocked-fleet", "");
    fs::write(&root, "not a directory").expect("blocking file should be writable");
    app.state_store = state::Store::new_at(root.join("state.json"));

    app.toggle_selected_mark();
    app.begin_group_save_input();
    for ch in "night".chars() {
        app.push_group_name_char(ch);
    }
    app.submit_group_input();

    assert!(
        app.status_message().starts_with("Saved fleet `night`"),
        "{}",
        app.status_message()
    );
    assert!(
        app.status_message().contains("Fleet save failed at "),
        "{}",
        app.status_message()
    );

    app.delete_selected_target_group();

    assert!(
        app.status_message().starts_with("Deleted fleet `night`"),
        "{}",
        app.status_message()
    );
    assert!(
        app.status_message().contains("Fleet save failed at "),
        "{}",
        app.status_message()
    );
    let _ = fs::remove_file(root);
}

#[test]
fn usability_acknowledgement_save_failures_stay_visible_after_mute_actions() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(
        vec![pane],
        vec![("%1", vec!["Waiting for approval. Continue?"])],
    );
    let root = unique_test_path("state-blocked-ack", "");
    fs::write(&root, "not a directory").expect("blocking file should be writable");
    app.state_store = state::Store::new_at(root.join("state.json"));

    app.acknowledge_selected_attention();

    assert!(
        app.status_message().starts_with("Muted alert for "),
        "{}",
        app.status_message()
    );
    assert!(
        app.status_message().contains("State save failed at "),
        "{}",
        app.status_message()
    );

    app.clear_selected_acknowledgement();

    assert!(
        app.status_message().starts_with("Unmuted alert for "),
        "{}",
        app.status_message()
    );
    assert!(
        app.status_message().contains("State save failed at "),
        "{}",
        app.status_message()
    );

    app.acknowledge_all_attention();

    assert!(
        app.status_message().starts_with("Muted 1 alert."),
        "{}",
        app.status_message()
    );
    assert!(
        app.status_message().contains("State save failed at "),
        "{}",
        app.status_message()
    );

    app.clear_all_acknowledgements();

    assert!(
        app.status_message().starts_with("Unmuted "),
        "{}",
        app.status_message()
    );
    assert!(
        app.status_message().contains("State save failed at "),
        "{}",
        app.status_message()
    );
    let _ = fs::remove_file(root);
}

#[test]
fn usability_stale_acknowledgement_save_failures_stay_visible() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    let root = unique_test_path("state-blocked-stale-ack", "");
    fs::write(&root, "not a directory").expect("blocking file should be writable");
    app.state_store = state::Store::new_at(root.join("state.json"));
    app.acknowledged_attention.insert(
        super::AttentionKey {
            session_name: String::from("gone"),
            window_name: String::from("agent"),
            pane_index: 7,
            current_path: String::from("/gone"),
            current_command: String::from("codex"),
            title: String::from("gone"),
        },
        PaneStatus::Waiting,
    );

    app.reconcile_acknowledgements();

    assert!(app.acknowledged_attention.is_empty());
    assert!(
        app.status_message().contains("State save failed at "),
        "{}",
        app.status_message()
    );
    let _ = fs::remove_file(root);
}

#[test]
fn usability_ssh_notification_mode_is_visible_and_never_promises_desktop_delivery() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.notifier = notifications::Notifier::from_env_map(&HashMap::from([
        (String::from("SSH_CONNECTION"), String::from("1 2 3 4")),
        (
            format!("TERM_{}", "PROGRAM"),
            String::from("Apple_Terminal"),
        ),
    ]));

    app.notification_settings.desktop_enabled = false;
    app.toggle_desktop_notifications();

    assert!(app.notification_settings.desktop_enabled);
    assert_eq!(
        app.status_message(),
        "Desktop alerts unavailable on SSH; terminal bell still works."
    );

    app.open_action_menu();
    let (_, lines) = app
        .overlay_panel()
        .expect("action menu overlay should be visible");
    assert!(
        lines
            .iter()
            .any(|line| line.contains("desktop alerts unavailable on SSH")),
        "{lines:?}"
    );
}

#[test]
fn handle_event_updates_local_snapshot_state_without_tmux() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@1");
    second.window_name = String::from("alpha");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);

    assert!(
        !app.handle_event(&crate::tmux::control::Event::WindowPaneChanged {
            window_id: String::from("@1"),
            pane_id: String::from("%2"),
        })
    );
    assert!(!app.snapshot.panes[0].active);
    assert!(app.snapshot.panes[1].active);

    assert!(
        !app.handle_event(&crate::tmux::control::Event::WindowRenamed {
            window_id: String::from("@1"),
            name: String::from("renamed"),
        })
    );
    assert_eq!(app.snapshot.windows[0].name, "renamed");
    assert_eq!(app.snapshot.panes[0].window_name, "renamed");

    assert!(
        !app.handle_event(&crate::tmux::control::Event::SessionRenamed {
            session_id: String::from("$0"),
            name: String::from("prod"),
        })
    );
    assert_eq!(app.snapshot.sessions[0].name, "prod");
    assert_eq!(app.snapshot.windows[0].session_name, "prod");
    assert_eq!(app.snapshot.panes[0].session_name, "prod");

    assert!(
        !app.handle_event(&crate::tmux::control::Event::SessionChanged {
            session_id: String::from("$9"),
            name: String::from("ops"),
        })
    );
    assert_eq!(app.control_state, "connected to $9 (ops)");

    assert!(
        !app.handle_event(&crate::tmux::control::Event::ClientSessionChanged {
            client: String::from("/dev/ttys001"),
            session_id: String::from("$9"),
            name: String::from("ops"),
        })
    );
    assert_eq!(
        app.status_message(),
        "Client /dev/ttys001 switched to $9 (ops)."
    );

    assert!(app.handle_event(&crate::tmux::control::Event::WindowAdd {
        window_id: String::from("@9"),
    }));
    assert!(!app.handle_event(&crate::tmux::control::Event::Exit {
        reason: Some(String::from("status 1")),
    }));
    assert_eq!(app.control_state, "disconnected: status 1");

    assert!(!app.handle_event(&crate::tmux::control::Event::Exit { reason: None }));
    assert_eq!(app.control_state, "disconnected");
}

#[test]
fn handle_event_tracks_dirty_output_and_recent_event_capacity() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    assert!(!app.handle_event(&crate::tmux::control::Event::Output {
        pane_id: String::from("%1"),
        payload: String::from("hello"),
    }));
    assert!(app.dirty_pane_ids.contains("%1"));

    for index in 0..20 {
        app.push_event(format!("event {index}"));
    }

    assert_eq!(app.recent_events.len(), 10);
    assert_eq!(
        app.recent_events.front().map(String::as_str),
        Some("event 19")
    );
    assert_eq!(
        app.recent_events.back().map(String::as_str),
        Some("event 10")
    );
}

#[tokio::test]
async fn tick_drains_control_events_recaptures_dirty_output_and_logs_structure() {
    let fake_tmux = fake_tmux_script(
        "tick-control",
        r#"#!/bin/sh
if [ "$1" = "list-panes" ]; then
  cat <<'EOF'
$0	demo	@0	agents	%1	0	100	workspace	codex	/workspace	1	0
EOF
  exit 0
fi

if [ "$1" = "capture-pane" ]; then
  printf 'STATUS=waiting | BLOCKER=approval | NEXT=approve deploy\n'
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    use_fake_tmux_for_test(&mut app, fake_tmux);
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    tx.send(crate::tmux::control::Event::Output {
        pane_id: String::from("%1"),
        payload: String::from("wake"),
    })
    .await
    .expect("test control event should enqueue");
    tx.send(crate::tmux::control::Event::WindowAdd {
        window_id: String::from("@9"),
    })
    .await
    .expect("test structural event should enqueue");
    drop(tx);
    app.control = Some(crate::tmux::control::Monitor::for_test(
        rx,
        tokio::spawn(async {}),
    ));
    tokio::task::yield_now().await;

    app.tick()
        .await
        .expect("tick should drain control events and recapture dirty panes");

    assert_eq!(app.notification_count, 2);
    assert_eq!(app.control_state, "disconnected");
    assert!(app.dirty_pane_ids.is_empty());
    assert_eq!(
        app.recent_events.front().map(String::as_str),
        Some("window added: @9")
    );

    let details = app.selected_pane_lines().join("\n");
    assert!(details.contains("Blocked: approval"), "{details}");
    assert!(details.contains("Action: : reply"), "{details}");
    assert!(!details.contains("reply for"), "{details}");
    assert!(!details.contains("STATUS="), "{details}");
}

#[test]
fn dirty_pane_capture_is_budgeted_per_tick() {
    let panes = (0..5)
        .map(|index| {
            let mut pane = sample_pane("codex");
            pane.id = format!("%{}", index + 1);
            pane.window_id = format!("@{index}");
            pane.window_name = format!("job-{index}");
            pane
        })
        .collect::<Vec<_>>();
    let mut app = app_with_panes(panes, vec![]);

    for index in 0..5 {
        assert!(!app.handle_event(&crate::tmux::control::Event::Output {
            pane_id: format!("%{}", index + 1),
            payload: String::from("hello"),
        }));
    }

    assert!(app.take_dirty_pane_batch(0).is_empty());
    assert_eq!(app.dirty_pane_ids.len(), 5);

    let first = app.take_dirty_pane_batch(super::DIRTY_CAPTURE_LIMIT_PER_TICK);
    let second = app.take_dirty_pane_batch(super::DIRTY_CAPTURE_LIMIT_PER_TICK);

    assert_eq!(first.len(), super::DIRTY_CAPTURE_LIMIT_PER_TICK);
    assert_eq!(second.len(), super::DIRTY_CAPTURE_LIMIT_PER_TICK);
    assert_eq!(app.dirty_pane_ids.len(), 1);
}

#[test]
fn append_output_captures_latest_agent_report() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.append_output(
        "%1",
        String::from("STATUS=waiting | BLOCKER=user input | NEXT=confirm deploy"),
        None,
    );

    let report = app
        .pane_reports
        .get("%1")
        .expect("pane report should exist");
    assert_eq!(report.status, "waiting");
    assert_eq!(report.blocker, "user input");
    assert_eq!(report.next, "confirm deploy");
}

#[test]
fn append_output_replaces_stale_agent_report_with_new_status_line() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.append_output(
        "%1",
        String::from("STATUS=running | BLOCKER=none | NEXT=old stale action\n"),
        None,
    );
    app.append_output(
        "%1",
        String::from("STATUS=running | BLOCKER=none | NEXT=ship fix\n"),
        None,
    );

    let report = app
        .pane_reports
        .get("%1")
        .expect("pane report should exist");
    assert_eq!(report.status, "running");
    assert_eq!(report.blocker, "none");
    assert_eq!(report.next, "ship fix");
}

#[test]
fn append_output_buffers_partial_chunks_until_newline() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.append_output("%1", String::from("hello"), None);
    let runtime = app.pane_runtime.get("%1").expect("runtime should exist");
    assert!(runtime.output.is_empty());
    assert_eq!(runtime.partial_line, "hello");

    app.append_output("%1", String::from(" world\nsecond line\n"), None);
    assert_eq!(
        app.latest_output_lines("%1", 4),
        vec![String::from("hello world"), String::from("second line")]
    );
}

#[test]
fn latest_output_lines_hide_partial_fragments_but_live_lines_keep_real_prompts() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.append_output("%1", String::from("c"), None);
    assert!(app.latest_output_lines("%1", 4).is_empty());
    assert!(app.latest_live_output_lines("%1", 4).is_empty());

    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.append_output("%1", String::from("Continue? [y/n]"), None);
    assert!(app.latest_output_lines("%1", 4).is_empty());
    assert_eq!(
        app.latest_live_output_lines("%1", 4),
        vec![String::from("Continue? [y/n]")]
    );
}

#[test]
fn append_output_handles_carriage_return_overwrites() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.append_output("%1", String::from("loading"), None);
    app.append_output("%1", String::from("\rready\n"), None);

    assert_eq!(
        app.latest_output_lines("%1", 4),
        vec![String::from("ready")]
    );
}

#[test]
fn append_output_handles_repeated_carriage_return_progress_updates() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);

    app.append_output("%1", String::from("loading"), None);
    app.append_output("%1", String::from("\rthinking"), None);
    app.append_output("%1", String::from("\rready\n"), None);

    assert_eq!(
        app.latest_output_lines("%1", 4),
        vec![String::from("ready")]
    );
}

#[test]
fn live_tail_lines_surface_long_meaningful_partial_progress() {
    let pane = sample_pane("claude");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.context_pane = super::ContextPane::Tail;
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::new(),
            last_output_at: Some(Instant::now()),
            corpus: String::from("tool bash running"),
            partial_line: String::from(
                "Tool Bash running for 300s while compiling a very long dependency graph",
            ),
        },
    );

    let lines = app.live_tail_lines();

    assert!(lines.iter().any(|line| line == "Summary"));
    assert!(lines.iter().any(|line| line == "  wait for Bash"));
    assert!(lines.iter().any(|line| line == "Latest"));
    assert!(
        lines
            .iter()
            .any(|line| { line.contains("Tool Bash running for 300s while compil") })
    );
}

#[test]
fn selected_pane_lines_prefer_structured_status_over_older_waiting_prompt_noise() {
    let pane = sample_pane("node");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec![
                "Waiting for approval. Continue?",
                "STATUS=running | BLOCKER=none | NEXT=write tests",
            ],
        )],
    );

    let lines = app.selected_pane_lines();

    assert!(!lines.iter().any(|line| line == "Agent report"));
    assert!(lines.iter().any(|line| line == "Now: write tests"));
    assert!(!lines.iter().any(|line| line == "Status: waiting"));
}

#[test]
fn board_rows_prefer_structured_provider_status_over_conflicting_wait_prompt() {
    let pane = sample_pane("node");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec![
                "codex",
                "Waiting for approval. Continue?",
                "STATUS=running | BLOCKER=none | NEXT=write tests",
            ],
        )],
    );

    let rows = app.board_rows(8);

    assert_eq!(rows[0].status, "running");
    assert_eq!(rows[0].command, "codex");
    assert_eq!(rows[0].title, "write tests");
}

#[test]
fn board_rows_and_selected_lines_distill_split_protocol_report() {
    let pane = sample_pane("codex");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec![
                "Waiting for approval. Continue?",
                "STATUS=running",
                "BLOCKER=none",
                "NEXT=continue build",
            ],
        )],
    );

    let row = app.board_rows(8).remove(0);
    let details = app.selected_pane_lines().join("\n");
    let combined = format!("{row:?}\n{details}");

    assert_eq!(row.status, "running");
    assert_eq!(row.title, "continue build");
    assert_eq!(row.standard_latest(), "codex: continue build");
    assert!(
        details.contains("State: Running   Tool: Codex"),
        "{details}"
    );
    assert!(details.contains("Now: continue build"), "{details}");
    assert!(!combined.contains("STATUS="), "{combined}");
    assert!(!combined.contains("BLOCKER="), "{combined}");
    assert!(!combined.contains("NEXT="), "{combined}");
}

#[test]
fn board_rows_and_selected_lines_use_fresh_runtime_report_over_stale_stored_report() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec![
                "STATUS=running | BLOCKER=none | NEXT=ship fix",
                "building renderer observability checks",
            ],
        )],
    );
    set_pane_report_fields(&mut app, "%1", "running", "none", "old stale action");

    let rows = app.board_rows(8);
    let details = app.selected_pane_lines();

    assert_eq!(rows[0].status, "running");
    assert_eq!(rows[0].title, "ship fix");
    assert_eq!(rows[0].standard_latest(), "codex: ship fix");
    assert!(details.iter().any(|line| line == "Now: ship fix"));
    assert!(!details.iter().any(|line| line.contains("old stale action")));
}

#[test]
fn board_rows_do_not_surface_summary_template_placeholders() {
    let pane = sample_pane("node");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec![
                "Reply in exactly one line as: STATUS=<status> | BLOCKER=<blocker> | NEXT=<next>.",
                "NEXT=<next>.",
            ],
        )],
    );

    let rows = app.board_rows(8);
    let details = app.selected_pane_lines();

    assert_eq!(rows[0].status, "running");
    assert!(!rows[0].standard_latest().contains("<next>"));
    assert!(!rows[0].standard_latest().contains("NEXT="));
    assert!(!details.iter().any(|line| line.contains("<next>")));
}

#[test]
fn board_rows_do_not_surface_wrapped_summary_template_tail() {
    let pane = sample_pane("codex");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec![
                "Reply in exactly one line as: STATUS=<status> | BLOCKER=<blocker> |",
                "NEXT=<next>.",
                "building renderer tests",
            ],
        )],
    );

    let rows = app.board_rows(8);
    let details = app.selected_pane_lines();

    assert_eq!(rows[0].status, "running");
    assert_eq!(rows[0].standard_latest(), "codex: build renderer tests");
    assert!(!rows[0].standard_latest().contains("<next>"));
    assert!(!details.iter().any(|line| line.contains("<next>")));
}

#[test]
fn board_rows_surface_user_intent_instead_of_agent_scaffolding() {
    let pane = sample_pane("node");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec![
                "Reply in exactly one line as: STATUS=<status> | BLOCKER=<blocker> |",
                "NEXT=<next>.",
                "gpt-5.4 high · ~/Projects/muxboard",
                "› Run /review on my current changes",
            ],
        )],
    );

    let rows = app.board_rows(8);
    let details = app.selected_pane_lines();

    assert_eq!(rows[0].status, "running");
    assert_eq!(rows[0].command, "codex");
    assert_eq!(rows[0].standard_latest(), "codex: review changes");
    assert!(details.iter().any(|line| line == "Now: review changes"));
    assert!(!details.iter().any(|line| line.contains("<next>")));
    assert!(!details.iter().any(|line| line.contains("gpt-5.4")));
}

#[test]
fn board_rows_label_generic_node_with_structured_status_as_agent() {
    let pane = sample_pane("node");
    let app = app_with_panes(
        vec![pane],
        vec![("%1", vec!["STATUS=running | BLOCKER=none | NEXT=ship fix"])],
    );

    let rows = app.board_rows(8);
    let details = app.selected_pane_lines();

    assert_eq!(rows[0].status, "running");
    assert_eq!(rows[0].command, "agent");
    assert_eq!(rows[0].standard_latest(), "ship fix");
    assert!(
        details
            .iter()
            .any(|line| line == "State: Running   Tool: Agent")
    );
}

#[test]
fn board_rows_treat_nonstandard_structured_status_wrappers_as_agents() {
    let pane = sample_pane("runner");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec!["STATUS=waiting | BLOCKER=approval | NEXT=approve"],
        )],
    );

    let rows = app.board_rows(8);
    let details = app.selected_pane_lines();

    assert_eq!(rows[0].status, "waiting");
    assert_eq!(rows[0].standard_latest(), "approval");
    assert!(
        details
            .iter()
            .any(|line| line == "State: Waiting   Tool: Agent"),
        "{details:?}"
    );
    assert!(!details.iter().any(|line| line.contains("Tool: Job")));
}

#[test]
fn generic_harness_provider_events_keep_source_identity_in_fleet_and_details() {
    let pane = sample_pane("python");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec![
                "{\"type\":\"question.asked\",\"properties\":{\"questions\":[{\"question\":\"Pick a target\"}]}}",
            ],
        )],
    );

    let rows = app.board_rows(8);
    let details = app.selected_pane_lines();

    assert_eq!(rows[0].status, "waiting");
    assert_eq!(rows[0].command, "opencode");
    assert_eq!(rows[0].standard_latest(), "input needed");
    assert!(
        details
            .iter()
            .any(|line| line == "State: Waiting   Tool: Opencode"),
        "{details:?}"
    );
    assert!(details.iter().any(|line| line == "Action: : reply"));
    assert!(!details.iter().any(|line| line.contains("Tool: Job")));
}

#[test]
fn explicit_tmux_agent_events_override_generic_pane_detection() {
    let mut pane = sample_pane("node");
    pane.agent_event = Some(crate::tmux::AgentBridgeEvent {
        agent: String::from("codex"),
        state: String::from("waiting"),
        summary: String::from("approval needed"),
        updated_at_unix_ms: Some(1_710_000_000_000),
        ..crate::tmux::AgentBridgeEvent::default()
    });
    let app = app_with_panes(vec![pane], vec![("%1", vec!["plain node harness output"])]);

    let insight = app.pane_insight(app.selected_pane().expect("pane should be selected"));
    let rows = app.board_rows(8);
    let details = app.selected_pane_lines();

    assert_eq!(insight.workload, WorkloadKind::Codex);
    assert_eq!(insight.status, PaneStatus::Waiting);
    assert_eq!(rows[0].command, "codex");
    assert_eq!(rows[0].status, "waiting");
    assert_eq!(rows[0].standard_latest(), "approval needed");
    assert!(
        details
            .iter()
            .any(|line| line == "State: Waiting   Tool: Codex"),
        "{details:?}"
    );
    assert!(
        details
            .iter()
            .any(|line| line == "Blocked: approval needed"),
        "{details:?}"
    );
}

#[test]
fn explicit_done_agent_events_become_review_attention_until_muted() {
    let mut pane = sample_pane("node");
    pane.agent_event = Some(crate::tmux::AgentBridgeEvent {
        agent: String::from("codex"),
        state: String::from("done"),
        summary: String::from("release ready"),
        thread_name: Some(String::from("Ship V1")),
        progress: Some(String::from("10/10 tests")),
        unseen: Some(true),
        ..crate::tmux::AgentBridgeEvent::default()
    });
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["plain node harness output"])]);

    let rows = app.board_rows(8);
    let details = app.selected_pane_lines();

    assert_eq!(app.attention_queue_len(), 1);
    assert_eq!(rows[0].status, "done");
    assert_eq!(rows[0].attention, "!");
    assert_eq!(rows[0].lifecycle, "review");
    assert_eq!(
        rows[0].standard_latest(),
        "codex: release ready · 10/10 tests · Ship V1"
    );
    assert!(
        details
            .iter()
            .any(|line| line == "State: Done   Tool: Codex"),
        "{details:?}"
    );
    assert!(
        details.iter().any(|line| line == "Queue: #1"),
        "{details:?}"
    );
    assert!(
        app.attention_queue_lines()[0].contains("review"),
        "{:?}",
        app.attention_queue_lines()
    );

    app.acknowledge_selected_attention();

    assert_eq!(app.attention_queue_len(), 0);
    assert_eq!(app.board_rows(8)[0].attention, "~");
}

#[test]
fn explicit_seen_terminal_agent_events_do_not_flash_attention() {
    let mut pane = sample_pane("node");
    pane.agent_event = Some(crate::tmux::AgentBridgeEvent {
        agent: String::from("codex"),
        state: String::from("done"),
        summary: String::from("already reviewed"),
        unseen: Some(false),
        ..crate::tmux::AgentBridgeEvent::default()
    });
    let app = app_with_panes(vec![pane], vec![("%1", vec!["plain node harness output"])]);
    let rows = app.board_rows(8);

    assert_eq!(app.attention_queue_len(), 0);
    assert_eq!(rows[0].status, "done");
    assert_eq!(rows[0].attention, " ");
    assert_eq!(rows[0].lifecycle, "done");
}

#[test]
fn native_agent_sources_enrich_obvious_matching_panes_without_scraping_prompt_text() {
    let mut pane = sample_pane("node");
    pane.title = String::from("Codex");
    pane.current_path = String::from("/workspace/muxboard");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["plain terminal frame"])]);

    let changed = app.apply_native_agent_source_events(
        vec![AgentSourceEvent {
            provider: AgentSourceProvider::Codex,
            cwd: Some(PathBuf::from("/workspace/muxboard")),
            encoded_cwd: None,
            status: PaneStatus::Done,
            thread_id: Some(String::from("turn-123")),
            thread_name: Some(String::from("Ship review attention")),
            summary: String::from("complete"),
            progress: None,
            log: Some(String::from("native transcript completed")),
            updated_at_unix_ms: 10,
        }],
        true,
    );

    assert!(changed);
    let insight = app.pane_insight(app.selected_pane().expect("pane should be selected"));
    let rows = app.board_rows(8);
    let details = app.selected_pane_lines();

    assert_eq!(insight.workload, WorkloadKind::Codex);
    assert_eq!(insight.status, PaneStatus::Done);
    assert_eq!(app.attention_queue_len(), 0);
    assert_eq!(rows[0].command, "codex");
    assert_eq!(
        rows[0].standard_latest(),
        "codex: complete · Ship review attention"
    );
    assert!(
        details
            .iter()
            .any(|line| line == "State: Done   Tool: Codex"),
        "{details:?}"
    );
    assert!(
        !rows[0].standard_latest().contains("plain terminal frame"),
        "{rows:?}"
    );
}

#[test]
fn native_agent_sources_surface_safe_progress_without_repeating_provider_names() {
    let mut pane = sample_pane("node");
    pane.title = String::from("Claude Code");
    pane.current_path = String::from("/workspace/muxboard");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["plain terminal frame"])]);

    app.apply_native_agent_source_events(
        vec![AgentSourceEvent {
            provider: AgentSourceProvider::ClaudeCode,
            cwd: Some(PathBuf::from("/workspace/muxboard")),
            encoded_cwd: None,
            status: PaneStatus::Running,
            thread_id: Some(String::from("claude-session")),
            thread_name: Some(String::from("Improve command center")),
            summary: String::from("working"),
            progress: Some(String::from("tightening Fleet and Details hierarchy")),
            log: Some(String::from("native transcript is active")),
            updated_at_unix_ms: 10,
        }],
        true,
    );

    let rows = app.board_rows(8);
    let details = app.selected_pane_lines().join("\n");

    assert_eq!(
        rows[0].standard_latest(),
        "claude: working · tightening Fleet and Details hierarchy · Improve command center"
    );
    assert!(
        details.contains(
            "Now: working · tightening Fleet and Details hierarchy · Improve command center"
        ),
        "{details}"
    );
    assert!(
        !rows[0]
            .standard_latest()
            .contains("Claude Code · Claude Code")
    );
}

#[test]
fn native_agent_sources_do_not_overwrite_visible_terminal_attention() {
    let mut pane = sample_pane("node");
    pane.title = String::from("Claude Code");
    pane.current_path = String::from("/workspace/muxboard");
    let mut app = app_with_panes(
        vec![pane],
        vec![("%1", vec!["error: command failed while testing"])],
    );

    app.apply_native_agent_source_events(
        vec![AgentSourceEvent {
            provider: AgentSourceProvider::ClaudeCode,
            cwd: Some(PathBuf::from("/workspace/muxboard")),
            encoded_cwd: None,
            status: PaneStatus::Done,
            thread_id: Some(String::from("stale-claude-session")),
            thread_name: Some(String::from("Old completed session")),
            summary: String::from("complete"),
            progress: Some(String::from("stale native transcript")),
            log: Some(String::from("native transcript completed")),
            updated_at_unix_ms: 10,
        }],
        true,
    );

    let rows = app.board_rows(8);
    let details = app.selected_pane_lines().join("\n");

    assert_eq!(rows[0].status, "error");
    assert!(rows[0].standard_latest().contains("command failed"));
    assert!(
        !rows[0].standard_latest().contains("Old completed session"),
        "{rows:?}"
    );
    assert!(details.contains("State: Error"), "{details}");
    assert!(app.snapshot.panes[0].agent_event.is_none());
}

#[test]
fn native_agent_sources_do_not_overwrite_visible_running_agent_output_with_stale_terminal_state() {
    let mut pane = sample_pane("codex");
    pane.current_path = String::from("/workspace/muxboard");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["Codex v0.99", "Thinking"])]);

    app.apply_native_agent_source_events(
        vec![AgentSourceEvent {
            provider: AgentSourceProvider::Codex,
            cwd: Some(PathBuf::from("/workspace/muxboard")),
            encoded_cwd: None,
            status: PaneStatus::Stuck,
            thread_id: Some(String::from("stale-codex-session")),
            thread_name: Some(String::from("Old interrupted session")),
            summary: String::from("interrupted"),
            progress: Some(String::from("stale native transcript")),
            log: Some(String::from("native transcript interrupted")),
            updated_at_unix_ms: 10,
        }],
        true,
    );

    let rows = app.board_rows(8);
    let details = app.selected_pane_lines().join("\n");

    assert_eq!(rows[0].status, "running");
    assert!(rows[0].standard_latest().contains("Thinking"));
    assert!(
        !rows[0]
            .standard_latest()
            .contains("Old interrupted session"),
        "{rows:?}"
    );
    assert!(details.contains("State: Running"), "{details}");
    assert!(app.snapshot.panes[0].agent_event.is_none());
}

#[test]
fn native_agent_sources_do_not_turn_visible_idle_provider_output_into_stale_attention() {
    let mut pane = sample_pane("codex");
    pane.current_path = String::from("/workspace/muxboard");
    let mut app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec![
                "Codex v0.99",
                "Thinking",
                "ready",
                "muxboard: ali@tau:~/Projects/muxboard$",
            ],
        )],
    );
    app.pane_runtime
        .get_mut("%1")
        .expect("runtime should exist")
        .last_output_at = None;

    app.apply_native_agent_source_events(
        vec![AgentSourceEvent {
            provider: AgentSourceProvider::Codex,
            cwd: Some(PathBuf::from("/workspace/muxboard")),
            encoded_cwd: None,
            status: PaneStatus::Stuck,
            thread_id: Some(String::from("stale-codex-session")),
            thread_name: Some(String::from("Old interrupted session")),
            summary: String::from("interrupted"),
            progress: Some(String::from("stale native transcript")),
            log: Some(String::from("native transcript interrupted")),
            updated_at_unix_ms: 10,
        }],
        true,
    );

    let rows = app.board_rows(8);
    let details = app.selected_pane_lines().join("\n");

    assert_eq!(rows[0].status, "idle");
    assert!(
        !rows[0]
            .standard_latest()
            .contains("Old interrupted session"),
        "{rows:?}"
    );
    assert!(details.contains("State: Idle"), "{details}");
    assert!(app.snapshot.panes[0].agent_event.is_none());
}

#[test]
fn native_agent_sources_raise_review_only_after_a_terminal_transition() {
    let mut pane = sample_pane("node");
    pane.title = String::from("Codex");
    pane.current_path = String::from("/workspace/muxboard");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["plain terminal frame"])]);
    let running = AgentSourceEvent {
        provider: AgentSourceProvider::Codex,
        cwd: Some(PathBuf::from("/workspace/muxboard")),
        encoded_cwd: None,
        status: PaneStatus::Running,
        thread_id: Some(String::from("turn-123")),
        thread_name: Some(String::from("Ship review attention")),
        summary: String::from("working"),
        progress: None,
        log: Some(String::from("native transcript is active")),
        updated_at_unix_ms: 10,
    };
    app.apply_native_agent_source_events(vec![running.clone()], true);

    let done = AgentSourceEvent {
        status: PaneStatus::Done,
        summary: String::from("complete"),
        log: Some(String::from("native transcript completed")),
        updated_at_unix_ms: 20,
        ..running
    };
    app.apply_native_agent_source_events(vec![done], false);

    let rows = app.board_rows(8);
    assert_eq!(app.attention_queue_len(), 1);
    assert_eq!(rows[0].status, "done");
    assert_eq!(rows[0].attention, "!");
    assert_eq!(rows[0].lifecycle, "review");
}

#[test]
fn native_agent_sources_raise_review_for_new_terminal_event_observed_after_startup() {
    let mut pane = sample_pane("node");
    pane.title = String::from("Codex");
    pane.current_path = String::from("/workspace/muxboard");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["plain terminal frame"])]);

    app.apply_native_agent_source_events(
        vec![AgentSourceEvent {
            provider: AgentSourceProvider::Codex,
            cwd: Some(PathBuf::from("/workspace/muxboard")),
            encoded_cwd: None,
            status: PaneStatus::Done,
            thread_id: Some(String::from("turn-new")),
            thread_name: Some(String::from("Quick finish")),
            summary: String::from("complete"),
            progress: None,
            log: Some(String::from("native transcript completed")),
            updated_at_unix_ms: 50,
        }],
        false,
    );

    let rows = app.board_rows(8);
    assert_eq!(app.attention_queue_len(), 1);
    assert_eq!(rows[0].attention, "!");
    assert_eq!(rows[0].standard_latest(), "codex: complete · Quick finish");
}

#[test]
fn native_agent_sources_refuse_duplicate_matching_panes_for_the_same_event() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.current_path = String::from("/workspace/muxboard");

    let mut second = sample_pane("codex");
    second.id = String::from("%2");
    second.pane_index = 1;
    second.current_path = String::from("/workspace/muxboard");

    let mut app = app_with_panes(vec![first, second], vec![]);
    let changed = app.apply_native_agent_source_events(
        vec![AgentSourceEvent {
            provider: AgentSourceProvider::Codex,
            cwd: Some(PathBuf::from("/workspace/muxboard")),
            encoded_cwd: None,
            status: PaneStatus::Running,
            thread_id: Some(String::from("turn-ambiguous")),
            thread_name: Some(String::from("Ambiguous source")),
            summary: String::from("working"),
            progress: None,
            log: Some(String::from("native transcript is active")),
            updated_at_unix_ms: 20,
        }],
        false,
    );

    assert!(!changed);
    assert!(
        app.snapshot
            .panes
            .iter()
            .all(|pane| pane.agent_event.is_none())
    );
}

#[test]
fn native_agent_sources_clear_stale_assignments_when_pane_stops_matching() {
    let mut pane = sample_pane("codex");
    pane.current_path = String::from("/workspace/muxboard");
    let mut app = app_with_panes(vec![pane], vec![]);
    let event = AgentSourceEvent {
        provider: AgentSourceProvider::Codex,
        cwd: Some(PathBuf::from("/workspace/muxboard")),
        encoded_cwd: None,
        status: PaneStatus::Running,
        thread_id: Some(String::from("turn-stale")),
        thread_name: Some(String::from("Stale source")),
        summary: String::from("working"),
        progress: None,
        log: Some(String::from("native transcript is active")),
        updated_at_unix_ms: 20,
    };

    assert!(app.apply_native_agent_source_events(vec![event.clone()], true));
    app.snapshot.panes[0].current_command = String::from("zsh");
    app.snapshot.panes[0].title.clear();
    app.snapshot.panes[0].window_name = String::from("shell");

    let changed = app.apply_native_agent_source_events(vec![event], false);

    assert!(!changed);
    assert!(app.snapshot.panes[0].agent_event.is_none());
}

#[test]
fn native_agent_sources_do_not_override_explicit_bridge_or_guess_ambiguous_shells() {
    let mut explicit = sample_pane("codex");
    explicit.id = String::from("%1");
    explicit.current_path = String::from("/workspace/muxboard");
    explicit.agent_event = Some(AgentBridgeEvent {
        agent: String::from("claude"),
        state: String::from("waiting"),
        summary: String::from("explicit bridge wins"),
        ..AgentBridgeEvent::default()
    });

    let mut shell = sample_pane("zsh");
    shell.id = String::from("%2");
    shell.pane_index = 1;
    shell.current_path = String::from("/workspace/muxboard");
    shell.title.clear();
    shell.window_name = String::from("shell");

    let mut app = app_with_panes(vec![explicit, shell], vec![]);
    app.apply_native_agent_source_events(
        vec![AgentSourceEvent {
            provider: AgentSourceProvider::Codex,
            cwd: Some(PathBuf::from("/workspace/muxboard")),
            encoded_cwd: None,
            status: PaneStatus::Done,
            thread_id: Some(String::from("turn-123")),
            thread_name: Some(String::from("Should not apply")),
            summary: String::from("complete"),
            progress: None,
            log: None,
            updated_at_unix_ms: 20,
        }],
        false,
    );

    let explicit = app
        .snapshot
        .panes
        .iter()
        .find(|pane| pane.id == "%1")
        .expect("explicit pane should still exist");
    let shell = app
        .snapshot
        .panes
        .iter()
        .find(|pane| pane.id == "%2")
        .expect("shell pane should still exist");

    assert_eq!(
        explicit
            .agent_event
            .as_ref()
            .expect("explicit event should remain")
            .agent,
        "claude"
    );
    assert!(shell.agent_event.is_none());
}

#[test]
fn board_rows_prefer_direct_continue_for_enter_prompts() {
    let pane = sample_pane("bash");
    let app = app_with_panes(vec![pane], vec![("%1", vec!["Press Enter to continue."])]);

    let rows = app.board_rows(8);

    assert_eq!(rows[0].status, "waiting");
    assert_eq!(rows[0].title, "continue");
}

#[test]
fn board_rows_prefer_specific_progress_over_generic_running_marker() {
    let pane = sample_pane("codex");
    let app = app_with_panes(
        vec![pane],
        vec![("%1", vec!["Running", "building release artifacts"])],
    );

    let rows = app.board_rows(8);

    assert_eq!(rows[0].status, "running");
    assert_eq!(rows[0].title, "build release artifacts");
}

#[test]
fn board_rows_trim_visual_ellipsis_from_running_progress() {
    let pane = sample_pane("bash");
    let app = app_with_panes(vec![pane], vec![("%1", vec!["building..."])]);

    let rows = app.board_rows(8);

    assert_eq!(rows[0].status, "running");
    assert_eq!(rows[0].title, "build");
}

#[test]
fn board_rows_compact_common_progress_phrases() {
    let pane = sample_pane("bash");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec![
                "step one preparing release image",
                "step two syncing artifacts",
                "step three validating checksums",
                "step four notifying staging",
                "step five completed handoff",
            ],
        )],
    );

    let rows = app.board_rows(8);

    assert_eq!(rows[0].status, "running");
    assert_eq!(rows[0].title, "complete handoff");
}

#[test]
fn board_rows_keep_object_when_progress_line_has_trailing_clauses() {
    let pane = sample_pane("bash");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec!["building release artifacts for staging deploys across regions"],
        )],
    );

    let rows = app.board_rows(8);

    assert_eq!(rows[0].status, "running");
    assert_eq!(rows[0].title, "build release artifacts");
}

#[test]
fn board_rows_prefer_signal_noun_phrase_when_object_is_crowded() {
    let pane = sample_pane("bash");
    let app = app_with_panes(
        vec![pane],
        vec![("%1", vec!["completed the final staging handoff"])],
    );

    let rows = app.board_rows(8);

    assert_eq!(rows[0].status, "running");
    assert_eq!(rows[0].title, "complete staging handoff");
}

#[test]
fn board_rows_prefer_high_signal_progress_over_newer_weaker_update() {
    let pane = sample_pane("bash");
    let app = app_with_panes(
        vec![pane],
        vec![("%1", vec!["building release artifacts", "writing logs"])],
    );

    let rows = app.board_rows(8);

    assert_eq!(rows[0].status, "running");
    assert_eq!(rows[0].title, "build release artifacts");
}

#[test]
fn board_rows_prefer_handoff_phase_over_newer_prep_update() {
    let pane = sample_pane("bash");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec!["completed staging handoff", "preparing release image"],
        )],
    );

    let rows = app.board_rows(8);

    assert_eq!(rows[0].status, "running");
    assert_eq!(rows[0].title, "complete staging handoff");
}

#[test]
fn board_rows_prefer_specific_progress_over_resume_event() {
    let pane = sample_pane("opencode");
    let app = app_with_panes(
        vec![pane],
        vec![("%1", vec!["question.replied", "building release artifacts"])],
    );

    let rows = app.board_rows(8);

    assert_eq!(rows[0].status, "running");
    assert_eq!(rows[0].title, "build release artifacts");
}

#[test]
fn board_rows_prefer_answer_prompt_for_yes_no_questions() {
    let pane = sample_pane("bash");
    let app = app_with_panes(vec![pane], vec![("%1", vec!["Continue? [y/n]"])]);

    let rows = app.board_rows(8);

    assert_eq!(rows[0].status, "waiting");
    assert_eq!(rows[0].title, "answer");
}

#[test]
fn selected_pane_lines_show_synthetic_report_for_tool_progress() {
    let pane = sample_pane("claude");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::new(),
            last_output_at: Some(Instant::now()),
            corpus: String::from("tool bash running"),
            partial_line: String::from("Tool Bash running for 3s..."),
        },
    );

    let lines = app.selected_pane_lines();

    assert!(!lines.iter().any(|line| line == "Agent report"));
    assert!(lines.iter().any(|line| line == "Now: wait for Bash"));
}

#[test]
fn malformed_tool_input_lines_do_not_become_visible_tool_names() {
    let pane = sample_pane("claude");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec!["Tool: Input: cargo test", "building release artifacts"],
        )],
    );

    let rows = app.board_rows(8);
    let lines = app.selected_pane_lines();

    assert_eq!(rows[0].standard_latest(), "claude: build release artifacts");
    assert!(!rows[0].standard_latest().contains("Input:"));
    assert!(
        lines
            .iter()
            .any(|line| line == "Now: build release artifacts")
    );
    assert!(!lines.iter().any(|line| line.contains("wait for Input")));
}

#[test]
fn generic_harness_claude_tool_progress_keeps_provider_identity() {
    let pane = sample_pane("python");
    let mut app = app_with_panes(vec![pane.clone()], vec![]);
    let mut runtime = PaneRuntime {
        output: VecDeque::new(),
        last_output_at: Some(Instant::now()),
        corpus: String::new(),
        partial_line: String::from("Tool Bash running for 3s…"),
    };
    runtime.corpus = build_runtime_corpus(&crate::core::ObservedPane::from(&pane), &runtime);
    app.pane_runtime.insert(String::from("%1"), runtime);

    let rows = app.board_rows(8);
    let lines = app.selected_pane_lines();

    assert_eq!(rows[0].status, "running");
    assert_eq!(rows[0].command, "claude");
    assert_eq!(rows[0].standard_latest(), "claude: wait for Bash");
    assert!(
        lines
            .iter()
            .any(|line| line == "State: Running   Tool: Claude Code")
    );
    assert!(lines.iter().any(|line| line == "Now: wait for Bash"));
}

#[test]
fn active_target_report_lines_use_synthetic_wait_reasons() {
    let pane = sample_pane("claude");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([String::from(
                "Waiting for leader to approve network access to api.example.com",
            )]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("waiting for leader to approve network access"),
            partial_line: String::new(),
        },
    );

    let lines = app.active_target_report_lines();

    assert_eq!(lines.len(), 1);
    assert_eq!(
        lines[0],
        "demo / agents: waiting | approval: network access | approve"
    );
    assert!(!lines[0].contains("%1"));
}

#[test]
fn selected_pane_lines_show_codex_pending_init_synthetic_report() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([String::from("Pending init")]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("pending init"),
            partial_line: String::new(),
        },
    );

    let lines = app.selected_pane_lines();

    assert!(!lines.iter().any(|line| line == "Agent report"));
    assert!(lines.iter().any(|line| line == "Now: initialize agent"));
}

#[test]
fn selected_pane_lines_show_codex_error_synthetic_report() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([String::from("Error tool timeout")]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("error tool timeout"),
            partial_line: String::new(),
        },
    );

    let lines = app.selected_pane_lines();

    assert!(!lines.iter().any(|line| line == "Agent report"));
    assert!(lines.iter().any(|line| line == "Problem: tool timeout"));
}

#[test]
fn selected_pane_lines_show_shell_error_report_after_waiting_prompt() {
    let pane = sample_pane("bash");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec!["Waiting for approval. Continue?", "error: command failed"],
        )],
    );

    let lines = app.selected_pane_lines();

    assert!(!lines.iter().any(|line| line == "Agent report"));
    assert!(lines.iter().any(|line| line == "Problem: command failed"));
    assert!(
        !lines
            .iter()
            .any(|line| line == "Problem: error: command failed")
    );
}

#[test]
fn selected_pane_lines_prefer_shell_error_report_over_stale_waiting_report() {
    let pane = sample_pane("bash");
    let mut app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec!["Waiting for approval. Continue?", "error: command failed"],
        )],
    );
    app.pane_reports.insert(
        String::from("%1"),
        crate::core::AgentReport {
            status: String::from("waiting"),
            blocker: String::from("approval"),
            next: String::from("press enter"),
            updated_at: Instant::now(),
        },
    );

    let lines = app.selected_pane_lines();

    assert!(!lines.iter().any(|line| line == "Agent report"));
    assert!(lines.iter().any(|line| line == "Problem: command failed"));
    assert!(
        !lines
            .iter()
            .any(|line| line == "Problem: error: command failed")
    );
}

#[test]
fn selected_pane_lines_put_report_before_latest_for_attention_states() {
    let pane = sample_pane("bash");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec!["Waiting for approval. Continue?", "error: command failed"],
        )],
    );

    let lines = app.selected_pane_lines();
    assert!(!lines.iter().any(|line| line == "Agent report"));
    assert!(lines.iter().any(|line| line == "Output"));
}

#[test]
fn selected_pane_lines_put_report_before_latest_when_report_conflicts_with_done_state() {
    let pane = sample_pane("bash");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec![
                "Waiting for approval. Continue?",
                "error: command failed",
                "done",
            ],
        )],
    );

    let lines = app.selected_pane_lines();
    assert!(!lines.iter().any(|line| line == "Agent report"));
    assert!(lines.iter().any(|line| line == "Output"));
}

#[test]
fn selected_pane_lines_suppress_low_value_report_for_normal_running_state() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec!["building release artifacts", "uploaded manifest to staging"],
        )],
    );
    set_pane_report_fields(&mut app, "%1", "running", "none", "show output");

    let lines = app.selected_pane_lines();
    lines
        .iter()
        .position(|line| line == "Output")
        .expect("output section should stay visible");

    assert!(
        lines
            .iter()
            .any(|line| line == "  uploaded manifest to staging"),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line == "Agent report"),
        "{lines:?}"
    );
    assert!(!lines.iter().any(|line| line == "Action: show output"));
}

#[test]
fn selected_pane_lines_surface_stored_reports_without_a_second_report_section() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec!["uploaded manifest to staging", "Continue? [y/n]"],
        )],
    );
    set_pane_report_fields(
        &mut app,
        "%1",
        "waiting",
        "review approval",
        "press enter after reviewing diff",
    );

    let lines = app.selected_pane_lines();
    let output_index = lines
        .iter()
        .position(|line| line == "Output")
        .expect("real output should stay visible");

    assert!(
        lines.iter().any(|line| line == "Blocked: review approval"),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line == "Action: . answer yes/no"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "  uploaded manifest to staging" || line == "  Continue? [y/n]"),
        "{lines:?}"
    );
    assert!(
        lines[..output_index]
            .iter()
            .any(|line| line.starts_with("Action:")),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line == "Agent report"),
        "{lines:?}"
    );
}

#[test]
fn selected_pane_latest_prefers_distilled_summary_over_raw_status_protocol() {
    let pane = sample_pane("node");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec!["codex", "STATUS=running | BLOCKER=none | NEXT=write tests"],
        )],
    );

    let lines = app.selected_pane_lines();

    assert!(lines.iter().any(|line| line == "Now: write tests"));
    assert!(!lines.iter().any(|line| line == "Output"));
    assert!(!lines.iter().any(|line| line.contains("STATUS=running")));
    assert!(!lines.iter().any(|line| line == "  codex"));
}

#[test]
fn selected_pane_latest_keeps_human_context_after_distilled_summary() {
    let pane = sample_pane("claude");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec!["Waiting for leader to approve network access to api.example.com"],
        )],
    );

    let lines = app.selected_pane_lines();

    let latest_index = lines
        .iter()
        .position(|line| line == "Output")
        .expect("output heading should exist");
    assert!(
        lines[latest_index + 1].contains("Waiting for leader to approve network access"),
        "{lines:?}"
    );
    assert!(!lines.iter().any(|line| line == "  network access"));
}

#[test]
fn selected_pane_latest_dedupes_raw_progress_against_distilled_summary() {
    let pane = sample_pane("bash");
    let app = app_with_panes(vec![pane], vec![("%1", vec!["building release artifacts"])]);

    let lines = app.selected_pane_lines();

    assert!(
        lines
            .iter()
            .any(|line| line == "Now: build release artifacts"),
        "{lines:?}"
    );
    assert!(!lines.iter().any(|line| line == "Output"), "{lines:?}");
    assert!(
        !lines
            .iter()
            .any(|line| line == "  building release artifacts"),
        "{lines:?}"
    );
}

#[test]
fn selected_pane_latest_cleans_visual_ellipsis_but_preserves_prompts() {
    let mut first = sample_pane("bash");
    first.id = String::from("%1");
    let mut second = sample_pane("opencode");
    second.id = String::from("%2");
    second.pane_index = 1;
    let mut app = app_with_panes(
        vec![first, second],
        vec![
            ("%1", vec!["building..."]),
            ("%2", vec!["Type your answer..."]),
        ],
    );

    app.selected_pane_id = Some(String::from("%1"));
    let lines = app.selected_pane_lines();
    assert!(lines.iter().any(|line| line == "Now: build"), "{lines:?}");
    assert!(!lines.iter().any(|line| line == "Output"), "{lines:?}");
    assert!(!lines.iter().any(|line| line == "  building..."));

    app.selected_pane_id = Some(String::from("%2"));
    let lines = app.selected_pane_lines();
    assert!(
        lines.iter().any(|line| line == "  Type your answer..."),
        "{lines:?}"
    );
}

#[test]
fn selected_pane_next_prefers_direct_continue_over_generic_review_request() {
    let pane = sample_pane("bash");
    let app = app_with_panes(vec![pane], vec![("%1", vec!["Press Enter to continue."])]);

    let lines = app.selected_pane_lines();

    assert!(lines.iter().any(|line| line == "Action: A continue"));
    assert!(!lines.iter().any(|line| line == "Action: review request"));
}

#[test]
fn active_target_report_lines_use_opencode_question_events() {
    let pane = sample_pane("opencode");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([String::from("question.asked")]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("question.asked"),
            partial_line: String::new(),
        },
    );

    let lines = app.active_target_report_lines();

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0], "demo / agents: waiting | input needed | answer");
    assert!(!lines[0].contains("%1"));
}

#[test]
fn active_target_report_lines_use_opencode_permission_ui() {
    let pane = sample_pane("opencode");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([String::from("Permission required")]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("permission required"),
            partial_line: String::new(),
        },
    );

    let lines = app.active_target_report_lines();

    assert_eq!(lines.len(), 1);
    assert_eq!(
        lines[0],
        "demo / agents: waiting | approval needed | approve"
    );
    assert!(!lines[0].contains("%1"));
}

#[test]
fn board_rows_show_metrics_when_available() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.metrics_mode = super::MetricsMode::Local;
    app.pane_metrics.insert(
        String::from("%1"),
        metrics::PaneMetrics {
            pid: 4242,
            cpu_percent: 12.5,
            mem_percent: 1.7,
            elapsed: String::from("01:23"),
            command: String::from("codex"),
        },
    );

    let rows = app.board_rows(8);
    assert_eq!(rows[0].cpu, "12.5");
    assert_eq!(rows[0].mem, "1.7");
}

#[test]
fn selected_pane_lines_put_local_metrics_after_output_when_available() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec!["building release artifacts", "uploaded manifest to staging"],
        )],
    );
    app.metrics_mode = super::MetricsMode::Local;
    app.pane_metrics.insert(
        String::from("%1"),
        metrics::PaneMetrics {
            pid: 4242,
            cpu_percent: 12.5,
            mem_percent: 1.7,
            elapsed: String::from("01:23"),
            command: String::from("codex"),
        },
    );

    let lines = app.selected_pane_lines();
    let output_index = lines
        .iter()
        .position(|line| line == "Output")
        .expect("output section should stay visible");
    let metrics_index = lines
        .iter()
        .position(|line| line.starts_with("pane CPU/mem: pid 4242"))
        .expect("metrics line should stay visible");

    assert!(output_index < metrics_index, "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|line| line == "  uploaded manifest to staging"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "pane CPU/mem: pid 4242 | cpu 12.5% | mem 1.7% | 01:23"),
        "{lines:?}"
    );
}

#[test]
fn board_rows_mark_active_targets_and_staged_targets() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.active = false;
    second.pane_index = 1;

    let mut app = app_with_panes(vec![first, second], vec![]);
    app.toggle_selected_mark();
    app.select_next_pane();
    app.toggle_selected_mark();
    app.pending_dispatch = Some(super::StagedDispatch {
        text: String::from("continue"),
        expanded: vec![(String::from("%1"), String::from("continue"))],
        remember: true,
        target_description: String::from("target set (2 panes)"),
    });

    let rows = app.board_rows(8);
    assert!(rows[0].targeted || rows[1].targeted);
    assert!(rows.iter().any(|row| row.staged));
}

#[test]
fn board_rows_keep_the_selected_pane_in_view() {
    let panes = (0..8)
        .map(|index| {
            let mut pane = sample_pane("codex");
            pane.id = format!("%{}", index + 1);
            pane.window_id = String::from("@1");
            pane.pane_index = index;
            pane.active = index == 0;
            pane
        })
        .collect::<Vec<_>>();

    let mut app = app_with_panes(panes, vec![]);
    app.selected_pane_id = Some(String::from("%7"));

    let rows = app.board_rows(4);

    assert_eq!(rows.len(), 4);
    assert!(rows.iter().any(|row| row.selected && row.pane == "%7"));
    assert_eq!(rows[0].pane, "%5");
}

#[test]
fn board_rows_keep_deep_selection_visible_through_state_churn() {
    let panes = (0..8)
        .map(|index| {
            let mut pane = sample_pane(if index % 2 == 0 { "codex" } else { "claude" });
            pane.id = format!("%{}", index + 1);
            pane.window_id = format!("@{}", index + 1);
            pane.window_name = format!("w{}", index + 1);
            pane.pane_index = 0;
            pane.active = index == 0;
            pane
        })
        .collect::<Vec<_>>();
    let mut app = app_with_panes(panes, vec![]);
    app.sort_mode = SortMode::Natural;
    app.selected_pane_id = Some(String::from("%7"));
    app.selected_window_id = Some(String::from("@7"));

    let phase_one = app.board_rows(4);
    assert_eq!(
        app.board_title(4),
        "Board | tmux order | 5-8 / 8 | all quiet"
    );
    assert_eq!(phase_one[0].pane, "%5");
    assert!(phase_one.iter().any(|row| row.selected && row.pane == "%7"));

    app.pane_runtime.insert(
        String::from("%2"),
        PaneRuntime {
            output: VecDeque::from([String::from("Waiting for approval. Continue?")]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("waiting for approval continue"),
            partial_line: String::new(),
        },
    );
    app.pane_runtime.insert(
        String::from("%4"),
        PaneRuntime {
            output: VecDeque::from([String::from("error: command failed")]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("error command failed"),
            partial_line: String::new(),
        },
    );
    app.pane_runtime.insert(
        String::from("%7"),
        PaneRuntime {
            output: VecDeque::from([String::from(
                "STATUS=waiting | BLOCKER=network access | NEXT=approve request",
            )]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("status waiting blocker network access next approve request"),
            partial_line: String::new(),
        },
    );

    let phase_two = app.board_rows(4);
    assert_eq!(
        app.board_title(4),
        "Board | tmux order | 5-8 / 8 | 3 need you"
    );
    assert_eq!(phase_two[0].pane, "%5");
    assert!(phase_two.iter().any(|row| row.selected && row.pane == "%7"));
    assert!(
        phase_two
            .iter()
            .any(|row| row.pane == "%7" && row.title == "network access")
    );
}

#[test]
fn attention_queue_reorders_cleanly_through_multi_pane_state_sequence() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_name = String::from("alpha");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_name = String::from("beta");
    second.active = false;
    second.pane_index = 1;

    let mut third = sample_pane("claude");
    third.id = String::from("%3");
    third.window_name = String::from("gamma");
    third.active = false;
    third.pane_index = 2;

    let mut app = app_with_panes(
        vec![first, second, third],
        vec![
            ("%1", vec!["building..."]),
            ("%2", vec!["Waiting for approval. Continue?"]),
            ("%3", vec!["done"]),
        ],
    );
    app.initialize_pane_status_cache();

    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([String::from("error: command failed")]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("error command failed"),
            partial_line: String::new(),
        },
    );
    app.pane_runtime.insert(
        String::from("%2"),
        PaneRuntime {
            output: VecDeque::from([String::from("Waiting for approval. Continue?")]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("waiting for approval continue"),
            partial_line: String::new(),
        },
    );
    app.pane_runtime.insert(
        String::from("%3"),
        PaneRuntime {
            output: VecDeque::from([String::from("thinking")]),
            last_output_at: Some(Instant::now() - Duration::from_secs(240)),
            corpus: String::from("thinking"),
            partial_line: String::new(),
        },
    );

    app.capture_attention_transitions();

    assert_eq!(
        app.attention_queue_lines(),
        vec![
            String::from("> output demo / alpha: command failed"),
            String::from("  reply to demo / beta: approval needed"),
            String::from("  output demo / gamma: thinking"),
        ]
    );

    app.pane_runtime.insert(
        String::from("%1"),
        PaneRuntime {
            output: VecDeque::from([String::from("done")]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("done"),
            partial_line: String::new(),
        },
    );
    app.pane_runtime.insert(
        String::from("%2"),
        PaneRuntime {
            output: VecDeque::from([String::from("error: network failed")]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("error network failed"),
            partial_line: String::new(),
        },
    );
    app.pane_runtime.insert(
        String::from("%3"),
        PaneRuntime {
            output: VecDeque::from([String::from("Waiting for approval. Continue?")]),
            last_output_at: Some(Instant::now()),
            corpus: String::from("waiting for approval continue"),
            partial_line: String::new(),
        },
    );

    app.capture_attention_transitions();

    assert_eq!(
        app.attention_queue_lines(),
        vec![
            String::from("  output demo / beta: network failed"),
            String::from("  reply to demo / gamma: approval needed"),
        ]
    );
}

#[test]
fn board_rows_show_provider_tool_for_generic_launchers() {
    let pane = sample_pane("node");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec!["codex", "STATUS=running | BLOCKER=none | NEXT=write tests"],
        )],
    );

    let rows = app.board_rows(8);

    assert_eq!(rows[0].command, "codex");
    assert_eq!(rows[0].title, "write tests");
    assert_eq!(rows[0].standard_latest(), "codex: write tests");
    assert_eq!(rows[0].compact_latest(), "codex write tests");
}

#[test]
fn board_rows_do_not_repeat_provider_name_when_latest_already_has_it() {
    let pane = sample_pane("bash");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec![
                "STATUS=running BLOCKER=none NEXT=Review renderer hierarchy",
                "codex applying focused polish step 02",
            ],
        )],
    );

    let rows = app.board_rows(8);

    assert_eq!(rows[0].command, "codex");
    assert_eq!(rows[0].title, "codex applying focused polish step 02");
    assert_eq!(
        rows[0].standard_latest(),
        "codex applying focused polish step 02"
    );
    assert_eq!(
        rows[0].compact_latest(),
        "codex applying focused polish step 02"
    );
}

#[test]
fn board_rows_use_working_directory_identity_for_generic_job_launchers() {
    let mut first = sample_pane("node");
    first.id = String::from("%1");
    first.window_name = String::from("node");
    first.current_path = String::from("/workspace/muxboard");

    let mut second = sample_pane("node");
    second.id = String::from("%2");
    second.window_name = String::from("node");
    second.pane_index = 1;
    second.active = false;
    second.current_path = String::from("/workspace/dotfiles");

    let app = app_with_panes(
        vec![first, second],
        vec![
            ("%1", vec!["building release artifacts"]),
            ("%2", vec!["syncing shell aliases"]),
        ],
    );

    let rows = app.board_rows(8);
    let rendered = rows
        .iter()
        .map(|row| {
            (
                row.location.as_str(),
                row.command.as_str(),
                row.standard_latest(),
            )
        })
        .collect::<Vec<_>>();

    assert!(rendered.contains(&(
        "demo/node#0",
        "muxboard",
        String::from("muxboard: build release artifacts")
    )));
    assert!(rendered.contains(&(
        "demo/node#1",
        "dotfiles",
        String::from("dotfiles: sync shell aliases")
    )));
}

#[test]
fn board_rows_ignore_generic_workspace_identity_for_generic_launchers() {
    let mut pane = sample_pane("bash");
    pane.window_name = String::from("shell");
    pane.current_path = String::from("/workspace");
    pane.title = String::from("workspace");

    let app = app_with_panes(vec![pane], vec![("%1", vec!["running bootstrap"])]);

    let rows = app.board_rows(8);

    assert_eq!(rows[0].command, "bash");
    assert_eq!(rows[0].standard_latest(), "run bootstrap");
}

#[test]
fn board_rows_prioritize_waiting_blocker_over_generic_next_text() {
    let pane = sample_pane("claude");
    let app = app_with_panes(
        vec![pane],
        vec![(
            "%1",
            vec!["STATUS=waiting | BLOCKER=network access | NEXT=approve request"],
        )],
    );

    let rows = app.board_rows(8);

    assert_eq!(rows[0].title, "network access");
    assert_eq!(rows[0].standard_latest(), "network access");
    assert_eq!(rows[0].compact_latest(), "needs you: network access");
}

#[test]
fn board_rows_prioritize_error_detail_over_provider_name() {
    let pane = sample_pane("node");
    let app = app_with_panes(
        vec![pane],
        vec![("%1", vec!["codex", "error: command failed"])],
    );

    let rows = app.board_rows(8);

    assert_eq!(rows[0].command, "codex");
    assert_eq!(rows[0].status, "error");
    assert_eq!(rows[0].standard_latest(), "command failed");
    assert_eq!(rows[0].compact_latest(), "failed: command failed");
}

#[test]
fn board_rows_disambiguate_duplicate_window_locations() {
    let mut first = sample_pane("node");
    first.id = String::from("%1");
    first.pane_index = 0;

    let mut second = sample_pane("node");
    second.id = String::from("%2");
    second.pane_index = 1;
    second.active = false;

    let app = app_with_panes(
        vec![first, second],
        vec![
            (
                "%1",
                vec!["codex", "STATUS=running | BLOCKER=none | NEXT=write tests"],
            ),
            (
                "%2",
                vec![
                    "claude",
                    "STATUS=waiting | BLOCKER=network access | NEXT=approve request",
                ],
            ),
        ],
    );

    let rows = app.board_rows(8);

    let labels = rows
        .iter()
        .map(|row| (row.location.as_str(), row.standard_latest()))
        .collect::<Vec<_>>();
    assert!(labels.contains(&("demo/agents#0", String::from("codex: write tests"))));
    assert!(labels.contains(&("demo/agents#1", String::from("network access"))));
}

#[test]
fn board_rows_keep_clean_location_when_window_is_unique() {
    let pane = sample_pane("codex");
    let app = app_with_panes(vec![pane], vec![]);

    let rows = app.board_rows(8);

    assert_eq!(rows[0].location, "demo/agents");
}

#[test]
fn attention_transition_raises_alert_and_bell() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["building..."])]);
    app.initialize_pane_status_cache();

    let runtime = app
        .pane_runtime
        .get_mut("%1")
        .expect("runtime should exist");
    runtime.output = VecDeque::from([String::from("Waiting for approval. Continue?")]);
    runtime.last_output_at = Some(Instant::now());

    app.capture_attention_transitions();

    assert!(app.take_pending_bell());
    assert_eq!(app.alert_count, 1);
    assert!(app.status_message().contains("moved Running -> Waiting"));
}

#[test]
fn initial_attention_autofocus_promotes_first_discovered_attention_once() {
    let mut first = sample_pane("opencode");
    first.id = String::from("%1");
    first.window_name = String::from("opencode");

    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("claude");
    second.pane_index = 1;
    second.active = false;

    let mut app = app_with_panes(vec![first, second], vec![("%1", vec!["building..."])]);
    app.selected_pane_id = Some(String::from("%1"));
    app.initial_attention_autofocus = true;
    app.initialize_pane_status_cache();

    app.append_output("%2", String::from("error: command failed\n"), None);
    app.capture_attention_transitions();

    assert_eq!(app.selected_pane_id.as_deref(), Some("%2"));

    app.append_output(
        "%1",
        String::from("Waiting for approval. Continue?\n"),
        None,
    );
    app.capture_attention_transitions();

    assert_eq!(
        app.selected_pane_id.as_deref(),
        Some("%2"),
        "autofocus should not keep stealing selection after the first promotion"
    );
}

#[test]
fn large_fleet_presentation_perf_smoke() {
    let panes = (0..250)
        .map(|index| {
            let mut pane = sample_pane(if index % 5 == 0 { "codex" } else { "bash" });
            pane.id = format!("%{}", index + 1);
            pane.session_id = format!("${}", index / 50);
            pane.session_name = format!("s{:02}", index / 50);
            pane.window_id = format!("@{}", index / 10);
            pane.window_name = format!("w{:02}", index / 10);
            pane.pane_index = index % 10;
            pane.active = index == 0;
            pane
        })
        .collect::<Vec<_>>();

    let pane_ids = panes.iter().map(|pane| pane.id.clone()).collect::<Vec<_>>();
    let runtimes = pane_ids
        .iter()
        .enumerate()
        .map(|(index, pane_id)| {
            let lines = match index % 4 {
                0 => vec!["Waiting for approval. Continue?"],
                1 => vec!["error: command failed"],
                2 => vec!["building..."],
                _ => vec!["done"],
            };
            (pane_id.as_str(), lines)
        })
        .collect::<Vec<_>>();

    let mut app = app_with_panes(panes, runtimes);
    app.metrics_mode = super::MetricsMode::Local;
    app.marked_pane_ids
        .extend(["%1", "%2", "%3", "%4", "%5"].into_iter().map(String::from));

    let started = Instant::now();
    let mut total = 0usize;
    for _ in 0..12 {
        total += app.board_rows(18).len();
        total += app.board_title_for_width(18, 120).len();
        total += app.control_lines().len();
        total += app.navigator_lines().len();
        app.select_next_pane();
    }
    let elapsed = started.elapsed();

    assert!(total > 0);
    let threshold = if coverage_instrumented() {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(3)
    };
    assert!(
        elapsed < threshold,
        "large fleet presentation smoke took {:?}",
        elapsed
    );
}

#[test]
fn alert_policy_can_suppress_waiting_alerts() {
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["building..."])]);
    app.notification_settings = NotificationSettings {
        alert_policy: AlertPolicy::ErrorsOnly,
        ..NotificationSettings::default()
    };
    app.initialize_pane_status_cache();

    let runtime = app
        .pane_runtime
        .get_mut("%1")
        .expect("runtime should exist");
    runtime.output = VecDeque::from([String::from("Waiting for approval. Continue?")]);
    runtime.last_output_at = Some(Instant::now());

    app.capture_attention_transitions();

    assert_eq!(app.alert_count, 0);
    assert!(!app.take_pending_bell());
}

#[test]
fn alert_policy_wait_and_error_alerts_on_actionable_states_only() {
    let mut waiting = sample_pane("codex");
    waiting.id = String::from("%1");
    let mut error = sample_pane("claude");
    error.id = String::from("%2");
    error.window_name = String::from("review");
    error.pane_index = 1;
    error.active = false;
    let mut done = sample_pane("opencode");
    done.id = String::from("%3");
    done.window_name = String::from("finished");
    done.pane_index = 2;
    done.active = false;

    let mut app = app_with_panes(
        vec![waiting, error, done],
        vec![
            ("%1", vec!["building..."]),
            ("%2", vec!["building..."]),
            ("%3", vec!["building..."]),
        ],
    );
    app.notification_settings = NotificationSettings {
        alert_policy: AlertPolicy::ErrorAndWaiting,
        ..NotificationSettings::default()
    };
    app.initialize_pane_status_cache();

    app.pane_runtime.get_mut("%1").expect("runtime").output =
        VecDeque::from([String::from("Waiting for approval. Continue?")]);
    app.pane_runtime.get_mut("%2").expect("runtime").output =
        VecDeque::from([String::from("error: command failed")]);
    app.pane_runtime.get_mut("%3").expect("runtime").output =
        VecDeque::from([String::from("done")]);

    app.capture_attention_transitions();

    assert_eq!(app.alert_count, 2);
    assert!(app.take_pending_bell());
    assert!(
        app.status_message().contains("moved Running -> Error")
            || app.status_message().contains("moved Running -> Waiting"),
        "{}",
        app.status_message()
    );
}

#[test]
fn debounce_suppresses_repeat_alerts_for_same_pane() {
    let mut no_debounce = app_with_panes(Vec::new(), vec![]);
    no_debounce.notification_settings.debounce_seconds = 0;
    no_debounce
        .last_alerted_at
        .insert(String::from("%1"), Instant::now());
    assert!(!no_debounce.is_within_alert_debounce("%1"));

    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["building..."])]);
    app.notification_settings.debounce_seconds = 300;
    app.initialize_pane_status_cache();

    let runtime = app
        .pane_runtime
        .get_mut("%1")
        .expect("runtime should exist");
    runtime.output = VecDeque::from([String::from("Waiting for approval. Continue?")]);
    runtime.last_output_at = Some(Instant::now());
    app.capture_attention_transitions();

    app.take_pending_bell();

    let runtime = app
        .pane_runtime
        .get_mut("%1")
        .expect("runtime should exist");
    runtime.output = VecDeque::from([String::from("building...")]);
    runtime.last_output_at = Some(Instant::now());
    app.capture_attention_transitions();

    let runtime = app
        .pane_runtime
        .get_mut("%1")
        .expect("runtime should exist");
    runtime.output = VecDeque::from([String::from("Waiting for approval. Continue?")]);
    runtime.last_output_at = Some(Instant::now());
    app.capture_attention_transitions();

    assert_eq!(app.alert_count, 1);
    assert!(!app.take_pending_bell());
}

#[tokio::test]
async fn app_state_reducers_cover_empty_and_modal_edges_without_tmux() {
    let mut empty = app_with_panes(Vec::new(), vec![]);
    empty.ensure_selection();
    assert!(empty.selected_pane_id.is_none());

    empty.request_quit();
    assert!(empty.should_quit());
    empty.toggle_help_overlay();
    assert!(empty.is_help_overlay_active());
    assert!(empty.close_help_overlay());
    assert!(!empty.close_help_overlay());

    empty.select_next_pane();
    empty.select_previous_pane();
    assert!(empty.selected_pane_id.is_none());

    empty.cycle_context_pane();
    assert_eq!(empty.status_message(), "");
    empty.view_scope = super::ViewScope::Window {
        id: String::from("@missing"),
        name: String::from("demo/missing"),
    };
    empty.clear_view_scope();
    assert_eq!(empty.view_scope, super::ViewScope::All);
    empty.clear_view_scope();
    assert!(empty.status_message().contains("Already"));

    empty.begin_command_input();
    assert_eq!(empty.status_message(), "Select a pane first.");
    empty.command_input_active = true;
    empty.push_command_char('\n');
    empty.push_command_char('x');
    empty.pop_command_char();
    assert!(empty.command_buffer.is_empty());
    empty
        .submit_command_input()
        .await
        .expect("empty command should not call tmux");
    assert_eq!(empty.status_message(), "Send text is empty.");
    empty.command_input_active = false;
    assert!(!empty.cancel_command_input());

    assert!(!empty.cancel_pending_dispatch());
    empty
        .confirm_pending_dispatch()
        .await
        .expect("missing pending dispatch should be a no-op");
    assert!(!empty.cancel_macro_assign());
    empty.begin_macro_assign();
    assert_eq!(empty.status_message(), "No recent command to pin.");
    empty.assign_recent_command_to_slot(99);
    assert_eq!(empty.status_message(), "No recent command to pin.");
    empty
        .run_macro_slot(0)
        .await
        .expect("empty macro should not call tmux");
    assert_eq!(empty.status_message(), "Macro slot 1 is empty.");
    empty
        .repeat_last_command()
        .await
        .expect("missing recent command should not call tmux");
    assert_eq!(empty.status_message(), "No recent command to replay.");

    empty.begin_group_save_input();
    assert!(empty.status_message().contains("Build a send list"));
    assert!(!empty.cancel_group_input());
    empty.load_next_target_group();
    assert_eq!(empty.status_message(), "No saved fleets.");
    empty.delete_selected_target_group();
    assert_eq!(empty.status_message(), "No saved fleets.");
    empty.toggle_selected_mark();
    assert_eq!(empty.status_message(), "Select a pane first.");
    empty.clear_marked_panes();
    assert_eq!(empty.status_message(), "The send list is already clear.");
    empty.toggle_fanout_mode();
    assert!(empty.status_message().contains("Select an agent pane"));
    empty
        .request_target_summaries()
        .await
        .expect("no target summary should not call tmux");
    assert!(empty.status_message().contains("No panes available"));
    empty
        .focus_selected_pane()
        .await
        .expect("missing selection should not call tmux");
    assert_eq!(empty.status_message(), "Select a pane first.");
    empty
        .jump_to_selected_pane()
        .await
        .expect("missing selection should not call tmux");
    assert_eq!(empty.status_message(), "Select a pane first.");
    empty.acknowledge_selected_attention();
    assert_eq!(empty.status_message(), "Select a pane first.");
    empty.clear_selected_acknowledgement();
    assert_eq!(empty.status_message(), "Select a pane first.");
    empty.acknowledge_all_attention();
    assert_eq!(empty.status_message(), "No new alerts to mute.");
    empty.clear_all_acknowledgements();
    assert_eq!(empty.status_message(), "No muted alerts to clear.");
    empty
        .send_enter_to_attention_queue()
        .await
        .expect("empty attention queue should not call tmux");
    assert_eq!(
        empty.status_message(),
        "No waiting panes are ready for Enter."
    );
}

#[test]
fn app_state_reducers_cover_navigation_groups_and_settings_edges() {
    let mut shell = sample_pane("zsh");
    shell.id = String::from("%1");
    shell.window_id = String::from("@1");
    shell.window_name = String::from("shell");
    let mut codex = sample_pane("codex");
    codex.id = String::from("%2");
    codex.window_id = String::from("@2");
    codex.window_name = String::from("agents");
    codex.pane_index = 1;
    let mut app = app_with_panes(
        vec![shell, codex],
        vec![("%2", vec!["Waiting for approval. Continue?"])],
    );

    app.selected_pane_id = Some(String::from("%2"));
    app.toggle_fanout_mode();
    assert_eq!(app.fanout_mode, FanoutMode::Lane);
    assert!(app.fanout_summary_for_selected().contains("send to"));
    app.toggle_fanout_mode();
    assert_eq!(app.fanout_mode, FanoutMode::Off);

    app.toggle_selected_mark();
    assert!(app.using_marked_targets());
    assert!(app.is_in_active_target_set("%2"));
    assert!(app.fanout_summary_for_selected().contains("send list"));
    app.command_buffer = String::from("inspect {session}/{window}/{id} {lane}");
    let preview = app.command_preview_lines();
    assert!(preview.join("\n").contains("inspect demo/agents/%2"));
    app.toggle_selected_mark();
    assert!(!app.using_marked_targets());

    assert!(app.upsert_target_group(super::TargetGroup {
        name: String::from("team"),
        members: vec![super::PaneLocator {
            session_name: String::from("demo"),
            window_name: String::from("agents"),
            pane_index: 1,
        }],
    }));
    app.apply_target_group(0);
    assert_eq!(app.active_group_name.as_deref(), Some("team"));
    assert!(app.marked_pane_ids.contains("%2"));
    app.apply_target_group(99);
    assert_eq!(app.status_message(), "Saved fleet no longer exists.");

    app.target_groups.push(super::TargetGroup {
        name: String::from("gone"),
        members: vec![super::PaneLocator {
            session_name: String::from("demo"),
            window_name: String::from("gone"),
            pane_index: 0,
        }],
    });
    app.apply_target_group(1);
    assert!(app.status_message().contains("none of its panes are live"));

    app.selected_group_index = None;
    app.delete_selected_target_group();
    assert_eq!(app.status_message(), "Load a fleet before deleting it.");

    app.begin_search();
    app.push_search_char('\n');
    app.push_search_char('a');
    app.pop_search_char();
    app.finish_search();
    assert_eq!(app.status_message(), "");

    app.open_action_menu();
    assert!(app.is_action_menu_active());
    assert!(app.dismiss_action_menu());
    assert!(!app.dismiss_action_menu());
    app.open_action_menu();
    assert!(app.close_action_menu());

    app.toggle_metrics_mode();
    assert_eq!(app.metrics_mode, MetricsMode::Local);
    app.toggle_metrics_mode();
    assert_eq!(app.metrics_mode, MetricsMode::Off);
    app.toggle_bell_notifications();
    assert!(!app.notification_settings.bell_enabled);
    app.toggle_desktop_notifications();
    assert!(!app.notification_settings.desktop_enabled);
    app.cycle_alert_policy();
    assert_eq!(
        app.notification_settings.alert_policy,
        AlertPolicy::ErrorAndWaiting
    );
    app.cycle_alert_debounce();
    assert_eq!(app.notification_settings.debounce_seconds, 60);

    app.clear_selected_acknowledgement();
    assert_eq!(app.status_message(), "demo / agents was not muted.");
    app.acknowledge_selected_attention();
    assert!(app.status_message().contains("Muted alert"));
    app.clear_selected_acknowledgement();
    assert!(app.status_message().contains("Unmuted alert"));

    app.context_pane = super::ContextPane::Navigator;
    app.selected_window_id = Some(String::from("@2"));
    app.cycle_panel_focus();
    app.select_next_pane();
    assert_eq!(app.status_message(), "");
    app.select_previous_pane();
    assert_eq!(app.status_message(), "");
    app.drill_into_selected_window();
    assert!(matches!(app.view_scope, super::ViewScope::Window { .. }));
}

#[test]
fn status_messages_stay_scan_ready_and_do_not_leak_tmux_ids() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_name = String::from("agents");
    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_name = String::from("agents");
    second.active = false;
    second.pane_index = 1;
    let mut app = app_with_panes(
        vec![first, second],
        vec![("%2", vec!["Waiting for approval. Continue?"])],
    );
    app.selected_pane_id = Some(String::from("%2"));

    let assert_status = |app: &App| {
        let message = app.status_message();
        for term in [
            " / %",
            "pane(s)",
            "group(s)",
            "alert(s)",
            "Details focused",
            "Secondary view switched",
            "Navigator selected",
        ] {
            assert!(
                !message.contains(term),
                "status should not contain `{term}`: {message}"
            );
        }
        assert!(
            !message.contains("%1") && !message.contains("%2"),
            "status should avoid raw tmux pane ids: {message}"
        );
    };

    app.toggle_selected_mark();
    assert_status(&app);
    app.toggle_selected_mark();
    assert_status(&app);
    app.clear_selected_acknowledgement();
    assert_status(&app);
    app.acknowledge_selected_attention();
    assert_status(&app);
    app.clear_selected_acknowledgement();
    assert_status(&app);
    app.acknowledge_all_attention();
    assert_status(&app);
    app.clear_all_acknowledgements();
    assert_status(&app);
    app.capture_attention_transitions();
    assert_status(&app);
}

#[test]
fn formatting_and_binding_helpers_cover_edges() {
    assert_eq!(super::format_age(Duration::from_secs(3)), "3s ago");
    assert_eq!(super::format_age(Duration::from_secs(180)), "3m ago");
    assert_eq!(super::format_age(Duration::from_secs(7_200)), "2h ago");
    assert_eq!(super::format_age_short(Duration::from_secs(3)), "3s");
    assert_eq!(super::format_age_short(Duration::from_secs(180)), "3m");
    assert_eq!(super::format_age_short(Duration::from_secs(7_200)), "2h");
    assert_eq!(super::format_key_label("enter"), "Enter");
    assert_eq!(super::format_key_label("space"), "Space");
    assert_eq!(super::format_key_label("tab"), "Tab");
    assert_eq!(super::format_key_label("esc"), "Esc");
    assert_eq!(super::format_key_label("down"), "Down");
    assert_eq!(super::format_key_label("up"), "Up");
    assert_eq!(super::format_key_label("x"), "X");
    assert_eq!(super::format_key_label("f13"), "f13");
    assert_eq!(super::truncate_for_width("abcdef", 2), "abcdef");
    assert_eq!(super::truncate_for_width("abcdefgh", 6), "abc...");
    assert_eq!(super::truncate_for_board("abcdef", 3), "abc...");
    assert_eq!(super::format_debounce(0), "off");
    assert_eq!(super::format_debounce(15), "15s");
    assert_eq!(super::format_debounce(120), "2m");

    let mut debounce = 0;
    for expected in [15, 30, 60, 120, 300, 0] {
        debounce = super::next_debounce_seconds(debounce);
        assert_eq!(debounce, expected);
    }
    assert_eq!(super::next_debounce_seconds(999), 0);

    let mut app = app_with_panes(vec![sample_pane("codex")], vec![]);
    app.remember_command("  build  ");
    app.remember_command("test");
    app.remember_command("build");
    assert_eq!(
        app.recent_commands.iter().cloned().collect::<Vec<_>>(),
        vec![String::from("build"), String::from("test")]
    );
    app.assign_recent_command_to_slot(99);
    assert_eq!(app.status_message(), "Invalid macro slot.");
    assert!(super::validate_binding_token("test", " ").is_err());
    assert!(super::validate_binding_token("test", " a").is_err());
    assert!(super::validate_binding_token("test", "ab").is_err());
    assert!(super::validate_binding_token("test", "enter").is_ok());
    assert!(super::validate_binding_token("test", "z").is_ok());
}

#[test]
fn presentation_modes_cover_empty_states_and_overlay_branches() {
    let mut empty = app_with_panes(Vec::new(), vec![]);
    assert_eq!(empty.title(), "muxboard");
    assert_eq!(empty.help_overlay_title(), "Help");
    assert!(empty.snapshot().panes.is_empty());
    assert_eq!(
        empty.pane_lines(),
        vec![
            String::from("No panes yet."),
            String::from("Start tmux panes, then R refresh.")
        ]
    );
    assert_eq!(
        empty.attention_queue_lines(),
        vec![String::from("All clear.")]
    );
    assert_eq!(
        empty.navigator_lines(),
        vec![
            String::from("No panes yet."),
            String::from("Start tmux panes, then R refresh.")
        ]
    );
    assert_eq!(empty.header_context_line_for_width(120), "No panes yet.");
    empty.context_pane = super::ContextPane::Tail;
    assert_eq!(
        empty.header_context_line_for_width(120),
        "Output  No panes yet."
    );
    assert_eq!(
        empty.fleet_picker_lines(),
        vec![
            String::from("No saved fleets."),
            String::from("Mark panes, then save a fleet from More.")
        ]
    );
    empty.context_pane = super::ContextPane::Inspect;
    empty.search_query = String::from("missing");
    assert!(
        empty
            .header_context_line_for_width(120)
            .contains("no matches")
    );
    assert!(!empty.board_title(8).is_empty());

    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");
    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.pane_index = 1;
    let mut app = app_with_panes(
        vec![first, second],
        vec![
            ("%1", vec!["Waiting for approval. Continue?"]),
            ("%2", vec!["Tool Bash running for 3s..."]),
        ],
    );
    app.selected_pane_id = Some(String::from("%1"));
    app.recent_alerts
        .extend([String::from("first alert"), String::from("second alert")]);
    app.recent_commands
        .extend([String::from("build"), String::from("test")]);
    app.macro_slots[0] = Some(String::from("cargo test"));
    app.target_groups.push(super::TargetGroup {
        name: String::from("saved"),
        members: vec![super::PaneLocator {
            session_name: String::from("demo"),
            window_name: String::from("alpha"),
            pane_index: 0,
        }],
    });
    app.selected_group_index = Some(0);
    app.active_group_name = Some(String::from("saved"));
    app.marked_pane_ids
        .extend([String::from("%1"), String::from("%2")]);
    app.pending_dispatch = Some(super::StagedDispatch {
        text: String::from("run {id}"),
        expanded: vec![
            (String::from("%1"), String::from("run %1")),
            (String::from("%2"), String::from("run %2")),
            (String::from("%3"), String::from("run %3")),
        ],
        remember: true,
        target_description: String::from("send list (3 panes)"),
    });

    let panes_title = app.panes_title();
    assert!(panes_title.contains("fleet saved"));
    assert!(panes_title.contains("send list"));
    let command_lines = app.command_lines();
    assert!(command_lines.iter().any(|line| line.contains("alerts")));
    assert!(command_lines.iter().any(|line| line.contains("... 1 more")));
    assert!(
        !command_lines
            .iter()
            .any(|line| line.to_ascii_lowercase().contains("macros"))
    );
    assert!(!command_lines.iter().any(|line| line.contains("vars")));
    assert_eq!(
        app.header_hint_line_for_width(120),
        "Enter send  Esc cancel"
    );
    assert!(
        app.header_context_line_for_width(120)
            .contains("Review send")
    );
    assert!(
        app.header_context_line_for_width(60)
            .contains("Review send")
    );
    assert!(app.status_hint_line_for_width(120).contains("Enter send"));
    assert!(app.chrome_line_for_width(32).chars().count() <= 32);
    assert!(app.footer_line_for_width(120).contains("Enter send"));
    assert_eq!(app.shell_panel(), super::presentation::ShellPanel::Send);
    assert!(app.overlay_panel().is_some());
    assert!(app.should_emphasize_context_panel());

    app.pending_dispatch = None;
    app.command_input_active = true;
    app.command_buffer = String::from("cargo test {id}");
    assert!(
        app.command_lines()
            .iter()
            .any(|line| line.contains("Preview"))
    );
    assert!(app.header_context_line_for_width(60).contains("Send to"));
    app.command_input_active = false;
    app.group_input_active = true;
    app.group_name_buffer = String::from("new group");
    assert!(app.panes_title().contains("fleet: new group"));
    assert!(app.header_hint_line_for_width(60).contains("Enter save"));
    assert!(app.status_hint_line_for_width(120).contains("type name"));
    app.group_input_active = false;
    app.macro_assign_active = true;
    assert!(app.header_hint_line_for_width(120).contains("pin latest"));
    assert!(app.status_hint_line_for_width(120).contains("pin"));
    app.macro_assign_active = false;
    app.action_menu_active = true;
    assert_eq!(app.context_panel_title(), "More");
    assert_eq!(app.header_context_line_for_width(120), "More");
    assert!(
        app.context_panel_lines()
            .iter()
            .any(|line| line.contains("Settings"))
    );
    app.action_menu_active = false;
    app.help_overlay_active = true;
    assert!(app.status_hint_line_for_width(120).contains("Esc close"));
    app.help_overlay_active = false;

    app.context_pane = super::ContextPane::Navigator;
    assert_eq!(app.header_hint_line_for_width(120), "");
    assert!(app.header_context_line_for_width(120).contains("Browse"));
    assert_eq!(app.context_panel_title(), "Browse");
    assert_eq!(app.shell_panel(), super::presentation::ShellPanel::Browse);
    app.context_pane = super::ContextPane::Tail;
    assert!(app.context_panel_title().contains("Output"));
    assert_eq!(app.shell_panel(), super::presentation::ShellPanel::Output);
    app.context_pane = super::ContextPane::Control;
    assert_eq!(app.context_panel_title(), "Command Center");
    assert_eq!(app.shell_panel(), super::presentation::ShellPanel::Overview);

    app.marked_pane_ids.clear();
    app.active_group_name = None;
    app.view_scope = super::ViewScope::Window {
        id: String::from("@1"),
        name: String::from("demo/alpha"),
    };
    let wide = app.status_hint_line_for_width(140);
    assert!(wide.contains("shows all panes"));
    let medium = app.status_hint_line_for_width(120);
    assert!(medium.contains("show all"));
    assert!(
        app.footer_line_for_width(40).contains("? help")
            || !app.footer_line_for_width(40).is_empty()
    );
}

#[test]
fn startup_failures_are_actionable_without_tmux_jargon() {
    let target = Target {
        binary: String::from("tmux"),
        socket: Some(String::from("agents")),
        session: Some(String::from("ops")),
    };

    let no_server = super::startup_snapshot_status_message(
        &target,
        &anyhow::anyhow!(
            "tmux command failed for socket `agents`, session `ops`: no server running"
        ),
    );
    assert_eq!(
        no_server,
        "No tmux server found for socket `agents`, session `ops`. Start tmux, then refresh."
    );

    let missing_session = super::startup_snapshot_status_message(
        &target,
        &anyhow::anyhow!(
            "tmux command failed for socket `agents`, session `ops`: can't find session: ops"
        ),
    );
    assert_eq!(
        missing_session,
        "Session not found for socket `agents`, session `ops`. Choose another session or refresh."
    );

    let unavailable =
        super::startup_snapshot_status_message(&target, &anyhow::anyhow!("permission denied"));
    assert!(
        unavailable.starts_with("Could not read tmux panes for socket `agents`, session `ops`:")
    );

    for message in [no_server, missing_session, unavailable] {
        assert!(!message.contains("Snapshot unavailable"), "{message}");
        assert!(!message.contains("tmux command failed"), "{message}");
    }
}

#[test]
fn usability_startup_load_warnings_compose_without_whitespace_noise() {
    let first = super::append_startup_status(
        String::new(),
        String::from(" State load failed: failed to parse persisted muxboard state "),
    );
    assert_eq!(
        first,
        "State load failed: failed to parse persisted muxboard state"
    );

    let combined = super::append_startup_status(
        String::from("No tmux server found for default session. Start tmux, then refresh."),
        String::from(" UI settings load failed: failed to parse muxboard config. Using defaults. "),
    );

    assert_eq!(
        combined,
        "No tmux server found for default session. Start tmux, then refresh. UI settings load failed: failed to parse muxboard config. Using defaults."
    );
    assert!(!combined.starts_with(' '), "{combined}");
    assert!(!combined.contains("  "), "{combined}");
}

#[tokio::test]
async fn startup_bootstrap_builds_live_app_from_fake_tmux_without_user_paths() {
    let fake_tmux = fake_tmux_script(
        "bootstrap",
        r#"#!/bin/sh
if [ "$1" = "list-panes" ]; then
  cat <<'EOF'
$0	demo	@0	agents	%1	0	100	workspace	codex	/workspace	1	0
$0	demo	@0	agents	%2	1	101	worker	claude	/workspace	0	0
EOF
  exit 0
fi

if [ "$1" = "capture-pane" ]; then
  case "$*" in
    *"%1"*) printf 'STATUS=waiting | BLOCKER=approval | NEXT=approve deploy\n' ;;
    *"%2"*) printf 'STATUS=running | BLOCKER=none | NEXT=write tests\n' ;;
    *) printf 'no output\n' ;;
  esac
  exit 0
fi

if [ "$1" = "-C" ]; then
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let temp_root = unique_test_path("bootstrap", "");
    let state_store = state::Store::new_at(temp_root.join("state.json"));
    let config_store = crate::config::Store::new_at(temp_root.join("config.json"));
    let cli = Cli {
        tmux_bin: fake_tmux.clone(),
        socket: None,
        session: None,
        dump_probe_json: false,
        print_config_example: false,
        print_default_keybindings: false,
        theme: None,
        theme_picker: false,
        command: None,
    };
    let probe = Probe {
        version: String::from("tmux 3.5a"),
        target: Target {
            binary: fake_tmux,
            socket: None,
            session: None,
        },
    };

    let app = App::bootstrap_from_probe(
        cli,
        probe,
        crate::tmux::RuntimeContext {
            socket_name: Some(String::from("default")),
            pane_id: Some(String::from("%muxboard")),
        },
        state_store,
        config_store,
        notifications::Notifier::with_mode_for_test(notifications::NotificationMode::TerminalOnly),
    )
    .await
    .expect("bootstrap should build an app from fake tmux without touching user paths");

    assert_eq!(app.snapshot.pane_count(), 2);
    assert_eq!(app.selected_pane_id.as_deref(), Some("%1"));
    assert_eq!(app.status_message(), "");
    assert_eq!(app.control_state, "connected");
    assert!(app.pane_runtime.contains_key("%1"));
    assert!(
        app.is_theme_picker_active(),
        "missing theme config should open the first-run picker"
    );
    assert_eq!(
        app.ui_settings.active_theme_preset(),
        ThemePreset::TerminalNative
    );

    let board = app.board_rows(10);
    assert!(
        board[0].location.starts_with("demo/agents"),
        "{:?}",
        board[0]
    );
    assert_eq!(board[0].status, "waiting");
    assert_eq!(board[0].mission, "approval");
    assert!(!board[0].title.contains("STATUS="), "{:?}", board[0]);
    assert!(!board[0].title.contains("NEXT="), "{:?}", board[0]);

    let details = app.selected_pane_lines().join("\n");
    assert!(details.contains("Blocked: approval"), "{details}");
    assert!(details.contains("Action: : reply"), "{details}");
    assert!(!details.contains("reply for"), "{details}");
    assert!(!details.contains("STATUS="), "{details}");
    assert!(!details.contains("NEXT="), "{details}");
}

#[tokio::test]
async fn startup_bootstrap_theme_onboarding_respects_existing_and_forced_configs() {
    let fake_tmux = fake_tmux_script(
        "bootstrap-theme",
        r#"#!/bin/sh
if [ "$1" = "list-panes" ]; then
  cat <<'EOF'
$0	demo	@0	agents	%1	0	100	workspace	codex	/workspace	1	0
EOF
  exit 0
fi

if [ "$1" = "capture-pane" ]; then
  printf 'ready\n'
  exit 0
fi

if [ "$1" = "-C" ]; then
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );

    let configured_root = unique_test_path("bootstrap-theme-configured", "");
    let configured_config =
        crate::config::Store::new_at(configured_root.join("configured-config.json"));
    configured_config
        .save_theme_preset(ThemePreset::TerminalNative)
        .expect("theme config should be saved");
    let configured = App::bootstrap_from_probe(
        Cli {
            tmux_bin: fake_tmux.clone(),
            socket: None,
            session: None,
            dump_probe_json: false,
            print_config_example: false,
            print_default_keybindings: false,
            theme: None,
            theme_picker: false,
            command: None,
        },
        Probe {
            version: String::from("tmux 3.5a"),
            target: Target {
                binary: fake_tmux.clone(),
                socket: None,
                session: None,
            },
        },
        crate::tmux::RuntimeContext::default(),
        state::Store::new_at(configured_root.join("configured-state.json")),
        configured_config,
        notifications::Notifier::with_mode_for_test(notifications::NotificationMode::TerminalOnly),
    )
    .await
    .expect("configured theme should bootstrap");

    assert!(
        !configured.is_theme_picker_active(),
        "existing theme config must not be overwritten by onboarding"
    );
    assert_eq!(
        configured.ui_settings.active_theme_preset(),
        ThemePreset::TerminalNative
    );

    let forced_root = unique_test_path("bootstrap-theme-forced", "");
    let forced_config = crate::config::Store::new_at(forced_root.join("forced-config.json"));
    forced_config
        .save_theme_preset(ThemePreset::TerminalNative)
        .expect("theme config should be saved");
    let forced = App::bootstrap_from_probe(
        Cli {
            tmux_bin: fake_tmux.clone(),
            socket: None,
            session: None,
            dump_probe_json: false,
            print_config_example: false,
            print_default_keybindings: false,
            theme: None,
            theme_picker: true,
            command: None,
        },
        Probe {
            version: String::from("tmux 3.5a"),
            target: Target {
                binary: fake_tmux,
                socket: None,
                session: None,
            },
        },
        crate::tmux::RuntimeContext::default(),
        state::Store::new_at(forced_root.join("forced-state.json")),
        forced_config,
        notifications::Notifier::with_mode_for_test(notifications::NotificationMode::TerminalOnly),
    )
    .await
    .expect("forced theme picker should bootstrap");

    assert!(forced.is_theme_picker_active());
    assert_eq!(
        forced.ui_settings.active_theme_preset(),
        ThemePreset::TerminalNative
    );
    assert!(
        forced.status_hint_line_for_width(80).contains("Esc keep"),
        "{}",
        forced.status_hint_line_for_width(80)
    );
}

#[tokio::test]
async fn startup_bootstrap_surfaces_recoverable_load_and_tmux_warnings() {
    let fake_tmux = fake_tmux_script(
        "bootstrap-warnings",
        r#"#!/bin/sh
if [ "$1" = "list-panes" ]; then
  echo 'no server running on /tmp/tmux-501/default' >&2
  exit 1
fi

if [ "$1" = "-C" ]; then
  echo 'control attach unavailable' >&2
  exit 2
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let temp_root = unique_test_path("bootstrap-warnings", "");
    fs::create_dir_all(&temp_root).expect("bootstrap warning temp root should be writable");
    let state_path = temp_root.join("state.json");
    let config_path = temp_root.join("config.json");
    fs::write(&state_path, "{not valid state").expect("malformed state should be writable");
    fs::write(&config_path, "{not valid config").expect("malformed config should be writable");

    let mut app = App::bootstrap_from_probe(
        Cli {
            tmux_bin: fake_tmux.clone(),
            socket: None,
            session: None,
            dump_probe_json: false,
            print_config_example: false,
            print_default_keybindings: false,
            theme: None,
            theme_picker: false,
            command: None,
        },
        Probe {
            version: String::from("tmux 3.5a"),
            target: Target {
                binary: fake_tmux,
                socket: None,
                session: None,
            },
        },
        crate::tmux::RuntimeContext::default(),
        state::Store::new_at(state_path),
        crate::config::Store::new_at(config_path),
        notifications::Notifier::with_mode_for_test(notifications::NotificationMode::TerminalOnly),
    )
    .await
    .expect("startup warnings should stay recoverable");

    assert_eq!(app.snapshot().pane_count(), 0);
    assert_eq!(app.control_state, "connected");

    let status = app.status_message();
    assert!(status.starts_with("No tmux server found"), "{status}");
    for expected in [
        "State load failed:",
        "Command state load failed:",
        "Fleet load failed:",
        "UI settings load failed:",
        "Notification settings load failed:",
    ] {
        assert!(status.contains(expected), "{status}");
    }
    assert!(!status.contains("  "), "{status}");

    for _ in 0..20 {
        app.tick()
            .await
            .expect("control attach exit should stay recoverable");
        if app.control_state.starts_with("disconnected") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        app.control_state
            .starts_with("disconnected: status exit status: 2"),
        "{}",
        app.control_state
    );
    assert!(
        app.recent_events
            .iter()
            .any(|event| event.contains("control client exited")),
        "{:?}",
        app.recent_events
    );
}

#[tokio::test]
async fn startup_bootstrap_recovers_when_control_client_cannot_spawn() {
    let missing_tmux = unique_test_path("bootstrap-missing-control-bin", "")
        .display()
        .to_string();
    let temp_root = unique_test_path("bootstrap-missing-control", "");
    let app = App::bootstrap_from_probe(
        Cli {
            tmux_bin: missing_tmux.clone(),
            socket: None,
            session: None,
            dump_probe_json: false,
            print_config_example: false,
            print_default_keybindings: false,
            theme: None,
            theme_picker: false,
            command: None,
        },
        Probe {
            version: String::from("tmux 3.5a"),
            target: Target {
                binary: missing_tmux.clone(),
                socket: None,
                session: None,
            },
        },
        crate::tmux::RuntimeContext::default(),
        state::Store::new_at(temp_root.join("state.json")),
        crate::config::Store::new_at(temp_root.join("config.json")),
        notifications::Notifier::with_mode_for_test(notifications::NotificationMode::TerminalOnly),
    )
    .await
    .expect("startup should stay recoverable when control mode cannot spawn");

    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(
        !app.is_theme_picker_active(),
        "no-server recovery should not be hidden behind onboarding"
    );
    assert!(
        app.control_state
            .starts_with("not connected: failed to start control client"),
        "{}",
        app.control_state
    );
    assert!(
        app.control_state.contains(&missing_tmux),
        "{}",
        app.control_state
    );
    assert!(
        app.status_message().contains("Could not read tmux panes"),
        "{}",
        app.status_message()
    );
    assert!(
        app.pane_lines()
            .iter()
            .any(|line| line.contains("Check socket/session, then R refresh")),
        "{:?}",
        app.pane_lines()
    );
}

#[tokio::test]
async fn manual_refresh_reports_snapshot_failure_without_false_success() {
    let fake_tmux = fake_tmux_script(
        "refresh-no-server",
        "#!/bin/sh\nif [ \"$1\" = \"-V\" ]; then printf 'tmux 3.5a\\n'; exit 0; fi\necho 'no server running on /tmp/tmux-501/default' >&2\nexit 1\n",
    );
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.probe.target.binary = fake_tmux;

    app.refresh()
        .await
        .expect("manual refresh failure should stay in the app");

    assert!(
        app.status_message().starts_with("No tmux server found"),
        "{}",
        app.status_message()
    );
    assert!(!app.status_message().contains("Refreshed."));
    assert_eq!(app.snapshot().pane_count(), 0);
    assert!(app.selected_pane_id.is_none());
}

#[tokio::test]
async fn manual_refresh_reconnects_control_monitor_without_real_tmux() {
    let fake_tmux = fake_tmux_script(
        "refresh-reconnect",
        r#"#!/bin/sh
if [ "$1" = "-V" ]; then
  printf 'tmux 3.5a\n'
  exit 0
fi

if [ "$1" = "list-panes" ]; then
  cat <<'EOF'
$0	demo	@0	agents	%1	0	100	workspace	codex	/workspace	1	0
EOF
  exit 0
fi

if [ "$1" = "capture-pane" ]; then
  printf 'STATUS=waiting | BLOCKER=approval | NEXT=approve deploy\n'
  exit 0
fi

if [ "$1" = "-C" ]; then
  exit 0
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    use_fake_tmux_for_test(&mut app, fake_tmux);
    app.control = None;
    app.control_state = String::from("not connected: previous attach failed");

    app.refresh()
        .await
        .expect("manual refresh should reconnect the monitor");

    assert_eq!(app.status_message(), "Refreshed.");
    assert_eq!(app.control_state, "connected");
    assert!(app.control.is_some());
    assert_eq!(app.refresh_count, 2);

    let details = app.selected_pane_lines().join("\n");
    assert!(details.contains("Blocked: approval"), "{details}");
    assert!(details.contains("Action: : reply"), "{details}");
    assert!(!details.contains("reply for"), "{details}");
    assert!(!details.contains("STATUS="), "{details}");
}

#[tokio::test]
async fn refresh_control_connection_reports_attach_spawn_failure() {
    let missing_tmux = unique_test_path("missing-control-bin", "")
        .display()
        .to_string();
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.probe.target.binary = missing_tmux.clone();
    app.control = None;
    app.control_state = String::from("not connected: previous attach failed");

    app.refresh_control_connection().await;

    assert!(app.control.is_none());
    assert!(
        app.control_state
            .starts_with("not connected: failed to start control client"),
        "{}",
        app.control_state
    );
    assert!(
        app.control_state.contains(&missing_tmux),
        "{}",
        app.control_state
    );
}

#[tokio::test]
async fn refresh_control_connection_keeps_existing_monitor_when_healthy() {
    let missing_tmux = unique_test_path("missing-healthy-control-bin", "")
        .display()
        .to_string();
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.probe.target.binary = missing_tmux;
    app.control = Some(live_control_monitor_for_test());
    app.control_state = String::from("connected");

    app.refresh_control_connection().await;

    assert!(app.control.is_some());
    assert_eq!(app.control_state, "connected");
}

#[tokio::test]
async fn refresh_control_connection_reconnects_when_state_is_stale() {
    for stale_state in [
        "disconnected: status 1",
        "not connected: previous attach failed",
    ] {
        let missing_tmux = unique_test_path("missing-stale-control-bin", "")
            .display()
            .to_string();
        let pane = sample_pane("codex");
        let mut app = app_with_panes(vec![pane], vec![]);
        app.probe.target.binary = missing_tmux.clone();
        app.control = Some(live_control_monitor_for_test());
        app.control_state = String::from(stale_state);

        app.refresh_control_connection().await;

        assert!(app.control.is_none(), "{stale_state}");
        assert!(
            app.control_state
                .starts_with("not connected: failed to start control client"),
            "{}",
            app.control_state
        );
        assert!(
            app.control_state.contains(&missing_tmux),
            "{}",
            app.control_state
        );
    }
}

#[tokio::test]
async fn manual_refresh_reports_probe_failure_without_exiting() {
    let fake_tmux = fake_tmux_script(
        "refresh-probe-fails",
        "#!/bin/sh\necho 'tmux probe failed' >&2\nexit 2\n",
    );
    let pane = sample_pane("codex");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.probe.target.binary = fake_tmux;

    app.refresh()
        .await
        .expect("probe failure should stay recoverable in the app");

    assert!(app.status_message().starts_with("Refresh failed:"));
    assert!(app.status_message().contains("tmux probe failed"));
    assert!(!app.status_message().contains("Refreshed."));
}

#[test]
fn empty_first_run_states_show_the_next_recovery_step() {
    let mut app = app_with_panes(Vec::new(), vec![]);
    app.set_status_message_for_test(
        "No tmux server found for socket `agents`, default session. Start tmux, then refresh.",
    );
    assert_eq!(
        app.pane_lines(),
        vec![
            String::from("No tmux server."),
            String::from("Start tmux, then R refresh.")
        ]
    );

    app.set_status_message_for_test(
        "Session not found for socket `agents`, session `ops`. Choose another session or refresh.",
    );
    assert_eq!(
        app.selected_pane_lines(),
        vec![
            String::from("Session not found."),
            String::from("Use another session, then R refresh.")
        ]
    );

    app.set_status_message_for_test(
        "Could not read tmux panes for socket `agents`, session `ops`: permission denied.",
    );
    assert_eq!(
        app.selected_pane_lines(),
        vec![
            String::from("Cannot read tmux panes."),
            String::from("Check socket/session, then R refresh.")
        ]
    );
}

#[tokio::test]
async fn app_private_state_maintenance_covers_prune_focus_and_wrap_edges() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("review");
    second.pane_index = 1;
    let mut app = app_with_panes(vec![first, second], vec![("%1", vec!["one"])]);

    app.selected_pane_id = Some(String::from("%1"));
    app.select_previous_pane();
    assert_eq!(app.selected_pane_id.as_deref(), Some("%2"));
    app.cycle_context_pane();
    app.cycle_context_pane();
    app.cycle_context_pane();
    app.cycle_context_pane();
    app.cycle_context_pane();
    assert_eq!(app.context_pane, super::ContextPane::Inspect);
    app.cycle_sort_mode();
    app.cycle_sort_mode();
    app.cycle_sort_mode();
    assert!(matches!(app.sort_mode, SortMode::Attention));
    app.cycle_filter_mode();
    app.cycle_filter_mode();
    app.cycle_filter_mode();
    assert!(matches!(app.filter_mode, FilterMode::All));

    app.context_pane = super::ContextPane::Inspect;
    app.focus_selected_pane()
        .await
        .expect("focus opens output panel without tmux");
    assert_eq!(app.context_pane, super::ContextPane::Tail);
    app.focus_selected_pane()
        .await
        .expect("enter keeps output open without moving backward");
    assert_eq!(app.context_pane, super::ContextPane::Tail);
    assert!(app.go_back());
    assert_eq!(app.context_pane, super::ContextPane::Inspect);

    app.append_output("%1", String::new(), None);
    app.append_output("%1", String::from("   "), None);
    app.append_output(
        "%1",
        String::from("STATUS=running | BLOCKER=none | NEXT=ship\n"),
        Some(5),
    );
    for index in 0..40 {
        app.append_output("%1", format!("line {index}\n"), None);
    }
    assert!(app.pane_runtime.get("%1").expect("runtime").output.len() <= 24);
    assert!(app.pane_reports.contains_key("%1"));

    app.dirty_pane_ids.insert(String::from("%missing"));
    app.pane_runtime
        .insert(String::from("%missing"), PaneRuntime::default());
    app.pane_last_status
        .insert(String::from("%missing"), PaneStatus::Running);
    app.last_alerted_at
        .insert(String::from("%missing"), Instant::now());
    app.pane_metrics.insert(
        String::from("%missing"),
        metrics::PaneMetrics {
            pid: 999_999,
            cpu_percent: 1.0,
            mem_percent: 2.0,
            elapsed: String::from("00:01"),
            command: String::from("codex"),
        },
    );
    set_pane_report_fields(&mut app, "%missing", "waiting", "approval", "approve");
    app.marked_pane_ids.insert(String::from("%missing"));
    app.acknowledged_attention.insert(
        super::AttentionKey {
            session_name: String::from("missing"),
            window_name: String::from("missing"),
            pane_index: 9,
            current_path: String::from("/missing"),
            current_command: String::from("codex"),
            title: String::from("missing"),
        },
        PaneStatus::Waiting,
    );
    app.prune_runtime();
    assert!(!app.dirty_pane_ids.contains("%missing"));
    assert!(!app.pane_runtime.contains_key("%missing"));
    assert!(!app.pane_last_status.contains_key("%missing"));
    assert!(!app.last_alerted_at.contains_key("%missing"));
    assert!(!app.pane_metrics.contains_key("%missing"));
    assert!(!app.pane_reports.contains_key("%missing"));
    assert!(!app.marked_pane_ids.contains("%missing"));
    assert!(app.acknowledged_attention.is_empty());

    let old_selection = app.selected_pane_id.clone();
    app.runtime_context = crate::tmux::RuntimeContext {
        socket_name: Some(String::from("default")),
        pane_id: old_selection.clone(),
    };
    assert!(!app.matches_base_filter(app.selected_pane().expect("selected pane")));
    app.sync_selected_window_from_selection();
    assert!(app.selected_window_id.is_some());

    app.last_metrics_refresh = None;
    assert!(app.should_refresh_metrics());
    app.last_metrics_refresh = Some(Instant::now());
    assert!(!app.should_refresh_metrics());
    app.refresh_metrics().await;
    assert!(app.last_metrics_refresh.is_some());
}

#[tokio::test]
async fn disappeared_pane_recovery_paths_are_plain_and_non_destructive() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("review");
    second.pane_index = 1;
    second.active = false;
    let mut app = app_with_panes(
        vec![first.clone(), second.clone()],
        vec![("%1", vec!["working"]), ("%2", vec!["waiting"])],
    );

    let outcome = app
        .send_keys_to_pane_ids(&[String::from("%gone")], &["Enter"])
        .await
        .expect("missing stale target should not call tmux");
    assert_eq!(outcome.sent_count, 0);
    assert_eq!(outcome.disappeared_count, 1);
    assert_eq!(app.snapshot.panes.len(), 2);

    let error = app
        .recover_missing_pane_action(&first, anyhow::anyhow!("permission denied"))
        .await
        .expect_err("non-disappearance errors must not be swallowed");
    assert!(error.to_string().contains("permission denied"));

    app.selected_pane_id = Some(String::from("%2"));
    app.selected_window_id = Some(String::from("@2"));
    app.remove_disappeared_panes(&[first, second]);

    assert!(app.snapshot.panes.is_empty());
    assert!(app.selected_pane_id.is_none());
    assert!(app.selected_window_id.is_none());
    assert!(app.pane_runtime.is_empty());
    assert_eq!(
        app.status_message(),
        "2 panes disappeared. Refreshed panes."
    );
}

#[tokio::test]
async fn empty_navigator_actions_recover_without_guesswork() {
    let mut app = app_with_panes(Vec::new(), vec![]);
    app.context_pane = super::ContextPane::Navigator;
    app.selected_window_id = Some(String::from("@missing"));

    app.select_next_window();
    assert!(app.selected_window_id.is_none());

    app.selected_window_id = Some(String::from("@missing"));
    app.select_previous_window();
    assert!(app.selected_window_id.is_none());

    app.jump_to_selected_window()
        .await
        .expect("empty navigator jump should stay inside muxboard");
    assert_eq!(app.status_message(), "No window selected in Browse.");

    app.selected_window_id = Some(String::from("@missing"));
    app.drill_into_selected_window();
    assert_eq!(app.status_message(), "No window selected in Browse.");

    let footer = app.status_hint_line_for_width(100);
    for term in ["? help", "R refresh", ". more", "Esc back", "Q quit"] {
        assert!(
            footer.contains(term),
            "empty Browse footer should keep recovery `{term}`:\n{footer}"
        );
    }
    for inert in ["J/K browse", "Enter window", "/ filter", "G show"] {
        assert!(
            !footer.contains(inert),
            "empty Browse footer advertised inert `{inert}`:\n{footer}"
        );
    }
}

#[tokio::test]
async fn tick_refreshes_local_metrics_when_the_metrics_panel_is_enabled() {
    let mut pane = sample_pane("codex");
    pane.pane_pid = std::process::id();
    let mut app = app_with_panes(vec![pane], vec![("%1", vec!["working"])]);
    app.metrics_mode = MetricsMode::Local;
    app.last_metrics_refresh = None;

    app.tick()
        .await
        .expect("metrics refresh should stay recoverable during the app tick");

    assert!(app.last_metrics_refresh.is_some());
    let metric = app
        .pane_metrics
        .get("%1")
        .expect("pane CPU/memory should attach to the live pane");
    assert_eq!(metric.pid, std::process::id());
}

#[tokio::test]
async fn app_tmux_action_paths_are_exercised_against_fake_tmux_binary() {
    let fake_tmux = fake_tmux_script(
        "actions",
        "#!/bin/sh\nif [ \"$1\" = \"-V\" ]; then echo 'tmux fake'; exit 0; fi\nif [ \"$1\" = \"display-message\" ]; then echo '/dev/ttys999'; exit 0; fi\nexit 0\n",
    );
    let mut waiting = sample_pane("codex");
    waiting.id = String::from("%1");
    let mut running = sample_pane("claude");
    running.id = String::from("%2");
    running.pane_index = 1;
    running.active = false;
    let mut app = app_with_panes(
        vec![waiting, running],
        vec![
            ("%1", vec!["Press Enter to continue"]),
            ("%2", vec!["thinking"]),
        ],
    );
    app.probe.target.binary = fake_tmux;
    app.runtime_context = crate::tmux::RuntimeContext {
        socket_name: Some(String::from("default")),
        pane_id: Some(String::from("%muxboard")),
    };

    app.command_input_active = true;
    app.command_buffer = String::from("hello {id}");
    app.submit_command_input()
        .await
        .expect("single target send should use fake tmux");
    assert!(app.status_message().contains("Sent command"));

    app.marked_pane_ids
        .extend([String::from("%1"), String::from("%2")]);
    app.send_command_text("fanout {id}")
        .await
        .expect("multi target send should stage");
    assert!(app.has_pending_dispatch());
    app.confirm_pending_dispatch()
        .await
        .expect("confirm should send through fake tmux");
    assert!(app.status_message().contains("Sent `fanout {id}`"));

    app.request_target_summaries()
        .await
        .expect("summary fanout should send immediately through fake tmux");
    assert!(
        app.status_message()
            .contains("Asked 2 panes for one-line summaries")
    );

    app.remember_command("cargo test");
    app.assign_recent_command_to_slot(0);
    app.run_macro_slot(0)
        .await
        .expect("macro send should use fake tmux");
    app.repeat_last_command()
        .await
        .expect("repeat send should use fake tmux");

    app.marked_pane_ids.clear();
    app.selected_pane_id = Some(String::from("%1"));
    app.toggle_selected_zoom()
        .await
        .expect("zoom should use fake tmux");
    app.send_enter_to_selected()
        .await
        .expect("enter should use fake tmux");
    app.send_yes_to_selected()
        .await
        .expect("yes should use fake tmux");
    app.send_no_to_selected()
        .await
        .expect("no should use fake tmux");
    app.perform_smart_action()
        .await
        .expect("waiting smart action should send enter through fake tmux");
    assert!(app.status_message().contains("Sent Enter"));

    app.marked_pane_ids
        .extend([String::from("%1"), String::from("%2")]);
    app.perform_smart_action()
        .await
        .expect("marked smart action should send only ready panes");
    assert!(app.status_message().contains("Send list"));

    app.send_enter_to_attention_queue()
        .await
        .expect("attention queue enter should use fake tmux");
    assert!(app.status_message().contains("waiting pane"));

    app.marked_pane_ids.clear();
    app.selected_pane_id = Some(String::from("%2"));
    app.perform_smart_action()
        .await
        .expect("focus smart action should use fake tmux");
    assert!(app.status_message().contains("Showing"));
    app.jump_to_selected_pane()
        .await
        .expect("jump should use fake tmux");
    assert!(app.status_message().contains("Muxboard is still running"));

    app.context_pane = super::ContextPane::Navigator;
    app.selected_window_id = Some(String::from("@0"));
    app.jump_to_selected_pane()
        .await
        .expect("navigator jump should focus selected window through fake tmux");
    assert!(app.selected_pane_id.is_some());
}

#[tokio::test]
async fn jump_to_selected_pane_switches_client_without_destructive_tmux_actions() {
    let log_path = unique_test_path("jump-client", ".log");
    let fake_tmux = fake_tmux_script(
        "jump-client",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = \"display-message\" ]; then echo '/dev/ttys999'; exit 0; fi\nexit 0\n",
            log_path.display().to_string().replace('\'', "'\\''")
        ),
    );
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");
    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.pane_index = 1;
    second.active = false;
    let mut app = app_with_panes(vec![first, second], vec![]);
    app.probe.target.binary = fake_tmux;
    app.runtime_context = crate::tmux::RuntimeContext {
        socket_name: Some(String::from("default")),
        pane_id: Some(String::from("%muxboard")),
    };
    app.selected_pane_id = Some(String::from("%2"));

    app.jump_to_selected_pane()
        .await
        .expect("jump should switch the existing client and keep muxboard alive");

    assert_eq!(
        app.status_message(),
        "Showing demo / beta in tmux. Muxboard is still running."
    );
    assert!(!app.should_quit());
    let recorded = fs::read_to_string(&log_path).expect("jump commands should be logged");
    assert!(
        recorded.contains("display-message -p -t %muxboard #{client_tty}"),
        "{recorded}"
    );
    assert!(
        recorded.contains("switch-client -c /dev/ttys999 -t demo"),
        "{recorded}"
    );
    assert!(recorded.contains("select-window -t @2"), "{recorded}");
    assert!(recorded.contains("select-pane -t %2"), "{recorded}");
    assert!(!recorded.contains("kill-pane"), "{recorded}");
    assert!(!recorded.contains("kill-session"), "{recorded}");
    assert!(!recorded.contains("detach-client"), "{recorded}");
}

#[tokio::test]
async fn jump_to_selected_pane_exits_after_successful_jump_when_requested() {
    let log_path = unique_test_path("jump-close-client", ".log");
    let fake_tmux = fake_tmux_script(
        "jump-close-client",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = \"display-message\" ]; then echo '/dev/ttys999'; exit 0; fi\nexit 0\n",
            log_path.display().to_string().replace('\'', "'\\''")
        ),
    );
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");
    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.pane_index = 1;
    second.active = false;
    let mut app = app_with_panes(vec![first, second], vec![]);
    app.probe.target.binary = fake_tmux;
    app.runtime_context = crate::tmux::RuntimeContext {
        socket_name: Some(String::from("default")),
        pane_id: Some(String::from("%muxboard")),
    };
    app.selected_pane_id = Some(String::from("%2"));
    app.close_after_jump = true;

    app.jump_to_selected_pane()
        .await
        .expect("close-after-jump should only exit after a successful jump");

    assert_eq!(app.status_message(), "Showing demo / beta in tmux.");
    assert!(app.should_quit());
    let recorded = fs::read_to_string(&log_path).expect("jump commands should be logged");
    assert!(
        recorded.contains("switch-client -c /dev/ttys999 -t demo"),
        "{recorded}"
    );
    assert!(recorded.contains("select-pane -t %2"), "{recorded}");
    assert!(!recorded.contains("kill-pane"), "{recorded}");
    assert!(!recorded.contains("kill-session"), "{recorded}");
    assert!(!recorded.contains("detach-client"), "{recorded}");
}

#[tokio::test]
async fn jump_to_selected_pane_marks_unseen_agent_bridge_review_seen_after_success() {
    let log_path = unique_test_path("jump-bridge-seen", ".log");
    let fake_tmux = fake_tmux_script(
        "jump-bridge-seen",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = \"display-message\" ]; then echo '/dev/ttys999'; exit 0; fi\nexit 0\n",
            log_path.display().to_string().replace('\'', "'\\''")
        ),
    );
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    first.window_id = String::from("@1");
    first.window_name = String::from("alpha");
    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.window_id = String::from("@2");
    second.window_name = String::from("beta");
    second.pane_index = 1;
    second.active = false;
    mark_pane_done_for_review(&mut second, "claude", "needs review", "Fix UI", "done");
    let mut app = app_with_panes(vec![first, second], vec![]);
    app.probe.target.binary = fake_tmux;
    app.runtime_context = crate::tmux::RuntimeContext {
        socket_name: Some(String::from("default")),
        pane_id: Some(String::from("%muxboard")),
    };
    app.selected_pane_id = Some(String::from("%2"));

    app.jump_to_selected_pane()
        .await
        .expect("successful jump should mark explicit review events seen");

    let selected = app
        .snapshot
        .panes
        .iter()
        .find(|pane| pane.id == "%2")
        .expect("selected pane should remain present");
    assert_eq!(
        selected
            .agent_event
            .as_ref()
            .expect("event should remain visible")
            .unseen,
        Some(false)
    );
    let recorded = fs::read_to_string(&log_path).expect("jump commands should be logged");
    assert!(
        recorded.contains("switch-client -c /dev/ttys999 -t demo"),
        "{recorded}"
    );
    assert!(
        recorded.contains("set-environment -g MUXBOARD_AGENT_PANE__2_UNSEEN 0"),
        "{recorded}"
    );
    assert!(
        recorded.contains("set-environment -g TMUX_AGENT_PANE_%2_UNSEEN 0"),
        "{recorded}"
    );
}

#[tokio::test]
async fn close_after_jump_does_not_close_internal_navigation_or_inspection() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.pane_index = 1;
    second.active = false;
    let mut app = app_with_panes(vec![first, second], vec![("%1", vec!["working"])]);
    app.close_after_jump = true;

    app.select_next_pane();
    assert!(!app.should_quit());

    app.toggle_selected_mark();
    assert!(!app.should_quit());

    app.begin_search();
    app.push_search_char('c');
    app.finish_search();
    assert!(!app.should_quit());

    app.focus_selected_pane()
        .await
        .expect("opening output must not close drawer mode");
    assert!(!app.should_quit());
}

#[tokio::test]
async fn jump_to_selected_pane_falls_back_to_focus_when_client_tty_is_unavailable() {
    let log_path = unique_test_path("jump-fallback", ".log");
    let fake_tmux = fake_tmux_script(
        "jump-fallback",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = \"display-message\" ]; then exit 1; fi\nexit 0\n",
            log_path.display().to_string().replace('\'', "'\\''")
        ),
    );
    let mut pane = sample_pane("codex");
    pane.window_id = String::from("@1");
    pane.window_name = String::from("agents");
    let mut app = app_with_panes(vec![pane], vec![]);
    app.probe.target.binary = fake_tmux;
    app.runtime_context = crate::tmux::RuntimeContext {
        socket_name: Some(String::from("default")),
        pane_id: Some(String::from("%muxboard")),
    };

    app.jump_to_selected_pane()
        .await
        .expect("jump should fall back to direct focus when the client tty is unavailable");

    assert_eq!(
        app.status_message(),
        "Showing demo / agents in tmux. Muxboard is still running."
    );
    assert!(!app.should_quit());
    let recorded = fs::read_to_string(&log_path).expect("fallback commands should be logged");
    assert!(
        recorded.contains("display-message -p -t %muxboard #{client_tty}"),
        "{recorded}"
    );
    assert!(!recorded.contains("switch-client"), "{recorded}");
    assert!(recorded.contains("select-window -t @1"), "{recorded}");
    assert!(recorded.contains("select-pane -t %1"), "{recorded}");
}

#[tokio::test]
async fn jump_to_selected_pane_recovers_when_target_disappears_on_every_route() {
    let fake_tmux = fake_tmux_script(
        "jump-route-target-loss",
        r#"#!/bin/sh
if [ "$1" = "display-message" ]; then
  case "$*" in
    *"%no-tty"*) exit 1 ;;
    *) echo '/dev/ttys999'; exit 0 ;;
  esac
fi

if [ "$1" = "list-panes" ]; then
  exit 0
fi

if [ "$1" = "switch-client" ] || [ "$1" = "select-window" ]; then
  exit 0
fi

if [ "$1" = "select-pane" ]; then
  echo "can't find pane: $3" >&2
  exit 1
fi

printf 'unexpected tmux args: %s\n' "$*" >&2
exit 64
"#,
    );

    let make_app = |fake_tmux: &str| {
        let mut pane = sample_pane("codex");
        pane.window_id = String::from("@1");
        let mut app = app_with_panes(vec![pane], vec![]);
        app.probe.target.binary = fake_tmux.to_owned();
        app
    };

    let mut same_server_client = make_app(&fake_tmux);
    same_server_client.runtime_context = crate::tmux::RuntimeContext {
        socket_name: Some(String::from("default")),
        pane_id: Some(String::from("%muxboard")),
    };
    same_server_client.close_after_jump = true;
    same_server_client
        .jump_to_selected_pane()
        .await
        .expect("same-server client switch should recover when the pane disappears");
    assert!(
        same_server_client
            .status_message()
            .contains("disappeared. Refreshed panes."),
        "{}",
        same_server_client.status_message()
    );
    assert_eq!(same_server_client.snapshot().pane_count(), 0);
    assert!(same_server_client.selected_pane_id.is_none());
    assert!(!same_server_client.should_quit());

    let mut same_server_no_tty = make_app(&fake_tmux);
    same_server_no_tty.runtime_context = crate::tmux::RuntimeContext {
        socket_name: Some(String::from("default")),
        pane_id: Some(String::from("%no-tty")),
    };
    same_server_no_tty
        .jump_to_selected_pane()
        .await
        .expect("same-server focus fallback should recover when the pane disappears");
    assert!(
        same_server_no_tty
            .status_message()
            .contains("disappeared. Refreshed panes."),
        "{}",
        same_server_no_tty.status_message()
    );
    assert_eq!(same_server_no_tty.snapshot().pane_count(), 0);
    assert!(same_server_no_tty.selected_pane_id.is_none());

    let mut same_server_no_context_pane = make_app(&fake_tmux);
    same_server_no_context_pane.runtime_context = crate::tmux::RuntimeContext {
        socket_name: Some(String::from("default")),
        pane_id: None,
    };
    same_server_no_context_pane
        .jump_to_selected_pane()
        .await
        .expect("same-server direct focus should recover when the pane disappears");
    assert!(
        same_server_no_context_pane
            .status_message()
            .contains("disappeared. Refreshed panes."),
        "{}",
        same_server_no_context_pane.status_message()
    );
    assert_eq!(same_server_no_context_pane.snapshot().pane_count(), 0);
    assert!(same_server_no_context_pane.selected_pane_id.is_none());

    let mut different_server = make_app(&fake_tmux);
    different_server.runtime_context = crate::tmux::RuntimeContext {
        socket_name: Some(String::from("other")),
        pane_id: Some(String::from("%muxboard")),
    };
    different_server
        .jump_to_selected_pane()
        .await
        .expect("cross-server focus should recover when the pane disappears");
    assert!(
        different_server
            .status_message()
            .contains("disappeared. Refreshed panes."),
        "{}",
        different_server.status_message()
    );
    assert_eq!(different_server.snapshot().pane_count(), 0);
    assert!(different_server.selected_pane_id.is_none());
}

#[tokio::test]
async fn navigator_jump_uses_visible_pane_when_active_pane_is_filtered_out() {
    let log_path = unique_test_path("navigator-visible-pane", ".log");
    let fake_tmux = fake_tmux_script(
        "navigator-visible-pane",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
            log_path.display().to_string().replace('\'', "'\\''")
        ),
    );
    let mut active_hidden = sample_pane("codex");
    active_hidden.id = String::from("%1");
    active_hidden.window_id = String::from("@1");
    active_hidden.window_name = String::from("agents");
    active_hidden.title = String::from("alpha");
    let mut visible_fallback = sample_pane("codex");
    visible_fallback.id = String::from("%2");
    visible_fallback.window_id = String::from("@1");
    visible_fallback.window_name = String::from("agents");
    visible_fallback.title = String::from("needle");
    visible_fallback.active = false;
    visible_fallback.pane_index = 1;
    let mut app = app_with_panes(
        vec![active_hidden, visible_fallback],
        vec![
            ("%1", vec!["background work"]),
            ("%2", vec!["needle waiting"]),
        ],
    );
    app.probe.target.binary = fake_tmux;
    app.context_pane = super::ContextPane::Navigator;
    app.selected_window_id = Some(String::from("@1"));
    app.selected_pane_id = Some(String::from("%1"));
    app.search_query = String::from("needle");

    app.jump_to_selected_pane()
        .await
        .expect("navigator jump should use a visible pane in the selected window");

    assert_eq!(app.selected_pane_id.as_deref(), Some("%2"));
    assert_eq!(
        app.status_message(),
        "Showing demo / agents #1 in tmux. Muxboard is still running."
    );
    let recorded = fs::read_to_string(&log_path).expect("focus commands should be logged");
    assert!(recorded.contains("select-window -t @1"), "{recorded}");
    assert!(recorded.contains("select-pane -t %2"), "{recorded}");
    assert!(!recorded.contains("select-pane -t %1"), "{recorded}");
}

#[test]
fn presentation_edge_branches_cover_status_events_lanes_and_empty_selection() {
    let mut first = sample_pane("codex");
    first.id = String::from("%1");
    let mut second = sample_pane("claude");
    second.id = String::from("%2");
    second.pane_index = 1;
    second.active = false;
    let mut app = app_with_panes(
        vec![first, second],
        vec![
            ("%1", vec!["Waiting for approval. Continue?"]),
            ("%2", vec!["thinking"]),
        ],
    );

    app.search_input_active = true;
    assert!(app.panes_title().contains("typing"));
    assert!(app.header_hint_line_for_width(50).contains("Enter apply"));
    app.search_input_active = false;
    let selected_before_command_input = app.selected_pane_id.clone();
    app.selected_pane_id = Some(String::from("%2"));
    app.command_input_active = true;
    assert!(app.header_hint_line_for_width(80).contains("type text"));
    assert!(app.header_hint_line_for_width(50).contains("Enter send"));
    app.command_input_active = false;
    app.selected_pane_id = selected_before_command_input;
    app.group_input_active = true;
    assert!(app.header_hint_line_for_width(50).contains("Enter save"));
    app.group_input_active = false;

    app.marked_pane_ids.insert(String::from("%1"));
    assert!(
        app.status_hint_line_for_width(80)
            .contains("send list 1 pane")
    );
    assert!(app.status_hint_line_for_width(110).contains("remove"));
    app.marked_pane_ids.clear();

    assert_eq!(
        app.recent_event_lines(),
        vec![String::from("No tmux events yet.")]
    );
    assert_eq!(app.recent_events_title(), "Recent events");
    app.push_event(String::from("window renamed"));
    assert_eq!(
        app.recent_event_lines(),
        vec![String::from("window renamed")]
    );
    assert!(app.recent_events_title().contains('1'));

    app.context_pane = super::ContextPane::Targets;
    assert_eq!(app.shell_panel(), super::presentation::ShellPanel::Send);
    assert!(!app.context_panel_lines().is_empty());
    app.context_pane = super::ContextPane::Tail;
    assert!(!app.context_panel_lines().is_empty());
    app.context_pane = super::ContextPane::Navigator;
    app.view_scope = super::ViewScope::Window {
        id: String::from("@0"),
        name: String::from("demo/agents"),
    };
    assert!(app.navigator_lines().join("\n").contains('*'));
    app.context_pane = super::ContextPane::Inspect;

    app.acknowledge_selected_attention();
    let pane_lines = app.pane_lines().join("\n");
    assert!(pane_lines.contains('~') || pane_lines.contains('!'));

    app.search_query = String::from("definitely-no-match");
    assert!(
        app.board_title(8).contains("no panes yet") || app.board_title(8).contains("no matches")
    );
    app.search_query.clear();
    app.selected_pane_id = None;
    assert_eq!(
        app.selected_pane_lines(),
        vec![String::from("No pane selected.")]
    );
    assert_eq!(
        app.live_tail_lines(),
        vec![String::from("No pane selected.")]
    );
}
