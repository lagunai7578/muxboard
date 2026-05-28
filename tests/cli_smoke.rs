use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn muxboard_binary() -> String {
    std::env::var("CARGO_BIN_EXE_muxboard")
        .expect("CARGO_BIN_EXE_muxboard should be set for integration tests")
}

fn script_path(name: &str, body: &str) -> TestResult<PathBuf> {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = std::env::temp_dir().join(format!(
        "muxboard-cli-smoke-{name}-{}-{unique}.sh",
        std::process::id()
    ));
    let body = if body.starts_with("#!") {
        body.to_owned()
    } else {
        format!("#!/usr/bin/env sh\n{body}")
    };
    fs::write(&path, body)?;
    let mut permissions = fs::metadata(&path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions)?;
    Ok(path)
}

fn temp_dir(name: &str) -> TestResult<PathBuf> {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = std::env::temp_dir().join(format!(
        "muxboard-cli-smoke-{name}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn shell_quote_path(path: &std::path::Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

#[test]
fn help_describes_core_flags() -> TestResult<()> {
    let output = Command::new(muxboard_binary()).arg("--help").output()?;
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("tmux binary to invoke"));
    assert!(stdout.contains("print a ready-to-copy default config file and exit"));
    assert!(stdout.contains("print the default keybindings block and exit"));
    assert!(stdout.contains("save a theme preset to the muxboard config and exit"));
    assert!(stdout.contains("open the theme picker on startup"));
    assert!(stdout.contains("agent-event"));

    Ok(())
}

#[test]
fn version_prints_package_version_without_tmux() -> TestResult<()> {
    let output = Command::new(muxboard_binary()).arg("--version").output()?;
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout)?;
    assert_eq!(
        stdout.trim(),
        format!("muxboard {}", env!("CARGO_PKG_VERSION"))
    );

    Ok(())
}

#[test]
fn missing_tmux_binary_exits_with_actionable_copy() -> TestResult<()> {
    let output = Command::new(muxboard_binary())
        .args([
            "--tmux-bin",
            "/tmp/muxboard-definitely-missing-tmux",
            "--dump-probe-json",
        ])
        .output()?;
    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("could not start"), "{stderr}");
    assert!(stderr.contains("Install tmux"), "{stderr}");
    assert!(stderr.contains("--tmux-bin"), "{stderr}");
    assert!(!stderr.contains("failed to execute"), "{stderr}");

    Ok(())
}

#[test]
fn non_tmux_binary_exits_with_actionable_probe_copy() -> TestResult<()> {
    let script = script_path("not-tmux", "printf 'not a tmux binary' >&2\nexit 42\n")?;
    let output = Command::new(muxboard_binary())
        .args([
            "--tmux-bin",
            script.to_str().expect("script path should be UTF-8"),
            "--dump-probe-json",
        ])
        .output()?;
    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("could not read tmux version"), "{stderr}");
    assert!(stderr.contains("not a tmux binary"), "{stderr}");
    assert!(!stderr.contains("-V` failed:"), "{stderr}");

    Ok(())
}

#[test]
fn print_config_example_outputs_valid_json() -> TestResult<()> {
    let output = Command::new(muxboard_binary())
        .arg("--print-config-example")
        .output()?;
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout)?;
    let json: Value = serde_json::from_str(&stdout)?;

    assert_eq!(json["ui_settings"]["layout_preset"], "Auto");
    assert_eq!(json["ui_settings"]["theme"]["preset"], "TerminalNative");
    assert!(
        json["ui_settings"]["theme"]["overrides"]
            .as_object()
            .is_some_and(|overrides| overrides.is_empty())
    );
    assert_eq!(
        json["ui_settings"]["keybindings"]["action_ack_selected"][0],
        "c"
    );
    assert_eq!(
        json["ui_settings"]["keybindings"]["action_ack_clear_selected"][0],
        "w"
    );

    Ok(())
}

#[test]
fn print_default_keybindings_outputs_valid_json() -> TestResult<()> {
    let output = Command::new(muxboard_binary())
        .arg("--print-default-keybindings")
        .output()?;
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout)?;
    let json: Value = serde_json::from_str(&stdout)?;

    assert_eq!(json["layout_preset"], "Auto");
    assert_eq!(json["theme"]["preset"], "TerminalNative");
    assert!(
        json["theme"]["overrides"]
            .as_object()
            .is_some_and(|overrides| overrides.is_empty())
    );
    assert_eq!(json["keybindings"]["actions"][0], ".");
    assert_eq!(json["keybindings"]["summaries"][0], "s");
    assert_eq!(json["keybindings"]["action_enter_queue"][0], "i");
    assert_eq!(json["keybindings"]["action_layout"][0], "L");

    Ok(())
}

