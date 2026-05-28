use std::process::Stdio;

use anyhow::{Context, Result};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::mpsc,
    task::JoinHandle,
};

use super::Target;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Begin {
        timestamp: i64,
        command_number: u64,
        flags: u64,
    },
    End {
        timestamp: i64,
        command_number: u64,
        flags: u64,
    },
    Error {
        timestamp: i64,
        command_number: u64,
        flags: u64,
    },
    Output {
        pane_id: String,
        payload: String,
    },
    ExtendedOutput {
        pane_id: String,
        age_millis: u64,
        payload: String,
    },
    PaneModeChanged {
        pane_id: String,
    },
    WindowPaneChanged {
        window_id: String,
        pane_id: String,
    },
    WindowClose {
        window_id: String,
    },
    WindowAdd {
        window_id: String,
    },
    WindowRenamed {
        window_id: String,
        name: String,
    },
    SessionChanged {
        session_id: String,
        name: String,
    },
    ClientSessionChanged {
        client: String,
        session_id: String,
        name: String,
    },
    SessionRenamed {
        session_id: String,
        name: String,
    },
    SessionsChanged,
    SessionWindowChanged {
        session_id: String,
        window_id: String,
    },
    Exit {
        reason: Option<String>,
    },
    Unknown {
        raw: String,
    },
}

impl Event {
    pub fn summary(&self) -> String {
        match self {
            Self::Begin { command_number, .. } => format!("command {command_number} started"),
            Self::End { command_number, .. } => format!("command {command_number} finished"),
            Self::Error { command_number, .. } => format!("command {command_number} failed"),
            Self::Output { pane_id, payload } => {
                format!("output from {pane_id}: {}", summarize_payload(payload))
            }
            Self::ExtendedOutput {
                pane_id,
                age_millis,
                payload,
            } => format!(
                "output from {pane_id} ({age_millis} ms buffered): {}",
                summarize_payload(payload)
            ),
            Self::PaneModeChanged { pane_id } => format!("pane mode changed: {pane_id}"),
            Self::WindowPaneChanged { window_id, pane_id } => {
                format!("active pane changed: {window_id} -> {pane_id}")
            }
            Self::WindowClose { window_id } => format!("window closed: {window_id}"),
            Self::WindowAdd { window_id } => format!("window added: {window_id}"),
            Self::WindowRenamed { window_id, name } => {
                format!("window renamed: {window_id} -> {name}")
            }
            Self::SessionChanged { session_id, name } => {
                format!("session changed: {session_id} -> {name}")
            }
            Self::ClientSessionChanged {
                client,
                session_id,
                name,
            } => format!("client {client} switched to {session_id} ({name})"),
            Self::SessionRenamed { session_id, name } => {
                format!("session renamed: {session_id} -> {name}")
            }
            Self::SessionsChanged => String::from("sessions changed"),
            Self::SessionWindowChanged {
                session_id,
                window_id,
            } => format!("session {session_id} current window -> {window_id}"),
            Self::Exit { reason } => match reason {
                Some(reason) => format!("control client exited: {reason}"),
                None => String::from("control client exited"),
            },
            Self::Unknown { raw } => format!("notification: {}", truncate(raw)),
        }
    }

    pub fn is_structural(&self) -> bool {
        matches!(
            self,
            Self::WindowClose { .. }
                | Self::WindowAdd { .. }
                | Self::SessionsChanged
                | Self::SessionChanged { .. }
                | Self::ClientSessionChanged { .. }
        )
    }

    pub fn output_summary(&self) -> Option<(&str, String)> {
        match self {
            Self::Output { pane_id, payload } => Some((pane_id, summarize_payload(payload))),
            Self::ExtendedOutput {
                pane_id, payload, ..
            } => Some((pane_id, summarize_payload(payload))),
            _ => None,
        }
    }

    pub fn output_text(&self) -> Option<(&str, String)> {
        match self {
            Self::Output { pane_id, payload } => Some((pane_id, normalize_payload(payload))),
            Self::ExtendedOutput {
                pane_id, payload, ..
            } => Some((pane_id, normalize_payload(payload))),
            _ => None,
        }
    }

