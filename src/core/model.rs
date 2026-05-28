use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone)]
pub struct PaneRuntime {
    pub output: VecDeque<String>,
    pub last_output_at: Option<Instant>,
    pub corpus: String,
    pub partial_line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneInsight {
    pub workload: WorkloadKind,
    pub status: PaneStatus,
    pub last_output_age: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct AgentReport {
    pub status: String,
    pub blocker: String,
    pub next: String,
    pub updated_at: Instant,
}

#[derive(Debug, Clone, Copy)]
pub struct ObservedPane<'a> {
    pub current_command: &'a str,
    pub title: &'a str,
    pub window_name: &'a str,
    pub current_path: &'a str,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkloadKind {
    Codex,
    ClaudeCode,
    Opencode,
    Aider,
    Gemini,
    Agent,
    Shell,
    Job,
}

impl WorkloadKind {
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude",
            Self::Opencode => "opencode",
            Self::Aider => "aider",
            Self::Gemini => "gemini",
            Self::Agent => "agent",
            Self::Shell => "shell",
            Self::Job => "job",
        }
    }

    pub fn display_label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code",
            Self::Opencode => "Opencode",
            Self::Aider => "Aider",
            Self::Gemini => "Gemini",
            Self::Agent => "Agent",
            Self::Shell => "Shell",
            Self::Job => "Job",
        }
    }

    pub fn is_agent(self) -> bool {
        matches!(
            self,
            Self::Codex
                | Self::ClaudeCode
                | Self::Opencode
                | Self::Aider
                | Self::Gemini
                | Self::Agent
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneStatus {
    Running,
    Waiting,
    Done,
    Error,
    Stuck,
    Idle,
    Unknown,
}

impl PaneStatus {
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Running => "run ",
            Self::Waiting => "wait",
            Self::Done => "done",
            Self::Error => "err ",
            Self::Stuck => "stck",
            Self::Idle => "idle",
            Self::Unknown => "chk ",
        }
    }

    pub fn display_label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Waiting => "Waiting",
            Self::Done => "Done",
            Self::Error => "Error",
            Self::Stuck => "Stuck",
            Self::Idle => "Idle",
            Self::Unknown => "Checking",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PaneStatus, WorkloadKind};

    #[test]
    fn workload_labels_cover_every_supported_agent_family() {
        let labels = [
            (WorkloadKind::Codex, "codex", "Codex", true),
            (WorkloadKind::ClaudeCode, "claude", "Claude Code", true),
            (WorkloadKind::Opencode, "opencode", "Opencode", true),
            (WorkloadKind::Aider, "aider", "Aider", true),
            (WorkloadKind::Gemini, "gemini", "Gemini", true),
            (WorkloadKind::Agent, "agent", "Agent", true),
            (WorkloadKind::Shell, "shell", "Shell", false),
            (WorkloadKind::Job, "job", "Job", false),
        ];

        for (kind, short, display, is_agent) in labels {
            assert_eq!(kind.short_label(), short);
            assert_eq!(kind.display_label(), display);
            assert_eq!(kind.is_agent(), is_agent);
        }
    }

    #[test]
    fn pane_status_labels_are_plain_and_never_render_unknown_jargon() {
        let labels = [
            (PaneStatus::Running, "run ", "Running"),
            (PaneStatus::Waiting, "wait", "Waiting"),
            (PaneStatus::Done, "done", "Done"),
            (PaneStatus::Error, "err ", "Error"),
            (PaneStatus::Stuck, "stck", "Stuck"),
            (PaneStatus::Idle, "idle", "Idle"),
            (PaneStatus::Unknown, "chk ", "Checking"),
        ];

        for (status, short, display) in labels {
            assert_eq!(status.short_label(), short);
            assert_eq!(status.display_label(), display);
            assert_ne!(short.trim(), "unk");
            assert_ne!(display, "Unknown");
        }
    }
}
