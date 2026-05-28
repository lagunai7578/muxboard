use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const POLL_INTERVAL: Duration = Duration::from_millis(200);
const FAST_POLL_INTERVAL: Duration = Duration::from_millis(25);
const WAIT_TIMEOUT: Duration = Duration::from_secs(15);
const INERT_ACTION_SETTLE_TIMEOUT: Duration = Duration::from_millis(350);
const RESPONSIVE_NAVIGATION_TIMEOUT: Duration = Duration::from_secs(2);
const RESPONSIVE_STATE_UPDATE_TIMEOUT: Duration = Duration::from_secs(3);
const LIVE_TEST_BASE_CONFIG: &str = r#"{"ui_settings":{"theme":{"preset":"CatppuccinLatte"}}}"#;

struct IsolatedMuxboard {
    command: String,
    config_file: PathBuf,
    state_file: PathBuf,
}

struct TmuxServer {
    socket: String,
}

impl TmuxServer {
    fn new(prefix: &str) -> Self {
        Self {
            socket: format!("{prefix}-{}", unique_suffix()),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new("tmux");
        command.arg("-L").arg(&self.socket);
        command
    }

    fn run(&self, args: &[&str]) -> TestResult<String> {
        let output = self.command().args(args).output()?;
        if !output.status.success() {
            return Err(format!(
                "tmux -L {} {} failed: {}",
                self.socket,
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }

        Ok(String::from_utf8(output.stdout)?)
    }

    fn send_keys(&self, pane: &str, keys: &[&str]) -> TestResult<()> {
        let mut args = vec!["send-keys", "-t", pane];
        args.extend(keys.iter().copied());
        self.run(&args).map(|_| ())
    }

    fn send_literal(&self, pane: &str, text: &str) -> TestResult<()> {
        self.run(&["send-keys", "-t", pane, "-l", "--", text])
            .map(|_| ())
    }

    fn type_keys_slowly(&self, pane: &str, keys: &[&str], delay: Duration) -> TestResult<()> {
        for key in keys {
            self.send_keys(pane, &[*key])?;
            thread::sleep(delay);
        }
        Ok(())
    }

    fn type_literal_slowly(&self, pane: &str, text: &str, delay: Duration) -> TestResult<()> {
        for ch in text.chars() {
            self.send_literal(pane, &ch.to_string())?;
            thread::sleep(delay);
        }
        Ok(())
    }

    fn resize_window(&self, target: &str, width: u16, height: u16) -> TestResult<()> {
        self.run(&[
            "resize-window",
            "-t",
            target,
            "-x",
            &width.to_string(),
            "-y",
            &height.to_string(),
        ])
        .map(|_| ())
    }

    fn capture(&self, pane: &str) -> TestResult<String> {
        self.run(&["capture-pane", "-p", "-S", "-40", "-t", pane])
    }

    fn pane_field(&self, pane: &str, format: &str) -> TestResult<String> {
        Ok(self
            .run(&["list-panes", "-t", pane, "-F", format])?
            .trim()
            .to_owned())
    }

    fn display_field(&self, target: &str, format: &str) -> TestResult<String> {
        Ok(self
            .run(&["display-message", "-p", "-t", target, format])?
            .trim()
            .to_owned())
    }

    fn wait_for_field(&self, pane: &str, format: &str, expected: &str) -> TestResult<()> {
        let start = std::time::Instant::now();
        loop {
            let value = self.pane_field(pane, format)?;
            if value == expected {
                return Ok(());
            }

            if start.elapsed() > WAIT_TIMEOUT {
                return Err(format!(
                    "timed out waiting for pane {pane} field {format} to become `{expected}`, got `{value}`"
                )
                .into());
            }

            thread::sleep(POLL_INTERVAL);
        }
    }

    fn wait_for_display_field(&self, target: &str, format: &str, expected: &str) -> TestResult<()> {
        let start = std::time::Instant::now();
        loop {
            let value = self.display_field(target, format)?;
            if value == expected {
                return Ok(());
            }

            if start.elapsed() > WAIT_TIMEOUT {
                return Err(format!(
                    "timed out waiting for target {target} field {format} to become `{expected}`, got `{value}`"
                )
                .into());
            }

            thread::sleep(POLL_INTERVAL);
        }
    }

    fn wait_for_text(&self, pane: &str, needle: &str) -> TestResult<String> {
        self.wait_for_text_with_poll(pane, needle, WAIT_TIMEOUT, POLL_INTERVAL)
    }

    fn wait_for_soft_unwrapped_text(&self, pane: &str, needle: &str) -> TestResult<String> {
        let start = Instant::now();
        loop {
            let screen = self.capture(pane)?;
            let unwrapped = soft_unwrapped_screen(&screen);
            if unwrapped.contains(needle) {
                return Ok(screen);
            }

            if start.elapsed() > WAIT_TIMEOUT {
                return Err(format!(
                    "timed out waiting for unwrapped `{needle}` in pane {pane}\nlast capture:\n{screen}"
                )
                .into());
            }

            thread::sleep(POLL_INTERVAL);
        }
    }

    fn wait_for_text_with_poll(
        &self,
        pane: &str,
        needle: &str,
        timeout: Duration,
        poll_interval: Duration,
    ) -> TestResult<String> {
        let start = Instant::now();
        loop {
            let screen = self.capture(pane)?;
            if screen.contains(needle) {
                return Ok(screen);
            }

            if start.elapsed() > timeout {
                return Err(format!(
                    "timed out waiting for `{needle}` in pane {pane}\nlast capture:\n{screen}"
                )
                .into());
            }

            thread::sleep(poll_interval);
        }
    }
}

impl Drop for TmuxServer {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .arg("-L")
            .arg(&self.socket)
            .arg("kill-server")
            .output();
    }
}

fn soft_unwrapped_screen(screen: &str) -> String {
    screen.lines().map(str::trim_end).collect::<String>()
}

fn github_actions() -> bool {
    std::env::var_os("GITHUB_ACTIONS").is_some()
}

#[test]
#[ignore = "requires tmux and verifies the TPM helper against live pane geometry"]
fn tmux_plugin_dock_opens_full_height_sidebar_not_quadrant_split() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-plugin-dock");
    let session = "plugin";

    setup_plugin_grid(&target, session, 120, 40)?;

    run_tmux_plugin_helper_against(&target)?;

    let (pane_id, left, top, width, height) = dock_pane_geometry(&target, "plugin:grid")?;
    assert_eq!(left, 0, "dock must start at the left edge");
    assert_eq!(top, 0, "dock must start at the top edge");
    assert_eq!(height, 40, "dock must span the full window height");
    assert!(
        (50..=54).contains(&width),
        "dock width should adapt to about 52 columns at 120 columns\n{pane_id} {width}"
    );

    Ok(())
}

#[test]
#[ignore = "requires tmux and verifies the TPM helper against live pane geometry"]
fn tmux_plugin_dock_adapts_width_across_window_sizes() -> TestResult<()> {
    for &(window_width, expected_width) in &[(90_u16, 40_u16), (120, 52), (240, 72)] {
        let target = TmuxServer::new("muxboard-e2e-plugin-dock-size");
        setup_plugin_grid(&target, "plugin", window_width, 40)?;

        run_tmux_plugin_helper_against(&target)?;

        let (_, left, top, width, height) = dock_pane_geometry(&target, "plugin:grid")?;
        assert_eq!(left, 0, "{window_width}: dock must start at left edge");
        assert_eq!(top, 0, "{window_width}: dock must start at top edge");
        assert_eq!(height, 40, "{window_width}: dock must span full height");
        assert!(
            width.abs_diff(expected_width) <= 2,
            "{window_width}: expected adaptive dock width around {expected_width}, got {width}"
        );
    }

    Ok(())
}

#[test]
#[ignore = "requires tmux and verifies the TPM helper against live pane geometry"]
fn tmux_plugin_dock_toggle_closes_only_the_muxboard_sidebar() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-plugin-dock-toggle");
    setup_plugin_grid(&target, "plugin", 120, 40)?;
    let before = target.run(&["list-panes", "-t", "plugin:grid", "-F", "#{pane_id}"])?;
    let before_panes = before.lines().count();

    run_tmux_plugin_helper_against(&target)?;
    let dock_pane = dock_pane_geometry(&target, "plugin:grid")?.0;
    let with_dock = target.run(&["list-panes", "-t", "plugin:grid", "-F", "#{pane_id}"])?;
    assert_eq!(
        with_dock.lines().count(),
        before_panes + 1,
        "dock should add exactly one pane"
    );

    run_tmux_plugin_helper_against(&target)?;

    let after = target.run(&[
        "list-panes",
        "-t",
        "plugin:grid",
        "-F",
        "#{pane_id}\t#{@muxboard_dock}",
    ])?;
    assert_eq!(
        after.lines().count(),
        before_panes,
        "toggle should close only the dock pane\n{after}"
    );
    assert!(
        !after.contains(&dock_pane),
        "toggle should remove the marked muxboard pane\n{after}"
    );
    assert!(
        after.lines().all(|line| !line.ends_with("\t1")),
        "toggle must not leave a marked dock pane behind\n{after}"
    );

    Ok(())
}

#[test]
#[ignore = "requires tmux and verifies the TPM helper against live pane geometry"]
fn tmux_plugin_dock_close_after_jump_env_still_reaches_muxboard() -> TestResult<()> {
    if github_actions() {
        return Ok(());
    }

    let target = TmuxServer::new("muxboard-e2e-plugin-dock-close-env");
    let session = "plugin";
    let marker = std::env::temp_dir().join(format!("muxboard-close-env-{}", unique_suffix()));
    setup_plugin_grid(&target, session, 120, 40)?;
    target.run(&["set-option", "-g", "@muxboard-close-after-jump", "on"])?;
    target.run(&[
        "set-option",
        "-g",
        "@muxboard-command",
        &format!(
            "printf %s \"$MUXBOARD_CLOSE_AFTER_JUMP\" > {}; sleep 1000",
            shell_quote(&marker.display().to_string())
        ),
    ])?;

    run_tmux_plugin_helper_against(&target)?;

    let start = Instant::now();
    loop {
        if fs::read_to_string(&marker).unwrap_or_default() == "1" {
            return Ok(());
        }
        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for close-after-jump env marker at {}",
                marker.display()
            )
            .into());
        }
        thread::sleep(POLL_INTERVAL);
    }
}

#[test]
#[ignore = "requires tmux and verifies the TPM helper against live pane geometry"]
fn tmux_plugin_drawer_preserves_layout_while_default_is_dock() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-plugin-drawer-layout");
    setup_plugin_grid(&target, "plugin", 120, 40)?;
    target.run(&["set-option", "-g", "@muxboard-open-preset", "dock"])?;
    target.run(&["set-option", "-g", "@muxboard-command", "true"])?;
    let mut client = attach_script_tmux_client(&target, "plugin")?;

    let before = pane_geometry_snapshot(&target, "plugin:grid")?;
    run_tmux_plugin_helper_args_against(&target, &["--preset", "drawer"])?;
    let after = pane_geometry_snapshot(&target, "plugin:grid")?;
    let _ = client.kill();
    let _ = client.wait();

    assert_eq!(
        after, before,
        "peek drawer must not alter the real tmux pane layout"
    );
    let markers = target.run(&[
        "list-panes",
        "-t",
        "plugin:grid",
        "-F",
        "#{pane_id}\t#{@muxboard_dock}",
    ])?;
    assert!(
        markers.lines().all(|line| !line.ends_with("\t1")),
        "peek drawer must not create a dock pane marker\n{markers}"
    );

    Ok(())
}

#[test]
#[ignore = "requires tmux and verifies the TPM helper against live pane geometry"]
fn tmux_plugin_drawer_binding_targets_drawer_while_default_is_dock() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-plugin-drawer-key");
    setup_plugin_grid(&target, "plugin", 120, 40)?;
    target.run(&["set-option", "-g", "@muxboard-open-preset", "dock"])?;
    target.run(&["set-option", "-g", "@muxboard-drawer-key", "P"])?;

    run_tmux_plugin_entrypoint_against(&target)?;

    let primary = target.run(&["list-keys", "-T", "prefix", "M"])?;
    assert!(
        primary.contains("muxboard-open"),
        "primary key should open muxboard\n{primary}"
    );
    assert!(
        !primary.contains("--preset drawer"),
        "primary key must keep the configured default preset\n{primary}"
    );

    let drawer = target.run(&["list-keys", "-T", "prefix", "P"])?;
    assert!(
        drawer.contains("muxboard-open") && drawer.contains("--toggle-peek"),
        "drawer key should toggle peek drawer mode\n{drawer}"
    );

    Ok(())
}

#[test]
#[ignore = "requires tmux and verifies status helpers against live tmux state"]
fn tmux_plugin_status_widgets_render_agent_names_and_custom_dots_live() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-plugin-status-widgets");
    target.run(&[
        "new-session",
        "-d",
        "-s",
        "demo",
        "-n",
        "grid",
        "bash",
        "-lc",
        "sleep 1000",
    ])?;
    target.run(&[
        "new-session",
        "-d",
        "-s",
        "ops",
        "-n",
        "grid",
        "bash",
        "-lc",
        "sleep 1000",
    ])?;

    let first_pane = target
        .run(&["list-panes", "-t", "demo:grid", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let quiet_dots = run_tmux_plugin_script_against(&target, "muxboard-session-dots", &["ops"])?;
    assert_eq!(quiet_dots.trim(), ".*");

    set_live_agent_bridge_event(&target, &first_pane, "codex", "waiting", "")?;

    target.run(&["set-option", "-g", "@muxboard-session-dots-attention", "A"])?;
    target.run(&["set-option", "-g", "@muxboard-session-dots-current", "C"])?;
    target.run(&["set-option", "-g", "@muxboard-session-dots-running", "R"])?;
    target.run(&["set-option", "-g", "@muxboard-session-dots-quiet", "Q"])?;
    target.run(&[
        "set-option",
        "-g",
        "@muxboard-session-dots-color",
        "colour244",
    ])?;
    target.run(&[
        "set-option",
        "-g",
        "@muxboard-session-dots-attention-color",
        "yellow",
    ])?;
    target.run(&[
        "set-option",
        "-g",
        "@muxboard-session-dots-current-color",
        "green",
    ])?;

    let status = run_tmux_plugin_script_against(&target, "muxboard-status", &["demo"])?;
    assert_eq!(status.trim(), "mux ! codex");

    let dots = run_tmux_plugin_script_against(&target, "muxboard-session-dots", &["ops"])?;
    assert_eq!(
        dots.trim(),
        "#[fg=colour244]#[fg=yellow]A#[fg=colour244]#[fg=green]C#[fg=colour244]#[default]"
    );

    target.run(&["split-window", "-t", "demo:grid", "bash -lc 'sleep 1000'"])?;
    let second_pane = target
        .run(&["list-panes", "-t", "demo:grid", "-F", "#{pane_id}"])?
        .lines()
        .find(|pane| *pane != first_pane)
        .ok_or("expected a second demo pane")?
        .to_owned();
    set_live_agent_bridge_event(&target, &second_pane, "claude", "done", "1")?;

    let multi_status = run_tmux_plugin_script_against(&target, "muxboard-status", &["demo"])?;
    assert_eq!(multi_status.trim(), "mux !2");

    Ok(())
}

#[test]
#[ignore = "requires tmux and verifies status helpers against live tmux state"]
fn tmux_plugin_focus_marks_terminal_review_seen_live() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-plugin-focus-seen");
    target.run(&[
        "new-session",
        "-d",
        "-s",
        "demo",
        "-n",
        "grid",
        "bash",
        "-lc",
        "sleep 1000",
    ])?;
    target.run(&["split-window", "-t", "demo:grid", "bash -lc 'sleep 1000'"])?;

    let panes = target.run(&["list-panes", "-t", "demo:grid", "-F", "#{pane_id}"])?;
    let panes = panes.lines().map(str::to_owned).collect::<Vec<_>>();
    let first_pane = panes.first().ok_or("expected first pane")?.to_owned();
    let review_pane = panes.get(1).ok_or("expected review pane")?.to_owned();
    let review_fragment = pane_env_fragment(&review_pane);
    let first_fragment = pane_env_fragment(&first_pane);

    set_live_agent_bridge_event(&target, &review_pane, "codex", "done", "1")?;
    set_live_agent_bridge_event(&target, &first_pane, "claude", "waiting", "1")?;
    run_tmux_plugin_entrypoint_against(&target)?;

    target.run(&["select-pane", "-t", &first_pane])?;
    target.run(&["select-pane", "-t", &review_pane])?;
    wait_for_tmux_env_value(
        &target,
        &format!("MUXBOARD_AGENT_PANE_{review_fragment}_UNSEEN"),
        "0",
    )?;
    wait_for_tmux_env_value(
        &target,
        &format!("TMUX_AGENT_PANE_{review_pane}_UNSEEN"),
        "0",
    )?;

    target.run(&["select-pane", "-t", &first_pane])?;
    target.wait_for_display_field(&first_pane, "#{pane_active}", "1")?;
    let waiting = target.run(&[
        "show-environment",
        "-g",
        &format!("MUXBOARD_AGENT_PANE_{first_fragment}_UNSEEN"),
    ])?;
    assert!(
        waiting.trim().ends_with("=1"),
        "waiting prompts should stay visible until answered, got {waiting:?}"
    );

    Ok(())
}

#[test]
#[ignore = "requires tmux, expect, and verifies the peek drawer through a live client"]
fn tmux_plugin_peek_toggle_closes_live_muxboard_popup_without_layout_change() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-plugin-peek-toggle");
    let session = "plugin";
    let binary = muxboard_binary();
    let marker_root = std::env::temp_dir().join(format!("muxboard-peek-{}", unique_suffix()));
    let toggle_marker = marker_root.join("toggle");
    let repeat_marker = marker_root.join("repeat");
    let quit_marker = marker_root.join("quit");
    let escape_marker = marker_root.join("escape");
    let jump_marker = marker_root.join("jump");
    fs::create_dir_all(&marker_root)?;

    target.run(&[
        "new-session",
        "-d",
        "-x",
        "80",
        "-y",
        "24",
        "-s",
        session,
        "-n",
        "grid",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&["set-option", "-g", "status", "off"])?;
    target.run(&[
        "split-window",
        "-t",
        "plugin:grid",
        "-h",
        "bash -lc 'sleep 1000'",
    ])?;
    let shell_pane = target
        .run(&[
            "list-panes",
            "-t",
            "plugin:grid",
            "-F",
            "#{pane_id}\t#{pane_current_command}",
        ])?
        .lines()
        .find_map(|line| {
            let (pane_id, command) = line.split_once('\t')?;
            (command == "bash").then(|| pane_id.to_owned())
        })
        .ok_or("expected a bash pane for marker commands")?;
    let sidebar_pane = target
        .run(&["list-panes", "-t", "plugin:grid", "-F", "#{pane_id}"])?
        .lines()
        .find(|pane_id| *pane_id != shell_pane.as_str())
        .ok_or("expected a second pane to stand in for an existing sidebar")?
        .to_owned();
    target.run(&[
        "set-option",
        "-p",
        "-t",
        &sidebar_pane,
        "@muxboard_dock",
        "1",
    ])?;
    target.run(&["select-pane", "-t", &shell_pane])?;
    target.run(&["set-option", "-g", "prefix", "C-b"])?;
    target.run(&["set-option", "-g", "@muxboard-key", "M"])?;
    target.run(&["set-option", "-g", "@muxboard-drawer-key", "P"])?;
    target.run(&["set-option", "-g", "@muxboard-open-preset", "dock"])?;
    target.run(&["set-option", "-g", "@muxboard-close-after-jump", "on"])?;
    target.run(&["set-option", "-gu", "@muxboard-open-mode"])?;
    target.run(&[
        "set-option",
        "-g",
        "@muxboard-command",
        &isolated_muxboard_command(&binary, &target.socket, session, "peek-toggle"),
    ])?;
    run_tmux_plugin_entrypoint_against(&target)?;

    let before_geometry = pane_geometry_snapshot(&target, "plugin:grid")?;
    let before_panes = target.run(&["list-panes", "-t", "plugin:grid", "-F", "#{pane_id}"])?;
    assert!(
        before_geometry.lines().any(|line| line.ends_with("\t1")),
        "test should start with an existing sidebar marker\n{before_geometry}"
    );
    let script = live_peek_toggle_expect_script(
        &target.socket,
        session,
        &[
            marker_command("PEEK_TOGGLE_CLOSED", &toggle_marker),
            marker_command("PEEK_REPEAT_CLOSED", &repeat_marker),
            marker_command("PEEK_Q_CLOSED", &quit_marker),
            marker_command("PEEK_ESC_CLOSED", &escape_marker),
            marker_command("PEEK_JUMP_CLOSED", &jump_marker),
        ],
    );
    run_expect_script("peek-toggle", &script)?;

    for (label, marker) in [
        ("toggle", &toggle_marker),
        ("repeat", &repeat_marker),
        ("q", &quit_marker),
        ("escape", &escape_marker),
        ("jump", &jump_marker),
    ] {
        wait_for_file_text(marker, "1")
            .map_err(|error| format!("{label} marker was not written: {error}"))?;
    }

    let after_geometry = pane_geometry_snapshot(&target, "plugin:grid")?;
    let after_panes = target.run(&["list-panes", "-t", "plugin:grid", "-F", "#{pane_id}"])?;
    assert_eq!(
        after_geometry, before_geometry,
        "peek drawer must not alter tmux pane geometry"
    );
    assert_eq!(
        after_panes, before_panes,
        "peek drawer must not create or kill tmux panes"
    );
    let options = target.run(&["show-options", "-g"])?;
    assert!(
        !options.contains("@muxboard_peek_"),
        "peek marker should be cleaned up after close\n{options}"
    );

    Ok(())
}

