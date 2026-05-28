pub mod control;

use std::{
    collections::HashMap,
    env,
    path::Path,
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::cli::Cli;

const FIELD_SEPARATOR: char = '\t';

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Target {
    pub binary: String,
    pub socket: Option<String>,
    pub session: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeContext {
    pub socket_name: Option<String>,
    pub pane_id: Option<String>,
}

impl From<&Cli> for Target {
    fn from(cli: &Cli) -> Self {
        Self {
            binary: cli.tmux_bin.clone(),
            socket: cli.socket.clone(),
            session: cli.session.clone(),
        }
    }
}

impl Target {
    pub fn display_target(&self) -> String {
        match (&self.socket, &self.session) {
            (Some(socket), Some(session)) => format!("socket `{socket}`, session `{session}`"),
            (Some(socket), None) => format!("socket `{socket}`, default session"),
            (None, Some(session)) => format!("default socket, session `{session}`"),
            (None, None) => String::from("default socket, default session"),
        }
    }

    pub fn command_preview(&self) -> String {
        let mut parts = vec![self.binary.clone()];

        if let Some(socket) = &self.socket {
            parts.push(format!("-L {socket}"));
        }

        parts.push(String::from("attach-session"));

        if let Some(session) = &self.session {
            parts.push(format!("-t {session}"));
        }

        parts.join(" ")
    }
}

impl RuntimeContext {
    pub fn from_env() -> Self {
        let tmux = env::var_os("TMUX");
        let pane_id = env::var("TMUX_PANE").ok();
        Self::from_env_values(tmux.as_deref(), pane_id.as_deref())
    }

    fn from_env_values(tmux: Option<&std::ffi::OsStr>, pane_id: Option<&str>) -> Self {
        let socket_name = tmux.and_then(socket_name_from_tmux_env);
        let pane_id = pane_id
            .filter(|pane_id| !pane_id.is_empty())
            .map(ToOwned::to_owned);
        Self {
            socket_name,
            pane_id,
        }
    }

    pub fn is_same_server(&self, target: &Target) -> bool {
        self.socket_name.as_deref() == Some(target.socket.as_deref().unwrap_or("default"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Probe {
    pub version: String,
    pub target: Target,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Snapshot {
    pub sessions: Vec<Session>,
    pub windows: Vec<Window>,
    pub panes: Vec<Pane>,
}

impl Snapshot {
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Window {
    pub id: String,
    pub session_id: String,
    pub session_name: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Pane {
    pub id: String,
    pub session_id: String,
    pub session_name: String,
    pub window_id: String,
    pub window_name: String,
    pub pane_index: u32,
    pub pane_pid: u32,
    pub title: String,
    pub current_command: String,
    pub current_path: String,
    pub active: bool,
    #[serde(default)]
    pub alternate_on: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_event: Option<AgentBridgeEvent>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentBridgeEvent {
    pub agent: String,
    pub state: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unseen: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at_unix_ms: Option<u64>,
}

pub async fn probe(target: Target) -> Result<Probe> {
    let output = Command::new(&target.binary)
        .arg("-V")
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .output()
        .await
        .with_context(|| tmux_start_error(&target.binary))?;

    if !output.status.success() {
        bail!(
            "could not read tmux version from `{}`: {}",
            target.binary,
            command_failure_detail(&output)
        );
    }

    let version = String::from_utf8(output.stdout)
        .context("tmux version output was not valid UTF-8")?
        .trim()
        .to_owned();

    Ok(Probe { version, target })
}

pub async fn snapshot(target: Target) -> Result<Snapshot> {
    let format = [
        "#{session_id}",
        "#{session_name}",
        "#{window_id}",
        "#{window_name}",
        "#{pane_id}",
        "#{pane_index}",
        "#{pane_pid}",
        "#{pane_title}",
        "#{pane_current_command}",
        "#{pane_current_path}",
        "#{pane_active}",
        "#{alternate_on}",
    ]
    .join("\t");

    let args = snapshot_command_args(&target, &format);
    let agent_events = agent_bridge_events(&target).await.unwrap_or_default();
    let output = run_tmux_command(&target, args.iter().map(String::as_str)).await?;
    let mut snapshot = Snapshot::default();

    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let mut pane = parse_pane_row(line)?;
        pane.agent_event = agent_events
            .get(&agent_bridge_key_fragment(&pane.id))
            .cloned();

        if !snapshot
            .sessions
            .iter()
            .any(|session| session.id == pane.session_id)
        {
            snapshot.sessions.push(Session {
                id: pane.session_id.clone(),
                name: pane.session_name.clone(),
            });
        }

        if !snapshot
            .windows
            .iter()
            .any(|window| window.id == pane.window_id)
        {
            snapshot.windows.push(Window {
                id: pane.window_id.clone(),
                session_id: pane.session_id.clone(),
                session_name: pane.session_name.clone(),
                name: pane.window_name.clone(),
            });
        }

        snapshot.panes.push(pane);
    }

    snapshot
        .sessions
        .sort_by(|left, right| left.name.cmp(&right.name));
    snapshot.windows.sort_by(|left, right| {
        left.session_name
            .cmp(&right.session_name)
            .then_with(|| left.name.cmp(&right.name))
    });
    snapshot.panes.sort_by(|left, right| {
        left.session_name
            .cmp(&right.session_name)
            .then_with(|| left.window_name.cmp(&right.window_name))
            .then_with(|| left.pane_index.cmp(&right.pane_index))
    });

    Ok(snapshot)
}

fn snapshot_command_args(target: &Target, format: &str) -> Vec<String> {
    let mut args = vec![String::from("list-panes")];

    if let Some(session) = &target.session {
        args.push(String::from("-s"));
        args.push(String::from("-t"));
        args.push(session.clone());
    } else {
        args.push(String::from("-a"));
    }

    args.push(String::from("-F"));
    args.push(format.to_owned());
    args
}

pub async fn capture_pane_tail(target: &Target, pane: &Pane, lines: usize) -> Result<Vec<String>> {
    let args = capture_pane_args(pane, lines);
    let output = run_tmux_command(target, args.iter().map(String::as_str)).await?;

    Ok(normalize_capture_lines(&output))
}

pub async fn focus_pane(target: &Target, pane: &Pane) -> Result<()> {
    run_tmux_no_output(target, ["select-window", "-t", pane.window_id.as_str()]).await?;
    run_tmux_no_output(target, ["select-pane", "-t", pane.id.as_str()]).await
}

pub async fn current_client_tty(target: &Target, pane_id: &str) -> Result<String> {
    let tty = run_tmux_command(
        target,
        ["display-message", "-p", "-t", pane_id, "#{client_tty}"],
    )
    .await?;
    Ok(tty.trim().to_owned())
}

pub async fn jump_client_to_pane(target: &Target, client_tty: &str, pane: &Pane) -> Result<()> {
    run_tmux_no_output(
        target,
        [
            "switch-client",
            "-c",
            client_tty,
            "-t",
            pane.session_name.as_str(),
        ],
    )
    .await?;
    focus_pane(target, pane).await
}

pub async fn toggle_zoom(target: &Target, pane_id: &str) -> Result<()> {
    run_tmux_no_output(target, ["resize-pane", "-Z", "-t", pane_id]).await
}

pub async fn new_window(
    target: &Target,
    session_name: &str,
    window_name: &str,
    current_path: &str,
    command: &str,
) -> Result<()> {
    let mut args = vec![
        String::from("new-window"),
        String::from("-d"),
        String::from("-t"),
        session_name.to_owned(),
        String::from("-n"),
        window_name.to_owned(),
    ];

    if !current_path.trim().is_empty() {
        args.push(String::from("-c"));
        args.push(current_path.to_owned());
    }

    args.push(command.to_owned());
    run_tmux_no_output(target, args.iter().map(String::as_str)).await
}

pub async fn send_keys(target: &Target, pane_id: &str, keys: &[&str]) -> Result<()> {
    let mut args = vec!["send-keys", "-t", pane_id];
    args.extend(keys.iter().copied());
    run_tmux_no_output(target, args).await
}

pub async fn send_text(
    target: &Target,
    pane_id: &str,
    text: &str,
    press_enter: bool,
) -> Result<()> {
    let args = [
        String::from("send-keys"),
        String::from("-t"),
        pane_id.to_owned(),
        String::from("-l"),
        String::from("--"),
        text.to_owned(),
    ];
    run_tmux_no_output(target, args.iter().map(String::as_str)).await?;

    if press_enter {
        send_keys(target, pane_id, &["Enter"]).await?;
    }

    Ok(())
}

pub async fn current_pane(target: &Target) -> Result<String> {
    let output = run_tmux_command(target, ["display-message", "-p", "#{pane_id}"]).await?;
    Ok(output.trim().to_owned())
}

pub async fn set_agent_bridge_event(
    target: &Target,
    pane_id: &str,
    event: AgentBridgeEvent,
) -> Result<()> {
    let fragment = agent_bridge_key_fragment(pane_id);
    let updated_at = event
        .updated_at_unix_ms
        .unwrap_or_else(current_unix_millis)
        .to_string();
    let fields = [
        ("AGENT", event.agent.as_str()),
        ("STATE", event.state.as_str()),
        ("SUMMARY", event.summary.as_str()),
        ("UPDATED_AT", updated_at.as_str()),
    ];

    for (field, value) in fields {
        let key = agent_bridge_env_key(&fragment, field);
        run_tmux_no_output(target, ["set-environment", "-g", key.as_str(), value]).await?;
    }

    for (field, value) in [
        ("THREAD_ID", event.thread_id.as_deref()),
        ("THREAD_NAME", event.thread_name.as_deref()),
        ("PROGRESS", event.progress.as_deref()),
        ("LOG", event.log.as_deref()),
    ] {
        let key = agent_bridge_env_key(&fragment, field);
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            run_tmux_no_output(target, ["set-environment", "-g", key.as_str(), value]).await?;
        } else {
            run_tmux_no_output(target, ["set-environment", "-gu", key.as_str()]).await?;
        }
    }

    let unseen_key = agent_bridge_env_key(&fragment, "UNSEEN");
    if let Some(unseen) = event.unseen {
        let value = if unseen { "1" } else { "0" };
        run_tmux_no_output(
            target,
            ["set-environment", "-g", unseen_key.as_str(), value],
        )
        .await?;
    } else {
        run_tmux_no_output(target, ["set-environment", "-gu", unseen_key.as_str()]).await?;
    }

    Ok(())
}

pub async fn clear_agent_bridge_event(target: &Target, pane_id: &str) -> Result<()> {
    let fragment = agent_bridge_key_fragment(pane_id);
    for field in [
        "AGENT",
        "STATE",
        "SUMMARY",
        "THREAD_ID",
        "THREAD_NAME",
        "PROGRESS",
        "LOG",
        "UNSEEN",
        "UPDATED_AT",
    ] {
        let key = agent_bridge_env_key(&fragment, field);
        run_tmux_no_output(target, ["set-environment", "-gu", key.as_str()]).await?;
    }

    Ok(())
}

pub async fn mark_agent_bridge_event_seen(target: &Target, pane_id: &str) -> Result<()> {
    let fragment = agent_bridge_key_fragment(pane_id);
    let muxboard_key = agent_bridge_env_key(&fragment, "UNSEEN");
    let legacy_key = format!("TMUX_AGENT_PANE_{pane_id}_UNSEEN");
    for key in [muxboard_key.as_str(), legacy_key.as_str()] {
        run_tmux_no_output(target, ["set-environment", "-g", key, "0"]).await?;
    }
    run_tmux_no_output(target, ["refresh-client", "-S"]).await?;
    Ok(())
}

pub fn agent_status_segment(snapshot: &Snapshot, session: Option<&str>) -> String {
    let counts = agent_bridge_counts(snapshot, session);
    if counts.attention > 0 {
        if counts.attention == 1
            && let Some(agent) = counts.attention_agent
        {
            format!("mux ! {agent}")
        } else {
            format!("mux !{}", counts.attention)
        }
    } else if counts.running > 0 {
        if counts.running == 1
            && let Some(agent) = counts.running_agent
        {
            format!("mux + {agent}")
        } else {
            format!("mux run{}", counts.running)
        }
    } else if counts.done > 0 {
        if counts.done == 1
            && let Some(agent) = counts.done_agent
        {
            format!("mux done {agent}")
        } else {
            format!("mux done{}", counts.done)
        }
    } else {
        String::new()
    }
}

pub fn agent_session_dots(snapshot: &Snapshot, current_session: Option<&str>) -> String {
    snapshot
        .sessions
        .iter()
        .map(|session| {
            let counts = agent_bridge_counts(snapshot, Some(&session.name));
            if counts.attention > 0 {
                '!'
            } else if current_session == Some(session.name.as_str()) {
                '*'
            } else if counts.running > 0 {
                '+'
            } else {
                '.'
            }
        })
        .collect()
}

pub fn agent_bridge_key_fragment(pane_id: &str) -> String {
    let fragment = pane_id
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();

    if fragment.is_empty() {
        String::from("_")
    } else {
        fragment
    }
}

pub fn normalize_agent_bridge_state(state: &str) -> Option<&'static str> {
    match state.trim().to_ascii_lowercase().as_str() {
        "" | "off" | "clear" | "none" => None,
        "run" | "running" | "working" | "thinking" | "tool-running" | "tool_running" => {
            Some("running")
        }
        "wait" | "waiting" | "needs-input" | "needs_input" | "input-required"
        | "input_required" | "approval" | "blocked" | "ask-user" | "ask_user" => Some("waiting"),
        "done" | "complete" | "completed" | "finished" | "success" | "stop" => Some("done"),
        "error" | "failed" | "failure" | "panic" => Some("error"),
        "stuck" | "stale" | "interrupted" | "hung" => Some("stuck"),
        "idle" | "quiet" => Some("idle"),
        _ => None,
    }
}

pub fn agent_bridge_event_needs_review(event: &AgentBridgeEvent) -> bool {
    matches!(normalize_agent_bridge_state(&event.state), Some("done"))
        && event.unseen.unwrap_or(false)
}

pub fn agent_bridge_event_suppresses_attention(event: &AgentBridgeEvent) -> bool {
    matches!(
        normalize_agent_bridge_state(&event.state),
        Some("done" | "error" | "stuck")
    ) && event.unseen == Some(false)
}

pub fn parse_agent_bridge_environment(raw: &str) -> HashMap<String, AgentBridgeEvent> {
    #[derive(Default)]
    struct PartialEvent {
        agent: Option<String>,
        state: Option<String>,
        summary: Option<String>,
        thread_id: Option<String>,
        thread_name: Option<String>,
        progress: Option<String>,
        log: Option<String>,
        unseen: Option<bool>,
        updated_at_unix_ms: Option<u64>,
    }

    let mut partials = HashMap::<String, PartialEvent>::new();

    for line in raw.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(rest) = key
            .strip_prefix("MUXBOARD_AGENT_PANE_")
            .or_else(|| key.strip_prefix("TMUX_AGENT_PANE_"))
        else {
            continue;
        };
        let Some((fragment, field)) = agent_bridge_env_fragment_and_field(rest) else {
            continue;
        };
        let fragment = agent_bridge_key_fragment(fragment);
        let partial = partials.entry(fragment).or_default();
        match field {
            "AGENT" => partial.agent = Some(value.trim().to_owned()),
            "STATE" => partial.state = Some(value.trim().to_owned()),
            "SUMMARY" => partial.summary = Some(value.trim().to_owned()),
            "THREAD_ID" => partial.thread_id = non_empty_env_value(value),
            "THREAD_NAME" => partial.thread_name = non_empty_env_value(value),
            "PROGRESS" => partial.progress = non_empty_env_value(value),
            "LOG" => partial.log = non_empty_env_value(value),
            "UNSEEN" => partial.unseen = parse_env_bool(value),
            "UPDATED_AT" => partial.updated_at_unix_ms = value.trim().parse().ok(),
            _ => {}
        }
    }

    partials
        .into_iter()
        .filter_map(|(fragment, partial)| {
            let state = normalize_agent_bridge_state(partial.state.as_deref()?)?;
            Some((
                fragment,
                AgentBridgeEvent {
                    agent: partial
                        .agent
                        .filter(|agent| !agent.trim().is_empty())
                        .unwrap_or_else(|| String::from("agent")),
                    state: state.to_owned(),
                    summary: partial.summary.unwrap_or_default(),
                    thread_id: partial.thread_id,
                    thread_name: partial.thread_name,
                    progress: partial.progress,
                    log: partial.log,
                    unseen: partial.unseen,
                    updated_at_unix_ms: partial.updated_at_unix_ms,
                },
            ))
        })
        .collect()
}

async fn agent_bridge_events(target: &Target) -> Result<HashMap<String, AgentBridgeEvent>> {
    let output = run_tmux_command(target, ["show-environment", "-g"]).await?;
    Ok(parse_agent_bridge_environment(&output))
}

fn agent_bridge_env_key(fragment: &str, field: &str) -> String {
    format!("MUXBOARD_AGENT_PANE_{fragment}_{field}")
}

fn agent_bridge_env_fragment_and_field(rest: &str) -> Option<(&str, &'static str)> {
    for (suffix, field) in [
        ("_AGENT", "AGENT"),
        ("_STATE", "STATE"),
        ("_SUMMARY", "SUMMARY"),
        ("_THREAD_ID", "THREAD_ID"),
        ("_THREAD_NAME", "THREAD_NAME"),
        ("_PROGRESS", "PROGRESS"),
        ("_LOG", "LOG"),
        ("_UNSEEN", "UNSEEN"),
        ("_UPDATED_AT", "UPDATED_AT"),
    ] {
        if let Some(fragment) = rest.strip_suffix(suffix)
            && !fragment.is_empty()
        {
            return Some((fragment, field));
        }
    }
    None
}

fn non_empty_env_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn parse_env_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "unseen" => Some(true),
        "0" | "false" | "no" | "off" | "seen" => Some(false),
        _ => None,
    }
}

#[derive(Debug, Default)]
struct AgentBridgeCounts {
    attention: usize,
    running: usize,
    done: usize,
    attention_agent: Option<String>,
    running_agent: Option<String>,
    done_agent: Option<String>,
}

fn agent_bridge_counts(snapshot: &Snapshot, session: Option<&str>) -> AgentBridgeCounts {
    let mut counts = AgentBridgeCounts::default();
    for pane in &snapshot.panes {
        if session.is_some_and(|session| pane.session_name != session) {
            continue;
        }
        let Some(event) = &pane.agent_event else {
            continue;
        };
        match normalize_agent_bridge_state(&event.state) {
            Some("waiting") => {
                add_agent_status_count(&mut counts.attention, &mut counts.attention_agent, event);
            }
            Some("error" | "stuck") if !agent_bridge_event_suppresses_attention(event) => {
                add_agent_status_count(&mut counts.attention, &mut counts.attention_agent, event);
            }
            Some("done") if agent_bridge_event_needs_review(event) => {
                add_agent_status_count(&mut counts.attention, &mut counts.attention_agent, event);
            }
            Some("running") => {
                add_agent_status_count(&mut counts.running, &mut counts.running_agent, event);
            }
            Some("done") => {
                add_agent_status_count(&mut counts.done, &mut counts.done_agent, event);
            }
            _ => {}
        }
    }
    counts
}

fn add_agent_status_count(
    count: &mut usize,
    single_agent: &mut Option<String>,
    event: &AgentBridgeEvent,
) {
    *count += 1;
    if *count == 1 {
        *single_agent = compact_status_agent(&event.agent);
    } else {
        *single_agent = None;
    }
}

fn compact_status_agent(agent: &str) -> Option<String> {
    let mut label = String::new();
    let mut previous_dash = false;
    for ch in agent.trim().chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-') {
            label.push(ch);
            previous_dash = false;
        } else if ch.is_whitespace() && !label.is_empty() && !previous_dash {
            label.push('-');
            previous_dash = true;
        }
        if label.len() >= 18 {
            break;
        }
    }
    while label.ends_with('-') {
        label.pop();
    }
    if label.is_empty() || matches!(label.as_str(), "agent" | "default") {
        None
    } else {
        Some(label)
    }
}

fn current_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

async fn run_tmux_command<'a>(
    target: &Target,
    args: impl IntoIterator<Item = &'a str>,
) -> Result<String> {
    let mut command = Command::new(&target.binary);

    if let Some(socket) = &target.socket {
        command.arg("-L").arg(socket);
    }

    command.args(args);

    command
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());

    let output = command
        .output()
        .await
        .with_context(|| tmux_start_error(&target.binary))?;

    if !output.status.success() {
        bail!(
            "tmux command failed for {}: {}",
            target.display_target(),
            command_failure_detail(&output)
        );
    }

    String::from_utf8(output.stdout).context("tmux output was not valid UTF-8")
}