    pub fn output_chunk(&self) -> Option<(&str, String)> {
        match self {
            Self::Output { pane_id, payload } => Some((pane_id, normalize_output_chunk(payload))),
            Self::ExtendedOutput {
                pane_id, payload, ..
            } => Some((pane_id, normalize_output_chunk(payload))),
            _ => None,
        }
    }

    pub fn output_age_millis(&self) -> Option<u64> {
        match self {
            Self::Output { .. } => None,
            Self::ExtendedOutput { age_millis, .. } => Some(*age_millis),
            _ => None,
        }
    }

    pub fn is_loggable(&self) -> bool {
        !matches!(
            self,
            Self::Begin { .. }
                | Self::End { .. }
                | Self::Output { .. }
                | Self::ExtendedOutput { .. }
        )
    }
}

#[derive(Debug)]
pub struct Monitor {
    rx: mpsc::Receiver<Event>,
    task: JoinHandle<()>,
}

impl Monitor {
    pub fn try_recv(&mut self) -> Option<Event> {
        self.rx.try_recv().ok()
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    #[cfg(test)]
    pub(crate) fn for_test(rx: mpsc::Receiver<Event>, task: JoinHandle<()>) -> Self {
        Self { rx, task }
    }

    #[cfg(test)]
    pub(crate) async fn recv_for_test(&mut self) -> Option<Event> {
        self.rx.recv().await
    }
}

pub async fn start(target: &Target) -> Result<Monitor> {
    let mut command = Command::new(&target.binary);

    if let Some(socket) = &target.socket {
        command.arg("-L").arg(socket);
    }

    command
        .arg("-C")
        .arg("attach-session")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());

    if let Some(session) = &target.session {
        command.arg("-t").arg(session);
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to start control client with `{}`", target.binary))?;

    let stdout = child
        .stdout
        .take()
        .context("control client stdout was not piped")?;
    let stderr = child
        .stderr
        .take()
        .context("control client stderr was not piped")?;
    let stdin_guard = child.stdin.take();

    let (tx, rx) = mpsc::channel(256);

    let task = tokio::spawn(async move {
        let _stdin_guard = stdin_guard;

        let stdout_task = {
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();

                loop {
                    match lines.next_line().await {
                        Ok(Some(line)) => {
                            let event = parse_line(&line);
                            if tx.send(event).await.is_err() {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(error) => {
                            let _ = tx
                                .send(Event::Unknown {
                                    raw: format!("stdout read error: {error}"),
                                })
                                .await;
                            break;
                        }
                    }
                }
            })
        };

        let stderr_task = {
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();

                loop {
                    match lines.next_line().await {
                        Ok(Some(line)) => {
                            let _ = tx
                                .send(Event::Unknown {
                                    raw: format!("stderr: {line}"),
                                })
                                .await;
                        }
                        Ok(None) => break,
                        Err(error) => {
                            let _ = tx
                                .send(Event::Unknown {
                                    raw: format!("stderr read error: {error}"),
                                })
                                .await;
                            break;
                        }
                    }
                }
            })
        };

        let status = child.wait().await;
        let _ = stdout_task.await;
        let _ = stderr_task.await;

        let exit_event = exit_event_from_wait(status);

        let _ = tx.send(exit_event).await;
    });

    Ok(Monitor { rx, task })
}

fn exit_event_from_wait(status: std::io::Result<std::process::ExitStatus>) -> Event {
    match status {
        Ok(status) if status.success() => Event::Exit { reason: None },
        Ok(status) => Event::Exit {
            reason: Some(format!("status {}", status)),
        },
        Err(error) => Event::Exit {
            reason: Some(error.to_string()),
        },
    }
}

