mod agent_sources;
mod inference;
mod model;
mod progress;
mod providers;
mod reports;
mod runtime;

pub use agent_sources::{
    AgentSourceEvent, AgentSourceProvider, AgentSourceRoots, AgentSourceScanner,
    agent_source_matches_path, pane_text_has_provider_hint,
};
pub use inference::{infer_pane_insight, pane_heat_score};
pub use model::{AgentReport, ObservedPane, PaneInsight, PaneRuntime, PaneStatus, WorkloadKind};
pub(crate) use progress::classify_fallback_summary;
pub use providers::{matches_choice_hint, matches_enter_hint, matches_waiting_hint};
pub(crate) use reports::parse_agent_report_lines;
pub use reports::{
    activity_summary, agent_report_summary, effective_agent_report, is_agent_report_protocol_line,
    parse_agent_report_line,
};
pub use runtime::{
    build_pane_corpus, build_runtime_corpus, collect_runtime_live_lines,
    collect_runtime_recent_lines, is_meaningful_live_fragment, pane_corpus, visible_partial_line,
};
pub(crate) use runtime::{is_shell_prompt_noise, is_terminal_chatter_noise};

#[cfg(test)]
pub(crate) use inference::infer_workload_kind;
pub(crate) use reports::infer_status_from_report;
#[cfg(test)]
pub(crate) use runtime::append_output_chunk;

#[cfg(test)]
mod tests;