fn tmux_start_error(binary: &str) -> String {
    format!("could not start `{binary}`. Install tmux or pass --tmux-bin <path>")
}

fn command_failure_detail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if !stderr.is_empty() {
        return stderr;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !stdout.is_empty() {
        return stdout;
    }

    format!("exit status {}", output.status)
}

async fn run_tmux_no_output<'a>(
    target: &Target,
    args: impl IntoIterator<Item = &'a str>,
) -> Result<()> {
    run_tmux_command(target, args).await.map(|_| ())
}

fn parse_pane_row(row: &str) -> Result<Pane> {
    let mut fields = row.split(FIELD_SEPARATOR);

    let session_id = next_field(&mut fields, "session_id", row)?;
    let session_name = next_field(&mut fields, "session_name", row)?;
    let window_id = next_field(&mut fields, "window_id", row)?;
    let window_name = next_field(&mut fields, "window_name", row)?;
    let pane_id = next_field(&mut fields, "pane_id", row)?;
    let pane_index = next_field(&mut fields, "pane_index", row)?;
    let pane_pid = next_field(&mut fields, "pane_pid", row)?;
    let title = next_field(&mut fields, "pane_title", row)?;
    let current_command = next_field(&mut fields, "pane_current_command", row)?;
    let current_path = next_field(&mut fields, "pane_current_path", row)?;
    let active = next_field(&mut fields, "pane_active", row)?;
    let alternate_on = next_field(&mut fields, "alternate_on", row)?;
    if fields.next().is_some() {
        bail!("extra field in tmux row `{row}`");
    }

    Ok(Pane {
        id: pane_id.to_owned(),
        session_id: session_id.to_owned(),
        session_name: session_name.to_owned(),
        window_id: window_id.to_owned(),
        window_name: window_name.to_owned(),
        pane_index: pane_index
            .parse::<u32>()
            .with_context(|| format!("invalid pane_index in `{row}`"))?,
        pane_pid: pane_pid
            .parse::<u32>()
            .with_context(|| format!("invalid pane_pid in `{row}`"))?,
        title: title.to_owned(),
        current_command: current_command.to_owned(),
        current_path: current_path.to_owned(),
        active: active == "1",
        alternate_on: alternate_on == "1",
        agent_event: None,
    })
}