#[test]
#[ignore = "requires tmux, expect, and verifies peek drawer respects the user's tmux prefix"]
fn tmux_plugin_peek_toggle_honors_custom_tmux_prefix() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-plugin-peek-prefix");
    let session = "plugin";
    let binary = muxboard_binary();
    let marker_root =
        std::env::temp_dir().join(format!("muxboard-peek-prefix-{}", unique_suffix()));
    let close_marker = marker_root.join("closed");
    fs::create_dir_all(&marker_root)?;

    target.run(&[
        "new-session",
        "-d",
        "-x",
        "80",
        "-y",
        "24",
        "-s",
        session,
        "-n",
        "grid",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&["set-option", "-g", "status", "off"])?;
    target.run(&["set-option", "-g", "prefix", "C-a"])?;
    target.run(&["set-option", "-g", "@muxboard-key", "M"])?;
    target.run(&["set-option", "-g", "@muxboard-drawer-key", "P"])?;
    target.run(&["set-option", "-g", "@muxboard-open-preset", "dock"])?;
    target.run(&["set-option", "-g", "@muxboard-close-after-jump", "on"])?;
    target.run(&[
        "set-option",
        "-g",
        "@muxboard-command",
        &isolated_muxboard_command(&binary, &target.socket, session, "peek-prefix"),
    ])?;
    run_tmux_plugin_entrypoint_against(&target)?;

    let before_geometry = pane_geometry_snapshot(&target, "plugin:grid")?;
    let script = live_custom_prefix_peek_expect_script(
        &target.socket,
        session,
        &marker_command("PEEK_CUSTOM_PREFIX_CLOSED", &close_marker),
    );
    run_expect_script("peek-prefix", &script)?;
    wait_for_file_text(&close_marker, "1")?;

    let after_geometry = pane_geometry_snapshot(&target, "plugin:grid")?;
    assert_eq!(
        after_geometry, before_geometry,
        "custom-prefix peek toggle must not alter tmux pane geometry"
    );

    Ok(())
}

#[test]
#[ignore = "requires tmux, expect, and verifies peek drawer respects tmux prefix2"]
fn tmux_plugin_peek_toggle_honors_tmux_prefix2() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-plugin-peek-prefix2");
    let session = "plugin";
    let binary = muxboard_binary();
    let marker_root =
        std::env::temp_dir().join(format!("muxboard-peek-prefix2-{}", unique_suffix()));
    let close_marker = marker_root.join("closed");
    fs::create_dir_all(&marker_root)?;

    target.run(&[
        "new-session",
        "-d",
        "-x",
        "80",
        "-y",
        "24",
        "-s",
        session,
        "-n",
        "grid",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&["set-option", "-g", "status", "off"])?;
    target.run(&["set-option", "-g", "prefix", "C-b"])?;
    target.run(&["set-option", "-g", "prefix2", "C-a"])?;
    target.run(&["set-option", "-g", "@muxboard-key", "M"])?;
    target.run(&["set-option", "-g", "@muxboard-drawer-key", "P"])?;
    target.run(&["set-option", "-g", "@muxboard-open-preset", "dock"])?;
    target.run(&["set-option", "-g", "@muxboard-close-after-jump", "on"])?;
    target.run(&[
        "set-option",
        "-g",
        "@muxboard-command",
        &isolated_muxboard_command(&binary, &target.socket, session, "peek-prefix2"),
    ])?;
    run_tmux_plugin_entrypoint_against(&target)?;

    let before_geometry = pane_geometry_snapshot(&target, "plugin:grid")?;
    let script = live_prefix2_peek_expect_script(
        &target.socket,
        session,
        &marker_command("PEEK_PREFIX2_CLOSED", &close_marker),
    );
    run_expect_script("peek-prefix2", &script)?;
    wait_for_file_text(&close_marker, "1")?;

    let after_geometry = pane_geometry_snapshot(&target, "plugin:grid")?;
    assert_eq!(
        after_geometry, before_geometry,
        "prefix2 peek toggle must not alter tmux pane geometry"
    );

    Ok(())
}

#[test]
#[ignore = "requires tmux and verifies the TPM helper against live pane geometry"]
fn tmux_plugin_drawer_close_after_jump_env_defaults_on() -> TestResult<()> {
    if github_actions() {
        return Ok(());
    }

    let target = TmuxServer::new("muxboard-e2e-plugin-drawer-close-env");
    let marker = std::env::temp_dir().join(format!("muxboard-drawer-env-{}", unique_suffix()));
    setup_plugin_grid(&target, "plugin", 120, 40)?;
    let mut client = attach_script_tmux_client(&target, "plugin")?;
    target.run(&["set-option", "-g", "@muxboard-open-preset", "dock"])?;
    target.run(&[
        "set-option",
        "-g",
        "@muxboard-command",
        &format!(
            "printf %s \"$MUXBOARD_CLOSE_AFTER_JUMP\" > {}",
            shell_quote(&marker.display().to_string())
        ),
    ])?;

    run_tmux_plugin_helper_args_against(&target, &["--preset", "drawer"])?;
    let _ = client.kill();
    let _ = client.wait();

    let start = Instant::now();
    loop {
        if fs::read_to_string(&marker).unwrap_or_default() == "1" {
            return Ok(());
        }
        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for drawer close-after-jump marker at {}",
                marker.display()
            )
            .into());
        }
        thread::sleep(POLL_INTERVAL);
    }
}

