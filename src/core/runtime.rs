use std::{borrow::Cow, collections::VecDeque};

use super::{ObservedPane, PaneRuntime};

pub fn pane_corpus<'a>(pane: &ObservedPane<'_>, runtime: Option<&'a PaneRuntime>) -> Cow<'a, str> {
    if let Some(runtime) = runtime
        && !runtime.corpus.is_empty()
    {
        return Cow::Borrowed(runtime.corpus.as_str());
    }

    Cow::Owned(build_pane_corpus(pane, &VecDeque::new()))
}

pub fn build_pane_corpus(pane: &ObservedPane<'_>, output: &VecDeque<String>) -> String {
    let mut corpus = format!(
        "{} {} {} {}",
        pane.current_command, pane.title, pane.window_name, pane.current_path
    )
    .to_ascii_lowercase();

    for line in output {
        corpus.push(' ');
        corpus.push_str(&line.to_ascii_lowercase());
    }

    corpus
}

pub fn build_runtime_corpus(pane: &ObservedPane<'_>, runtime: &PaneRuntime) -> String {
    let mut corpus = build_pane_corpus(pane, &runtime.output);
    if let Some(partial) = visible_partial_line(runtime) {
        corpus.push(' ');
        corpus.push_str(&partial.to_ascii_lowercase());
    }
    corpus
}

pub fn collect_runtime_recent_lines(runtime: &PaneRuntime, limit: usize) -> Vec<String> {
    runtime.output.iter().rev().take(limit).cloned().collect()
}

pub fn collect_runtime_live_lines(runtime: &PaneRuntime, limit: usize) -> Vec<String> {
    let mut lines = collect_runtime_recent_lines(runtime, limit);
    if let Some(partial) = visible_partial_line(runtime) {
        if lines.len() == limit {
            lines.pop();
        }
        lines.insert(0, partial.to_owned());
    }
    lines
}

pub fn visible_partial_line(runtime: &PaneRuntime) -> Option<&str> {
    let trimmed = runtime.partial_line.trim();
    if is_meaningful_live_fragment(trimmed) {
        Some(trimmed)
    } else {
        None
    }
}

pub fn is_meaningful_live_fragment(line: &str) -> bool {
    if line.is_empty() {
        return false;
    }

    if is_terminal_chatter_noise(line) {
        return false;
    }

    let char_count = line.chars().count();
    if char_count <= 1 {
        return false;
    }

    !(char_count <= 2
        && !line.chars().any(char::is_whitespace)
        && line.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(ch, '_' | '-' | '.' | '/' | ':' | ';' | ',' | '\'' | '"')
        }))
}

pub(crate) fn is_shell_prompt_noise(line: &str) -> bool {
    let trimmed = line.trim();
    if matches!(trimmed, "❯" | ">" | ">>" | "$" | "%" | "#") {
        return true;
    }

    if !trimmed.ends_with(['$', '%', '#']) {
        return false;
    }

    let normalized = trimmed.to_ascii_lowercase();
    let has_prompt_path = normalized.contains("~/")
        || normalized.contains(":/")
        || normalized.contains(":~")
        || normalized.contains("/users/")
        || normalized.contains("/home/");
    let has_shell_identity = normalized.contains('@') || normalized.contains(':');

    has_prompt_path && has_shell_identity
}

pub(crate) fn is_terminal_chatter_noise(line: &str) -> bool {
    is_shell_prompt_noise(line) || is_shell_startup_banner_noise(line)
}

fn is_shell_startup_banner_noise(line: &str) -> bool {
    let trimmed = line.trim();
    let normalized = trimmed.to_ascii_lowercase();

    trimmed.starts_with("The default interactive shell is now zsh.")
        || trimmed.starts_with("To update your account to use zsh")
        || trimmed.starts_with("For more details, please visit")
        || normalized.contains("chsh -s /bin/zsh")
}

#[cfg(test)]
pub(crate) fn append_output_chunk(runtime: &mut PaneRuntime, chunk: &str) -> Option<String> {
    let mut latest_line = None;

    for ch in chunk.chars() {
        match ch {
            '\r' => runtime.partial_line.clear(),
            '\n' => {
                let line = runtime.partial_line.trim();
                if !line.is_empty() {
                    latest_line = Some(line.to_owned());
                    runtime.output.push_back(line.to_owned());
                }
                runtime.partial_line.clear();
            }
            '\u{8}' | '\u{7f}' => {
                runtime.partial_line.pop();
            }
            _ => runtime.partial_line.push(ch),
        }
    }

    latest_line
}