fn capture_pane_args(pane: &Pane, lines: usize) -> Vec<String> {
    let start = format!("-{}", lines.max(1));
    let mut args = vec![String::from("capture-pane")];
    if pane.alternate_on {
        args.push(String::from("-a"));
    }
    args.extend([
        String::from("-J"),
        String::from("-p"),
        String::from("-S"),
        start,
        String::from("-t"),
        pane.id.clone(),
    ]);
    args
}

fn normalize_capture_lines(output: &str) -> Vec<String> {
    let mut lines = output
        .lines()
        .map(str::trim_end)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    lines
}

fn next_field<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
    name: &str,
    row: &str,
) -> Result<&'a str> {
    fields
        .next()
        .with_context(|| format!("missing {name} in tmux row `{row}`"))
}

fn socket_name_from_tmux_env(raw: &std::ffi::OsStr) -> Option<String> {
    let raw = raw.to_str()?;
    let socket_path = raw.split(',').next()?;
    Path::new(socket_path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsStr,
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        AgentBridgeEvent, Pane, RuntimeContext, Session, Snapshot, Target, Window,
        agent_bridge_event_suppresses_attention, agent_bridge_key_fragment, agent_session_dots,
        agent_status_segment, capture_pane_args, capture_pane_tail, current_client_tty, focus_pane,
        jump_client_to_pane, new_window, normalize_agent_bridge_state, normalize_capture_lines,
        parse_agent_bridge_environment, parse_pane_row, probe, send_keys, send_text, snapshot,
        snapshot_command_args, socket_name_from_tmux_env, toggle_zoom,
    };

    static SCRIPT_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_test_path(name: &str, suffix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should work")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "muxboard-{name}-{}-{}-{unique}{suffix}",
            std::process::id(),
            SCRIPT_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn script_path(name: &str, body: &str) -> PathBuf {
        let path = unique_test_path(name, ".sh");
        let body = if body.starts_with("#!") {
            body.to_owned()
        } else {
            format!("#!/usr/bin/env sh\n{body}")
        };
        fs::write(&path, body).expect("script should be writable");
        let mut permissions = fs::metadata(&path)
            .expect("script metadata should exist")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("script should be executable");
        path
    }

    fn target_for_binary(binary: String) -> Target {
        Target {
            binary,
            socket: None,
            session: None,
        }
    }

    fn shell_quote_path(path: &Path) -> String {
        format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
    }

    #[test]
    fn command_preview_includes_socket_and_session() {
        let target = Target {
            binary: String::from("tmux"),
            socket: Some(String::from("agents")),
            session: Some(String::from("ops")),
        };

        assert_eq!(
            target.command_preview(),
            "tmux -L agents attach-session -t ops"
        );
    }

    #[test]
    fn display_target_describes_socket_and_session_combinations() {
        let cases = [
            (
                Target {
                    binary: String::from("tmux"),
                    socket: Some(String::from("agents")),
                    session: Some(String::from("ops")),
                },
                "socket `agents`, session `ops`",
                "tmux -L agents attach-session -t ops",
            ),
            (
                Target {
                    binary: String::from("tmux"),
                    socket: Some(String::from("agents")),
                    session: None,
                },
                "socket `agents`, default session",
                "tmux -L agents attach-session",
            ),
            (
                Target {
                    binary: String::from("tmux"),
                    socket: None,
                    session: Some(String::from("ops")),
                },
                "default socket, session `ops`",
                "tmux attach-session -t ops",
            ),
            (
                Target {
                    binary: String::from("tmux"),
                    socket: None,
                    session: None,
                },
                "default socket, default session",
                "tmux attach-session",
            ),
        ];

        for (target, display, preview) in cases {
            assert_eq!(target.display_target(), display);
            assert_eq!(target.command_preview(), preview);
        }
    }

    #[test]
    fn parses_snapshot_row() {
        let row = "$0\tdemo\t@0\tops\t%2\t1\t4242\tagent\tclaude\t/workspace\t1\t0";
        let pane = parse_pane_row(row).expect("row should parse");

        assert_eq!(pane.session_name, "demo");
        assert_eq!(pane.window_name, "ops");
        assert_eq!(pane.current_command, "claude");
        assert_eq!(pane.pane_pid, 4242);
        assert!(pane.active);
        assert!(!pane.alternate_on);
    }

    #[test]
    fn parse_snapshot_row_reports_missing_and_invalid_fields() {
        let missing = parse_pane_row("$0\tdemo");
        assert!(missing.is_err());
        assert!(
            missing
                .expect_err("short row should report the first missing field")
                .to_string()
                .contains("missing window_id")
        );

        let invalid_index =
            parse_pane_row("$0\tdemo\t@0\tops\t%2\tnope\t4242\tagent\tclaude\t/workspace\t1\t0");
        assert!(invalid_index.is_err());
        assert!(
            invalid_index
                .expect_err("invalid pane index should be rejected")
                .to_string()
                .contains("invalid pane_index")
        );

        let invalid_pid =
            parse_pane_row("$0\tdemo\t@0\tops\t%2\t1\tnope\tagent\tclaude\t/workspace\t1\t0");
        assert!(invalid_pid.is_err());
        assert!(
            invalid_pid
                .expect_err("invalid pane pid should be rejected")
                .to_string()
                .contains("invalid pane_pid")
        );

        let extra_field = parse_pane_row(
            "$0\tdemo\t@0\tops\t%2\t1\t4242\tagent\tclaude\t/workspace\t1\t0\tshifted",
        );
        assert!(extra_field.is_err());
        assert!(
            extra_field
                .expect_err("extra fields should not silently shift pane data")
                .to_string()
                .contains("extra field")
        );
    }

    #[test]
    fn parses_agent_bridge_environment_by_sanitized_pane_id() {
        let raw = "\
MUXBOARD_AGENT_PANE__1_AGENT=codex
MUXBOARD_AGENT_PANE__1_STATE=needs-input
MUXBOARD_AGENT_PANE__1_SUMMARY=approval needed
MUXBOARD_AGENT_PANE__1_THREAD_ID=turn-123
MUXBOARD_AGENT_PANE__1_THREAD_NAME=Fix release UI
MUXBOARD_AGENT_PANE__1_PROGRESS=3/10 tests
MUXBOARD_AGENT_PANE__1_LOG=waiting on approval
MUXBOARD_AGENT_PANE__1_UNSEEN=1
MUXBOARD_AGENT_PANE__1_UPDATED_AT=1710000000000
MUXBOARD_AGENT_PANE__2_AGENT=claude
MUXBOARD_AGENT_PANE__2_STATE=off
TMUX_AGENT_PANE_%3_AGENT=opencode
TMUX_AGENT_PANE_%3_STATE=running
TMUX_AGENT_PANE_%3_SUMMARY=writing tests
MUXBOARD_AGENT_PANE__4_AGENT=codex
MUXBOARD_AGENT_PANE__4_STATE=done
MUXBOARD_AGENT_PANE__4_UNSEEN=0
";

        let events = parse_agent_bridge_environment(raw);
        let event = events.get("_1").expect("waiting event should parse");
        let legacy_event = events
            .get("_3")
            .expect("tmux-agent-indicator style event should parse");
        let seen_event = events.get("_4").expect("seen done event should parse");

        assert_eq!(agent_bridge_key_fragment("%1"), "_1");
        assert_eq!(event.agent, "codex");
        assert_eq!(event.state, "waiting");
        assert_eq!(event.summary, "approval needed");
        assert_eq!(event.thread_id.as_deref(), Some("turn-123"));
        assert_eq!(event.thread_name.as_deref(), Some("Fix release UI"));
        assert_eq!(event.progress.as_deref(), Some("3/10 tests"));
        assert_eq!(event.log.as_deref(), Some("waiting on approval"));
        assert_eq!(event.unseen, Some(true));
        assert_eq!(event.updated_at_unix_ms, Some(1_710_000_000_000));
        assert!(
            !events.contains_key("_2"),
            "off events should be ignored by snapshots"
        );
        assert_eq!(legacy_event.agent, "opencode");
        assert_eq!(legacy_event.state, "running");
        assert_eq!(legacy_event.summary, "writing tests");
        assert_eq!(seen_event.unseen, Some(false));
        assert!(agent_bridge_event_suppresses_attention(seen_event));
    }

    #[test]
    fn normalizes_agent_bridge_states_from_common_hooks() {
        assert_eq!(
            normalize_agent_bridge_state("tool-running"),
            Some("running")
        );
        assert_eq!(normalize_agent_bridge_state("needs_input"), Some("waiting"));
        assert_eq!(normalize_agent_bridge_state("interrupted"), Some("stuck"));
        assert_eq!(normalize_agent_bridge_state("off"), None);
        assert_eq!(normalize_agent_bridge_state("mystery"), None);
    }

    #[test]
    fn agent_status_segment_and_session_dots_surface_attention() {
        let mut waiting =
            parse_pane_row("$0\talpha\t@0\tagents\t%1\t0\t100\tworkspace\tnode\t/workspace\t1\t0")
                .expect("pane row should parse");
        waiting.agent_event = Some(AgentBridgeEvent {
            agent: String::from("codex"),
            state: String::from("waiting"),
            summary: String::from("approval"),
            ..AgentBridgeEvent::default()
        });
        let mut review =
            parse_pane_row("$2\tgamma\t@2\tagents\t%3\t0\t102\tworkspace\tcodex\t/workspace\t1\t0")
                .expect("pane row should parse");
        review.agent_event = Some(AgentBridgeEvent {
            agent: String::from("codex"),
            state: String::from("done"),
            summary: String::from("review changes"),
            unseen: Some(true),
            updated_at_unix_ms: None,
            ..AgentBridgeEvent::default()
        });
        let mut running =
            parse_pane_row("$1\tbeta\t@1\tagents\t%2\t0\t101\tworkspace\tclaude\t/workspace\t1\t0")
                .expect("pane row should parse");
        running.agent_event = Some(AgentBridgeEvent {
            agent: String::from("claude"),
            state: String::from("running"),
            summary: String::from("writing tests"),
            ..AgentBridgeEvent::default()
        });
        let snapshot = Snapshot {
            sessions: vec![
                Session {
                    id: String::from("$0"),
                    name: String::from("alpha"),
                },
                Session {
                    id: String::from("$1"),
                    name: String::from("beta"),
                },
                Session {
                    id: String::from("$2"),
                    name: String::from("gamma"),
                },
            ],
            windows: vec![
                Window {
                    id: String::from("@0"),
                    session_id: String::from("$0"),
                    session_name: String::from("alpha"),
                    name: String::from("agents"),
                },
                Window {
                    id: String::from("@1"),
                    session_id: String::from("$1"),
                    session_name: String::from("beta"),
                    name: String::from("agents"),
                },
                Window {
                    id: String::from("@2"),
                    session_id: String::from("$2"),
                    session_name: String::from("gamma"),
                    name: String::from("agents"),
                },
            ],
            panes: vec![waiting, running, review],
        };

        assert_eq!(agent_status_segment(&snapshot, None), "mux !2");
        assert_eq!(
            agent_status_segment(&snapshot, Some("alpha")),
            "mux ! codex"
        );
        assert_eq!(
            agent_status_segment(&snapshot, Some("beta")),
            "mux + claude"
        );
        assert_eq!(
            agent_status_segment(&snapshot, Some("gamma")),
            "mux ! codex"
        );
        assert_eq!(agent_session_dots(&snapshot, Some("beta")), "!*!");
    }

    #[test]
    fn session_dots_still_show_current_session_when_all_quiet() {
        let snapshot = Snapshot {
            sessions: vec![
                Session {
                    id: String::from("$0"),
                    name: String::from("alpha"),
                },
                Session {
                    id: String::from("$1"),
                    name: String::from("beta"),
                },
                Session {
                    id: String::from("$2"),
                    name: String::from("gamma"),
                },
            ],
            windows: Vec::new(),
            panes: Vec::new(),
        };

        assert_eq!(agent_status_segment(&snapshot, None), "");
        assert_eq!(agent_session_dots(&snapshot, Some("beta")), ".*.");
    }

    #[test]
    fn single_agent_status_segment_uses_compact_label_only_when_useful() {
        let mut pane =
            parse_pane_row("$0\talpha\t@0\tagents\t%1\t0\t100\tworkspace\tnode\t/workspace\t1\t0")
                .expect("pane row should parse");
        pane.agent_event = Some(AgentBridgeEvent {
            agent: String::from("Claude Code"),
            state: String::from("running"),
            summary: String::from("writing tests"),
            ..AgentBridgeEvent::default()
        });
        let mut generic = pane.clone();
        generic.agent_event = Some(AgentBridgeEvent {
            agent: String::from("agent"),
            state: String::from("running"),
            summary: String::from("writing tests"),
            ..AgentBridgeEvent::default()
        });
        let snapshot = |pane| Snapshot {
            sessions: vec![Session {
                id: String::from("$0"),
                name: String::from("alpha"),
            }],
            windows: vec![Window {
                id: String::from("@0"),
                session_id: String::from("$0"),
                session_name: String::from("alpha"),
                name: String::from("agents"),
            }],
            panes: vec![pane],
        };

        assert_eq!(
            agent_status_segment(&snapshot(pane), None),
            "mux + claude-code"
        );
        assert_eq!(agent_status_segment(&snapshot(generic), None), "mux run1");
    }

    #[test]
    fn capture_pane_uses_alternate_screen_when_present() {
        let pane = Pane {
            id: String::from("%9"),
            session_id: String::from("$0"),
            session_name: String::from("ops"),
            window_id: String::from("@0"),
            window_name: String::from("agents"),
            pane_index: 0,
            pane_pid: 4242,
            title: String::from("agent"),
            current_command: String::from("codex"),
            current_path: String::from("/workspace"),
            active: true,
            alternate_on: true,
            agent_event: None,
        };

        assert_eq!(
            capture_pane_args(&pane, 24),
            vec!["capture-pane", "-a", "-J", "-p", "-S", "-24", "-t", "%9"]
        );
    }

    #[test]
    fn capture_pane_omits_alternate_flag_and_clamps_line_count() {
        let pane = Pane {
            id: String::from("%9"),
            session_id: String::from("$0"),
            session_name: String::from("ops"),
            window_id: String::from("@0"),
            window_name: String::from("agents"),
            pane_index: 0,
            pane_pid: 4242,
            title: String::from("agent"),
            current_command: String::from("codex"),
            current_path: String::from("/workspace"),
            active: true,
            alternate_on: false,
            agent_event: None,
        };

        assert_eq!(
            capture_pane_args(&pane, 0),
            vec!["capture-pane", "-J", "-p", "-S", "-1", "-t", "%9"]
        );
    }

    #[test]
    fn normalize_capture_lines_trims_outer_blank_rows_only() {
        let lines = normalize_capture_lines("\n\nheader\n\nbody\n\n");
        assert_eq!(lines, vec!["header", "", "body"]);
    }

    #[test]
    fn snapshot_targets_selected_session_when_present() {
        let target = Target {
            binary: String::from("tmux"),
            socket: Some(String::from("agents")),
            session: Some(String::from("ops")),
        };

        let args = snapshot_command_args(&target, "format");

        assert_eq!(args, vec!["list-panes", "-s", "-t", "ops", "-F", "format"]);
    }

    #[test]
    fn snapshot_uses_all_sessions_when_no_session_is_requested() {
        let target = Target {
            binary: String::from("tmux"),
            socket: None,
            session: None,
        };

        let args = snapshot_command_args(&target, "format");

        assert_eq!(args, vec!["list-panes", "-a", "-F", "format"]);
    }

    #[test]
    fn runtime_context_matches_default_socket_when_inside_default_tmux() {
        let context = RuntimeContext {
            socket_name: Some(String::from("default")),
            pane_id: Some(String::from("%9")),
        };
        let target = Target {
            binary: String::from("tmux"),
            socket: None,
            session: Some(String::from("ops")),
        };

        assert!(context.is_same_server(&target));
    }

    #[test]
    fn runtime_context_matches_named_socket_only_when_target_matches() {
        let context = RuntimeContext {
            socket_name: Some(String::from("agents")),
            pane_id: Some(String::from("%9")),
        };
        let matching = Target {
            binary: String::from("tmux"),
            socket: Some(String::from("agents")),
            session: None,
        };
        let other = Target {
            binary: String::from("tmux"),
            socket: Some(String::from("other")),
            session: None,
        };

        assert!(context.is_same_server(&matching));
        assert!(!context.is_same_server(&other));
    }

    #[test]
    fn runtime_context_from_env_values_handles_local_and_detached_shells() {
        let local_tmux = RuntimeContext::from_env_values(
            Some(OsStr::new("/tmp/tmux-501/agents,1234,0")),
            Some("%42"),
        );
        assert_eq!(
            local_tmux,
            RuntimeContext {
                socket_name: Some(String::from("agents")),
                pane_id: Some(String::from("%42")),
            }
        );

        let detached_shell = RuntimeContext::from_env_values(None, Some(""));
        assert_eq!(detached_shell, RuntimeContext::default());
    }

    #[test]
    fn runtime_context_from_env_is_safe_to_call_without_assuming_tmux() {
        let context = RuntimeContext::from_env();

        assert_ne!(context.socket_name.as_deref(), Some(""));
        assert_ne!(context.pane_id.as_deref(), Some(""));
    }

    #[test]
    fn socket_name_is_parsed_from_tmux_environment() {
        let socket = socket_name_from_tmux_env(OsStr::new("/tmp/tmux-501/review-socket,1234,0"))
            .expect("tmux env should include a socket name");

        assert_eq!(socket, "review-socket");
    }

    #[test]
    fn socket_name_parser_rejects_invalid_or_empty_values() {
        assert_eq!(socket_name_from_tmux_env(OsStr::new("")), None);
        assert_eq!(socket_name_from_tmux_env(OsStr::new(",")), None);
    }

    #[tokio::test]
    async fn probe_and_tmux_wrappers_cover_success_and_failure_without_real_tmux() {
        let version_script = script_path("version", "printf 'tmux 3.5a\\n'\n");
        let target = target_for_binary(version_script.display().to_string());
        let probed = probe(target.clone())
            .await
            .expect("probe should parse version");
        assert_eq!(probed.version, "tmux 3.5a");

        let failure_script = script_path("failure", "printf 'boom' >&2\nexit 2\n");
        let failed = probe(target_for_binary(failure_script.display().to_string()))
            .await
            .expect_err("non-zero version command should fail")
            .to_string();
        assert!(failed.contains("could not read tmux version"));
        assert!(failed.contains("boom"));

        let stdout_failure_script = script_path(
            "stdout-failure",
            "printf 'plain stdout failure\\n'\nexit 3\n",
        );
        let stdout_failed = probe(target_for_binary(
            stdout_failure_script.display().to_string(),
        ))
        .await
        .expect_err("stdout-only failure should be visible")
        .to_string();
        assert!(stdout_failed.contains("plain stdout failure"));

        let silent_failure_script = script_path("silent-failure", "exit 4\n");
        let silent_failed = probe(target_for_binary(
            silent_failure_script.display().to_string(),
        ))
        .await
        .expect_err("silent failure should include the exit status")
        .to_string();
        assert!(silent_failed.contains("exit status"));

        let missing = probe(target_for_binary(String::from(
            "/tmp/muxboard-missing-tmux-bin",
        )))
        .await
        .expect_err("missing binary should fail")
        .to_string();
        assert!(missing.contains("could not start"));
        assert!(missing.contains("--tmux-bin"));
    }

    #[tokio::test]
    async fn snapshot_and_capture_use_command_output_parsers() {
        let row_two = "$1\tbeta\t@2\tbuild\t%4\t0\t101\tworker\tzsh\t/tmp\t0\t1";
        let row_one = "$0\talpha\t@1\tagents\t%3\t2\t100\ttitle\tcodex\t/work\t1\t0";
        let snapshot_script = script_path(
            "snapshot",
            &format!("cat <<'EOF'\n{}\n{}\nEOF\n", row_two, row_one),
        );
        let snap = snapshot(target_for_binary(snapshot_script.display().to_string()))
            .await
            .expect("snapshot should parse scripted rows");

        assert_eq!(snap.session_count(), 2);
        assert_eq!(snap.window_count(), 2);
        assert_eq!(snap.pane_count(), 2);
        assert_eq!(snap.sessions[0].name, "alpha");
        assert_eq!(snap.panes[0].id, "%3");

        let capture_script = script_path("capture", "printf '\\nfirst\\nsecond\\n\\n'\n");
        let lines = capture_pane_tail(
            &target_for_binary(capture_script.display().to_string()),
            &snap.panes[0],
            24,
        )
        .await
        .expect("capture should normalize scripted output");
        assert_eq!(lines, vec!["first", "second"]);
    }

    #[tokio::test]
    async fn pane_action_wrappers_pass_socket_and_exact_targets_to_tmux() {
        let log_path = unique_test_path("tmux-calls", ".log");
        let _ = fs::remove_file(&log_path);
        let record_script = script_path(
            "record",
            &format!("printf '%s\\n' \"$*\" >> {}\n", shell_quote_path(&log_path)),
        );
        let target = Target {
            binary: record_script.display().to_string(),
            socket: Some(String::from("agents")),
            session: Some(String::from("ops")),
        };
        let pane = Pane {
            id: String::from("%9"),
            session_id: String::from("$0"),
            session_name: String::from("ops"),
            window_id: String::from("@7"),
            window_name: String::from("agents"),
            pane_index: 0,
            pane_pid: 4242,
            title: String::from("agent"),
            current_command: String::from("codex"),
            current_path: String::from("/workspace"),
            active: true,
            alternate_on: false,
            agent_event: None,
        };

        focus_pane(&target, &pane)
            .await
            .expect("focus wrapper should run both commands");
        toggle_zoom(&target, &pane.id)
            .await
            .expect("zoom wrapper should run");
        send_text(&target, &pane.id, "hello world", true)
            .await
            .expect("send text wrapper should run literal send plus enter");
        new_window(
            &target,
            &pane.session_name,
            "codex",
            &pane.current_path,
            "codex --dangerously-bypass-approvals-and-sandbox",
        )
        .await
        .expect("new window wrapper should pass launch command");
        current_client_tty(&target, &pane.id)
            .await
            .expect("tty wrapper should trim command output");
        jump_client_to_pane(&target, "/dev/ttys001", &pane)
            .await
            .expect("jump wrapper should switch then focus");

        let recorded = fs::read_to_string(&log_path).expect("tmux calls should be recorded");
        let calls = recorded.lines().collect::<Vec<_>>();
        assert_eq!(
            calls,
            vec![
                "-L agents select-window -t @7",
                "-L agents select-pane -t %9",
                "-L agents resize-pane -Z -t %9",
                "-L agents send-keys -t %9 -l -- hello world",
                "-L agents send-keys -t %9 Enter",
                "-L agents new-window -d -t ops -n codex -c /workspace codex --dangerously-bypass-approvals-and-sandbox",
                "-L agents display-message -p -t %9 #{client_tty}",
                "-L agents switch-client -c /dev/ttys001 -t ops",
                "-L agents select-window -t @7",
                "-L agents select-pane -t %9",
            ]
        );
    }

    #[tokio::test]
    async fn send_text_only_presses_enter_when_requested() {
        let log_path = unique_test_path("tmux-send-text", ".log");
        let _ = fs::remove_file(&log_path);
        let record_script = script_path(
            "record-send-text",
            &format!("printf '%s\\n' \"$*\" >> {}\n", shell_quote_path(&log_path)),
        );
        let target = Target {
            binary: record_script.display().to_string(),
            socket: Some(String::from("agents")),
            session: Some(String::from("ops")),
        };

        send_text(&target, "%9", "summarize this pane", false)
            .await
            .expect("literal-only send should not press enter");
        send_text(&target, "%9", "run it now", true)
            .await
            .expect("submitted send should press enter after literal text");

        let recorded = fs::read_to_string(&log_path).expect("tmux calls should be recorded");
        let calls = recorded.lines().collect::<Vec<_>>();
        assert_eq!(
            calls,
            vec![
                "-L agents send-keys -t %9 -l -- summarize this pane",
                "-L agents send-keys -t %9 -l -- run it now",
                "-L agents send-keys -t %9 Enter",
            ]
        );
    }

    #[tokio::test]
    async fn tmux_action_wrapper_errors_include_stderr() {
        let failure_script = script_path("action-failure", "printf 'send denied' >&2\nexit 42\n");
        let target = target_for_binary(failure_script.display().to_string());

        let error = send_keys(&target, "%9", &["Enter"])
            .await
            .expect_err("failed tmux action should surface stderr")
            .to_string();

        assert!(error.contains("tmux command failed for"));
        assert!(error.contains("send denied"));
    }
}