#[test]
#[ignore = "requires tmux and verifies the TPM helper against live pane geometry"]
fn tmux_plugin_dock_close_after_jump_closes_live_muxboard_pane() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-plugin-dock-close-live");
    let session = "plugin";
    let binary = muxboard_binary();
    setup_plugin_grid(&target, session, 120, 40)?;
    let muxboard = isolated_muxboard_command(&binary, &target.socket, session, "dock-close-live");
    target.run(&["set-option", "-g", "@muxboard-close-after-jump", "on"])?;
    target.run(&["set-option", "-g", "@muxboard-command", &muxboard])?;

    run_tmux_plugin_helper_against(&target)?;

    let dock_pane = dock_pane_geometry(&target, "plugin:grid")?.0;
    target.wait_for_text(&dock_pane, "Fleet")?;
    target.wait_for_text(&dock_pane, "Details")?;
    target.send_keys(&dock_pane, &["g"])?;
    wait_for_pane_to_disappear(&target, &dock_pane)?;

    let panes = target.run(&[
        "list-panes",
        "-t",
        "plugin:grid",
        "-F",
        "#{pane_id}\t#{@muxboard_dock}",
    ])?;
    assert!(
        panes.lines().all(|line| !line.ends_with("\t1")),
        "close-after-jump should remove the dock pane marker with the pane\n{panes}"
    );

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn search_mark_and_confirmed_multi_send_work_against_live_tmux() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-target");
    let driver = TmuxServer::new("muxboard-e2e-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "agents",
        "bash",
        "-lc",
        "while true; do echo 'error: command failed'; sleep 5; done",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt2",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "multi-send");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt_pane = target
        .run(&["list-panes", "-t", "ops:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt2_pane = target
        .run(&["list-panes", "-t", "ops:prompt2", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;

    driver.type_keys_slowly(
        &driver_pane,
        &["/", "p", "r", "o", "m", "p", "t", "Enter"],
        Duration::from_millis(80),
    )?;
    let search_screen = wait_for_search_result(&driver, &driver_pane, "prompt", "ops/prompt")?;
    assert!(search_screen.contains("ops/prompt"));
    assert!(search_screen.contains("ops/prompt2"));

    driver.send_keys(&driver_pane, &["Space"])?;
    wait_for_send_list_target_state(&driver, &driver_pane, "send list (1 pane)", "ops/prompt")?;
    driver.send_keys(&driver_pane, &["j"])?;
    driver.send_keys(&driver_pane, &["Space"])?;
    let marked_screen = wait_for_send_list_target_state(
        &driver,
        &driver_pane,
        "send list (2 panes)",
        "ops/prompt2",
    )?;
    assert!(marked_screen.contains("send list"));

    driver.send_keys(&driver_pane, &[":"])?;
    wait_for_send_surface(&driver, &driver_pane)?;
    driver.send_literal(&driver_pane, "printf LIVE_E2E_MULTI\\n")?;
    wait_for_send_command_text(&driver, &driver_pane, "LIVE_E2E_MULTI")?;
    driver.send_keys(&driver_pane, &["Enter"])?;
    wait_for_review_surface(&driver, &driver_pane, "Review send to 2 panes.")?;
    assert!(!target.capture(&prompt_pane)?.contains("LIVE_E2E_MULTI"));
    assert!(!target.capture(&prompt2_pane)?.contains("LIVE_E2E_MULTI"));

    driver.send_keys(&driver_pane, &["Enter"])?;
    target.wait_for_text(&prompt_pane, "LIVE_E2E_MULTI")?;
    target.wait_for_text(&prompt2_pane, "LIVE_E2E_MULTI")?;
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn single_target_send_labels_enter_send_and_dispatches_immediately() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-single-send-target");
    let driver = TmuxServer::new("muxboard-e2e-single-send-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "single-send");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt_pane = target
        .run(&["list-panes", "-t", "ops:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_selected_row(&driver, &driver_pane, "ops/prompt")?;
    driver.send_keys(&driver_pane, &[":"])?;
    let command_screen = wait_for_send_surface(&driver, &driver_pane)?;
    assert!(!command_screen.contains("Enter review"), "{command_screen}");

    driver.send_literal(&driver_pane, "printf LIVE_E2E_SINGLE\\n")?;
    wait_for_send_command_text(&driver, &driver_pane, "LIVE_E2E_SINGLE")?;
    driver.send_keys(&driver_pane, &["Enter"])?;

    target.wait_for_text(&prompt_pane, "LIVE_E2E_SINGLE")?;
    assert_muxboard_still_running(&driver, &driver_pane)?;
    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn summary_action_sends_one_line_prompt_to_live_tmux() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-summary-target");
    let driver = TmuxServer::new("muxboard-e2e-summary-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command = isolated_muxboard_command(&binary, &target.socket, session, "summary");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt_pane = target
        .run(&["list-panes", "-t", "ops:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_selected_row(&driver, &driver_pane, "ops/prompt")?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "S summarize panes")?;

    driver.send_keys(&driver_pane, &["s"])?;
    wait_for_fleet_action_feedback(
        &driver,
        &driver_pane,
        "Asked 1 pane for a one-line summary",
        "ops/prompt",
    )?;
    let target_screen = target.wait_for_soft_unwrapped_text(&prompt_pane, "STATUS=<status>")?;
    let target_unwrapped = soft_unwrapped_screen(&target_screen);
    assert!(
        target_unwrapped.contains("BLOCKER=<blocker>"),
        "{target_screen}"
    );
    assert!(target_unwrapped.contains("NEXT=<next>"), "{target_screen}");
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn zoom_action_toggles_live_tmux_pane_without_leaving_muxboard() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-zoom-target");
    let driver = TmuxServer::new("muxboard-e2e-zoom-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "split",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&["split-window", "-t", "ops:split", "bash"])?;

    let muxboard_command = isolated_muxboard_command(&binary, &target.socket, session, "zoom");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let target_pane = target
        .run(&["list-panes", "-t", "ops:split", "-F", "#{pane_id}"])?
        .lines()
        .next()
        .ok_or("expected a split pane")?
        .trim()
        .to_owned();

    target.wait_for_display_field(&target_pane, "#{window_zoomed_flag}", "0")?;
    wait_for_selected_row(&driver, &driver_pane, "ops/split")?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "Z zoom pane")?;

    driver.send_keys(&driver_pane, &["z"])?;
    target.wait_for_display_field(&target_pane, "#{window_zoomed_flag}", "1")?;
    wait_for_fleet_action_feedback(&driver, &driver_pane, "Toggled zoom", "ops/split")?;
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn action_menu_show_in_tmux_jumps_to_selected_pane() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-more-jump-target");
    let driver = TmuxServer::new("muxboard-e2e-more-jump-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "other",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&["select-window", "-t", "ops:other"])?;

    let prompt_pane = target
        .run(&["list-panes", "-t", "ops:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let muxboard_command = isolated_muxboard_command(&binary, &target.socket, session, "more-jump");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.type_keys_slowly(
        &driver_pane,
        &["/", "p", "r", "o", "m", "p", "t", "Enter"],
        Duration::from_millis(80),
    )?;
    wait_for_search_result(&driver, &driver_pane, "prompt", "ops/prompt")?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "G show in tmux")?;

    driver.send_keys(&driver_pane, &["g"])?;

    assert_muxboard_still_running(&driver, &driver_pane)?;
    target.wait_for_field(&prompt_pane, "#{pane_active}", "1")?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn smart_action_sends_enter_to_a_waiting_pane() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-smart-target");
    let driver = TmuxServer::new("muxboard-e2e-smart-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "wait",
        "bash",
        "-lc",
        "echo 'Press Enter to continue.'; read _; echo SMART_ACTION_OK; exec bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "smart-action");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let wait_pane = target
        .run(&["list-panes", "-t", "ops:wait", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    target.wait_for_text(&wait_pane, "Press Enter to continue.")?;
    wait_for_selected_action(&driver, &driver_pane, "Action: A continue", "ops/wait")?;

    driver.send_keys(&driver_pane, &["a"])?;
    target.wait_for_text(&wait_pane, "SMART_ACTION_OK")?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn lane_smart_action_sends_enter_to_waiting_agents_only() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-lane-smart-target");
    let driver = TmuxServer::new("muxboard-e2e-lane-smart-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "agent-one",
        "bash",
        "-lc",
        "echo 'Agent waiting. Press Enter to continue.'; read _; echo LANE_ONE_OK; exec bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "agent-two",
        "bash",
        "-lc",
        "echo 'Agent waiting. Press Enter to continue.'; read _; echo LANE_TWO_OK; exec bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "agent-busy",
        "bash",
        "-lc",
        "while true; do echo 'agent still working'; sleep 5; done",
    ])?;

    let wait_one_pane = target
        .run(&["list-panes", "-t", "ops:agent-one", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let wait_two_pane = target
        .run(&["list-panes", "-t", "ops:agent-two", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    target.wait_for_text(&wait_one_pane, "Press Enter to continue.")?;
    target.wait_for_text(&wait_two_pane, "Press Enter to continue.")?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "lane-smart-action");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_selected_action(&driver, &driver_pane, "Action: A continue", "ops/agent-one")?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "send lane")?;
    driver.send_keys(&driver_pane, &["b"])?;
    wait_for_fleet_action_feedback(&driver, &driver_pane, "Lane send enabled", "ops/agent-one")?;
    driver.send_keys(&driver_pane, &["a"])?;

    target.wait_for_text(&wait_one_pane, "LANE_ONE_OK")?;
    target.wait_for_text(&wait_two_pane, "LANE_TWO_OK")?;
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn first_screen_prioritizes_attention_and_hides_secondary_details() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-first-screen-target");
    let driver = TmuxServer::new("muxboard-e2e-first-screen-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "wait",
        "bash",
        "-lc",
        "echo 'Waiting for approval. Continue?'; read _; exec bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "first-screen");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let screen = wait_for_main_board_surface(
        &driver,
        &driver_pane,
        "ops/wait",
        &[
            "Fleet | 1-2 / 2 | 1 needs you",
            "State: Waiting   Tool: Shell",
            "Action:",
        ],
    )?;
    assert!(screen.contains(">! ops/wait"), "{screen}");
    assert!(!screen.contains("cmd "));
    assert!(!screen.contains("path "));

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn free_form_reply_journey_uses_reply_copy_and_dispatches_live() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-reply-target");
    let driver = TmuxServer::new("muxboard-e2e-reply-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "printf 'Type your answer to continue.\\n'; IFS= read -r answer; echo LIVE_REPLY:$answer; exec bash",
    ])?;

    let muxboard_command = isolated_muxboard_command(&binary, &target.socket, session, "reply");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;
    driver.resize_window("driver:ui", 110, 20)?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let target_pane = target
        .run(&["list-panes", "-t", "ops:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let screen = wait_for_main_board_surface_without(
        &driver,
        &driver_pane,
        "ops/prompt",
        &["Action: : reply", ": reply"],
        &[": send"],
    )?;
    assert!(screen.contains("Enter output"), "{screen}");

    driver.send_keys(&driver_pane, &["?"])?;
    let help = wait_for_help_surface(&driver, &driver_pane, "Now: : reply")?;
    assert!(help.contains("Send: Space add/remove pane"), "{help}");
    assert!(!help.contains("Send: : command pane"), "{help}");
    driver.send_keys(&driver_pane, &["Escape"])?;
    wait_for_help_escape_returns_to_details(&driver, &driver_pane, "ops/prompt")?;

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "command center")?;
    driver.send_keys(&driver_pane, &["]"])?;
    let command_center = wait_for_command_center_surface(&driver, &driver_pane)?;
    assert!(
        command_center.contains("Action: : reply"),
        "{command_center}"
    );
    assert!(command_center.contains("Target:"), "{command_center}");
    assert!(!command_center.contains("Send:"), "{command_center}");
    driver.send_keys(&driver_pane, &["Escape"])?;
    wait_for_command_center_escape_returns_to_details(&driver, &driver_pane, "ops/prompt")?;

    driver.send_keys(&driver_pane, &[":"])?;
    wait_for_reply_surface(&driver, &driver_pane, "ops / prompt")?;
    driver.send_literal(&driver_pane, "live-reply")?;
    wait_for_reply_command_text(&driver, &driver_pane, "live-reply")?;
    driver.send_keys(&driver_pane, &["Enter"])?;

    target.wait_for_text(&target_pane, "LIVE_REPLY:live-reply")?;
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn no_tmux_server_first_run_explains_recovery() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-no-server-target");
    let driver = TmuxServer::new("muxboard-e2e-no-server-driver");
    let binary = muxboard_binary();
    let muxboard_command =
        isolated_muxboard_command_without_config(&binary, &target.socket, "ops", "no-server");

    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_recovery_surface(
        &driver,
        &driver_pane,
        "No tmux server.",
        "Start tmux, then R refresh.",
    )?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn manual_refresh_survives_target_tmux_server_disappearing() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-refresh-lost-target");
    let driver = TmuxServer::new("muxboard-e2e-refresh-lost-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command = isolated_muxboard_command(&binary, &target.socket, session, "lost");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_selected_row(&driver, &driver_pane, "ops/prompt")?;
    target.run(&["kill-server"])?;
    driver.send_keys(&driver_pane, &["r"])?;

    let screen = wait_for_refresh_result(
        &driver,
        &driver_pane,
        &["No tmux server", "Start tmux, then R refresh."],
        &["Refreshed.", "Snapshot refresh failed"],
    )?;
    assert!(!screen.contains("Refreshed."), "{screen}");
    assert!(!screen.contains("Snapshot refresh failed"), "{screen}");
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn manual_refresh_reconnects_live_updates_after_tmux_reappears() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-refresh-reconnect-target");
    let driver = TmuxServer::new("muxboard-e2e-refresh-reconnect-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command = isolated_muxboard_command(&binary, &target.socket, session, "reconnect");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_selected_row(&driver, &driver_pane, "ops/prompt")?;
    target.run(&["kill-server"])?;
    driver.send_keys(&driver_pane, &["r"])?;
    wait_for_refresh_result(&driver, &driver_pane, &["No tmux server"], &["ops/prompt"])?;

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;
    let prompt_pane = target
        .run(&["list-panes", "-t", "ops:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    driver.send_keys(&driver_pane, &["r"])?;
    wait_for_refresh_result(
        &driver,
        &driver_pane,
        &["ops/prompt"],
        &["No tmux server", "Start tmux, then R refresh."],
    )?;
    target.send_literal(&prompt_pane, "echo LIVE_RECONNECTED")?;
    target.send_keys(&prompt_pane, &["Enter"])?;

    let live_screen = wait_for_main_board_surface_with_poll(
        &driver,
        &driver_pane,
        "ops/prompt",
        &["LIVE_RECONNECTED"],
        &[],
        RESPONSIVE_STATE_UPDATE_TIMEOUT,
        FAST_POLL_INTERVAL,
    )?;
    assert!(live_screen.contains("ops/prompt"), "{live_screen}");
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn missing_session_first_run_explains_recovery() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-missing-session-target");
    let driver = TmuxServer::new("muxboard-e2e-missing-session-driver");
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        "ops",
        "-n",
        "agents",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command = isolated_muxboard_command_without_config(
        &binary,
        &target.socket,
        "missing",
        "missing-session",
    );
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_recovery_surface(
        &driver,
        &driver_pane,
        "Session not found.",
        "Use another session, then R refresh.",
    )?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn narrow_terminal_keeps_the_board_scannable() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-narrow-target");
    let driver = TmuxServer::new("muxboard-e2e-narrow-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "wait",
        "bash",
        "-lc",
        "echo 'Waiting for approval. Continue?'; read _; exec bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "narrow-layout");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;
    driver.resize_window("driver:ui", 70, 14)?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let screen = wait_for_main_board_surface(
        &driver,
        &driver_pane,
        "ops/wait",
        &["Fleet | 1-2 / 2 | 1 needs you", "Action:"],
    )?;
    assert!(screen.contains("Fleet | 1-2 / 2 | 1 needs you"));
    assert!(screen.contains("Where"));
    assert!(screen.contains("State"));
    assert!(screen.contains("Latest"));
    assert!(screen.contains("Details"));
    assert!(screen.contains("Action:"));

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn ssh_like_dumb_terminal_keeps_the_board_legible() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-server-terminal-target");
    let driver = TmuxServer::new("muxboard-e2e-server-terminal-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "wait",
        "bash",
        "-lc",
        "echo 'Waiting for approval. Continue?'; read _; exec bash",
    ])?;

    let muxboard_command = isolated_muxboard_command_with_env(
        &binary,
        &target.socket,
        session,
        "server-terminal",
        &[
            ("SSH_CONNECTION", "10.0.0.1 55555 10.0.0.2 22"),
            ("TERM_PROGRAM", "Apple_Terminal"),
            ("TERM", "dumb"),
            ("LANG", "C"),
            ("LC_CTYPE", "C"),
            ("NO_COLOR", "1"),
        ],
    )?;
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let screen =
        wait_for_main_board_surface(&driver, &driver_pane, "ops/wait", &["muxboard", "? help"])?;
    assert!(screen.contains("muxboard"), "{screen}");
    assert!(screen.contains("Fleet"), "{screen}");
    assert!(screen.contains("Details"), "{screen}");
    assert!(screen.contains("ops/wait"), "{screen}");
    assert!(screen.contains("? help"), "{screen}");
    assert!(screen.contains("+") && screen.contains("|"), "{screen}");
    for ch in ['┌', '┐', '└', '┘', '│', '─', '�'] {
        assert!(
            !screen.contains(ch),
            "server-like terminal should avoid Unicode artifacts `{ch}`:\n{screen}"
        );
    }

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn fleet_keeps_plain_session_window_locations_readable_live() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-location-target");
    let driver = TmuxServer::new("muxboard-e2e-location-driver");
    let session = "muxdog";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "codex",
        "bash",
        "-lc",
        "printf 'Waiting for approval. Continue?'; sleep 600",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "claude",
        "bash",
        "-lc",
        "printf 'error: command failed'; sleep 600",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "opencode",
        "bash",
        "-lc",
        "printf 'Building renderer tests'; sleep 600",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "location-width");
    driver.run(&[
        "new-session",
        "-d",
        "-x",
        "120",
        "-y",
        "36",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let screen = wait_for_main_board_surface(
        &driver,
        &driver_pane,
        "muxdog/",
        &["muxdog/claude", "muxdog/codex"],
    )?;
    let claude_row = visible_line_containing(&screen, "muxdog/claude")?;
    let codex_row = visible_line_containing(&screen, "muxdog/codex")?;
    let opencode_row = visible_line_containing(&screen, "muxdog/opencode")?;

    assert!(
        !screen.contains("muxdog/cl..."),
        "plain session/window labels should not truncate prematurely:\n{screen}"
    );
    assert!(claude_row.contains("muxdog/claude"), "{screen}");
    assert!(codex_row.contains("muxdog/codex"), "{screen}");
    assert!(opencode_row.contains("muxdog/opencode"), "{screen}");

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn idle_shell_prompt_noise_stays_out_of_fleet_latest() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-shell-prompt-target");
    let driver = TmuxServer::new("muxboard-e2e-shell-prompt-driver");
    let session = "muxdog";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "shell",
        "bash",
        "-lc",
        "printf 'ready\\n'; export PS1='muxboard: local@host:~/Projects/muxboard$ '; exec bash --noprofile --norc -i",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "shell-prompt");
    driver.run(&[
        "new-session",
        "-d",
        "-x",
        "120",
        "-y",
        "36",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let screen =
        wait_for_main_board_surface(&driver, &driver_pane, "muxdog/shell", &["muxboard: ready"])?;
    let shell_row = visible_line_containing(&screen, ">+ muxdog/shell")?;
    assert!(
        !shell_row.contains("local@host"),
        "fleet latest should not spend space on shell prompts:\n{screen}"
    );
    assert!(
        !shell_row.contains("~/Projects/muxboard"),
        "fleet latest should not spend space on shell prompt paths:\n{screen}"
    );
    assert!(
        !shell_row.contains("For more details"),
        "fleet latest should not spend space on shell startup banners:\n{screen}"
    );
    assert!(
        shell_row.contains("ready"),
        "fleet latest should keep the last useful shell output after filtering startup noise:\n{screen}"
    );

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn visible_agent_thinking_state_is_running_not_idle_live() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-thinking-target");
    let driver = TmuxServer::new("muxboard-e2e-thinking-driver");
    let session = "muxdog";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "codex",
        "bash",
        "-lc",
        "printf 'Codex v0.99\\nThinking...\\n'; while true; do sleep 30; done",
    ])?;

    let muxboard_command = isolated_muxboard_command(&binary, &target.socket, session, "thinking");
    driver.run(&[
        "new-session",
        "-d",
        "-x",
        "120",
        "-y",
        "36",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let screen = wait_for_main_board_surface(
        &driver,
        &driver_pane,
        "muxdog/codex",
        &["State: Running", "Now: Thinking"],
    )?;
    assert!(
        screen.contains("State: Running"),
        "visible agent thinking state should not be labeled idle:\n{screen}"
    );
    assert!(
        screen.contains("Now: Thinking"),
        "visible agent thinking should keep its current activity visible:\n{screen}"
    );
    assert!(
        !screen.contains("State: Idle"),
        "visible agent thinking state should not conflict with the action:\n{screen}"
    );

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn shell_prompt_after_agent_activity_is_idle_not_running_live() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-prompt-return-target");
    let driver = TmuxServer::new("muxboard-e2e-prompt-return-driver");
    let session = "muxdog";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "codex",
        "bash",
        "-lc",
        "printf 'Codex v0.99\\nThinking...\\nready\\n'; export PS1='muxboard: local@host:~/Projects/muxboard$ '; exec bash --noprofile --norc -i",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "prompt-return");
    driver.run(&[
        "new-session",
        "-d",
        "-x",
        "120",
        "-y",
        "36",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let screen = wait_for_main_board_surface(&driver, &driver_pane, "muxdog/codex", &["ready"])?;
    assert!(
        screen.contains("State: Idle") || !screen.contains("Now: Thinking"),
        "a returned shell prompt should make older active agent text stale:\n{screen}"
    );
    assert!(
        !screen.contains("Now: Thinking"),
        "a returned shell prompt should not surface old thinking as current work:\n{screen}"
    );

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn resize_churn_preserves_selection_and_attention_context() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-resize-target");
    let driver = TmuxServer::new("muxboard-e2e-resize-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "wait",
        "bash",
        "-lc",
        "echo 'Waiting for approval. Continue?'; read _; exec bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "resize-churn");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_main_board_surface(&driver, &driver_pane, "ops/wait", &["1 needs you"])?;
    driver.send_keys(&driver_pane, &["j"])?;
    let baseline =
        wait_for_main_board_surface(&driver, &driver_pane, "ops/prompt", &["1 needs you"])?;
    assert!(baseline.contains("! ops/wait"));

    let narrow = resize_window_and_wait_for_board_surface(
        &driver,
        "driver:ui",
        &driver_pane,
        70,
        14,
        "ops/prompt",
        &["Fleet | 1-2 / 2 | 1 needs you", "ops/wait"],
    )?;
    assert!(narrow.contains("Fleet | 1-2 / 2 | 1 needs you"));
    assert!(narrow.contains("ops/prompt"));
    assert!(narrow.contains("ops/wait"));

    let stacked = resize_window_and_wait_for_board_surface(
        &driver,
        "driver:ui",
        &driver_pane,
        90,
        20,
        "ops/prompt",
        &["Fleet | 1-2 / 2 | 1 needs you", "ops/wait", "Details"],
    )?;
    assert!(stacked.contains("Fleet | 1-2 / 2 | 1 needs you"));
    assert!(stacked.contains("ops/prompt"));
    assert!(stacked.contains("ops/wait"));
    assert!(stacked.contains("Details"));

    let wide = resize_window_and_wait_for_board_surface(
        &driver,
        "driver:ui",
        &driver_pane,
        120,
        16,
        "ops/prompt",
        &["Fleet | 1-2 / 2 | 1 needs you", "ops/wait"],
    )?;
    assert!(wide.contains("Fleet | 1-2 / 2 | 1 needs you"));
    assert!(wide.contains("ops/prompt"));
    assert!(wide.contains("ops/wait"));

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn action_menu_uses_rebound_secondary_keys() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-rebind-target");
    let driver = TmuxServer::new("muxboard-e2e-rebind-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "wait",
        "bash",
        "-lc",
        "echo 'Continue? [y/n]'; read answer; echo ACTION_REBIND_OK:$answer; exec bash",
    ])?;

    let config = r#"{
  "ui_settings": {
    "theme": {
      "preset": "CatppuccinLatte"
    },
    "keybindings": {
      "action_send_yes": ["1"]
    }
  }
}"#;
    let muxboard_command = isolated_muxboard_command_with_config(
        &binary,
        &target.socket,
        session,
        "action-rebind",
        Some(config),
    )?;
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let wait_pane = target
        .run(&["list-panes", "-t", "ops:wait", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    target.wait_for_text(&wait_pane, "Continue? [y/n]")?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "1 answer yes")?;
    driver.send_keys(&driver_pane, &["1"])?;
    target.wait_for_text(&wait_pane, "ACTION_REBIND_OK:y")?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn small_board_scrolls_to_keep_deep_selections_visible() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-many-panes-target");
    let driver = TmuxServer::new("muxboard-e2e-many-panes-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "w1",
        "bash",
        "-lc",
        "bash",
    ])?;
    for index in 2..=8 {
        target.run(&[
            "new-window",
            "-t",
            session,
            "-n",
            &format!("w{index}"),
            "bash",
            "-lc",
            "bash",
        ])?;
    }

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "many-panes");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;
    driver.resize_window("driver:ui", 90, 10)?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_main_board_surface(
        &driver,
        &driver_pane,
        "ops/w1",
        &["Fleet | 1-5 / 8 | all quiet"],
    )?;
    driver.type_keys_slowly(
        &driver_pane,
        &["j", "j", "j", "j", "j", "j", "j"],
        Duration::from_millis(70),
    )?;

    let screen = wait_for_main_board_surface(
        &driver,
        &driver_pane,
        "ops/w8",
        &["Fleet | 4-8 / 8 | all quiet"],
    )?;
    assert!(screen.contains("Fleet | 4-8 / 8 | all quiet"));
    assert!(!screen.contains("ops/w1"));

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn large_fleet_navigation_holds_up_with_twenty_panes() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-large-fleet-target");
    let driver = TmuxServer::new("muxboard-e2e-large-fleet-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "w01",
        "bash",
        "-lc",
        "bash",
    ])?;
    for index in 2..=20 {
        target.run(&[
            "new-window",
            "-t",
            session,
            "-n",
            &format!("w{index:02}"),
            "bash",
            "-lc",
            "bash",
        ])?;
    }

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "large-fleet");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;
    driver.resize_window("driver:ui", 90, 10)?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_main_board_surface(
        &driver,
        &driver_pane,
        "ops/w01",
        &["Fleet | 1-5 / 20 | all quiet"],
    )?;
    let navigation_started = Instant::now();
    driver.type_keys_slowly(
        &driver_pane,
        &[
            "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j",
            "j", "j",
        ],
        Duration::from_millis(20),
    )?;

    let screen = wait_for_main_board_surface_with_poll(
        &driver,
        &driver_pane,
        "ops/w20",
        &["Fleet | 16-20 / 20 | all quiet"],
        &[],
        RESPONSIVE_NAVIGATION_TIMEOUT,
        FAST_POLL_INTERVAL,
    )?;
    let navigation_elapsed = navigation_started.elapsed();
    assert!(
        navigation_elapsed < RESPONSIVE_NAVIGATION_TIMEOUT,
        "large fleet navigation should finish inside {RESPONSIVE_NAVIGATION_TIMEOUT:?}, took {navigation_elapsed:?}"
    );
    assert!(screen.contains("Fleet | 16-20 / 20 | all quiet"));
    assert!(!screen.contains("ops/w01"));
    assert!(!screen.contains("ops/w02"));

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn launch_agent_creates_new_tmux_window_without_leaving_muxboard() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-launch-target");
    let driver = TmuxServer::new("muxboard-e2e-launch-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&["new-session", "-d", "-s", session, "-n", "agents", "bash"])?;

    let muxboard_command = isolated_muxboard_command(&binary, &target.socket, session, "launch");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;
    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "start agent")?;
    driver.send_keys(&driver_pane, &["+"])?;
    wait_for_start_agent_surface(&driver, &driver_pane, "In: ops / agents")?;
    driver.send_literal(
        &driver_pane,
        "bash -lc 'printf MUXBOARD_LAUNCHED; sleep 30'",
    )?;
    driver.send_keys(&driver_pane, &["Enter"])?;

    let launched_pane = wait_for_any_pane_text(&target, "MUXBOARD_LAUNCHED")?;
    let launched_window = target.pane_field(&launched_pane, "#{window_name}")?;
    assert_eq!(launched_window, "bash");
    let screen = wait_for_launch_feedback(&driver, &driver_pane, "Started `bash -lc")?;
    assert!(screen.contains("muxboard"));
    assert!(screen.contains("Fleet"));

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn launch_agent_recovers_when_target_server_disappears() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-launch-lost-target");
    let driver = TmuxServer::new("muxboard-e2e-launch-lost-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&["new-session", "-d", "-s", session, "-n", "agents", "bash"])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "launch-lost");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;
    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "start agent")?;
    driver.send_keys(&driver_pane, &["+"])?;
    wait_for_start_agent_surface(&driver, &driver_pane, "In: ops / agents")?;
    driver.send_literal(&driver_pane, "bash -lc 'echo SHOULD_NOT_START'")?;

    target.run(&["kill-server"])?;
    driver.send_keys(&driver_pane, &["Enter"])?;

    let screen = wait_for_recovery_surface(
        &driver,
        &driver_pane,
        "No tmux server",
        "Start tmux, then R refresh.",
    )?;
    assert!(screen.contains("Start tmux, then R refresh."), "{screen}");
    assert!(!screen.contains("In: ops / agents"), "{screen}");
    assert!(!screen.contains("Start failed:"), "{screen}");
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn action_menu_can_acknowledge_and_restore_selected_attention() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-ack-target");
    let driver = TmuxServer::new("muxboard-e2e-ack-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "wait",
        "bash",
        "-lc",
        "echo 'Waiting for approval. Continue?'; read _; exec bash",
    ])?;

    let muxboard = isolated_muxboard_environment_with_config(
        &binary,
        &target.socket,
        session,
        "ack-selected",
        None,
    )?;
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard.command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let wait_pane = target
        .run(&["list-panes", "-t", "ops:wait", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    target.wait_for_text(&wait_pane, "Waiting for approval. Continue?")?;
    wait_for_main_board_surface(&driver, &driver_pane, "ops/wait", &["Waiting"])?;

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "mute alert")?;
    driver.send_keys(&driver_pane, &["c"])?;
    wait_for_ack_count(&muxboard.state_file, 1)?;

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "unmute alert")?;
    driver.send_keys(&driver_pane, &["w"])?;
    wait_for_ack_count(&muxboard.state_file, 0)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn invalid_config_falls_back_to_defaults_and_still_starts() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-invalid-config-target");
    let driver = TmuxServer::new("muxboard-e2e-invalid-config-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "wait",
        "bash",
        "-lc",
        "echo 'Waiting for approval. Continue?'; read _; exec bash",
    ])?;

    let invalid_config = r#"{
  "ui_settings": {
    "theme": {
      "preset": "CatppuccinLatte"
    },
    "keybindings": {
      "action_ack_selected": ["c"],
      "action_ack_clear_selected": ["c"]
    }
  }
}"#;
    let muxboard = isolated_muxboard_environment_with_config(
        &binary,
        &target.socket,
        session,
        "invalid-config-fallback",
        Some(invalid_config),
    )?;

    driver.run(&["new-session", "-d", "-s", "driver", "-n", "ui", "bash"])?;
    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    launch_muxboard_in_pane(&driver, &driver_pane, &muxboard.command)?;
    wait_for_muxboard_surface(&driver, &driver_pane)?;

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "mute alert")?;
    driver.send_keys(&driver_pane, &["c"])?;
    wait_for_ack_count(&muxboard.state_file, 1)?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "unmute alert")?;
    driver.send_keys(&driver_pane, &["w"])?;
    wait_for_ack_count(&muxboard.state_file, 0)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn notification_settings_persist_across_restart_and_stay_ssh_safe() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-notifications-target");
    let driver = TmuxServer::new("muxboard-e2e-notifications-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "wait",
        "bash",
        "-lc",
        "echo 'Waiting for approval. Continue?'; read _; exec bash",
    ])?;

    let muxboard = isolated_muxboard_environment_with_config_and_env(
        &binary,
        &target.socket,
        session,
        "notification-settings",
        Some(LIVE_TEST_BASE_CONFIG),
        &[("SSH_CONNECTION", "127.0.0.1 1 127.0.0.1 2")],
    )?;

    driver.run(&["new-session", "-d", "-s", "driver", "-n", "ui", "bash"])?;
    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    launch_muxboard_in_pane(&driver, &driver_pane, &muxboard.command)?;
    wait_for_main_board_surface(&driver, &driver_pane, "ops/wait", &["1 needs you"])?;

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "desktop alerts unavailable on SSH")?;
    driver.send_keys(&driver_pane, &["o"])?;
    wait_for_fleet_action_feedback(&driver, &driver_pane, "Desktop alerts off.", "ops/wait")?;
    wait_for_notification_flags(&muxboard.config_file, false, true)?;

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "bell")?;
    driver.send_keys(&driver_pane, &["v"])?;
    wait_for_notification_flags(&muxboard.config_file, false, false)?;

    quit_muxboard_in_pane(&driver, &driver_pane)?;
    launch_muxboard_in_pane(&driver, &driver_pane, &muxboard.command)?;
    wait_for_muxboard_surface(&driver, &driver_pane)?;

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "desktop alerts unavailable on SSH")?;
    driver.send_keys(&driver_pane, &["o"])?;
    wait_for_notification_flags(&muxboard.config_file, true, false)?;

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "bell")?;
    driver.send_keys(&driver_pane, &["v"])?;
    wait_for_notification_flags(&muxboard.config_file, true, true)?;
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn acknowledgement_persists_across_restart() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-ack-restart-target");
    let driver = TmuxServer::new("muxboard-e2e-ack-restart-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "wait",
        "bash",
        "-lc",
        "echo 'Waiting for approval. Continue?'; read _; exec bash",
    ])?;

    let muxboard = isolated_muxboard_environment_with_config(
        &binary,
        &target.socket,
        session,
        "ack-restart",
        None,
    )?;

    driver.run(&["new-session", "-d", "-s", "driver", "-n", "ui", "bash"])?;
    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    launch_muxboard_in_pane(&driver, &driver_pane, &muxboard.command)?;
    wait_for_main_board_surface(&driver, &driver_pane, "ops/wait", &["Waiting"])?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "mute alert")?;
    driver.send_keys(&driver_pane, &["c"])?;
    wait_for_ack_count(&muxboard.state_file, 1)?;

    quit_muxboard_in_pane(&driver, &driver_pane)?;
    launch_muxboard_in_pane(&driver, &driver_pane, &muxboard.command)?;
    wait_for_main_board_surface(
        &driver,
        &driver_pane,
        "ops/wait",
        &["Fleet | 1-1 / 1 | all quiet"],
    )?;

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "unmute alert")?;
    driver.send_keys(&driver_pane, &["w"])?;
    wait_for_ack_count(&muxboard.state_file, 0)?;
    wait_for_main_board_surface(&driver, &driver_pane, "ops/wait", &["1 needs you"])?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn saved_group_persists_across_restart_and_can_be_reloaded() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-group-restart-target");
    let driver = TmuxServer::new("muxboard-e2e-group-restart-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt2",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard = isolated_muxboard_environment_with_config(
        &binary,
        &target.socket,
        session,
        "group-restart",
        None,
    )?;

    driver.run(&["new-session", "-d", "-s", "driver", "-n", "ui", "bash"])?;
    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt_pane = target
        .run(&["list-panes", "-t", "ops:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt2_pane = target
        .run(&["list-panes", "-t", "ops:prompt2", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    launch_muxboard_in_pane(&driver, &driver_pane, &muxboard.command)?;
    driver.type_keys_slowly(
        &driver_pane,
        &["/", "p", "r", "o", "m", "p", "t", "Enter"],
        Duration::from_millis(80),
    )?;
    wait_for_search_result(&driver, &driver_pane, "prompt", "ops/prompt")?;
    driver.send_keys(&driver_pane, &["Space"])?;
    wait_for_send_list_target_state(&driver, &driver_pane, "send list (1 pane)", "ops/prompt")?;
    driver.send_keys(&driver_pane, &["j"])?;
    driver.send_keys(&driver_pane, &["Space"])?;
    wait_for_send_list_target_state(&driver, &driver_pane, "send list (2 panes)", "ops/prompt2")?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "save fleet")?;
    driver.send_keys(&driver_pane, &["g"])?;
    wait_for_save_fleet_input(&driver, &driver_pane)?;
    driver.type_keys_slowly(
        &driver_pane,
        &["t", "r", "i", "a", "g", "e"],
        Duration::from_millis(80),
    )?;
    driver.send_keys(&driver_pane, &["Enter"])?;
    wait_for_saved_fleet_active(&driver, &driver_pane, "triage", "2 panes")?;

    quit_muxboard_in_pane(&driver, &driver_pane)?;
    launch_muxboard_in_pane(&driver, &driver_pane, &muxboard.command)?;
    wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "choose fleet")?;
    driver.send_keys(&driver_pane, &["l"])?;
    wait_for_saved_fleet_picker(&driver, &driver_pane, "triage", "2/2 live", true)?;
    driver.send_keys(&driver_pane, &["Enter"])?;
    wait_for_saved_fleet_active(&driver, &driver_pane, "triage", "2 panes")?;

    driver.send_keys(&driver_pane, &[":"])?;
    wait_for_send_surface(&driver, &driver_pane)?;
    driver.send_literal(&driver_pane, "printf GROUP_RELOAD_OK\\n")?;
    wait_for_send_command_text(&driver, &driver_pane, "GROUP_RELOAD_OK")?;
    driver.send_keys(&driver_pane, &["Enter"])?;
    wait_for_review_surface(&driver, &driver_pane, "Review send to 2 panes.")?;
    assert!(!target.capture(&prompt_pane)?.contains("GROUP_RELOAD_OK"));
    assert!(!target.capture(&prompt2_pane)?.contains("GROUP_RELOAD_OK"));

    driver.send_keys(&driver_pane, &["Enter"])?;
    target.wait_for_text(&prompt_pane, "GROUP_RELOAD_OK")?;
    target.wait_for_text(&prompt2_pane, "GROUP_RELOAD_OK")?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn enter_opens_output_without_exiting_and_jump_keeps_muxboard_running() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-focus-target");
    let session = "ops";
    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "agents",
        "bash",
        "-lc",
        "while true; do echo 'error: command failed'; sleep 5; done",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;

    run_output_or_jump_flow(&target, false)?;
    run_output_or_jump_flow(&target, true)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn output_panel_shows_real_tmux_tail_before_metadata() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-output-tail-target");
    let driver = TmuxServer::new("muxboard-e2e-output-tail-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "tail",
        "bash",
        "-lc",
        "for i in $(seq 1 24); do printf 'OUTPUT_VISIBLE_E2E_%02d\\n' \"$i\"; done; sleep 30",
    ])?;
    let tail_pane = target
        .run(&["list-panes", "-t", "ops:tail", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    target.wait_for_text(&tail_pane, "OUTPUT_VISIBLE_E2E_24")?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "output-tail");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let main_before_tab = wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.send_keys(&driver_pane, &["Tab"])?;
    let main_after_tab = wait_for_muxboard_surface(&driver, &driver_pane)?;
    assert_eq!(
        screen_text_position(&main_before_tab, "Fleet")?,
        screen_text_position(&main_after_tab, "Fleet")?,
        "Tab focus must not move Fleet in live tmux:\nbefore:\n{main_before_tab}\n\nafter:\n{main_after_tab}"
    );
    assert_eq!(
        screen_text_position(&main_before_tab, "Details")?,
        screen_text_position(&main_after_tab, "Details")?,
        "Tab focus must not move Details in live tmux:\nbefore:\n{main_before_tab}\n\nafter:\n{main_after_tab}"
    );
    assert_eq!(
        screen_panel_border_signature(&main_before_tab),
        screen_panel_border_signature(&main_after_tab),
        "Tab focus must not move panel borders in live tmux"
    );

    driver.send_keys(
        &driver_pane,
        &[
            "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k",
            "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k",
        ],
    )?;
    let details_scrolled = wait_for_main_board_surface_without(
        &driver,
        &driver_pane,
        "ops/tail",
        &[
            "Details",
            "Output",
            "OUTPUT_VISIBLE_E2E_01",
            "K older/J newer",
            "Enter output",
        ],
        &[],
    )?;
    assert!(
        !details_scrolled.contains("OUTPUT_VISIBLE_E2E_23"),
        "Details focus should scroll older output without leaving stale newest rows:\n{details_scrolled}"
    );

    driver.send_keys(&driver_pane, &["End"])?;
    let details_recovered = wait_for_main_board_surface_without(
        &driver,
        &driver_pane,
        "ops/tail",
        &[
            "Details",
            "Output",
            "OUTPUT_VISIBLE_E2E_23",
            "K older/J newer",
            "Enter output",
        ],
        &[],
    )?;
    assert!(
        !details_recovered.contains("OUTPUT_VISIBLE_E2E_06"),
        "Details focus should recover newest output after scrolling back down:\n{details_recovered}"
    );

    driver.send_keys(&driver_pane, &["Escape"])?;
    wait_for_output_escape_returns_to_details(&driver, &driver_pane, "ops/tail")?;

    driver.send_keys(&driver_pane, &["Enter"])?;
    let screen = wait_for_output_surface_with_text_with_poll(
        &driver,
        &driver_pane,
        "OUTPUT_VISIBLE_E2E_24",
        RESPONSIVE_STATE_UPDATE_TIMEOUT,
        FAST_POLL_INTERVAL,
    )?;

    assert!(screen.contains("Output"), "{screen}");
    assert!(screen.contains("K older/J newer"), "{screen}");
    assert!(screen.contains("OUTPUT_VISIBLE_E2E_23"), "{screen}");
    assert!(!screen.contains("No output yet."), "{screen}");
    assert!(!screen.contains("Updated: no output yet"), "{screen}");

    driver.send_keys(&driver_pane, &["k", "k", "k", "k", "k", "k", "k", "k"])?;
    let scrolled = wait_for_output_surface_with_text_with_poll(
        &driver,
        &driver_pane,
        "OUTPUT_VISIBLE_E2E_06",
        RESPONSIVE_STATE_UPDATE_TIMEOUT,
        FAST_POLL_INTERVAL,
    )?;
    assert!(scrolled.contains("K older/J newer"), "{scrolled}");
    assert!(
        !scrolled.contains("OUTPUT_VISIBLE_E2E_23"),
        "scrolling older should move the newest line out of view:\n{scrolled}"
    );

    driver.resize_window("driver:ui", 100, 24)?;
    driver.wait_for_display_field("driver:ui", "#{window_width}x#{window_height}", "100x24")?;
    let resized_scrolled = wait_for_output_surface_with_text_with_poll(
        &driver,
        &driver_pane,
        "OUTPUT_VISIBLE_E2E_06",
        RESPONSIVE_STATE_UPDATE_TIMEOUT,
        FAST_POLL_INTERVAL,
    )?;
    assert!(
        resized_scrolled.contains("K older/J newer"),
        "{resized_scrolled}"
    );
    assert!(
        !resized_scrolled.contains("OUTPUT_VISIBLE_E2E_23"),
        "resize while scrolled should preserve the older viewport without jumping bottom:\n{resized_scrolled}"
    );

    driver.send_keys(
        &driver_pane,
        &[
            "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j",
            "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j", "j",
        ],
    )?;
    let newest = wait_for_output_surface_with_text_with_poll(
        &driver,
        &driver_pane,
        "OUTPUT_VISIBLE_E2E_23",
        RESPONSIVE_STATE_UPDATE_TIMEOUT,
        FAST_POLL_INTERVAL,
    )?;
    assert!(newest.contains("K older/J newer"), "{newest}");

    driver.send_keys(&driver_pane, &["Escape"])?;
    let details = wait_for_output_escape_returns_to_details(&driver, &driver_pane, "ops/tail")?;
    assert!(details.contains("Enter output"), "{details}");
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn output_panel_updates_while_open_after_real_pane_output() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-output-live-target");
    let driver = TmuxServer::new("muxboard-e2e-output-live-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "tail",
        "bash",
        "-lc",
        "for i in $(seq 1 24); do printf 'LIVE_OUTPUT_OPEN_%02d\\n' \"$i\"; done; exec bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "output-live");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let target_pane = target
        .run(&["list-panes", "-t", "ops:tail", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.send_keys(&driver_pane, &["Enter"])?;
    wait_for_output_surface(&driver, &driver_pane)?;

    let first_update = wait_for_output_surface_with_text_with_poll(
        &driver,
        &driver_pane,
        "LIVE_OUTPUT_OPEN_24",
        RESPONSIVE_STATE_UPDATE_TIMEOUT,
        FAST_POLL_INTERVAL,
    )?;
    assert!(first_update.contains("Output"), "{first_update}");
    assert!(
        first_update.contains("LIVE_OUTPUT_OPEN_23"),
        "{first_update}"
    );
    assert!(!first_update.contains("No output yet."), "{first_update}");

    driver.send_keys(&driver_pane, &["k", "k", "k", "k", "k", "k", "k", "k"])?;
    let scrolled = wait_for_output_surface_with_text_with_poll(
        &driver,
        &driver_pane,
        "LIVE_OUTPUT_OPEN_06",
        RESPONSIVE_STATE_UPDATE_TIMEOUT,
        FAST_POLL_INTERVAL,
    )?;
    assert!(
        !scrolled.contains("LIVE_OUTPUT_OPEN_23"),
        "older scroll should move the newest output out of view before append:\n{scrolled}"
    );

    target.send_literal(&target_pane, "printf 'LIVE_OUTPUT_OPEN_25\\n'")?;
    target.send_keys(&target_pane, &["Enter"])?;
    driver.send_keys(&driver_pane, &["j", "j", "j", "j", "j", "j", "j", "j"])?;
    let second_update = wait_for_output_surface_with_text_with_poll(
        &driver,
        &driver_pane,
        "LIVE_OUTPUT_OPEN_25",
        RESPONSIVE_STATE_UPDATE_TIMEOUT,
        FAST_POLL_INTERVAL,
    )?;
    assert!(second_update.contains("Output"), "{second_update}");
    assert!(
        second_update.contains("LIVE_OUTPUT_OPEN_24"),
        "{second_update}"
    );
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn same_server_enter_keeps_muxboard_visible_and_jump_leaves_it_running() -> TestResult<()> {
    run_same_server_output_or_jump_flow(false)?;
    run_same_server_output_or_jump_flow(true)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn same_server_jump_handles_cross_session_targets() -> TestResult<()> {
    let server = TmuxServer::new("muxboard-e2e-same-cross-session");
    let binary = muxboard_binary();

    server.run(&["new-session", "-d", "-s", "ops", "-n", "ui", "bash"])?;
    server.run(&["new-session", "-d", "-s", "review", "-n", "prompt", "bash"])?;

    let ui_pane = server
        .run(&["list-panes", "-t", "ops:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt_pane = server
        .run(&["list-panes", "-t", "review:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let command = isolated_muxboard_command(
        &binary,
        &server.socket,
        "review",
        "same-server-cross-session",
    );
    server.send_literal(&ui_pane, &command)?;
    server.send_keys(&ui_pane, &["Enter"])?;

    wait_for_muxboard_surface(&server, &ui_pane)?;
    wait_for_selected_row(&server, &ui_pane, "review/prompt")?;
    server.send_keys(&ui_pane, &["g"])?;
    assert_muxboard_still_running(&server, &ui_pane)?;
    server.wait_for_field(&prompt_pane, "#{pane_active}", "1")?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn search_cancel_restores_the_previous_filter() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-search-target");
    let driver = TmuxServer::new("muxboard-e2e-search-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt2",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "search-cancel");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.type_keys_slowly(
        &driver_pane,
        &["/", "p", "r", "o", "m", "p", "t", "2", "Enter"],
        Duration::from_millis(80),
    )?;

    let filtered = wait_for_search_result(&driver, &driver_pane, "prompt2", "ops/prompt2")?;
    assert!(filtered.contains("ops/prompt2"));
    assert!(!filtered.contains("* prompt ("));

    driver.send_keys(&driver_pane, &["/"])?;
    wait_for_search_input_surface(&driver, &driver_pane)?;
    driver.type_keys_slowly(
        &driver_pane,
        &[
            "BSpace", "BSpace", "BSpace", "BSpace", "BSpace", "BSpace", "BSpace",
        ],
        Duration::from_millis(40),
    )?;
    driver.type_keys_slowly(
        &driver_pane,
        &["p", "r", "o", "m", "p", "t"],
        Duration::from_millis(80),
    )?;
    driver.send_keys(&driver_pane, &["Escape"])?;
    let restored = wait_for_search_result(&driver, &driver_pane, "prompt2", "ops/prompt2")?;

    assert!(restored.contains("ops/prompt2"));
    assert!(!restored.contains("* prompt ("));

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn target_set_stays_obvious_while_selection_moves() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-target-clarity-target");
    let driver = TmuxServer::new("muxboard-e2e-target-clarity-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt2",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "target-clarity");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;

    driver.send_keys(&driver_pane, &["Space"])?;
    let targeted =
        wait_for_send_list_target_state(&driver, &driver_pane, "send list (1 pane)", "ops/prompt")?;
    assert!(targeted.contains("send list (1 pane)"));
    assert!(targeted.contains("+ ops/prompt"));

    driver.send_keys(&driver_pane, &["j"])?;
    let moved = wait_for_main_board_surface(
        &driver,
        &driver_pane,
        "ops/prompt2",
        &["send list (1 pane)", "+ ops/prompt"],
    )?;
    assert!(moved.contains("send list (1 pane)"));
    assert!(moved.contains("send list (1 pane)"));
    assert!(moved.contains("+ ops/prompt"));
    assert!(moved.contains("ops/prompt2"));

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn action_menu_clear_marks_resets_targeting_to_the_selected_pane() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-actions-target");
    let driver = TmuxServer::new("muxboard-e2e-actions-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt2",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "clear-marks");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt_pane = target
        .run(&["list-panes", "-t", "ops:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt2_pane = target
        .run(&["list-panes", "-t", "ops:prompt2", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.type_keys_slowly(
        &driver_pane,
        &["/", "p", "r", "o", "m", "p", "t", "Enter"],
        Duration::from_millis(80),
    )?;
    wait_for_search_result(&driver, &driver_pane, "prompt", "ops/prompt")?;

    driver.send_keys(&driver_pane, &["Space"])?;
    wait_for_send_list_target_state(&driver, &driver_pane, "send list (1 pane)", "ops/prompt")?;
    driver.send_keys(&driver_pane, &["j"])?;
    driver.send_keys(&driver_pane, &["Space"])?;
    wait_for_send_list_target_state(&driver, &driver_pane, "send list (2 panes)", "ops/prompt2")?;

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "X clear")?;
    driver.send_keys(&driver_pane, &["x"])?;
    wait_for_clear_send_list_action(&driver, &driver_pane, "ops/prompt2")?;

    driver.send_keys(&driver_pane, &[":"])?;
    wait_for_send_surface(&driver, &driver_pane)?;
    driver.send_literal(&driver_pane, "printf CLEAR_TARGET_SET_E2E\\n")?;
    wait_for_send_command_text(&driver, &driver_pane, "CLEAR_TARGET_SET_E2E")?;
    driver.send_keys(&driver_pane, &["Enter"])?;

    assert!(
        !target
            .capture(&prompt_pane)?
            .contains("CLEAR_TARGET_SET_E2E")
    );
    target.wait_for_text(&prompt2_pane, "CLEAR_TARGET_SET_E2E")?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn review_send_cancel_keeps_targets_safe_and_recovers_cleanly() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-stage-cancel-target");
    let driver = TmuxServer::new("muxboard-e2e-stage-cancel-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt2",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "stage-cancel");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt_pane = target
        .run(&["list-panes", "-t", "ops:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt2_pane = target
        .run(&["list-panes", "-t", "ops:prompt2", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.type_keys_slowly(
        &driver_pane,
        &["/", "p", "r", "o", "m", "p", "t", "Enter"],
        Duration::from_millis(80),
    )?;
    wait_for_search_result(&driver, &driver_pane, "prompt", "ops/prompt")?;

    driver.send_keys(&driver_pane, &["Space"])?;
    wait_for_send_list_target_state(&driver, &driver_pane, "send list (1 pane)", "ops/prompt")?;
    driver.send_keys(&driver_pane, &["j"])?;
    driver.send_keys(&driver_pane, &["Space"])?;
    wait_for_send_list_target_state(&driver, &driver_pane, "send list (2 panes)", "ops/prompt2")?;

    driver.send_keys(&driver_pane, &[":"])?;
    wait_for_send_surface(&driver, &driver_pane)?;
    driver.send_literal(&driver_pane, "printf STAGED_CANCEL_SHOULD_NOT_SEND\\n")?;
    wait_for_send_command_text(&driver, &driver_pane, "STAGED_CANCEL_SHOULD_NOT_SEND")?;
    driver.send_keys(&driver_pane, &["Enter"])?;
    let review = wait_for_review_surface(&driver, &driver_pane, "Review send to 2 panes.")?;
    assert!(review.contains("To: the send list (2 panes)"), "{review}");
    assert!(
        review.contains("Text: printf STAGED_CANCEL_SHOULD_NOT_SEND"),
        "{review}"
    );
    assert!(review.contains("Targets"), "{review}");
    assert!(review.contains("Enter send"), "{review}");
    assert!(review.contains("Esc cancel"), "{review}");

    driver.send_keys(&driver_pane, &["Escape"])?;
    let canceled = wait_for_send_list_target_state(
        &driver,
        &driver_pane,
        "send list (2 panes)",
        "ops/prompt2",
    )?;
    assert_muxboard_still_running(&driver, &driver_pane)?;
    assert!(canceled.contains("ops/prompt"));
    assert!(canceled.contains("ops/prompt2"));
    assert!(!canceled.contains(": ops/prompt"));
    assert!(!canceled.contains(">: ops/prompt2"));

    wait_for_target_text_absent(&target, &prompt_pane, "STAGED_CANCEL_SHOULD_NOT_SEND")?;
    wait_for_target_text_absent(&target, &prompt2_pane, "STAGED_CANCEL_SHOULD_NOT_SEND")?;

    driver.send_keys(&driver_pane, &[":"])?;
    wait_for_send_surface(&driver, &driver_pane)?;
    driver.send_literal(&driver_pane, "printf STAGED_CANCEL_RECOVERED\\n")?;
    wait_for_send_command_text(&driver, &driver_pane, "STAGED_CANCEL_RECOVERED")?;
    driver.send_keys(&driver_pane, &["Enter"])?;
    let recovered_review =
        wait_for_review_surface(&driver, &driver_pane, "Review send to 2 panes.")?;
    assert!(
        recovered_review.contains("To: the send list (2 panes)"),
        "{recovered_review}"
    );
    assert!(
        recovered_review.contains("Text: printf STAGED_CANCEL_RECOVERED"),
        "{recovered_review}"
    );
    assert!(recovered_review.contains("Targets"), "{recovered_review}");
    assert!(
        recovered_review.contains("Enter send"),
        "{recovered_review}"
    );
    assert!(
        recovered_review.contains("Esc cancel"),
        "{recovered_review}"
    );
    driver.send_keys(&driver_pane, &["Enter"])?;

    target.wait_for_text(&prompt_pane, "STAGED_CANCEL_RECOVERED")?;
    target.wait_for_text(&prompt2_pane, "STAGED_CANCEL_RECOVERED")?;
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn review_send_survives_target_pane_disappearing_before_confirm() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-stage-disappear-target");
    let driver = TmuxServer::new("muxboard-e2e-stage-disappear-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt2",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "stage-disappear");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt_pane = target
        .run(&["list-panes", "-t", "ops:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt2_pane = target
        .run(&["list-panes", "-t", "ops:prompt2", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.type_keys_slowly(
        &driver_pane,
        &["/", "p", "r", "o", "m", "p", "t", "Enter"],
        Duration::from_millis(80),
    )?;
    wait_for_search_result(&driver, &driver_pane, "prompt", "ops/prompt")?;
    driver.send_keys(&driver_pane, &["Space"])?;
    wait_for_send_list_target_state(&driver, &driver_pane, "send list (1 pane)", "ops/prompt")?;
    driver.send_keys(&driver_pane, &["j"])?;
    driver.send_keys(&driver_pane, &["Space"])?;
    wait_for_send_list_target_state(&driver, &driver_pane, "send list (2 panes)", "ops/prompt2")?;

    driver.send_keys(&driver_pane, &[":"])?;
    wait_for_send_surface(&driver, &driver_pane)?;
    driver.send_literal(&driver_pane, "echo DISAPPEAR_SURVIVOR")?;
    wait_for_send_command_text(&driver, &driver_pane, "DISAPPEAR_SURVIVOR")?;
    driver.send_keys(&driver_pane, &["Enter"])?;
    let review = wait_for_review_surface(&driver, &driver_pane, "Review send to 2 panes.")?;
    assert!(review.contains("Enter send"), "{review}");
    assert!(review.contains("Esc cancel"), "{review}");

    target.run(&["kill-pane", "-t", &prompt2_pane])?;
    driver.send_keys(&driver_pane, &["Enter"])?;

    target.wait_for_text(&prompt_pane, "DISAPPEAR_SURVIVOR")?;
    let screen = wait_for_review_dispatch_result(
        &driver,
        &driver_pane,
        &[
            "1 pane disappeared",
            "send list 1 pane",
            "DISAPPEAR_SURVIVOR",
            "1-1 / 1",
            "ops/prompt",
        ],
        &["ops/prompt2"],
    )?;
    assert!(screen.contains("send list 1 pane"), "{screen}");
    assert!(screen.contains("DISAPPEAR_SURVIVOR"), "{screen}");
    assert!(screen.contains("1-1 / 1"), "{screen}");
    assert!(!screen.contains("ops/prompt2"), "{screen}");
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn review_send_recovers_when_every_target_pane_disappears_before_confirm() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-stage-all-disappear-target");
    let driver = TmuxServer::new("muxboard-e2e-stage-all-disappear-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "anchor",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt2",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "stage-all-disappear");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let anchor_pane = target
        .run(&["list-panes", "-t", "ops:anchor", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt_pane = target
        .run(&["list-panes", "-t", "ops:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt2_pane = target
        .run(&["list-panes", "-t", "ops:prompt2", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.type_keys_slowly(
        &driver_pane,
        &["/", "p", "r", "o", "m", "p", "t", "Enter"],
        Duration::from_millis(80),
    )?;
    wait_for_search_result(&driver, &driver_pane, "prompt", "ops/prompt")?;
    driver.send_keys(&driver_pane, &["Space"])?;
    wait_for_send_list_target_state(&driver, &driver_pane, "send list (1 pane)", "ops/prompt")?;
    driver.send_keys(&driver_pane, &["j"])?;
    driver.send_keys(&driver_pane, &["Space"])?;
    wait_for_send_list_target_state(&driver, &driver_pane, "send list (2 panes)", "ops/prompt2")?;

    driver.send_keys(&driver_pane, &[":"])?;
    wait_for_send_surface(&driver, &driver_pane)?;
    driver.send_literal(&driver_pane, "echo DISAPPEAR_NONE")?;
    wait_for_send_command_text(&driver, &driver_pane, "DISAPPEAR_NONE")?;
    driver.send_keys(&driver_pane, &["Enter"])?;
    let review = wait_for_review_surface(&driver, &driver_pane, "Review send to 2 panes.")?;
    assert!(review.contains("Enter send"), "{review}");
    assert!(review.contains("Esc cancel"), "{review}");

    target.run(&["kill-pane", "-t", &prompt_pane])?;
    target.run(&["kill-pane", "-t", &prompt2_pane])?;
    driver.send_keys(&driver_pane, &["Enter"])?;

    let screen = wait_for_review_dispatch_result(
        &driver,
        &driver_pane,
        &["No panes remain", "2 panes disappeared"],
        &["ops/prompt", "send list"],
    )?;
    assert!(screen.contains("2 panes disappeared"), "{screen}");
    assert!(!screen.contains("ops/prompt"), "{screen}");
    assert!(!screen.contains("send list"), "{screen}");
    assert!(
        !target.capture(&anchor_pane)?.contains("DISAPPEAR_NONE"),
        "anchor pane must not receive a command meant only for vanished targets"
    );
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn refresh_recovers_from_stale_waiting_output_after_a_real_state_change() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-refresh-target");
    let driver = TmuxServer::new("muxboard-e2e-refresh-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "wait",
        "bash",
        "-lc",
        "echo 'Waiting for approval. Continue?'; read _; echo 'error: command failed'; sleep 1; echo 'done'; sleep 30",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "refresh-after-wait");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let wait_pane = target
        .run(&["list-panes", "-t", "ops:wait", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_main_board_surface(
        &driver,
        &driver_pane,
        "ops/wait",
        &["Fleet | 1-1 / 1 | 1 needs you", "State: Waiting"],
    )?;
    target.send_keys(&wait_pane, &["Enter"])?;
    target.wait_for_text(&wait_pane, "error: command failed")?;
    driver.send_keys(&driver_pane, &["r"])?;

    let error_screen = wait_for_refresh_result(
        &driver,
        &driver_pane,
        &["Problem: command failed", "State: Error   Tool: Shell"],
        &["State: Waiting"],
    )?;
    assert!(error_screen.contains("1 needs you"));
    assert!(error_screen.contains("State: Error   Tool: Shell"));
    assert!(error_screen.contains("Tool: Shell"));
    assert!(error_screen.contains("Problem: command failed"));

    target.wait_for_text(&wait_pane, "done")?;
    driver.send_keys(&driver_pane, &["r"])?;

    let done_screen = wait_for_refresh_result(
        &driver,
        &driver_pane,
        &["Fleet | 1-1 / 1 | all quiet", "State: Done"],
        &["State: Error"],
    )?;
    assert!(done_screen.contains("Fleet | 1-1 / 1 | all quiet"));
    assert!(done_screen.contains("State: Done"));
    assert!(!done_screen.contains("Problem: command failed"));
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn live_status_update_replaces_stale_latest_and_next() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-fresh-status-target");
    let driver = TmuxServer::new("muxboard-e2e-fresh-status-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "agent",
        "bash",
        "-lc",
        "while IFS= read -r line; do printf '%s\\n' \"$line\"; done",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "fresh-status");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let agent_pane = target
        .run(&["list-panes", "-t", "ops:agent", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;
    target.send_literal(
        &agent_pane,
        "STATUS=running | BLOCKER=none | NEXT=old stale action",
    )?;
    target.send_keys(&agent_pane, &["Enter"])?;
    wait_for_live_status_summary(&driver, &driver_pane, "ops/agent", "old stale action", &[])?;

    target.send_literal(&agent_pane, "STATUS=running | BLOCKER=none | NEXT=ship fix")?;
    target.send_keys(&agent_pane, &["Enter"])?;
    let update_started = Instant::now();
    let fresh = wait_for_live_status_summary_with_poll(
        &driver,
        &driver_pane,
        "ops/agent",
        "ship fix",
        &["old stale action"],
        RESPONSIVE_STATE_UPDATE_TIMEOUT,
        FAST_POLL_INTERVAL,
    )?;
    let update_elapsed = update_started.elapsed();
    assert!(
        update_elapsed < RESPONSIVE_STATE_UPDATE_TIMEOUT,
        "live status update should replace stale text inside {RESPONSIVE_STATE_UPDATE_TIMEOUT:?}, took {update_elapsed:?}"
    );

    assert!(!fresh.contains("old stale action"));
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn carriage_return_progress_updates_follow_visible_pane_state() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-cr-progress-target");
    let driver = TmuxServer::new("muxboard-e2e-cr-progress-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "job",
        "bash",
        "-lc",
        "printf 'loading'; sleep 0.4; printf '\\rthinking'; sleep 0.4; printf '\\rready      \\n'; sleep 30",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "carriage-return-progress");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let screen = wait_for_main_board_surface(&driver, &driver_pane, "ops/job", &["ready"])?;
    assert!(screen.contains("ready"));
    assert!(!screen.contains("loadingthinking"));
    assert!(!screen.contains("loadingready"));

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn multi_pane_churn_keeps_attention_current() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-churn-target");
    let driver = TmuxServer::new("muxboard-e2e-churn-driver");
    let session = "ops";
    let binary = muxboard_binary();

    let churn_script = "echo 'Waiting for approval. Continue?'; read _; echo 'error: command failed'; sleep 5; echo 'done'; sleep 30";

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "wait01",
        "bash",
        "-lc",
        churn_script,
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "wait02",
        "bash",
        "-lc",
        churn_script,
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "multi-pane-churn");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let wait01_pane = target
        .run(&["list-panes", "-t", "ops:wait01", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let wait02_pane = target
        .run(&["list-panes", "-t", "ops:wait02", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let initial = wait_for_main_board_surface(
        &driver,
        &driver_pane,
        "ops/wait01",
        &["Fleet | 1-3 / 3 | 2 need you", "! ops/wait02"],
    )?;
    assert!(initial.contains(">! ops/wait01"));
    assert!(initial.contains("! ops/wait02"));

    target.send_keys(&wait01_pane, &["Enter"])?;
    target.wait_for_text(&wait01_pane, "error: command failed")?;
    driver.send_keys(&driver_pane, &["r"])?;
    let first_error = wait_for_refresh_result(
        &driver,
        &driver_pane,
        &[
            "Fleet | 1-3 / 3 | 2 need you",
            ">! ops/wait01",
            "! ops/wait02",
            "Problem: command failed",
        ],
        &["ops/wait01     waiting  "],
    )?;
    assert!(first_error.contains("Fleet | 1-3 / 3 | 2 need you"));
    assert!(first_error.contains(">! ops/wait01"));
    assert!(first_error.contains("! ops/wait02"));
    assert!(first_error.contains("Problem: command failed"));

    target.send_keys(&wait02_pane, &["Enter"])?;
    target.wait_for_text(&wait02_pane, "error: command failed")?;
    driver.send_keys(&driver_pane, &["r"])?;
    let second_error = wait_for_refresh_result(
        &driver,
        &driver_pane,
        &["ops/wait02", "failed"],
        &["ops/wait02     waiting"],
    )?;
    assert!(second_error.contains("Fleet | 1-3 / 3 |"));
    assert!(second_error.contains("! ops/wait02"));
    assert!(second_error.contains("command failed"));
    assert!(!second_error.contains("ops/wait01     waiting"));
    assert!(!second_error.contains("ops/wait01     waiting  "));

    target.wait_for_text(&wait01_pane, "done")?;
    target.wait_for_text(&wait02_pane, "done")?;
    driver.send_keys(&driver_pane, &["r"])?;
    let settled = wait_for_refresh_result(
        &driver,
        &driver_pane,
        &["Fleet | 1-3 / 3 | all quiet", "ops/wait01", "ops/wait02"],
        &["State: Error"],
    )?;
    assert!(settled.contains("Fleet | 1-3 / 3 | all quiet"));
    assert!(settled.contains("ops/wait01"));
    assert!(settled.contains("ops/wait02"));

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn saved_target_group_can_be_reloaded_and_used_for_broadcast() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-groups-target");
    let driver = TmuxServer::new("muxboard-e2e-groups-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt2",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command = isolated_muxboard_command(&binary, &target.socket, session, "groups");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt_pane = target
        .run(&["list-panes", "-t", "ops:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt2_pane = target
        .run(&["list-panes", "-t", "ops:prompt2", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.type_keys_slowly(
        &driver_pane,
        &["/", "p", "r", "o", "m", "p", "t", "Enter"],
        Duration::from_millis(80),
    )?;
    wait_for_search_result(&driver, &driver_pane, "prompt", "ops/prompt")?;

    driver.send_keys(&driver_pane, &["Space"])?;
    wait_for_send_list_target_state(&driver, &driver_pane, "send list (1 pane)", "ops/prompt")?;
    driver.send_keys(&driver_pane, &["j"])?;
    driver.send_keys(&driver_pane, &["Space"])?;
    wait_for_send_list_target_state(&driver, &driver_pane, "send list (2 panes)", "ops/prompt2")?;

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "save fleet")?;
    driver.send_keys(&driver_pane, &["g"])?;
    wait_for_save_fleet_input(&driver, &driver_pane)?;
    driver.type_keys_slowly(
        &driver_pane,
        &["t", "r", "i", "a", "g", "e"],
        Duration::from_millis(80),
    )?;
    driver.send_keys(&driver_pane, &["Enter"])?;
    wait_for_saved_fleet_active(&driver, &driver_pane, "triage", "2 panes")?;

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "X clear")?;
    driver.send_keys(&driver_pane, &["x"])?;
    wait_for_clear_send_list_action(&driver, &driver_pane, "ops/prompt2")?;

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "choose fleet")?;
    driver.send_keys(&driver_pane, &["l"])?;
    wait_for_saved_fleet_picker(&driver, &driver_pane, "triage", "2/2 live", true)?;
    driver.send_keys(&driver_pane, &["Enter"])?;
    wait_for_saved_fleet_active(&driver, &driver_pane, "triage", "2 panes")?;

    driver.send_keys(&driver_pane, &[":"])?;
    wait_for_send_surface(&driver, &driver_pane)?;
    driver.send_literal(&driver_pane, "printf GROUP_RESTORED_E2E\\n")?;
    wait_for_send_command_text(&driver, &driver_pane, "GROUP_RESTORED_E2E")?;
    driver.send_keys(&driver_pane, &["Enter"])?;
    wait_for_review_surface(&driver, &driver_pane, "Review send to 2 panes.")?;
    assert!(!target.capture(&prompt_pane)?.contains("GROUP_RESTORED_E2E"));
    assert!(
        !target
            .capture(&prompt2_pane)?
            .contains("GROUP_RESTORED_E2E")
    );

    driver.send_keys(&driver_pane, &["Enter"])?;
    target.wait_for_text(&prompt_pane, "GROUP_RESTORED_E2E")?;
    target.wait_for_text(&prompt2_pane, "GROUP_RESTORED_E2E")?;
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn stale_saved_fleet_stays_recoverable_after_live_pane_disappears() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-stale-fleet-target");
    let driver = TmuxServer::new("muxboard-e2e-stale-fleet-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "keep",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "stale-fleet");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;
    driver.resize_window("driver:ui", 100, 24)?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt_pane = target
        .run(&["list-panes", "-t", "ops:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.type_keys_slowly(
        &driver_pane,
        &["/", "p", "r", "o", "m", "p", "t", "Enter"],
        Duration::from_millis(80),
    )?;
    wait_for_search_result(&driver, &driver_pane, "prompt", "ops/prompt")?;
    driver.send_keys(&driver_pane, &["Space"])?;
    wait_for_send_list_target_state(&driver, &driver_pane, "send list (1 pane)", "ops/prompt")?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "save fleet")?;
    driver.send_keys(&driver_pane, &["g"])?;
    wait_for_save_fleet_input(&driver, &driver_pane)?;
    driver.type_keys_slowly(
        &driver_pane,
        &["t", "r", "i", "a", "g", "e"],
        Duration::from_millis(80),
    )?;
    driver.send_keys(&driver_pane, &["Enter"])?;
    wait_for_saved_fleet_active(&driver, &driver_pane, "triage", "1 pane")?;

    driver.send_keys(&driver_pane, &["BSpace"])?;
    wait_for_main_board_surface(
        &driver,
        &driver_pane,
        "ops/prompt",
        &["ops/keep", "Send: fleet triage (1 pane)"],
    )?;

    target.run(&["kill-pane", "-t", &prompt_pane])?;
    driver.send_keys(&driver_pane, &["r"])?;
    let stale_screen = wait_for_refresh_result(
        &driver,
        &driver_pane,
        &[
            "fleet triage has no live panes",
            "Target: fleet triage has no live panes",
            "fleet stale",
        ],
        &[": send"],
    )?;
    assert!(
        stale_screen.contains("Target: fleet triage has no live panes"),
        "{stale_screen}"
    );
    assert!(stale_screen.contains("fleet stale"), "{stale_screen}");
    assert!(!stale_screen.contains(": send"), "{stale_screen}");

    driver.send_keys(&driver_pane, &[":"])?;
    let blocked_send =
        wait_for_inert_send_key(&driver, &driver_pane, "fleet triage has no live panes")?;
    assert!(
        blocked_send.contains("fleet triage has no live panes")
            || blocked_send.contains("Fleet `triage` has no live pa"),
        "{blocked_send}"
    );
    assert!(!blocked_send.contains("Send to"), "{blocked_send}");

    driver.send_keys(&driver_pane, &["."])?;
    let more = wait_for_more_row(&driver, &driver_pane, "D delete stale triage")?;
    assert!(more.contains("L choose fleet"), "{more}");
    assert!(!more.contains(": send text"), "{more}");
    driver.send_keys(&driver_pane, &["l"])?;
    wait_for_saved_fleet_picker(&driver, &driver_pane, "triage", "0/1 live", false)?;
    driver.send_keys(&driver_pane, &["Escape"])?;
    wait_for_stale_fleet_board_after_picker_escape(&driver, &driver_pane)?;

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "D delete stale triage")?;
    driver.send_keys(&driver_pane, &["d"])?;
    let deleted = wait_for_main_board_surface_without(
        &driver,
        &driver_pane,
        "ops/keep",
        &["Deleted fleet `triage`."],
        &["fleet triage has no live panes"],
    )?;
    assert!(
        !deleted.contains("fleet triage has no live panes"),
        "{deleted}"
    );
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

fn run_output_or_jump_flow(target: &TmuxServer, should_jump: bool) -> TestResult<()> {
    let driver = TmuxServer::new(if should_jump {
        "muxboard-e2e-jump-driver"
    } else {
        "muxboard-e2e-focus-driver"
    });
    let binary = muxboard_binary();
    let driver_session = if should_jump { "jump" } else { "focus" };

    driver.run(&[
        "new-session",
        "-d",
        "-s",
        driver_session,
        "-n",
        "ui",
        "bash",
    ])?;

    let driver_pane = driver
        .run(&[
            "list-panes",
            "-t",
            &format!("{driver_session}:ui"),
            "-F",
            "#{pane_id}",
        ])?
        .trim()
        .to_owned();
    let prompt_pane = target
        .run(&["list-panes", "-t", "ops:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let command = isolated_muxboard_command(
        &binary,
        &target.socket,
        "ops",
        if should_jump { "jump" } else { "focus" },
    );
    driver.send_literal(&driver_pane, &command)?;
    driver.send_keys(&driver_pane, &["Enter"])?;

    wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.type_keys_slowly(
        &driver_pane,
        &["/", "p", "r", "o", "m", "p", "t", "Enter"],
        Duration::from_millis(80),
    )?;
    wait_for_search_result(&driver, &driver_pane, "prompt", "ops/prompt")?;

    if should_jump {
        driver.send_keys(&driver_pane, &["g"])?;
        assert_muxboard_still_running(&driver, &driver_pane)?;
        target.wait_for_field(&prompt_pane, "#{pane_active}", "1")?;
    } else {
        driver.send_keys(&driver_pane, &["Enter"])?;
        let screen = wait_for_output_surface(&driver, &driver_pane)?;
        assert!(screen.contains("muxboard"));
        assert!(screen.contains("Output"));
        assert!(screen.contains("No output yet."), "{screen}");
        assert!(!screen.contains("J/K move"), "{screen}");
        assert!(!screen.contains("K older/J newer"), "{screen}");
        assert!(screen.contains("Esc back"), "{screen}");
        assert!(!screen.contains("Enter details"), "{screen}");
        driver.send_keys(&driver_pane, &["Enter"])?;
        let repeated = wait_for_output_surface_to_stay_open(&driver, &driver_pane)?;
        assert!(repeated.contains("Output"), "{repeated}");
        assert!(repeated.contains("Esc back"), "{repeated}");
        assert!(!repeated.contains("Enter details"), "{repeated}");
        driver.send_keys(&driver_pane, &["Escape"])?;
        let details =
            wait_for_output_escape_returns_to_details(&driver, &driver_pane, "ops/prompt")?;
        assert!(details.contains("muxboard"), "{details}");
        assert_muxboard_still_running(&driver, &driver_pane)?;
    }

    Ok(())
}

fn run_same_server_output_or_jump_flow(should_jump: bool) -> TestResult<()> {
    let server = TmuxServer::new(if should_jump {
        "muxboard-e2e-same-jump"
    } else {
        "muxboard-e2e-same-focus"
    });
    let binary = muxboard_binary();

    server.run(&["new-session", "-d", "-s", "ops", "-n", "ui", "bash"])?;
    server.run(&[
        "new-window",
        "-t",
        "ops",
        "-n",
        "prompt",
        "bash",
        "-lc",
        "bash",
    ])?;

    let ui_pane = server
        .run(&["list-panes", "-t", "ops:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let prompt_pane = server
        .run(&["list-panes", "-t", "ops:prompt", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let command = isolated_muxboard_command(
        &binary,
        &server.socket,
        "ops",
        if should_jump {
            "same-server-jump"
        } else {
            "same-server-focus"
        },
    );
    server.send_literal(&ui_pane, &command)?;
    server.send_keys(&ui_pane, &["Enter"])?;

    wait_for_muxboard_surface(&server, &ui_pane)?;
    apply_search_query(&server, &ui_pane, "prompt", "ops/prompt")?;

    if should_jump {
        server.send_keys(&ui_pane, &["g"])?;
        assert_muxboard_still_running(&server, &ui_pane)?;
        server.wait_for_field(&prompt_pane, "#{pane_active}", "1")?;
    } else {
        server.send_keys(&ui_pane, &["Enter"])?;
        let screen = wait_for_output_surface(&server, &ui_pane)?;
        assert!(screen.contains("muxboard"));
        assert!(screen.contains("Output"));
        assert!(screen.contains("No output yet."), "{screen}");
        assert!(!screen.contains("J/K move"), "{screen}");
        assert!(!screen.contains("K older/J newer"), "{screen}");
        assert!(screen.contains("Esc back"), "{screen}");
        assert!(!screen.contains("Enter details"), "{screen}");
        server.send_keys(&ui_pane, &["Enter"])?;
        let repeated = wait_for_output_surface_to_stay_open(&server, &ui_pane)?;
        assert!(repeated.contains("Output"), "{repeated}");
        assert!(repeated.contains("Esc back"), "{repeated}");
        assert!(!repeated.contains("Enter details"), "{repeated}");
        server.send_keys(&ui_pane, &["Escape"])?;
        let details = wait_for_output_escape_returns_to_details(&server, &ui_pane, "ops/prompt")?;
        assert!(details.contains("muxboard"), "{details}");
        assert_muxboard_still_running(&server, &ui_pane)?;
    }

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn opening_output_marks_explicit_agent_review_seen_live() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-bridge-seen-target");
    let driver = TmuxServer::new("muxboard-e2e-bridge-seen-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "review",
        "bash",
        "-lc",
        "printf 'all checks passed\\n'; bash",
    ])?;
    let agent_pane = target
        .run(&["list-panes", "-t", "ops:review", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let fragment = pane_env_fragment(&agent_pane);
    for (suffix, value) in [
        ("AGENT", "codex"),
        ("STATE", "done"),
        ("SUMMARY", "release ready"),
        ("THREAD_NAME", "Ship V1"),
        ("PROGRESS", "10/10"),
        ("UNSEEN", "1"),
    ] {
        target.run(&[
            "set-environment",
            "-g",
            &format!("MUXBOARD_AGENT_PANE_{fragment}_{suffix}"),
            value,
        ])?;
    }

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "bridge-seen-live");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;
    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_main_board_surface(
        &driver,
        &driver_pane,
        "ops/review",
        &["release ready", "10/10"],
    )?;
    driver.send_keys(&driver_pane, &["Enter"])?;
    let output = wait_for_output_surface(&driver, &driver_pane)?;
    assert!(output.contains("Output"), "{output}");
    wait_for_tmux_env_value(
        &target,
        &format!("MUXBOARD_AGENT_PANE_{fragment}_UNSEEN"),
        "0",
    )?;
    wait_for_tmux_env_value(
        &target,
        &format!("TMUX_AGENT_PANE_{agent_pane}_UNSEEN"),
        "0",
    )?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn command_center_escape_returns_to_fleet_details_in_live_tmux() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-command-center-escape-target");
    let driver = TmuxServer::new("muxboard-e2e-command-center-escape-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "triage",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "command-center-escape");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;
    driver.resize_window("driver:ui", 80, 20)?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "command center")?;
    driver.send_keys(&driver_pane, &["]"])?;

    let command_center = wait_for_command_center_surface(&driver, &driver_pane)?;
    assert!(command_center.contains("Esc back"), "{command_center}");

    driver.send_keys(&driver_pane, &["Escape"])?;
    let restored =
        wait_for_command_center_escape_returns_to_details(&driver, &driver_pane, "ops/triage")?;
    assert!(restored.contains("Details"), "{restored}");
    assert!(!restored.contains("Command Center"), "{restored}");
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn command_center_primary_action_continues_waiting_agent() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-command-center-action-target");
    let driver = TmuxServer::new("muxboard-e2e-command-center-action-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "wait",
        "bash",
        "-lc",
        "echo 'Waiting for approval. Press Enter to continue.'; read _; echo CENTER_CONTINUE_OK; exec bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "command-center-action");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;
    driver.resize_window("driver:ui", 60, 12)?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let wait_pane = target
        .run(&["list-panes", "-t", "ops:wait", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    target.wait_for_text(&wait_pane, "Press Enter to continue.")?;
    wait_for_main_board_surface(&driver, &driver_pane, "ops/wait", &["needs you: continue"])?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "command center")?;
    driver.send_keys(&driver_pane, &["]"])?;

    let command_center = wait_for_command_center_surface(&driver, &driver_pane)?;
    assert!(command_center.contains("Action:"), "{command_center}");
    assert!(command_center.contains("continue"), "{command_center}");
    assert!(
        command_center.contains("ops/wait") || command_center.contains("ops / wait"),
        "{command_center}"
    );
    assert!(command_center.contains("A continue"), "{command_center}");
    assert!(command_center.contains("Esc back"), "{command_center}");
    assert!(!command_center.contains("Enter output"), "{command_center}");

    driver.send_keys(&driver_pane, &["a"])?;
    target.wait_for_text(&wait_pane, "CENTER_CONTINUE_OK")?;
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn command_center_large_attention_queue_shows_overflow_live() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-command-center-overflow-target");
    let driver = TmuxServer::new("muxboard-e2e-command-center-overflow-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "agent-0",
        "bash",
        "-lc",
        "printf 'Press Enter to continue.\\n'; IFS= read -r _; exec bash",
    ])?;
    for index in 1..8 {
        target.run(&[
            "new-window",
            "-t",
            session,
            "-n",
            &format!("agent-{index}"),
            "bash",
            "-lc",
            "printf 'Press Enter to continue.\\n'; IFS= read -r _; exec bash",
        ])?;
    }

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "command-center-overflow");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;
    driver.resize_window("driver:ui", 80, 14)?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_main_board_surface(&driver, &driver_pane, "ops/agent-0", &["8 need you"])?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "command center")?;
    driver.send_keys(&driver_pane, &["]"])?;

    let command_center = wait_for_command_center_surface(&driver, &driver_pane)?;
    assert!(
        command_center.contains("Needs you: 8 waiting"),
        "{command_center}"
    );
    assert!(
        command_center.contains("> continue ops / agent-0")
            || command_center.contains("> continue ops/agent-0"),
        "{command_center}"
    );
    assert!(
        command_center.contains("+ 2 more need you: continue"),
        "compact live Command Center must not hide attention overflow:\n{command_center}"
    );
    assert!(!command_center.contains("agent-6"), "{command_center}");
    assert!(!command_center.contains("agent-7"), "{command_center}");
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn command_center_primary_action_answers_choice_prompt() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-command-center-answer-target");
    let driver = TmuxServer::new("muxboard-e2e-command-center-answer-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "choice",
        "bash",
        "-lc",
        "echo 'Allow command? [y/n]'; read answer; echo CENTER_ANSWER_OK:$answer; exec bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "command-center-answer");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;
    driver.resize_window("driver:ui", 72, 14)?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    let choice_pane = target
        .run(&["list-panes", "-t", "ops:choice", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    target.wait_for_text(&choice_pane, "Allow command? [y/n]")?;
    wait_for_main_board_surface(
        &driver,
        &driver_pane,
        "ops/choice",
        &["needs you", "answer"],
    )?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "command center")?;
    driver.send_keys(&driver_pane, &["]"])?;

    let command_center = wait_for_command_center_surface(&driver, &driver_pane)?;
    assert!(
        command_center.contains("Action: . answer"),
        "{command_center}"
    );
    assert!(
        command_center.contains("> answer ops/choice")
            || command_center.contains("> answer ops / choice"),
        "{command_center}"
    );
    assert!(command_center.contains(". answer"), "{command_center}");
    assert!(!command_center.contains(". more"), "{command_center}");

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "Y answer yes")?;
    driver.send_keys(&driver_pane, &["y"])?;
    target.wait_for_text(&choice_pane, "CENTER_ANSWER_OK:y")?;
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn command_center_answer_targets_attention_pane_without_attaching_when_selection_differs()
-> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-command-center-show-target");
    let driver = TmuxServer::new("muxboard-e2e-command-center-show-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "build",
        "bash",
        "-lc",
        "while :; do echo 'building'; sleep 1; done",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "approval",
        "bash",
        "-lc",
        "echo 'Allow command? [y/n]'; read answer; echo CENTER_SHOW_OK:$answer; exec bash",
    ])?;
    target.run(&["select-window", "-t", "ops:build"])?;

    let approval_pane = target
        .run(&["list-panes", "-t", "ops:approval", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();
    target.wait_for_text(&approval_pane, "Allow command? [y/n]")?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "command-center-answer");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;
    driver.resize_window("driver:ui", 110, 18)?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_main_board_surface(
        &driver,
        &driver_pane,
        "ops/approval",
        &["needs you", "answer"],
    )?;
    driver.send_keys(&driver_pane, &["j"])?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "command center")?;
    driver.send_keys(&driver_pane, &["]"])?;

    let command_center = wait_for_command_center_surface(&driver, &driver_pane)?;
    assert!(
        command_center.contains("Action: . answer ops/approval")
            || command_center.contains("Action: . answer ops / approval"),
        "{command_center}"
    );
    assert!(
        command_center.contains("Selected: ops/build")
            || command_center.contains("Selected: ops / build"),
        "{command_center}"
    );
    assert_eq!(target.display_field("ops:", "#{window_name}")?, "build");

    driver.send_keys(&driver_pane, &["."])?;
    let answer_menu = wait_for_more_row(&driver, &driver_pane, "Y answer yes")?;
    assert!(
        answer_menu.contains("ops/approval") || answer_menu.contains("ops / approval"),
        "{answer_menu}"
    );
    assert_eq!(target.display_field("ops:", "#{window_name}")?, "build");
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn browse_escape_returns_to_fleet_details_in_live_tmux() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-browse-escape-target");
    let driver = TmuxServer::new("muxboard-e2e-browse-escape-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "triage",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "browse-escape");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;
    driver.resize_window("driver:ui", 80, 20)?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    wait_for_muxboard_surface(&driver, &driver_pane)?;
    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "browse windows")?;
    driver.send_keys(&driver_pane, &["["])?;

    let browse = wait_for_browse_surface(&driver, &driver_pane)?;
    assert!(browse.contains("Esc back"), "{browse}");

    driver.send_keys(&driver_pane, &["Escape"])?;
    let restored = wait_for_browse_escape_returns_to_details(&driver, &driver_pane, "ops/triage")?;
    assert!(restored.contains("Details"), "{restored}");
    assert!(!restored.contains("Browse"), "{restored}");
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

#[test]
#[ignore = "requires tmux and runs a live muxboard instance"]
fn browse_enter_scopes_to_live_window_and_backspace_recovers() -> TestResult<()> {
    let target = TmuxServer::new("muxboard-e2e-browse-scope-target");
    let driver = TmuxServer::new("muxboard-e2e-browse-scope-driver");
    let session = "ops";
    let binary = muxboard_binary();

    target.run(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "triage",
        "bash",
        "-lc",
        "bash",
    ])?;
    target.run(&[
        "new-window",
        "-t",
        session,
        "-n",
        "deploy",
        "bash",
        "-lc",
        "bash",
    ])?;

    let muxboard_command =
        isolated_muxboard_command(&binary, &target.socket, session, "browse-scope");
    driver.run(&[
        "new-session",
        "-d",
        "-s",
        "driver",
        "-n",
        "ui",
        &muxboard_command,
    ])?;
    driver.resize_window("driver:ui", 80, 20)?;

    let driver_pane = driver
        .run(&["list-panes", "-t", "driver:ui", "-F", "#{pane_id}"])?
        .trim()
        .to_owned();

    let first_screen =
        wait_for_main_board_surface(&driver, &driver_pane, "ops/deploy", &["ops/triage"])?;
    assert!(first_screen.contains("ops/deploy"), "{first_screen}");

    driver.send_keys(&driver_pane, &["."])?;
    wait_for_more_row(&driver, &driver_pane, "browse windows")?;
    driver.send_keys(&driver_pane, &["["])?;
    let browse = wait_for_browse_surface(&driver, &driver_pane)?;
    assert!(browse.contains("Enter window"), "{browse}");
    assert!(browse.contains("triage"), "{browse}");
    assert!(browse.contains("deploy"), "{browse}");

    driver.send_keys(&driver_pane, &["j"])?;
    driver.send_keys(&driver_pane, &["Enter"])?;
    let scoped = wait_for_browse_scope(&driver, &driver_pane, "triage", &["1-1 / 1"], &["deploy"])?;
    assert!(scoped.contains("triage"), "{scoped}");
    assert!(!scoped.contains("deploy"), "{scoped}");

    driver.send_keys(&driver_pane, &["BSpace"])?;
    let restored =
        wait_for_browse_scope(&driver, &driver_pane, "triage", &["1-2 / 2", "deploy"], &[])?;
    assert!(restored.contains("triage"), "{restored}");
    assert!(restored.contains("deploy"), "{restored}");
    assert_muxboard_still_running(&driver, &driver_pane)?;

    Ok(())
}

fn apply_search_query(
    server: &TmuxServer,
    pane: &str,
    query: &str,
    selected_text: &str,
) -> TestResult<String> {
    server.send_keys(pane, &["/"])?;
    wait_for_search_input_surface(server, pane)?;

    server.type_literal_slowly(pane, query, Duration::from_millis(80))?;
    server.send_keys(pane, &["Enter"])?;

    wait_for_search_result(server, pane, query, selected_text)
}

fn muxboard_binary() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_muxboard")
        .map(PathBuf::from)
        .expect("CARGO_BIN_EXE_muxboard should be set for integration tests")
}

fn isolated_muxboard_command(
    binary: &std::path::Path,
    socket: &str,
    session: &str,
    label: &str,
) -> String {
    isolated_muxboard_command_with_config(binary, socket, session, label, None)
        .expect("isolated muxboard command should build")
}

fn isolated_muxboard_command_without_config(
    binary: &std::path::Path,
    socket: &str,
    session: &str,
    label: &str,
) -> String {
    isolated_muxboard_environment_with_config_and_env(binary, socket, session, label, None, &[])
        .map(|muxboard| muxboard.command)
        .expect("isolated muxboard command without config should build")
}

fn isolated_muxboard_command_with_env(
    binary: &std::path::Path,
    socket: &str,
    session: &str,
    label: &str,
    env: &[(&str, &str)],
) -> TestResult<String> {
    isolated_muxboard_environment_with_config_and_env(
        binary,
        socket,
        session,
        label,
        Some(LIVE_TEST_BASE_CONFIG),
        env,
    )
    .map(|muxboard| muxboard.command)
}

fn isolated_muxboard_command_with_config(
    binary: &std::path::Path,
    socket: &str,
    session: &str,
    label: &str,
    config_json: Option<&str>,
) -> TestResult<String> {
    isolated_muxboard_environment_with_config_and_env(
        binary,
        socket,
        session,
        label,
        config_json.or(Some(LIVE_TEST_BASE_CONFIG)),
        &[],
    )
    .map(|muxboard| muxboard.command)
}

fn isolated_muxboard_environment_with_config(
    binary: &std::path::Path,
    socket: &str,
    session: &str,
    label: &str,
    config_json: Option<&str>,
) -> TestResult<IsolatedMuxboard> {
    isolated_muxboard_environment_with_config_and_env(
        binary,
        socket,
        session,
        label,
        config_json.or(Some(LIVE_TEST_BASE_CONFIG)),
        &[],
    )
}

fn isolated_muxboard_environment_with_config_and_env(
    binary: &std::path::Path,
    socket: &str,
    session: &str,
    label: &str,
    config_json: Option<&str>,
    env: &[(&str, &str)],
) -> TestResult<IsolatedMuxboard> {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = std::env::temp_dir().join(format!("muxboard-live-{label}-{}", unique_suffix()));
    let config_home = root.join("config");
    let state_home = root.join("state");

    if let Some(config_json) = config_json {
        let app_config_dir = config_home.join("muxboard");
        fs::create_dir_all(&app_config_dir)?;
        fs::write(app_config_dir.join("config.json"), config_json)?;
    }

    let mut env_parts = vec![
        format!(
            "XDG_CONFIG_HOME={}",
            shell_quote(&config_home.display().to_string())
        ),
        format!(
            "XDG_STATE_HOME={}",
            shell_quote(&state_home.display().to_string())
        ),
    ];
    env_parts.extend(
        env.iter()
            .map(|(key, value)| format!("{key}={}", shell_quote(value))),
    );

    Ok(IsolatedMuxboard {
        command: format!(
            "cd {} && {} {} --socket {} --session {}",
            shell_quote(&project_root.display().to_string()),
            env_parts.join(" "),
            shell_quote(&binary.display().to_string()),
            shell_quote(socket),
            shell_quote(session)
        ),
        config_file: config_home.join("muxboard").join("config.json"),
        state_file: state_home.join("muxboard").join("state.json"),
    })
}

fn wait_for_notification_flags(
    config_file: &Path,
    desktop_enabled: bool,
    bell_enabled: bool,
) -> TestResult<()> {
    let start = Instant::now();
    loop {
        let raw = fs::read_to_string(config_file).unwrap_or_default();
        let parsed = serde_json::from_str::<Value>(&raw).ok();
        let settings = parsed
            .as_ref()
            .and_then(|value| value.get("notification_settings"));

        let desktop_matches = settings
            .and_then(|settings| settings.get("desktop_enabled"))
            .and_then(Value::as_bool)
            == Some(desktop_enabled);
        let bell_matches = settings
            .and_then(|settings| settings.get("bell_enabled"))
            .and_then(Value::as_bool)
            == Some(bell_enabled);

        if desktop_matches && bell_matches {
            return Ok(());
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for notification settings in {} to become desktop={desktop_enabled} bell={bell_enabled}\nlast config:\n{raw}",
                config_file.display()
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn marker_command(_label: &str, path: &Path) -> String {
    format!("printf 1 > {}", shell_quote(&path.display().to_string()))
}

fn wait_for_file_text(path: &Path, expected: &str) -> TestResult<()> {
    let start = Instant::now();
    loop {
        let actual = fs::read_to_string(path).unwrap_or_default();
        if actual == expected {
            return Ok(());
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for {} to contain `{expected}`, got `{actual}`",
                path.display()
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn run_expect_script(label: &str, script: &str) -> TestResult<()> {
    let root = std::env::temp_dir().join(format!("muxboard-expect-{label}-{}", unique_suffix()));
    fs::create_dir_all(&root)?;
    let script_path = root.join("script.exp");
    fs::write(&script_path, script)?;

    let output = Command::new("expect").arg(&script_path).output()?;
    if !output.status.success() {
        return Err(format!(
            "expect script {} failed: status={} stdout={} stderr={}",
            script_path.display(),
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }

    Ok(())
}

fn live_peek_toggle_expect_script(
    socket: &str,
    session: &str,
    marker_commands: &[String],
) -> String {
    assert_eq!(marker_commands.len(), 5);
    let mut script = String::new();
    script.push_str("set timeout 30\n");
    script.push_str("set env(TERM) xterm-256color\n");
    script.push_str(&format!("set socket {}\n", tcl_braced(socket)));
    script.push_str(&format!("set session {}\n", tcl_braced(session)));
    for (index, command) in marker_commands.iter().enumerate() {
        script.push_str(&format!("set marker_cmd_{index} {}\n", tcl_braced(command)));
    }
    script.push_str(
        r#"
proc wait_for_muxboard {label} {
    expect {
        -re {(Fleet|Details)} {}
        timeout { puts "timed out waiting for muxboard during $label"; exit 20 }
        eof { puts "tmux client exited while waiting for muxboard during $label"; exit 21 }
    }
    after 750
}

proc send_marker {command} {
    after 1500
    send -- "$command\r"
    after 900
}

proc open_peek {label} {
    send "\002P"
    wait_for_muxboard $label
}

spawn tmux -L $socket attach-session -t $session
after 700

open_peek "toggle-open"
send "\002P"
send_marker $marker_cmd_0

open_peek "repeat-open"
send "\002P"
send_marker $marker_cmd_1

open_peek "q-open"
send "q"
send_marker $marker_cmd_2

open_peek "escape-open"
send "\033"
send_marker $marker_cmd_3

open_peek "jump-open"
send "g"
send_marker $marker_cmd_4

send "\002d"
expect eof
"#,
    );
    script
}

fn live_custom_prefix_peek_expect_script(
    socket: &str,
    session: &str,
    marker_command: &str,
) -> String {
    let mut script = String::new();
    script.push_str("set timeout 30\n");
    script.push_str("set env(TERM) xterm-256color\n");
    script.push_str(&format!("set socket {}\n", tcl_braced(socket)));
    script.push_str(&format!("set session {}\n", tcl_braced(session)));
    script.push_str(&format!("set marker_cmd {}\n", tcl_braced(marker_command)));
    script.push_str(
        r#"
spawn tmux -L $socket attach-session -t $session
after 700
send "\001P"
expect {
    -re {(Fleet|Details)} {}
    timeout { puts "timed out waiting for muxboard with C-a prefix"; exit 20 }
    eof { puts "tmux client exited while waiting for custom-prefix peek"; exit 21 }
}
after 750
send "\002P"
after 750
send "\001P"
after 1500
send -- "$marker_cmd\r"
after 900
send "\001d"
expect {
    eof {}
    timeout { puts "timed out detaching after custom-prefix peek close"; exit 22 }
}
"#,
    );
    script
}

fn live_prefix2_peek_expect_script(socket: &str, session: &str, marker_command: &str) -> String {
    let mut script = String::new();
    script.push_str("set timeout 30\n");
    script.push_str("set env(TERM) xterm-256color\n");
    script.push_str(&format!("set socket {}\n", tcl_braced(socket)));
    script.push_str(&format!("set session {}\n", tcl_braced(session)));
    script.push_str(&format!("set marker_cmd {}\n", tcl_braced(marker_command)));
    script.push_str(
        r#"
spawn tmux -L $socket attach-session -t $session
after 700
send "\001P"
expect {
    -re {(Fleet|Details)} {}
    timeout { puts "timed out waiting for muxboard with C-a prefix2"; exit 20 }
    eof { puts "tmux client exited while waiting for prefix2 peek"; exit 21 }
}
after 750
send "\001P"
after 1500
send -- "$marker_cmd\r"
after 900
send "\001d"
expect {
    eof {}
    timeout { puts "timed out detaching after prefix2 peek close"; exit 22 }
}
"#,
    );
    script
}

fn tcl_braced(value: &str) -> String {
    format!("{{{}}}", value.replace('\\', "\\\\").replace('}', "\\}"))
}

fn setup_plugin_grid(
    server: &TmuxServer,
    session: &str,
    width: u16,
    height: u16,
) -> TestResult<()> {
    server.run(&[
        "new-session",
        "-d",
        "-x",
        &width.to_string(),
        "-y",
        &height.to_string(),
        "-s",
        session,
        "-n",
        "grid",
        "bash",
        "-lc",
        "sleep 1000",
    ])?;
    server.run(&[
        "split-window",
        "-t",
        &format!("{session}:grid"),
        "-h",
        "-l",
        "50%",
        "bash -lc 'sleep 1000'",
    ])?;

    let first_pane = server
        .run(&[
            "list-panes",
            "-t",
            &format!("{session}:grid"),
            "-F",
            "#{pane_id}",
        ])?
        .lines()
        .next()
        .ok_or("expected an initial pane")?
        .trim()
        .to_owned();
    server.run(&["select-pane", "-t", &first_pane])?;
    server.run(&[
        "split-window",
        "-t",
        &first_pane,
        "-v",
        "-l",
        "50%",
        "bash -lc 'sleep 1000'",
    ])?;

    let right_pane = server
        .run(&[
            "list-panes",
            "-t",
            &format!("{session}:grid"),
            "-F",
            "#{pane_id}\t#{pane_left}",
        ])?
        .lines()
        .find_map(|pane| {
            let (pane_id, left) = pane.split_once('\t')?;
            (left.parse::<u16>().ok()? > 0).then(|| pane_id.to_owned())
        })
        .ok_or("expected a right pane")?;
    server.run(&["select-pane", "-t", &right_pane])?;
    server.run(&[
        "split-window",
        "-t",
        &right_pane,
        "-v",
        "-l",
        "50%",
        "bash -lc 'sleep 1000'",
    ])?;

    let selected_quadrant = server
        .run(&[
            "list-panes",
            "-t",
            &format!("{session}:grid"),
            "-F",
            "#{pane_id}\t#{pane_left}\t#{pane_top}",
        ])?
        .lines()
        .find_map(|pane| {
            let mut fields = pane.split('\t');
            let pane_id = fields.next()?;
            let left = fields.next()?.parse::<u16>().ok()?;
            let top = fields.next()?.parse::<u16>().ok()?;
            (left > 0 && top > 0).then(|| pane_id.to_owned())
        })
        .ok_or("expected a bottom-right selected quadrant")?;
    server.run(&["select-pane", "-t", &selected_quadrant])?;
    server.run(&["set-option", "-gu", "@muxboard-dock-width"])?;
    server.run(&["set-option", "-gu", "@muxboard-dock-percent"])?;
    server.run(&["set-option", "-gu", "@muxboard-open-mode"])?;
    server.run(&["set-option", "-gu", "@muxboard-close-after-jump"])?;
    server.run(&["set-option", "-g", "@muxboard-command", "cat"])?;
    server.run(&["set-option", "-g", "@muxboard-open-preset", "dock"])?;
    server.run(&["set-option", "-g", "@muxboard-dock-side", "left"])?;

    Ok(())
}

fn dock_pane_geometry(
    server: &TmuxServer,
    target: &str,
) -> TestResult<(String, u16, u16, u16, u16)> {
    let panes = server.run(&[
        "list-panes",
        "-t",
        target,
        "-F",
        "#{pane_id}\t#{pane_left}\t#{pane_top}\t#{pane_width}\t#{pane_height}\t#{@muxboard_dock}",
    ])?;
    for pane in panes.lines() {
        let fields: Vec<&str> = pane.split('\t').collect();
        if fields.len() != 6 {
            return Err(format!("unexpected pane row: {pane}").into());
        }
        if fields[5] != "1" {
            continue;
        }

        return Ok((
            fields[0].to_owned(),
            fields[1].parse()?,
            fields[2].parse()?,
            fields[3].parse()?,
            fields[4].parse()?,
        ));
    }

    Err(format!("expected a marked muxboard dock pane\n{panes}").into())
}

fn pane_geometry_snapshot(server: &TmuxServer, target: &str) -> TestResult<String> {
    server.run(&[
        "list-panes",
        "-t",
        target,
        "-F",
        "#{pane_id}\t#{pane_left}\t#{pane_top}\t#{pane_width}\t#{pane_height}\t#{@muxboard_dock}",
    ])
}

fn wait_for_pane_to_disappear(server: &TmuxServer, pane_id: &str) -> TestResult<()> {
    let start = Instant::now();
    loop {
        let panes = server.run(&["list-panes", "-a", "-F", "#{pane_id}"])?;
        if !panes.lines().any(|pane| pane == pane_id) {
            return Ok(());
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!("timed out waiting for pane {pane_id} to disappear").into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn run_tmux_plugin_helper_against(server: &TmuxServer) -> TestResult<()> {
    run_tmux_plugin_helper_args_against(server, &[])
}

fn run_tmux_plugin_helper_args_against(server: &TmuxServer, args: &[&str]) -> TestResult<()> {
    let root = std::env::temp_dir().join(format!("muxboard-plugin-helper-{}", unique_suffix()));
    let path = tmux_wrapper_path(server, &root)?;
    let helper = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("extras")
        .join("tmux")
        .join("scripts")
        .join("muxboard-open");
    let output = Command::new(&helper)
        .env("PATH", path)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "{} failed: stdout={} stderr={}",
            helper.display(),
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }

    Ok(())
}

fn run_tmux_plugin_entrypoint_against(server: &TmuxServer) -> TestResult<()> {
    let root = std::env::temp_dir().join(format!("muxboard-plugin-entrypoint-{}", unique_suffix()));
    let path = tmux_wrapper_path(server, &root)?;
    let entrypoint = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("muxboard.tmux");
    let output = Command::new(&entrypoint).env("PATH", path).output()?;
    if !output.status.success() {
        return Err(format!(
            "{} failed: stdout={} stderr={}",
            entrypoint.display(),
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }

    Ok(())
}

fn run_tmux_plugin_script_against(
    server: &TmuxServer,
    script_name: &str,
    args: &[&str],
) -> TestResult<String> {
    let root = std::env::temp_dir().join(format!(
        "muxboard-plugin-script-{}-{}",
        script_name,
        unique_suffix()
    ));
    let path = tmux_wrapper_path(server, &root)?;
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("extras")
        .join("tmux")
        .join("scripts")
        .join(script_name);
    let output = Command::new(&script)
        .env("PATH", path)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "{} failed: stdout={} stderr={}",
            script.display(),
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }

    Ok(String::from_utf8(output.stdout)?)
}

fn set_live_agent_bridge_event(
    server: &TmuxServer,
    pane_id: &str,
    agent: &str,
    state: &str,
    unseen: &str,
) -> TestResult<()> {
    let fragment = pane_env_fragment(pane_id);
    server.run(&[
        "set-environment",
        "-g",
        &format!("MUXBOARD_AGENT_PANE_{fragment}_AGENT"),
        agent,
    ])?;
    server.run(&[
        "set-environment",
        "-g",
        &format!("MUXBOARD_AGENT_PANE_{fragment}_STATE"),
        state,
    ])?;
    if unseen.is_empty() {
        server.run(&[
            "set-environment",
            "-gu",
            &format!("MUXBOARD_AGENT_PANE_{fragment}_UNSEEN"),
        ])?;
    } else {
        server.run(&[
            "set-environment",
            "-g",
            &format!("MUXBOARD_AGENT_PANE_{fragment}_UNSEEN"),
            unseen,
        ])?;
    }
    Ok(())
}

fn tmux_wrapper_path(server: &TmuxServer, root: &Path) -> TestResult<String> {
    fs::create_dir_all(root)?;
    let tmux_output = Command::new("sh")
        .args(["-lc", "command -v tmux"])
        .output()?;
    if !tmux_output.status.success() {
        return Err("tmux must be available for the plugin helper live test".into());
    }
    let tmux_path = String::from_utf8(tmux_output.stdout)?;
    let wrapper_path = root.join("tmux");
    fs::write(
        &wrapper_path,
        format!(
            "#!/bin/sh\nexec {} -L {} \"$@\"\n",
            shell_quote(tmux_path.trim()),
            shell_quote(&server.socket)
        ),
    )?;
    fs::set_permissions(&wrapper_path, fs::Permissions::from_mode(0o755))?;

    Ok(format!(
        "{}:{}",
        root.display(),
        std::env::var("PATH").unwrap_or_default()
    ))
}

fn attach_script_tmux_client(server: &TmuxServer, session: &str) -> TestResult<Child> {
    let transcript = std::env::temp_dir().join(format!("muxboard-script-{}", unique_suffix()));
    let util_linux_script = Command::new("script")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    let mut command = Command::new("script");
    command.env("TERM", "xterm-256color").arg("-q");
    if util_linux_script {
        command
            .arg("-c")
            .arg(format!(
                "tmux -L {} attach-session -t {}",
                shell_quote(&server.socket),
                shell_quote(session)
            ))
            .arg(&transcript);
    } else {
        command
            .arg(&transcript)
            .arg("tmux")
            .arg("-L")
            .arg(&server.socket)
            .arg("attach-session")
            .arg("-t")
            .arg(session);
    }
    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let start = Instant::now();
    loop {
        if !server
            .run(&["list-clients", "-F", "#{client_name}"])?
            .trim()
            .is_empty()
        {
            return Ok(child);
        }

        if let Some(status) = child.try_wait()? {
            return Err(format!("script tmux client exited before attach: {status}").into());
        }
        if start.elapsed() > WAIT_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err("timed out waiting for an attached tmux client".into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn launch_muxboard_in_pane(server: &TmuxServer, pane: &str, command: &str) -> TestResult<()> {
    server.send_literal(pane, command)?;
    server.send_keys(pane, &["Enter"])?;
    wait_for_muxboard_surface(server, pane)?;
    Ok(())
}

fn quit_muxboard_in_pane(server: &TmuxServer, pane: &str) -> TestResult<()> {
    server.send_keys(pane, &["q"])?;
    server.wait_for_field(pane, "#{pane_current_command}", "bash")?;
    Ok(())
}

fn wait_for_any_pane_text(server: &TmuxServer, needle: &str) -> TestResult<String> {
    let start = std::time::Instant::now();
    loop {
        let panes = server.run(&["list-panes", "-a", "-F", "#{pane_id}"])?;
        for pane in panes.lines().filter(|line| !line.trim().is_empty()) {
            let pane = pane.trim();
            let screen = server.capture(pane)?;
            if screen.contains(needle) {
                return Ok(pane.to_owned());
            }
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!("timed out waiting for `{needle}` in any pane").into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_ack_count(state_file: &Path, expected: usize) -> TestResult<()> {
    let start = std::time::Instant::now();
    loop {
        let count = read_ack_count(state_file)?;
        if count == expected {
            return Ok(());
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for {expected} persisted acknowledgement(s) in {}. Last count was {count}.",
                state_file.display()
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn visible_line_containing<'a>(screen: &'a str, needle: &str) -> TestResult<&'a str> {
    screen
        .lines()
        .find(|line| line.contains(needle))
        .ok_or_else(|| format!("expected visible line containing `{needle}`\n{screen}").into())
}

fn resize_window_and_wait_for_board_surface(
    server: &TmuxServer,
    target: &str,
    pane: &str,
    width: u16,
    height: u16,
    selected_text: &str,
    required_text: &[&str],
) -> TestResult<String> {
    server.resize_window(target, width, height)?;
    let expected_size = format!("{width}x{height}");
    server.wait_for_display_field(target, "#{window_width}x#{window_height}", &expected_size)?;
    wait_for_board_surface(server, pane, selected_text, required_text)
}

fn wait_for_board_surface(
    server: &TmuxServer,
    pane: &str,
    selected_text: &str,
    required_text: &[&str],
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        let selected_row_visible = screen
            .lines()
            .any(|line| line.contains('>') && line.contains(selected_text));
        let has_required = required_text.iter().all(|text| screen.contains(text));
        if selected_row_visible
            && screen.contains("Fleet")
            && has_required
            && !screen.contains("More")
            && !screen.contains("Send to")
            && !screen.contains("Review send")
            && !screen.contains("Command Center")
            && !screen.contains("Browse")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for board surface with selected row `{selected_text}` and {required_text:?} in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_screen_with_texts_without(
    server: &TmuxServer,
    pane: &str,
    required_text: &[&str],
    forbidden_text: &[&str],
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        let has_required = required_text.iter().all(|text| screen.contains(text));
        let has_forbidden = forbidden_text.iter().any(|text| screen.contains(text));
        if has_required && !has_forbidden {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for {required_text:?} without {forbidden_text:?} in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_recovery_surface(
    server: &TmuxServer,
    pane: &str,
    headline: &str,
    next_step: &str,
) -> TestResult<String> {
    wait_for_screen_with_texts_without(
        server,
        pane,
        &[headline, next_step, "? help"],
        &["Snapshot unavailable", "tmux command failed"],
    )
}

fn wait_for_start_agent_surface(
    server: &TmuxServer,
    pane: &str,
    destination: &str,
) -> TestResult<String> {
    wait_for_screen_with_texts_without(
        server,
        pane,
        &[destination, "Command:", "Enter start", "Esc cancel"],
        &["More", "Send to", "Review send", "Command Center", "Browse"],
    )
}

fn wait_for_launch_feedback(server: &TmuxServer, pane: &str, feedback: &str) -> TestResult<String> {
    wait_for_screen_with_texts_without(
        server,
        pane,
        &[feedback, "Fleet"],
        &[
            "Start agent.",
            "Enter start",
            "More",
            "Send to",
            "Review send",
            "Command Center",
            "Browse",
        ],
    )
}

fn wait_for_review_dispatch_result(
    server: &TmuxServer,
    pane: &str,
    required_text: &[&str],
    forbidden_text: &[&str],
) -> TestResult<String> {
    let mut forbidden = vec!["Review send", "Send to", "More", "Command Center", "Browse"];
    forbidden.extend_from_slice(forbidden_text);
    wait_for_screen_with_texts_without(server, pane, required_text, &forbidden)
}

fn wait_for_refresh_result(
    server: &TmuxServer,
    pane: &str,
    required_text: &[&str],
    forbidden_text: &[&str],
) -> TestResult<String> {
    const FORBIDDEN_REFRESH_SURFACES: &[&str] =
        &["More", "Send to", "Review send", "Command Center", "Browse"];

    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        let has_required = required_text.iter().all(|text| screen.contains(text));
        let has_forbidden = forbidden_text
            .iter()
            .chain(FORBIDDEN_REFRESH_SURFACES.iter())
            .any(|text| screen.contains(text));
        if screen.contains("muxboard") && has_required && !has_forbidden {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for top-level refresh result {required_text:?} without {forbidden_text:?} in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_search_input_surface(server: &TmuxServer, pane: &str) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        if screen.contains("type to filter")
            && screen.contains("Enter apply")
            && screen.contains("Esc cancel")
            && !screen.contains("More")
            && !screen.contains("Send to")
            && !screen.contains("Review send")
            && !screen.contains("Command Center")
            && !screen.contains("Browse")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for Search input surface in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_search_result(
    server: &TmuxServer,
    pane: &str,
    query: &str,
    selected_text: &str,
) -> TestResult<String> {
    let start = Instant::now();
    let search_label = format!("search: {query}");
    loop {
        let screen = server.capture(pane)?;
        let selected_result_visible = screen
            .lines()
            .any(|line| line.contains('>') && line.contains(selected_text));
        if screen.contains(&search_label)
            && screen.contains("backspace show all")
            && selected_result_visible
            && !screen.contains("type to filter")
            && !screen.contains("More")
            && !screen.contains("Send to")
            && !screen.contains("Review send")
            && !screen.contains("Command Center")
            && !screen.contains("Browse")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for applied search `{query}` with selected result `{selected_text}` in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_selected_row(
    server: &TmuxServer,
    pane: &str,
    selected_text: &str,
) -> TestResult<String> {
    wait_for_main_board_surface(server, pane, selected_text, &[])
}

fn wait_for_main_board_surface(
    server: &TmuxServer,
    pane: &str,
    selected_text: &str,
    required_text: &[&str],
) -> TestResult<String> {
    wait_for_main_board_surface_without(server, pane, selected_text, required_text, &[])
}

fn wait_for_main_board_surface_without(
    server: &TmuxServer,
    pane: &str,
    selected_text: &str,
    required_text: &[&str],
    forbidden_text: &[&str],
) -> TestResult<String> {
    wait_for_main_board_surface_with_poll(
        server,
        pane,
        selected_text,
        required_text,
        forbidden_text,
        WAIT_TIMEOUT,
        POLL_INTERVAL,
    )
}

fn wait_for_main_board_surface_with_poll(
    server: &TmuxServer,
    pane: &str,
    selected_text: &str,
    required_text: &[&str],
    forbidden_text: &[&str],
    timeout: Duration,
    poll_interval: Duration,
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        let selected_row_visible = screen
            .lines()
            .any(|line| line.contains('>') && line.contains(selected_text));
        let has_forbidden = forbidden_text.iter().any(|text| screen.contains(text));
        if selected_row_visible
            && screen.contains("Fleet")
            && screen.contains("Details")
            && required_text.iter().all(|text| screen.contains(text))
            && !has_forbidden
            && !screen.contains("More")
            && !screen.contains("Send to")
            && !screen.contains("Review send")
            && !screen.contains("Command Center")
            && !screen.contains("Browse")
        {
            return Ok(screen);
        }

        if start.elapsed() > timeout {
            return Err(format!(
                "timed out waiting for main board surface with selected row `{selected_text}`, {required_text:?}, and without {forbidden_text:?} in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(poll_interval);
    }
}

fn wait_for_live_status_summary(
    server: &TmuxServer,
    pane: &str,
    selected_text: &str,
    summary_text: &str,
    forbidden_text: &[&str],
) -> TestResult<String> {
    wait_for_live_status_summary_with_poll(
        server,
        pane,
        selected_text,
        summary_text,
        forbidden_text,
        WAIT_TIMEOUT,
        POLL_INTERVAL,
    )
}

fn wait_for_live_status_summary_with_poll(
    server: &TmuxServer,
    pane: &str,
    selected_text: &str,
    summary_text: &str,
    forbidden_text: &[&str],
    timeout: Duration,
    poll_interval: Duration,
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        let selected_row_visible = screen.lines().any(|line| {
            fleet_line_segment(line).contains('>')
                && fleet_line_segment(line).contains(selected_text)
        });
        let fleet_summary_visible =
            selected_fleet_block_contains(&screen, selected_text, summary_text);
        let details_summary_visible = screen
            .lines()
            .any(|line| line.contains("Now:") && line.contains(summary_text));
        let has_forbidden = forbidden_text.iter().any(|text| screen.contains(text));
        if selected_row_visible
            && fleet_summary_visible
            && details_summary_visible
            && screen.contains("Fleet")
            && screen.contains("Details")
            && !has_forbidden
            && !screen.contains("More")
            && !screen.contains("Send to")
            && !screen.contains("Review send")
            && !screen.contains("Command Center")
            && !screen.contains("Browse")
        {
            return Ok(screen);
        }

        if start.elapsed() > timeout {
            return Err(format!(
                "timed out waiting for live status summary `{summary_text}` on selected row `{selected_text}` without {forbidden_text:?} in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(poll_interval);
    }
}

fn pane_env_fragment(pane_id: &str) -> String {
    pane_id
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn wait_for_tmux_env_value(server: &TmuxServer, key: &str, expected: &str) -> TestResult<()> {
    let start = Instant::now();
    loop {
        let raw = server
            .run(&["show-environment", "-g", key])
            .unwrap_or_default();
        let value = raw
            .trim()
            .strip_prefix(&format!("{key}="))
            .unwrap_or(raw.trim());
        if value == expected {
            return Ok(());
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for tmux env {key}={expected}; last value was `{value}`"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn selected_fleet_block_contains(screen: &str, selected_text: &str, summary_text: &str) -> bool {
    let lines = screen.lines().collect::<Vec<_>>();
    let Some(start) = lines.iter().position(|line| {
        let fleet = fleet_line_segment(line);
        fleet.contains('>') && fleet.contains(selected_text)
    }) else {
        return false;
    };

    lines
        .iter()
        .skip(start)
        .take(3)
        .any(|line| fleet_line_segment(line).contains(summary_text))
}

fn fleet_line_segment(line: &str) -> &str {
    const PANEL_SEPARATOR: &str = "\u{2502} \u{2502}";
    line.split_once(PANEL_SEPARATOR)
        .map_or(line, |(fleet, _details)| fleet)
}

fn wait_for_fleet_action_feedback(
    server: &TmuxServer,
    pane: &str,
    feedback_text: &str,
    selected_text: &str,
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        let selected_row_visible = screen
            .lines()
            .any(|line| line.contains('>') && line.contains(selected_text));
        if screen.contains(feedback_text)
            && selected_row_visible
            && !screen.contains("More")
            && !screen.contains("Send to")
            && !screen.contains("Review send")
            && !screen.contains("Command Center")
            && !screen.contains("Browse")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for Fleet feedback `{feedback_text}` with selected row `{selected_text}` in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_selected_action(
    server: &TmuxServer,
    pane: &str,
    action_text: &str,
    selected_text: &str,
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        let selected_row_visible = screen
            .lines()
            .any(|line| line.contains('>') && line.contains(selected_text));
        if screen.contains(action_text)
            && selected_row_visible
            && !screen.contains("More")
            && !screen.contains("Send to")
            && !screen.contains("Review send")
            && !screen.contains("Command Center")
            && !screen.contains("Browse")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for selected action `{action_text}` on `{selected_text}` in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_send_list_target_state(
    server: &TmuxServer,
    pane: &str,
    count_text: &str,
    selected_text: &str,
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        let selected_line_is_targeted = screen
            .lines()
            .any(|line| line.contains(">+") && line.contains(selected_text));
        if screen.contains(count_text)
            && selected_line_is_targeted
            && !screen.contains("More")
            && !screen.contains("Send to")
            && !screen.contains("Review send")
            && !screen.contains("Command Center")
            && !screen.contains("Browse")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for {count_text} with selected target `{selected_text}` in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_clear_send_list_action(
    server: &TmuxServer,
    pane: &str,
    selected_text: &str,
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        let selected_line_is_default_target = screen
            .lines()
            .any(|line| line.contains(">+") && line.contains(selected_text));
        if selected_line_is_default_target
            && !screen.contains("send list (2 panes)")
            && !screen.contains("More")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for clear-send-list reset to selected pane `{selected_text}` in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_save_fleet_input(server: &TmuxServer, pane: &str) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        if screen.contains("Save this send list as a reusable fleet.")
            && screen.contains("Enter save")
            && screen.contains("Esc cancel")
            && screen.contains("type name")
            && !screen.contains("More")
            && !screen.contains("Fleets")
            && !screen.contains("Send to")
            && !screen.contains("Review send")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for Save Fleet input surface in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_saved_fleet_picker(
    server: &TmuxServer,
    pane: &str,
    fleet_name: &str,
    live_summary: &str,
    can_load: bool,
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        let selected_fleet_row = screen.lines().any(|line| {
            line.contains('>') && line.contains(fleet_name) && line.contains(live_summary)
        });
        let load_visibility_matches =
            can_load == screen.lines().any(|line| line.contains("Enter load"));
        if screen.contains("Fleets")
            && screen.contains("Choose a saved fleet.")
            && selected_fleet_row
            && load_visibility_matches
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for saved fleet picker `{fleet_name}` with `{live_summary}` and can_load={can_load} in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_saved_fleet_active(
    server: &TmuxServer,
    pane: &str,
    fleet_name: &str,
    pane_summary: &str,
) -> TestResult<String> {
    let start = Instant::now();
    let target = format!("Send: fleet {fleet_name} ({pane_summary})");
    loop {
        let screen = server.capture(pane)?;
        if screen.contains(&target)
            && !screen.contains("Choose a saved fleet.")
            && !screen.contains("Fleets")
            && !screen.contains("Save this send list as a reusable fleet.")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for saved fleet target `{target}` to become active in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_stale_fleet_board_after_picker_escape(
    server: &TmuxServer,
    pane: &str,
) -> TestResult<String> {
    wait_for_main_board_surface_without(
        server,
        pane,
        "ops/keep",
        &[
            "fleet triage has no live panes",
            "Target: fleet triage has no live panes",
        ],
        &["Choose a saved fleet.", "Enter load"],
    )
}

fn wait_for_output_surface(server: &TmuxServer, pane: &str) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        if screen.contains("Output")
            && screen.contains("Esc back")
            && !screen.contains("Enter output")
            && !screen.contains("Enter details")
            && !screen.contains("More")
            && !screen.contains("Send to")
            && !screen.contains("Review send")
            && !screen.contains("Command Center")
            && !screen.contains("Browse")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for Output surface in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_output_surface_with_text_with_poll(
    server: &TmuxServer,
    pane: &str,
    output_text: &str,
    timeout: Duration,
    poll_interval: Duration,
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        if screen.contains("Output")
            && screen.contains("Esc back")
            && screen.contains(output_text)
            && !screen.contains("Enter output")
            && !screen.contains("Enter details")
            && !screen.contains("More")
            && !screen.contains("Send to")
            && !screen.contains("Review send")
            && !screen.contains("Command Center")
            && !screen.contains("Browse")
        {
            return Ok(screen);
        }

        if start.elapsed() > timeout {
            return Err(format!(
                "timed out waiting for Output surface with `{output_text}` in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(poll_interval);
    }
}

fn wait_for_output_surface_to_stay_open(server: &TmuxServer, pane: &str) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        if !screen.contains("Output")
            || !screen.contains("Esc back")
            || screen.contains("Enter details")
        {
            return Err(format!(
                "Output surface did not stay open after repeated Enter in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        if start.elapsed() > INERT_ACTION_SETTLE_TIMEOUT {
            return Ok(screen);
        }

        thread::sleep(FAST_POLL_INTERVAL);
    }
}

fn screen_text_position(screen: &str, needle: &str) -> TestResult<(usize, usize)> {
    screen
        .lines()
        .enumerate()
        .find_map(|(row, line)| line.find(needle).map(|column| (column, row)))
        .ok_or_else(|| format!("missing `{needle}` in screen:\n{screen}").into())
}

fn screen_panel_border_signature(screen: &str) -> Vec<(usize, usize, char)> {
    screen
        .lines()
        .enumerate()
        .flat_map(|(row, line)| {
            line.chars().enumerate().filter_map(move |(column, ch)| {
                matches!(ch, '┌' | '┐' | '└' | '┘' | '─' | '│').then_some((column, row, ch))
            })
        })
        .collect()
}

fn wait_for_output_escape_returns_to_details(
    server: &TmuxServer,
    pane: &str,
    selected_text: &str,
) -> TestResult<String> {
    wait_for_main_board_surface_without(
        server,
        pane,
        selected_text,
        &["muxboard"],
        &["Esc back", "Enter details", "K older/J newer"],
    )
}

fn wait_for_help_surface(
    server: &TmuxServer,
    pane: &str,
    required_text: &str,
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        if screen.contains("Help")
            && screen.contains("Esc close")
            && screen.contains(required_text)
            && !screen.contains("Send to")
            && !screen.contains("Review send")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for Help surface `{required_text}` in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_help_escape_returns_to_details(
    server: &TmuxServer,
    pane: &str,
    selected_text: &str,
) -> TestResult<String> {
    wait_for_main_board_surface_without(
        server,
        pane,
        selected_text,
        &["muxboard", "Details"],
        &["Help"],
    )
}

fn wait_for_command_center_surface(server: &TmuxServer, pane: &str) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        if screen.contains("Command Center")
            && screen.contains("Esc back")
            && !screen.contains("More")
            && !screen.contains("Browse")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for Command Center surface in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_command_center_escape_returns_to_details(
    server: &TmuxServer,
    pane: &str,
    selected_text: &str,
) -> TestResult<String> {
    wait_for_main_board_surface_without(
        server,
        pane,
        selected_text,
        &["muxboard"],
        &["Command Center"],
    )
}

fn wait_for_browse_surface(server: &TmuxServer, pane: &str) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        if screen.contains("Browse")
            && screen.contains("Esc back")
            && !screen.contains("More")
            && !screen.contains("Command Center")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for Browse surface in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_browse_escape_returns_to_details(
    server: &TmuxServer,
    pane: &str,
    selected_text: &str,
) -> TestResult<String> {
    wait_for_main_board_surface_without(server, pane, selected_text, &["muxboard"], &["Browse"])
}

fn wait_for_browse_scope(
    server: &TmuxServer,
    pane: &str,
    selected_window: &str,
    required_text: &[&str],
    forbidden_text: &[&str],
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        let selected_window_visible = screen
            .lines()
            .any(|line| line.contains('>') && line.contains(selected_window));
        let has_required = required_text.iter().all(|text| screen.contains(text));
        let has_forbidden = forbidden_text.iter().any(|text| screen.contains(text));
        if screen.contains("Browse")
            && screen.contains("Esc back")
            && selected_window_visible
            && has_required
            && !has_forbidden
            && !screen.contains("More")
            && !screen.contains("Command Center")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for Browse scope `{selected_window}` with {required_text:?} and without {forbidden_text:?} in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_send_surface(server: &TmuxServer, pane: &str) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        let has_send_footer = screen.contains("type text")
            && (screen.contains("Enter send") || screen.contains("Enter review"));
        if screen.contains("Send to")
            && screen.contains("Send")
            && has_send_footer
            && !screen.contains("Review send")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for Send surface in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_reply_surface(
    server: &TmuxServer,
    pane: &str,
    target_text: &str,
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        if screen.contains("Reply")
            && screen.contains("Reply to:")
            && screen.contains(target_text)
            && screen.contains("type text")
            && screen.contains("Enter reply")
            && screen.contains("Esc cancel")
            && !screen.contains("Review send")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for Reply surface `{target_text}` in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_send_command_text(
    server: &TmuxServer,
    pane: &str,
    command_text: &str,
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        let has_send_footer = screen.contains("type text")
            && (screen.contains("Enter send") || screen.contains("Enter review"));
        if screen.contains("Send to")
            && screen.contains("Send")
            && screen.contains(command_text)
            && has_send_footer
            && !screen.contains("Review send")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for typed Send command `{command_text}` in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_reply_command_text(
    server: &TmuxServer,
    pane: &str,
    command_text: &str,
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        if screen.contains("Reply")
            && screen.contains("Reply to:")
            && screen.contains(command_text)
            && screen.contains("Enter reply")
            && !screen.contains("Review send")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for typed Reply text `{command_text}` in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_review_surface(
    server: &TmuxServer,
    pane: &str,
    target_text: &str,
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        if screen.contains("Review send")
            && screen.contains(target_text)
            && screen.contains("Enter send")
            && screen.contains("Esc cancel")
            && !screen.contains("Send to")
            && !screen.contains("More")
            && !screen.contains("Fleets")
        {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for Review send surface `{target_text}` in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_inert_send_key(
    server: &TmuxServer,
    pane: &str,
    expected_text: &str,
) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        if screen.contains("Send to") || screen.contains("Review send") {
            return Err(format!(
                "send key unexpectedly opened a send surface in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }
        if !screen.contains(expected_text) {
            return Err(format!(
                "send key lost expected recovery text `{expected_text}` in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        if start.elapsed() > INERT_ACTION_SETTLE_TIMEOUT {
            return Ok(screen);
        }

        thread::sleep(FAST_POLL_INTERVAL);
    }
}

fn wait_for_target_text_absent(
    server: &TmuxServer,
    pane: &str,
    forbidden_text: &str,
) -> TestResult<()> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        if screen.contains(forbidden_text) {
            return Err(format!(
                "target pane {pane} unexpectedly contained `{forbidden_text}`\nlast capture:\n{screen}"
            )
            .into());
        }

        if start.elapsed() > INERT_ACTION_SETTLE_TIMEOUT {
            return Ok(());
        }

        thread::sleep(FAST_POLL_INTERVAL);
    }
}

fn wait_for_more_row(server: &TmuxServer, pane: &str, row_text: &str) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let screen = server.capture(pane)?;
        if screen.contains("More") && screen.contains(row_text) {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for More row `{row_text}` in pane {pane}\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_muxboard_surface(server: &TmuxServer, pane: &str) -> TestResult<String> {
    let start = Instant::now();
    loop {
        let command = server.pane_field(pane, "#{pane_current_command}")?;
        let screen = server.capture(pane)?;
        if command == "muxboard" && screen.contains("muxboard") {
            return Ok(screen);
        }

        if start.elapsed() > WAIT_TIMEOUT {
            return Err(format!(
                "timed out waiting for muxboard process and surface in pane {pane}, command `{command}`\nlast capture:\n{screen}"
            )
            .into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn assert_muxboard_still_running(server: &TmuxServer, pane: &str) -> TestResult<()> {
    wait_for_muxboard_surface(server, pane)?;
    Ok(())
}

fn read_ack_count(state_file: &Path) -> TestResult<usize> {
    if !state_file.exists() {
        return Ok(0);
    }

    let raw = fs::read_to_string(state_file)?;
    let value: Value = serde_json::from_str(&raw)?;
    Ok(value
        .get("acknowledged_attention")
        .and_then(Value::as_array)
        .map_or(0, Vec::len))
}

fn unique_suffix() -> String {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be valid")
        .as_nanos();
    format!("{pid}-{nanos}")
}
