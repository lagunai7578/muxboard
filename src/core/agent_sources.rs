use std::{
    cmp::Reverse,
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

use super::{PaneStatus, WorkloadKind};

const DEFAULT_MAX_FILES_PER_SOURCE: usize = 24;
const MAX_SCAN_CANDIDATES: usize = 512;
const MAX_SCAN_DEPTH: usize = 8;
const MAX_SOURCE_FILE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentSourceProvider {
    Codex,
    ClaudeCode,
}

impl AgentSourceProvider {
    pub fn workload(self) -> WorkloadKind {
        match self {
            Self::Codex => WorkloadKind::Codex,
            Self::ClaudeCode => WorkloadKind::ClaudeCode,
        }
    }

    pub fn agent_key(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude",
        }
    }

    pub fn display_label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code",
        }
    }

    fn hint_markers(self) -> &'static [&'static str] {
        match self {
            Self::Codex => &["codex"],
            Self::ClaudeCode => &["claude", "claudecode"],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSourceEvent {
    pub provider: AgentSourceProvider,
    pub cwd: Option<PathBuf>,
    pub encoded_cwd: Option<String>,
    pub status: PaneStatus,
    pub thread_id: Option<String>,
    pub thread_name: Option<String>,
    pub summary: String,
    pub progress: Option<String>,
    pub log: Option<String>,
    pub updated_at_unix_ms: u64,
}

impl AgentSourceEvent {
    pub fn identity_key(&self) -> String {
        if let Some(thread_id) = &self.thread_id {
            return format!("{}:{thread_id}", self.provider.agent_key());
        }

        if let Some(cwd) = &self.cwd {
            return format!(
                "{}:{}",
                self.provider.agent_key(),
                normalize_path_string(cwd)
            );
        }

        if let Some(encoded) = &self.encoded_cwd {
            return format!("{}:{encoded}", self.provider.agent_key());
        }

        self.provider.agent_key().to_owned()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentSourceRoots {
    pub codex_sessions_dir: Option<PathBuf>,
    pub codex_index_path: Option<PathBuf>,
    pub claude_projects_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSourceScanner {
    roots: AgentSourceRoots,
    max_files_per_source: usize,
}

impl Default for AgentSourceScanner {
    fn default() -> Self {
        Self {
            roots: AgentSourceRoots::default(),
            max_files_per_source: DEFAULT_MAX_FILES_PER_SOURCE,
        }
    }
}

impl AgentSourceScanner {
    pub fn from_env() -> Self {
        Self {
            roots: AgentSourceRoots::from_env(),
            max_files_per_source: DEFAULT_MAX_FILES_PER_SOURCE,
        }
    }

    pub fn disabled() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub(crate) fn with_roots_for_test(roots: AgentSourceRoots) -> Self {
        Self {
            roots,
            max_files_per_source: DEFAULT_MAX_FILES_PER_SOURCE,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.roots.codex_sessions_dir.is_some() || self.roots.claude_projects_dir.is_some()
    }

    pub fn scan(&self) -> Vec<AgentSourceEvent> {
        if !self.is_enabled() {
            return Vec::new();
        }

        let mut events = Vec::new();

        if let Some(root) = &self.roots.codex_sessions_dir {
            let titles = self
                .roots
                .codex_index_path
                .as_deref()
                .map(load_codex_thread_titles)
                .unwrap_or_default();
            for path in recent_jsonl_files(root, self.max_files_per_source) {
                if let Some(event) = parse_codex_event_file(&path, &titles) {
                    events.push(event);
                }
            }
        }

        if let Some(root) = &self.roots.claude_projects_dir {
            for path in recent_jsonl_files(root, self.max_files_per_source) {
                if let Some(event) = parse_claude_event_file(root, &path) {
                    events.push(event);
                }
            }
        }

        events.sort_by_key(|event| Reverse(event.updated_at_unix_ms));
        events
    }
}

impl AgentSourceRoots {
    pub fn from_env() -> Self {
        let home = home_dir();
        let codex_home = env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|home| home.join(".codex")));
        let codex_sessions_dir = codex_home
            .as_ref()
            .map(|home| home.join("sessions"))
            .filter(|path| path.is_dir());
        let codex_index_path = codex_home
            .as_ref()
            .map(|home| home.join("session_index.jsonl"))
            .filter(|path| path.is_file());
        let claude_projects_dir = home
            .as_ref()
            .map(|home| home.join(".claude").join("projects"))
            .filter(|path| path.is_dir());

        Self {
            codex_sessions_dir,
            codex_index_path,
            claude_projects_dir,
        }
    }
}

pub fn agent_source_matches_path(event: &AgentSourceEvent, pane_path: &str) -> bool {
    if pane_path.trim().is_empty() {
        return false;
    }

    if let Some(cwd) = &event.cwd
        && path_contains_pane(cwd, Path::new(pane_path))
    {
        return true;
    }

    if let Some(encoded) = &event.encoded_cwd {
        return encode_claude_project_path(pane_path) == *encoded;
    }

    false
}

pub fn pane_text_has_provider_hint(
    provider: AgentSourceProvider,
    command: &str,
    title: &str,
    window_name: &str,
) -> bool {
    let haystack = format!("{command} {title} {window_name}").to_ascii_lowercase();
    provider
        .hint_markers()
        .iter()
        .any(|marker| contains_provider_marker(&haystack, marker))
}

fn contains_provider_marker(haystack: &str, marker: &str) -> bool {
    haystack.match_indices(marker).any(|(index, _)| {
        let before = haystack[..index].chars().next_back();
        let after = haystack[index + marker.len()..].chars().next();
        !before.is_some_and(|ch| ch.is_ascii_alphanumeric())
            && !after.is_some_and(|ch| ch.is_ascii_alphanumeric())
    })
}

fn parse_codex_event_file(
    path: &Path,
    titles: &std::collections::HashMap<String, String>,
) -> Option<AgentSourceEvent> {
    let values = read_jsonl_values(path)?;
    let mut cwd = None;
    let mut thread_id = None;
    let mut status = None;
    let mut log = None;

    for value in values {
        if let Some(meta) = value
            .get("payload")
            .filter(|_| value_type(&value) == Some("session_meta"))
        {
            if thread_id.is_none() {
                thread_id = string_field(meta, "id");
            }
            if cwd.is_none() {
                cwd = string_field(meta, "cwd").map(PathBuf::from);
            }
            continue;
        }

        if let Some(payload) = value
            .get("payload")
            .filter(|_| value_type(&value) == Some("turn_context"))
        {
            if cwd.is_none() {
                cwd = string_field(payload, "cwd").map(PathBuf::from);
            }
            continue;
        }

        if let Some(next_status) = codex_status_from_entry(&value) {
            status = Some(next_status);
            log = Some(status_log(AgentSourceProvider::Codex, next_status).to_owned());
        }
    }

    let status = status?;
    let thread_name = thread_id.as_ref().and_then(|id| titles.get(id).cloned());
    let summary = event_summary(status);

    Some(AgentSourceEvent {
        provider: AgentSourceProvider::Codex,
        cwd,
        encoded_cwd: None,
        status,
        thread_id: thread_id.or_else(|| codex_thread_id_from_path(path)),
        thread_name,
        summary,
        progress: None,
        log,
        updated_at_unix_ms: modified_unix_ms(path),
    })
}

fn parse_claude_event_file(projects_root: &Path, path: &Path) -> Option<AgentSourceEvent> {
    let values = read_jsonl_values(path)?;
    let encoded_cwd = path
        .parent()
        .and_then(|parent| parent.strip_prefix(projects_root).ok())
        .and_then(|relative| relative.components().next())
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .filter(|value| !value.is_empty());
    let cwd = encoded_cwd
        .as_deref()
        .and_then(decode_claude_project_path_if_real);
    let thread_id = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .filter(|value| !value.is_empty());
    let mut thread_name = None;
    let mut generated_title = None;
    let mut progress = None;
    let mut status = None;
    let mut log = None;

    for value in values {
        if thread_name.is_none() {
            thread_name = claude_custom_title(&value);
        }
        if generated_title.is_none() {
            generated_title = claude_ai_title(&value);
        }
        if let Some(summary) = claude_task_summary(&value) {
            progress = Some(summary);
            status = Some(PaneStatus::Running);
            log = Some(status_log(AgentSourceProvider::ClaudeCode, PaneStatus::Running).to_owned());
        }

        if let Some(next_status) = claude_status_from_entry(&value) {
            status = Some(next_status);
            log = Some(status_log(AgentSourceProvider::ClaudeCode, next_status).to_owned());
        }
    }

    let status = status?;
    let thread_name = thread_name.or(generated_title);
    let summary = event_summary(status);

    Some(AgentSourceEvent {
        provider: AgentSourceProvider::ClaudeCode,
        cwd,
        encoded_cwd,
        status,
        thread_id,
        thread_name,
        summary,
        progress,
        log,
        updated_at_unix_ms: modified_unix_ms(path),
    })
}

fn codex_status_from_entry(value: &Value) -> Option<PaneStatus> {
    match value_type(value)? {
        "event_msg" => {
            let payload_type = value
                .get("payload")
                .and_then(|payload| string_field(payload, "type"))?;
            match payload_type.as_str() {
                "task_started" | "turn_started" | "user_message" => Some(PaneStatus::Running),
                "task_complete" | "turn_complete" => Some(PaneStatus::Done),
                "turn_aborted" => Some(PaneStatus::Stuck),
                "error" | "stream_error" => Some(PaneStatus::Error),
                "exec_approval_request"
                | "request_permissions"
                | "request_user_input"
                | "elicitation_request"
                | "apply_patch_approval_request" => Some(PaneStatus::Waiting),
                "raw_response_item" => value
                    .get("payload")
                    .and_then(|payload| payload.get("item"))
                    .and_then(codex_status_from_response_item)
                    .or(Some(PaneStatus::Running)),
                "agent_reasoning"
                | "agent_reasoning_raw_content"
                | "agent_reasoning_section_break"
                | "exec_command_begin"
                | "exec_command_output_delta"
                | "exec_command_end"
                | "mcp_tool_call_begin"
                | "mcp_tool_call_end"
                | "web_search_begin"
                | "web_search_end"
                | "patch_apply_begin"
                | "patch_apply_updated"
                | "patch_apply_end"
                | "item_started"
                | "item_completed" => Some(PaneStatus::Running),
                "agent_message" => {
                    let phase = value
                        .get("payload")
                        .and_then(|payload| string_field(payload, "phase"));
                    if phase.as_deref() == Some("final_answer") {
                        Some(PaneStatus::Done)
                    } else {
                        Some(PaneStatus::Running)
                    }
                }
                _ => None,
            }
        }
        "response_item" => {
            let payload = value.get("payload")?;
            codex_status_from_response_item(payload)
        }
        "message" if string_field(value, "role").as_deref() == Some("user") => {
            Some(PaneStatus::Running)
        }
        "message" if string_field(value, "role").as_deref() == Some("assistant") => {
            Some(PaneStatus::Running)
        }
        "function_call" | "function_call_output" | "reasoning" => Some(PaneStatus::Running),
        _ => None,
    }
}

fn codex_status_from_response_item(payload: &Value) -> Option<PaneStatus> {
    match string_field(payload, "type")?.as_str() {
        "function_call"
        | "function_call_output"
        | "reasoning"
        | "custom_tool_call"
        | "custom_tool_call_output"
        | "web_search_call" => Some(PaneStatus::Running),
        "message" if string_field(payload, "role").as_deref() == Some("user") => {
            Some(PaneStatus::Running)
        }
        "message" if string_field(payload, "role").as_deref() == Some("assistant") => {
            if string_field(payload, "phase").as_deref() == Some("final_answer") {
                Some(PaneStatus::Done)
            } else {
                Some(PaneStatus::Running)
            }
        }
        _ => None,
    }
}

fn claude_status_from_entry(value: &Value) -> Option<PaneStatus> {
    if let Some(message) = value.get("message") {
        if string_field(message, "role").as_deref() == Some("assistant") {
            if claude_message_has_tool_use(message) {
                return Some(PaneStatus::Running);
            }

            if claude_message_has_content_type(message, "thinking") {
                return Some(PaneStatus::Running);
            }

            if string_field(message, "stop_reason").is_none() {
                return Some(PaneStatus::Running);
            }

            if string_field(message, "stop_reason").as_deref() == Some("end_turn") {
                return Some(PaneStatus::Done);
            }

            if string_field(message, "stop_reason").as_deref() == Some("tool_use") {
                return Some(PaneStatus::Running);
            }

            return Some(PaneStatus::Done);
        }

        if string_field(message, "role").as_deref() == Some("user") {
            if let Some(text) = claude_user_text(message) {
                let lower = text.to_ascii_lowercase();
                if lower.starts_with("[request interrupted") {
                    return Some(PaneStatus::Stuck);
                }
                if lower.contains("/exit") {
                    return Some(PaneStatus::Done);
                }
                if lower.starts_with('<') || lower.starts_with('{') {
                    return None;
                }
            }

            if claude_message_has_content_type(message, "tool_result") {
                return Some(PaneStatus::Running);
            }

            return Some(PaneStatus::Running);
        }
    }

    None
}

fn claude_message_has_tool_use(message: &Value) -> bool {
    claude_message_has_content_type(message, "tool_use")
}

fn claude_message_has_content_type(message: &Value, content_type: &str) -> bool {
    message
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| string_field(item, "type").as_deref() == Some(content_type))
        })
}

fn claude_user_text(message: &Value) -> Option<String> {
    let content = message.get("content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_owned());
    }

    content.as_array().and_then(|items| {
        items.iter().find_map(|item| {
            if string_field(item, "type").as_deref() == Some("text") {
                string_field(item, "text")
            } else {
                None
            }
        })
    })
}

fn claude_custom_title(value: &Value) -> Option<String> {
    if value_type(value) == Some("custom-title") {
        string_field(value, "customTitle")
    } else {
        None
    }
}

fn claude_ai_title(value: &Value) -> Option<String> {
    if value_type(value) == Some("ai-title") {
        string_field(value, "aiTitle")
    } else {
        None
    }
}

fn claude_task_summary(value: &Value) -> Option<String> {
    if value_type(value) == Some("task-summary") {
        string_field(value, "summary")
    } else {
        None
    }
}

fn event_summary(status: PaneStatus) -> String {
    match status {
        PaneStatus::Running => "working",
        PaneStatus::Waiting => "needs input",
        PaneStatus::Done => "complete",
        PaneStatus::Error => "error",
        PaneStatus::Stuck => "interrupted",
        PaneStatus::Idle => "idle",
        PaneStatus::Unknown => "checking",
    }
    .to_owned()
}

fn status_log(provider: AgentSourceProvider, status: PaneStatus) -> &'static str {
    match (provider, status) {
        (_, PaneStatus::Running) => "native transcript is active",
        (_, PaneStatus::Done) => "native transcript completed",
        (_, PaneStatus::Stuck) => "native transcript interrupted",
        (_, PaneStatus::Error) => "native transcript errored",
        (_, PaneStatus::Waiting) => "native transcript needs input",
        _ => "native transcript updated",
    }
}