fn parse_line(line: &str) -> Event {
    if let Some(event) = parse_block_line(line, "%begin", BlockKind::Begin) {
        return event;
    }
    if let Some(event) = parse_block_line(line, "%end", BlockKind::End) {
        return event;
    }
    if let Some(event) = parse_block_line(line, "%error", BlockKind::Error) {
        return event;
    }

    if let Some(rest) = line.strip_prefix("%output ") {
        let mut parts = rest.splitn(2, ' ');
        let pane_id = parts.next().unwrap_or_default().to_owned();
        let payload = parts.next().unwrap_or_default().to_owned();
        return Event::Output { pane_id, payload };
    }

    if let Some(rest) = line.strip_prefix("%extended-output ") {
        let mut parts = rest.splitn(4, ' ');
        let pane_id = parts.next().unwrap_or_default().to_owned();
        let age_millis = parts
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or_default();
        let delimiter = parts.next().unwrap_or_default();
        let payload = if delimiter == ":" {
            parts.next().unwrap_or_default().to_owned()
        } else {
            String::new()
        };
        return Event::ExtendedOutput {
            pane_id,
            age_millis,
            payload,
        };
    }

    if let Some(rest) = line.strip_prefix("%pane-mode-changed ") {
        return Event::PaneModeChanged {
            pane_id: rest.to_owned(),
        };
    }

    if let Some(rest) = line.strip_prefix("%window-pane-changed ") {
        let mut parts = rest.splitn(2, ' ');
        return Event::WindowPaneChanged {
            window_id: parts.next().unwrap_or_default().to_owned(),
            pane_id: parts.next().unwrap_or_default().to_owned(),
        };
    }

    if let Some(rest) = line.strip_prefix("%window-close ") {
        return Event::WindowClose {
            window_id: rest.to_owned(),
        };
    }

    if let Some(rest) = line.strip_prefix("%window-add ") {
        return Event::WindowAdd {
            window_id: rest.to_owned(),
        };
    }

    if let Some(rest) = line.strip_prefix("%window-renamed ") {
        let mut parts = rest.splitn(2, ' ');
        return Event::WindowRenamed {
            window_id: parts.next().unwrap_or_default().to_owned(),
            name: parts.next().unwrap_or_default().to_owned(),
        };
    }

    if let Some(rest) = line.strip_prefix("%session-changed ") {
        let mut parts = rest.splitn(2, ' ');
        return Event::SessionChanged {
            session_id: parts.next().unwrap_or_default().to_owned(),
            name: parts.next().unwrap_or_default().to_owned(),
        };
    }

    if let Some(rest) = line.strip_prefix("%client-session-changed ") {
        let mut parts = rest.splitn(3, ' ');
        return Event::ClientSessionChanged {
            client: parts.next().unwrap_or_default().to_owned(),
            session_id: parts.next().unwrap_or_default().to_owned(),
            name: parts.next().unwrap_or_default().to_owned(),
        };
    }

    if let Some(rest) = line.strip_prefix("%session-renamed ") {
        let mut parts = rest.splitn(2, ' ');
        return Event::SessionRenamed {
            session_id: parts.next().unwrap_or_default().to_owned(),
            name: parts.next().unwrap_or_default().to_owned(),
        };
    }

    if line == "%sessions-changed" {
        return Event::SessionsChanged;
    }

    if let Some(rest) = line.strip_prefix("%session-window-changed ") {
        let mut parts = rest.splitn(2, ' ');
        return Event::SessionWindowChanged {
            session_id: parts.next().unwrap_or_default().to_owned(),
            window_id: parts.next().unwrap_or_default().to_owned(),
        };
    }

    if let Some(rest) = line.strip_prefix("%exit") {
        let reason = rest.trim();
        return Event::Exit {
            reason: if reason.is_empty() {
                None
            } else {
                Some(reason.to_owned())
            },
        };
    }

    Event::Unknown {
        raw: line.to_owned(),
    }
}

#[derive(Clone, Copy)]
enum BlockKind {
    Begin,
    End,
    Error,
}

fn parse_block_line(line: &str, prefix: &str, kind: BlockKind) -> Option<Event> {
    let rest = line.strip_prefix(prefix)?.trim();
    let mut parts = rest.split_whitespace();
    let timestamp = parts.next()?.parse::<i64>().ok()?;
    let command_number = parts.next()?.parse::<u64>().ok()?;
    let flags = parts.next()?.parse::<u64>().ok()?;

    Some(match kind {
        BlockKind::Begin => Event::Begin {
            timestamp,
            command_number,
            flags,
        },
        BlockKind::End => Event::End {
            timestamp,
            command_number,
            flags,
        },
        BlockKind::Error => Event::Error {
            timestamp,
            command_number,
            flags,
        },
    })
}

fn truncate(input: &str) -> String {
    const LIMIT: usize = 56;

    let compact = input.trim();
    if compact.chars().count() <= LIMIT {
        return compact.to_owned();
    }

    let prefix = compact.chars().take(LIMIT).collect::<String>();
    format!("{prefix}...")
}