#[test]
fn theme_flag_writes_xdg_config_without_invoking_tmux() -> TestResult<()> {
    let config_home = temp_dir("theme-config")?;
    let state_home = temp_dir("theme-state")?;

    let output = Command::new(muxboard_binary())
        .args([
            "--tmux-bin",
            "/tmp/muxboard-definitely-missing-tmux",
            "--theme",
            "dark",
        ])
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_STATE_HOME", &state_home)
        .output()?;
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("muxboard theme set to Dark"), "{stdout}");

    let config_path = config_home.join("muxboard").join("config.json");
    let raw = fs::read_to_string(&config_path)?;
    let json: Value = serde_json::from_str(&raw)?;
    assert_eq!(json["ui_settings"]["theme"]["preset"], "CatppuccinMocha");
    assert!(
        !raw.contains("/Users/"),
        "theme config should stay portable:\n{raw}"
    );

    Ok(())
}

#[test]
fn dump_probe_json_outputs_valid_json() -> TestResult<()> {
    let output = Command::new(muxboard_binary())
        .args([
            "--socket",
            "cli-smoke-socket",
            "--session",
            "cli-smoke-session",
            "--dump-probe-json",
        ])
        .output()?;
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout)?;
    let json: Value = serde_json::from_str(&stdout)?;

    assert!(
        json["version"]
            .as_str()
            .is_some_and(|value| value.starts_with("tmux "))
    );
    assert_eq!(json["target"]["socket"], "cli-smoke-socket");
    assert_eq!(json["target"]["session"], "cli-smoke-session");
    assert_eq!(json["target"]["binary"], "tmux");

    Ok(())
}

#[test]
fn agent_event_subcommand_writes_pane_state_without_probe() -> TestResult<()> {
    let log_path = temp_dir("agent-event-log")?.join("tmux.log");
    let script = script_path(
        "agent-event",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nexit 0\n",
            shell_quote_path(&log_path)
        ),
    )?;

    let output = Command::new(muxboard_binary())
        .args([
            "--tmux-bin",
            script.to_str().expect("script path should be UTF-8"),
            "agent-event",
            "--agent",
            "codex",
            "--state",
            "done",
            "--summary",
            "release ready",
            "--thread-id",
            "turn-123",
            "--thread-name",
            "Ship V1",
            "--progress",
            "10/10 tests",
            "--log",
            "all checks passed",
            "--pane",
            "%1",
        ])
        .output()?;
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("codex done for %1"), "{stdout}");

    let log = fs::read_to_string(log_path)?;
    assert!(log.contains("set-environment -g MUXBOARD_AGENT_PANE__1_AGENT codex"));
    assert!(log.contains("set-environment -g MUXBOARD_AGENT_PANE__1_STATE done"));
    assert!(log.contains("set-environment -g MUXBOARD_AGENT_PANE__1_SUMMARY release ready"));
    assert!(log.contains("set-environment -g MUXBOARD_AGENT_PANE__1_THREAD_ID turn-123"));
    assert!(log.contains("set-environment -g MUXBOARD_AGENT_PANE__1_THREAD_NAME Ship V1"));
    assert!(log.contains("set-environment -g MUXBOARD_AGENT_PANE__1_PROGRESS 10/10 tests"));
    assert!(log.contains("set-environment -g MUXBOARD_AGENT_PANE__1_LOG all checks passed"));
    assert!(log.contains("set-environment -g MUXBOARD_AGENT_PANE__1_UNSEEN 1"));
    assert!(log.contains("set-environment -g MUXBOARD_AGENT_PANE__1_UPDATED_AT"));

    Ok(())
}

#[test]
fn status_subcommands_render_agent_bridge_state() -> TestResult<()> {
    let script = script_path(
        "status-line",
        r#"#!/bin/sh
if [ "$1" = "show-environment" ]; then
  cat <<'EOF'
MUXBOARD_AGENT_PANE__1_AGENT=codex
MUXBOARD_AGENT_PANE__1_STATE=waiting
MUXBOARD_AGENT_PANE__1_SUMMARY=approval
MUXBOARD_AGENT_PANE__2_AGENT=claude
MUXBOARD_AGENT_PANE__2_STATE=running
MUXBOARD_AGENT_PANE__2_SUMMARY=writing tests
EOF
  exit 0
fi
if [ "$1" = "list-panes" ]; then
  cat <<'EOF'
$0	alpha	@0	agents	%1	0	100	workspace	node	/workspace	1	0
$1	beta	@1	agents	%2	0	101	workspace	claude	/workspace	0	0
EOF
  exit 0
fi
exit 64
"#,
    )?;

    let status = Command::new(muxboard_binary())
        .args([
            "--tmux-bin",
            script.to_str().expect("script path should be UTF-8"),
            "status-line",
        ])
        .output()?;
    assert!(status.status.success());
    assert_eq!(String::from_utf8(status.stdout)?.trim(), "mux ! codex");

    let dots = Command::new(muxboard_binary())
        .args([
            "--tmux-bin",
            script.to_str().expect("script path should be UTF-8"),
            "session-dots",
            "--current-session",
            "beta",
        ])
        .output()?;
    assert!(dots.status.success());
    assert_eq!(String::from_utf8(dots.stdout)?.trim(), "!*");

    Ok(())
}