fn value_type(value: &Value) -> Option<&str> {
    value.get("type").and_then(Value::as_str)
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn read_jsonl_values(path: &Path) -> Option<Vec<Value>> {
    if fs::metadata(path).ok()?.len() > MAX_SOURCE_FILE_BYTES {
        return None;
    }

    let content = fs::read_to_string(path).ok()?;
    Some(
        content
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .collect(),
    )
}

fn load_codex_thread_titles(path: &Path) -> std::collections::HashMap<String, String> {
    let Some(values) = read_jsonl_values(path) else {
        return std::collections::HashMap::new();
    };

    values
        .into_iter()
        .filter_map(|value| {
            let id = string_field(&value, "id")
                .or_else(|| string_field(&value, "session_id"))
                .or_else(|| string_field(&value, "thread_id"))?;
            let title = string_field(&value, "title")
                .or_else(|| string_field(&value, "name"))
                .or_else(|| string_field(&value, "summary"))?;
            Some((id, title))
        })
        .collect()
}

fn codex_thread_id_from_path(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_string_lossy();
    codex_uuid_suffix(&stem)
        .or_else(|| Some(stem.to_string()))
        .filter(|value| !value.is_empty())
}

fn codex_uuid_suffix(stem: &str) -> Option<String> {
    let suffix = stem
        .chars()
        .rev()
        .take(36)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let bytes = suffix.as_bytes();
    if bytes.len() != 36 {
        return None;
    }
    let dash_positions = [8, 13, 18, 23];
    let valid = bytes.iter().enumerate().all(|(index, byte)| {
        if dash_positions.contains(&index) {
            *byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        }
    });
    valid.then_some(suffix)
}

fn recent_jsonl_files(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    collect_jsonl_candidates(root, 0, &mut candidates);
    candidates.sort_by_key(|candidate| Reverse(candidate.0));
    candidates
        .into_iter()
        .take(limit)
        .map(|(_, path)| path)
        .collect()
}

fn collect_jsonl_candidates(
    root: &Path,
    depth: usize,
    candidates: &mut Vec<(SystemTime, PathBuf)>,
) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }

    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    let mut entries = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = entry.metadata().ok()?;
            let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
            Some((modified, path, metadata))
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|(modified, _, _)| Reverse(*modified));

    for (modified, path, metadata) in entries {
        if metadata.is_dir() {
            collect_jsonl_candidates(&path, depth + 1, candidates);
        } else if metadata.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            && metadata.len() <= MAX_SOURCE_FILE_BYTES
        {
            push_jsonl_candidate(candidates, modified, path);
        }
    }
}