fn summarize_payload(payload: &str) -> String {
    let compact = normalize_payload(payload);
    truncate(compact.trim())
}

fn normalize_payload(payload: &str) -> String {
    let decoded = normalize_output_chunk(payload);
    let mut compact = String::new();
    let mut previous_was_space = false;

    for ch in decoded.chars() {
        let normalized = if ch.is_control() { ' ' } else { ch };
        let is_space = normalized.is_whitespace();

        if is_space {
            if !previous_was_space {
                compact.push(' ');
            }
        } else {
            compact.push(normalized);
        }

        previous_was_space = is_space;
    }

    compact.trim().to_owned()
}

fn normalize_output_chunk(payload: &str) -> String {
    let decoded = strip_terminal_sequences(&decode_tmux_escapes(payload));
    let mut normalized = String::new();

    for ch in decoded.chars() {
        match ch {
            '\n' | '\r' | '\u{8}' | '\u{7f}' => normalized.push(ch),
            '\t' => normalized.push(' '),
            _ if ch.is_control() => {}
            _ => normalized.push(ch),
        }
    }

    normalized
}

fn decode_tmux_escapes(payload: &str) -> String {
    let bytes = payload.as_bytes();
    let mut index = 0;
    let mut decoded = Vec::with_capacity(bytes.len());

    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let octal = &bytes[index + 1..index + 4];
            if octal.iter().all(|digit| (b'0'..=b'7').contains(digit)) {
                let value = (octal[0] - b'0') * 64 + (octal[1] - b'0') * 8 + (octal[2] - b'0');
                decoded.push(value);
                index += 4;
                continue;
            }
        }

        decoded.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&decoded).into_owned()
}