fn push_jsonl_candidate(
    candidates: &mut Vec<(SystemTime, PathBuf)>,
    modified: SystemTime,
    path: PathBuf,
) {
    candidates.push((modified, path));
    if candidates.len() > MAX_SCAN_CANDIDATES {
        candidates.sort_by_key(|candidate| Reverse(candidate.0));
        candidates.truncate(MAX_SCAN_CANDIDATES);
    }
}

fn modified_unix_ms(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn decode_claude_project_path_if_real(encoded: &str) -> Option<PathBuf> {
    let naive = PathBuf::from(encoded.replace('-', "/"));
    naive.is_dir().then_some(naive)
}

fn encode_claude_project_path(path: &str) -> String {
    path.chars()
        .map(|ch| {
            if matches!(ch, '/' | '.' | '_') {
                '-'
            } else {
                ch
            }
        })
        .collect()
}

fn path_contains_pane(source_cwd: &Path, pane_path: &Path) -> bool {
    let source = normalize_path_string(source_cwd);
    let pane = normalize_path_string(pane_path);
    pane == source || pane.starts_with(&format!("{source}/"))
}

fn normalize_path_string(path: &Path) -> String {
    let text = path.to_string_lossy();
    let trimmed = text.trim_end_matches('/');
    if trimmed.is_empty() {
        String::from("/")
    } else {
        trimmed.to_owned()
    }
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        fs::{self, File},
        io::Write,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let dir = env::temp_dir().join(format!("muxboard-agent-source-{label}-{unique}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn write_jsonl(path: &Path, lines: &[&str]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("jsonl parent should be created");
        }
        let mut file = File::create(path).expect("jsonl file should be created");
        for line in lines {
            writeln!(file, "{line}").expect("jsonl line should be written");
        }
    }

    #[test]
    fn codex_source_parser_extracts_cwd_status_and_title_without_prompt_text() {
        let root = temp_dir("codex");
        let sessions = root.join("sessions").join("2026").join("05").join("24");
        let file = sessions.join("rollout-2026-05-24T00-00-00-thread-123.jsonl");
        write_jsonl(
            &file,
            &[
                r#"{"type":"session_meta","payload":{"id":"thread-123","cwd":"/work/muxboard"}}"#,
                r#"{"type":"event_msg","payload":{"type":"task_started"}}"#,
                r#"{"type":"response_item","payload":{"type":"function_call","name":"exec_command"}}"#,
                r#"{"type":"event_msg","payload":{"type":"task_complete"}}"#,
            ],
        );
        let index = root.join("session_index.jsonl");
        write_jsonl(
            &index,
            &[r#"{"id":"thread-123","title":"Ship agent board"}"#],
        );

        let scanner = AgentSourceScanner::with_roots_for_test(AgentSourceRoots {
            codex_sessions_dir: Some(root.join("sessions")),
            codex_index_path: Some(index),
            claude_projects_dir: None,
        });
        let events = scanner.scan();

        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.provider, AgentSourceProvider::Codex);
        assert_eq!(event.cwd.as_deref(), Some(Path::new("/work/muxboard")));
        assert_eq!(event.status, PaneStatus::Done);
        assert_eq!(event.thread_id.as_deref(), Some("thread-123"));
        assert_eq!(event.thread_name.as_deref(), Some("Ship agent board"));
        assert_eq!(event.summary, "complete");
        assert!(!event.summary.contains("exec_command"));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn codex_source_parser_ignores_vcs_metadata_and_omits_duplicate_provider_label() {
        let root = temp_dir("codex-vcs");
        let sessions = root.join("sessions").join("2026").join("05").join("24");
        let file = sessions.join("rollout-2026-05-24T00-00-00-thread-vcs.jsonl");
        write_jsonl(
            &file,
            &[
                r#"{"type":"session_meta","payload":{"id":"thread-vcs","cwd":"/work/muxboard","git":{"branch":"secret-branch","commit":"abc123"}}}"#,
                r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"ship secret prompt"}]}}"#,
                r#"{"type":"response_item","payload":{"type":"function_call","name":"exec_command"}}"#,
            ],
        );

        let scanner = AgentSourceScanner::with_roots_for_test(AgentSourceRoots {
            codex_sessions_dir: Some(root.join("sessions")),
            codex_index_path: None,
            claude_projects_dir: None,
        });
        let events = scanner.scan();

        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.status, PaneStatus::Running);
        assert_eq!(event.summary, "working");
        for surfaced in [
            event.summary.as_str(),
            event.log.as_deref().unwrap_or_default(),
            event.thread_name.as_deref().unwrap_or_default(),
            event.progress.as_deref().unwrap_or_default(),
        ] {
            assert!(!surfaced.contains("Codex"), "{surfaced}");
            assert!(!surfaced.contains("secret-branch"), "{surfaced}");
            assert!(!surfaced.contains("ship secret prompt"), "{surfaced}");
        }

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn codex_status_parser_tracks_current_codex_event_names_and_attention_requests() {
        for (entry, expected) in [
            (
                json!({"type":"event_msg","payload":{"type":"turn_started"}}),
                PaneStatus::Running,
            ),
            (
                json!({"type":"event_msg","payload":{"type":"turn_complete"}}),
                PaneStatus::Done,
            ),
            (
                json!({"type":"event_msg","payload":{"type":"request_permissions"}}),
                PaneStatus::Waiting,
            ),
            (
                json!({"type":"event_msg","payload":{"type":"exec_approval_request"}}),
                PaneStatus::Waiting,
            ),
            (
                json!({"type":"event_msg","payload":{"type":"error"}}),
                PaneStatus::Error,
            ),
            (
                json!({"type":"event_msg","payload":{"type":"raw_response_item","item":{"type":"function_call","name":"exec_command"}}}),
                PaneStatus::Running,
            ),
        ] {
            assert_eq!(codex_status_from_entry(&entry), Some(expected), "{entry}");
        }
    }

    #[test]
    fn claude_source_parser_matches_encoded_project_path_and_custom_title() {
        let root = temp_dir("claude");
        let pane_path = "/home/tester/Projects/muxboard";
        let encoded = encode_claude_project_path(pane_path);
        let file = root
            .join(".claude")
            .join("projects")
            .join(&encoded)
            .join("session-1.jsonl");
        write_jsonl(
            &file,
            &[
                r#"{"type":"custom-title","customTitle":"Polish command center"}"#,
                r#"{"message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash"}]}}"#,
                r#"{"message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"Done"}]}}"#,
            ],
        );

        let scanner = AgentSourceScanner::with_roots_for_test(AgentSourceRoots {
            codex_sessions_dir: None,
            codex_index_path: None,
            claude_projects_dir: Some(root.join(".claude").join("projects")),
        });
        let events = scanner.scan();

        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.provider, AgentSourceProvider::ClaudeCode);
        assert_eq!(event.encoded_cwd.as_deref(), Some(encoded.as_str()));
        assert_eq!(event.status, PaneStatus::Done);
        assert_eq!(event.thread_name.as_deref(), Some("Polish command center"));
        assert!(agent_source_matches_path(event, pane_path));
        assert!(!agent_source_matches_path(
            event,
            "/home/tester/Projects/other"
        ));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn claude_source_parser_never_surfaces_user_prompt_text() {
        let root = temp_dir("claude-prompt");
        let pane_path = "/home/tester/Projects/muxboard";
        let encoded = encode_claude_project_path(pane_path);
        let file = root
            .join(".claude")
            .join("projects")
            .join(&encoded)
            .join("session-1.jsonl");
        write_jsonl(
            &file,
            &[
                r#"{"message":{"role":"user","content":[{"type":"text","text":"secret launch prompt"}]}}"#,
                r#"{"message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"Done"}]}}"#,
            ],
        );

        let scanner = AgentSourceScanner::with_roots_for_test(AgentSourceRoots {
            codex_sessions_dir: None,
            codex_index_path: None,
            claude_projects_dir: Some(root.join(".claude").join("projects")),
        });
        let events = scanner.scan();

        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.provider, AgentSourceProvider::ClaudeCode);
        assert_eq!(event.status, PaneStatus::Done);
        assert_eq!(event.summary, "complete");
        for surfaced in [
            event.summary.as_str(),
            event.log.as_deref().unwrap_or_default(),
            event.thread_name.as_deref().unwrap_or_default(),
            event.progress.as_deref().unwrap_or_default(),
        ] {
            assert!(!surfaced.contains("secret launch prompt"), "{surfaced}");
        }

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn claude_source_parser_uses_safe_titles_and_task_summary_not_last_prompt_or_git() {
        let root = temp_dir("claude-task-summary");
        let pane_path = "/home/tester/Projects/muxboard";
        let encoded = encode_claude_project_path(pane_path);
        let file = root
            .join(".claude")
            .join("projects")
            .join(&encoded)
            .join("session-1.jsonl");
        write_jsonl(
            &file,
            &[
                r#"{"type":"ai-title","aiTitle":"Improve command center"}"#,
                r#"{"type":"last-prompt","lastPrompt":"secret mobile prompt"}"#,
                r#"{"type":"task-summary","summary":"tightening Fleet and Details hierarchy"}"#,
                r#"{"message":{"role":"assistant","content":[{"type":"thinking","text":"planning"}]},"gitBranch":"secret-branch"}"#,
            ],
        );

        let scanner = AgentSourceScanner::with_roots_for_test(AgentSourceRoots {
            codex_sessions_dir: None,
            codex_index_path: None,
            claude_projects_dir: Some(root.join(".claude").join("projects")),
        });
        let events = scanner.scan();

        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.status, PaneStatus::Running);
        assert_eq!(event.thread_name.as_deref(), Some("Improve command center"));
        assert_eq!(
            event.progress.as_deref(),
            Some("tightening Fleet and Details hierarchy")
        );
        for surfaced in [
            event.summary.as_str(),
            event.log.as_deref().unwrap_or_default(),
            event.thread_name.as_deref().unwrap_or_default(),
            event.progress.as_deref().unwrap_or_default(),
        ] {
            assert!(!surfaced.contains("secret mobile prompt"), "{surfaced}");
            assert!(!surfaced.contains("secret-branch"), "{surfaced}");
        }

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn provider_hints_require_obvious_agent_context_before_mapping() {
        assert!(pane_text_has_provider_hint(
            AgentSourceProvider::Codex,
            "codex",
            "",
            "agents"
        ));
        assert!(pane_text_has_provider_hint(
            AgentSourceProvider::ClaudeCode,
            "node",
            "Claude Code",
            "agents"
        ));
        assert!(pane_text_has_provider_hint(
            AgentSourceProvider::ClaudeCode,
            "node",
            "ClaudeCode",
            "agents"
        ));
        assert!(!pane_text_has_provider_hint(
            AgentSourceProvider::Codex,
            "zsh",
            "",
            "shell"
        ));
        assert!(!pane_text_has_provider_hint(
            AgentSourceProvider::Codex,
            "zsh",
            "codexnotes",
            "shell"
        ));
        assert!(!pane_text_has_provider_hint(
            AgentSourceProvider::ClaudeCode,
            "zsh",
            "notclaude",
            "shell"
        ));
    }

    #[test]
    fn codex_thread_id_fallback_keeps_the_full_uuid_suffix() {
        let path = Path::new(
            "/tmp/rollout-2026-05-24T00-00-00-019e1395-1708-7d63-9ba3-a80f59ab0459.jsonl",
        );

        assert_eq!(
            codex_thread_id_from_path(path).as_deref(),
            Some("019e1395-1708-7d63-9ba3-a80f59ab0459")
        );
    }

    #[test]
    fn recent_jsonl_scan_keeps_new_files_after_candidate_cap() {
        let root = temp_dir("recent-cap");
        let old_dir = root.join("old");
        let new_dir = root.join("new");
        fs::create_dir_all(&old_dir).expect("old dir should be created");
        fs::create_dir_all(&new_dir).expect("new dir should be created");

        for index in 0..(MAX_SCAN_CANDIDATES + 8) {
            write_jsonl(
                &old_dir.join(format!("old-{index:03}.jsonl")),
                &[r#"{"type":"event_msg","payload":{"type":"task_complete"}}"#],
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        let latest = new_dir.join("latest.jsonl");
        write_jsonl(
            &latest,
            &[r#"{"type":"event_msg","payload":{"type":"task_started"}}"#],
        );

        assert_eq!(recent_jsonl_files(&root, 1), vec![latest]);

        fs::remove_dir_all(root).ok();
    }
}