fn strip_terminal_sequences(input: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Normal,
        Escape,
        Csi,
        Osc,
        OscEscape,
    }

    let mut state = State::Normal;
    let mut stripped = String::new();

    for ch in input.chars() {
        match state {
            State::Normal => {
                if ch == '\u{1b}' {
                    state = State::Escape;
                } else {
                    stripped.push(ch);
                }
            }
            State::Escape => {
                state = match ch {
                    '[' => State::Csi,
                    ']' => State::Osc,
                    _ => State::Normal,
                };
            }
            State::Csi => {
                if ('@'..='~').contains(&ch) {
                    state = State::Normal;
                }
            }
            State::Osc => {
                if ch == '\u{07}' {
                    state = State::Normal;
                } else if ch == '\u{1b}' {
                    state = State::OscEscape;
                }
            }
            State::OscEscape => {
                state = if ch == '\\' {
                    State::Normal
                } else {
                    State::Osc
                };
            }
        }
    }

    stripped
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use super::super::Target;
    use super::{
        Event, Monitor, exit_event_from_wait, normalize_output_chunk, normalize_payload,
        parse_line, start, strip_terminal_sequences, summarize_payload, truncate,
    };

    static SCRIPT_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn script_path(name: &str, body: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should work")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "muxboard-control-{name}-{}-{}-{unique}.sh",
            std::process::id(),
            SCRIPT_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, body).expect("script should be writable");
        let mut permissions = fs::metadata(&path)
            .expect("script metadata should exist")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("script should be executable");
        path
    }

    #[test]
    fn parses_output_line() {
        let event = parse_line("%output %12 hello\\040world");
        assert_eq!(
            event,
            Event::Output {
                pane_id: String::from("%12"),
                payload: String::from("hello\\040world"),
            }
        );
    }

    #[test]
    fn parses_window_rename() {
        let event = parse_line("%window-renamed @9 agents");
        assert_eq!(
            event,
            Event::WindowRenamed {
                window_id: String::from("@9"),
                name: String::from("agents"),
            }
        );
    }

    #[test]
    fn parses_block_line() {
        let event = parse_line("%begin 1578920019 258 0");
        assert_eq!(
            event,
            Event::Begin {
                timestamp: 1578920019,
                command_number: 258,
                flags: 0,
            }
        );
    }

    #[test]
    fn parses_all_structural_notifications() {
        assert_eq!(
            parse_line("%end 1578920020 258 1"),
            Event::End {
                timestamp: 1578920020,
                command_number: 258,
                flags: 1,
            }
        );
        assert_eq!(
            parse_line("%error 1578920021 259 2"),
            Event::Error {
                timestamp: 1578920021,
                command_number: 259,
                flags: 2,
            }
        );
        assert_eq!(
            parse_line("%pane-mode-changed %3"),
            Event::PaneModeChanged {
                pane_id: String::from("%3"),
            }
        );
        assert_eq!(
            parse_line("%window-pane-changed @1 %3"),
            Event::WindowPaneChanged {
                window_id: String::from("@1"),
                pane_id: String::from("%3"),
            }
        );
        assert_eq!(
            parse_line("%window-close @2"),
            Event::WindowClose {
                window_id: String::from("@2"),
            }
        );
        assert_eq!(
            parse_line("%window-add @4"),
            Event::WindowAdd {
                window_id: String::from("@4"),
            }
        );
        assert_eq!(
            parse_line("%session-changed $1 ops"),
            Event::SessionChanged {
                session_id: String::from("$1"),
                name: String::from("ops"),
            }
        );
        assert_eq!(
            parse_line("%client-session-changed /dev/ttys001 $1 ops"),
            Event::ClientSessionChanged {
                client: String::from("/dev/ttys001"),
                session_id: String::from("$1"),
                name: String::from("ops"),
            }
        );
        assert_eq!(
            parse_line("%session-renamed $1 review"),
            Event::SessionRenamed {
                session_id: String::from("$1"),
                name: String::from("review"),
            }
        );
        assert_eq!(parse_line("%sessions-changed"), Event::SessionsChanged);
        assert_eq!(
            parse_line("%session-window-changed $1 @9"),
            Event::SessionWindowChanged {
                session_id: String::from("$1"),
                window_id: String::from("@9"),
            }
        );
    }

    #[test]
    fn parses_extended_output_and_exit_notifications() {
        assert_eq!(
            parse_line("%extended-output %7 42 : hello\\040world"),
            Event::ExtendedOutput {
                pane_id: String::from("%7"),
                age_millis: 42,
                payload: String::from("hello\\040world"),
            }
        );
        assert_eq!(
            parse_line("%extended-output %7 nope not-a-delimiter ignored"),
            Event::ExtendedOutput {
                pane_id: String::from("%7"),
                age_millis: 0,
                payload: String::new(),
            }
        );
        assert_eq!(parse_line("%exit"), Event::Exit { reason: None });
        assert_eq!(
            parse_line("%exit server exited"),
            Event::Exit {
                reason: Some(String::from("server exited")),
            }
        );
        assert_eq!(
            parse_line("%unknown thing"),
            Event::Unknown {
                raw: String::from("%unknown thing"),
            }
        );
    }

    #[test]
    fn malformed_control_lines_stay_loggable_without_panicking() {
        assert_eq!(
            parse_line("%begin not-a-time 258 0"),
            Event::Unknown {
                raw: String::from("%begin not-a-time 258 0"),
            }
        );
        assert_eq!(
            parse_line("%end 1578920020 missing-flags"),
            Event::Unknown {
                raw: String::from("%end 1578920020 missing-flags"),
            }
        );

        let unknown = parse_line(
            "%unknown this payload is deliberately long enough to require a visible suffix after truncation",
        );
        assert!(unknown.is_loggable());
        assert_eq!(unknown.output_summary(), None);
        assert_eq!(unknown.output_text(), None);
        assert_eq!(unknown.output_chunk(), None);
        assert_eq!(unknown.output_age_millis(), None);
        assert_eq!(
            unknown.summary(),
            "notification: %unknown this payload is deliberately long enough to req..."
        );
    }

    #[test]
    fn event_helpers_expose_output_and_loggability() {
        let output = parse_line("%output %12 hello\\040world");
        assert_eq!(
            output.output_summary(),
            Some(("%12", String::from("hello world")))
        );
        assert_eq!(
            output.output_text(),
            Some(("%12", String::from("hello world")))
        );
        assert_eq!(
            output.output_chunk(),
            Some(("%12", String::from("hello world")))
        );
        assert_eq!(output.output_age_millis(), None);
        assert!(!output.is_loggable());
        assert!(!output.is_structural());

        let extended = parse_line("%extended-output %12 99 : hello\\015\\012world");
        assert_eq!(
            extended.output_summary(),
            Some(("%12", String::from("hello world")))
        );
        assert_eq!(extended.output_age_millis(), Some(99));
        assert_eq!(
            extended.output_text(),
            Some(("%12", String::from("hello world")))
        );
        assert_eq!(
            extended.output_chunk(),
            Some(("%12", String::from("hello\r\nworld")))
        );

        let structural = parse_line("%window-add @4");
        assert!(structural.is_structural());
        assert!(structural.is_loggable());
        assert_eq!(structural.output_summary(), None);
    }

    #[test]
    fn event_summaries_cover_all_visible_variants() {
        let cases = [
            (parse_line("%begin 1 2 3"), "command 2 started"),
            (parse_line("%end 1 2 3"), "command 2 finished"),
            (parse_line("%error 1 2 3"), "command 2 failed"),
            (parse_line("%output %1 hello"), "output from %1: hello"),
            (
                parse_line("%extended-output %1 42 : hello"),
                "output from %1 (42 ms buffered): hello",
            ),
            (parse_line("%pane-mode-changed %1"), "pane mode changed: %1"),
            (
                parse_line("%window-pane-changed @1 %1"),
                "active pane changed: @1 -> %1",
            ),
            (parse_line("%window-close @1"), "window closed: @1"),
            (parse_line("%window-add @1"), "window added: @1"),
            (
                parse_line("%window-renamed @1 agents"),
                "window renamed: @1 -> agents",
            ),
            (
                parse_line("%session-changed $1 ops"),
                "session changed: $1 -> ops",
            ),
            (
                parse_line("%client-session-changed client $1 ops"),
                "client client switched to $1 (ops)",
            ),
            (
                parse_line("%session-renamed $1 ops"),
                "session renamed: $1 -> ops",
            ),
            (parse_line("%sessions-changed"), "sessions changed"),
            (
                parse_line("%session-window-changed $1 @1"),
                "session $1 current window -> @1",
            ),
            (parse_line("%exit"), "control client exited"),
            (
                parse_line("%exit status 1"),
                "control client exited: status 1",
            ),
        ];

        for (event, expected) in cases {
            assert_eq!(event.summary(), expected);
        }
    }

    #[test]
    fn summarizes_tmux_escaped_payload() {
        let summary = summarize_payload("\\033]0;hello\\007plain\\040text\\015\\012");
        assert_eq!(summary, "plain text");
    }

    #[test]
    fn summarizes_utf8_octal_payload_without_mojibake() {
        let summary =
            summarize_payload("\\342\\200\\242 Reply in exactly one line as: STATUS=running");
        assert_eq!(summary, "• Reply in exactly one line as: STATUS=running");
    }

    #[test]
    fn output_chunk_preserves_line_boundaries() {
        let chunk = normalize_output_chunk("hello\\015\\012world\\015tail\\010!");
        assert_eq!(chunk, "hello\r\nworld\rtail\u{8}!");
    }

    #[test]
    fn malformed_octal_and_control_sequences_are_safely_normalized() {
        assert_eq!(
            normalize_payload("hello\\999\tworld\u{1b}[31m!"),
            "hello\\999 world!"
        );
        assert_eq!(strip_terminal_sequences("a\u{1b}cb"), "ab");
        assert_eq!(strip_terminal_sequences("a\u{1b}]0;title\u{7}b"), "ab");
        assert_eq!(strip_terminal_sequences("a\u{1b}]0;title\u{1b}\\b"), "ab");
        assert_eq!(
            strip_terminal_sequences("a\u{1b}]0;title\u{1b}x\u{7}b"),
            "ab"
        );
    }

    #[test]
    fn truncate_is_character_safe() {
        let input = "•".repeat(60);
        let output = truncate(&input);

        assert_eq!(output.chars().count(), 59);
        assert!(output.ends_with("..."));
    }

    #[tokio::test]
    async fn monitor_try_recv_and_finished_are_observable_without_tmux() {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let task = tokio::spawn(async {});
        let mut monitor = Monitor { rx, task };

        assert_eq!(monitor.try_recv(), None);
        tx.send(Event::SessionsChanged)
            .await
            .expect("test channel should be open");
        assert_eq!(monitor.try_recv(), Some(Event::SessionsChanged));

        tokio::task::yield_now().await;
        assert!(monitor.is_finished());
    }

    #[tokio::test]
    async fn control_monitor_finishes_when_receiver_is_dropped() {
        let script = script_path(
            "drop-receiver",
            r#"#!/bin/sh
if [ "$1" = "-C" ]; then
  printf '%%output %%1 first\n'
  printf 'stderr warning\n' >&2
  exit 0
fi

printf 'unexpected args: %s\n' "$*" >&2
exit 64
"#,
        );
        let target = Target {
            binary: script.display().to_string(),
            socket: None,
            session: Some(String::from("demo")),
        };

        let monitor = start(&target)
            .await
            .expect("fake control client should start");
        let Monitor { rx, task } = monitor;
        drop(rx);

        tokio::time::timeout(Duration::from_secs(30), task)
            .await
            .expect("control reader should finish after receiver drop")
            .expect("control reader task should not panic");
    }

    #[test]
    fn wait_errors_report_control_client_exit_reason() {
        let event = exit_event_from_wait(Err(std::io::Error::other("wait failed")));

        assert_eq!(
            event,
            Event::Exit {
                reason: Some(String::from("wait failed"))
            }
        );
    }

    #[tokio::test]
    async fn control_start_streams_stdout_stderr_and_exit_without_real_tmux() {
        let script = script_path(
            "stream",
            r#"#!/bin/sh
if [ "$1" = "-L" ]; then
  shift 2
fi

if [ "$1" = "-C" ]; then
  printf '%%output %%1 hello\040world\n'
  printf 'stderr warning\n' >&2
  exit 7
fi

printf 'unexpected args: %s\n' "$*" >&2
exit 64
"#,
        );
        let target = Target {
            binary: script.display().to_string(),
            socket: Some(String::from("agents")),
            session: Some(String::from("demo")),
        };
        let mut monitor = start(&target)
            .await
            .expect("fake control client should start");

        let events = tokio::time::timeout(Duration::from_secs(30), async {
            let mut events = Vec::new();
            while let Some(event) = monitor.recv_for_test().await {
                let exited = matches!(event, Event::Exit { .. });
                events.push(event);
                if exited {
                    tokio::task::yield_now().await;
                    assert!(monitor.is_finished());
                    return events;
                }
            }
            events
        })
        .await
        .expect("control monitor timed out before process exit");

        assert!(
            events.iter().any(|event| {
                matches!(
                    event,
                    Event::Output { pane_id, payload }
                        if pane_id == "%1" && payload == "hello world"
                )
            }),
            "{events:?}"
        );
        assert!(
            events.iter().any(|event| {
                matches!(event, Event::Unknown { raw } if raw == "stderr: stderr warning")
            }),
            "{events:?}"
        );
        assert_eq!(
            events.last(),
            Some(&Event::Exit {
                reason: Some(String::from("status exit status: 7"))
            }),
            "control events did not include process exit: {events:?}"
        );
    }

    #[tokio::test]
    async fn control_start_reports_malformed_streams_without_hanging() {
        let script = script_path(
            "invalid-streams",
            r#"#!/bin/sh
if [ "$1" = "-L" ]; then
  shift 2
fi

if [ "$1" = "-C" ]; then
  printf '\377\n'
  printf '\377\n' >&2
  exit 0
fi

printf 'unexpected args: %s\n' "$*" >&2
exit 64
"#,
        );
        let target = Target {
            binary: script.display().to_string(),
            socket: None,
            session: Some(String::from("demo")),
        };
        let mut monitor = start(&target)
            .await
            .expect("fake control client should start");

        let events = tokio::time::timeout(Duration::from_secs(30), async {
            let mut events = Vec::new();
            while let Some(event) = monitor.recv_for_test().await {
                let exited = matches!(event, Event::Exit { .. });
                events.push(event);
                if exited {
                    return events;
                }
            }
            events
        })
        .await
        .expect("control monitor timed out after invalid stream data");

        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::Unknown { raw } if raw.starts_with("stdout read error:"))),
            "{events:?}"
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::Unknown { raw } if raw.starts_with("stderr read error:"))),
            "{events:?}"
        );
        assert_eq!(
            events.last(),
            Some(&Event::Exit { reason: None }),
            "control monitor should still report process exit: {events:?}"
        );
    }
}
