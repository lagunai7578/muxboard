use clap::{Parser, Subcommand};

#[derive(Debug, Clone, Parser)]
#[command(
    name = "muxboard",
    version,
    about = "A tmux control center for panes, agents, and terminal workloads."
)]
pub struct Cli {
    #[arg(
        long,
        default_value = "tmux",
        help = "tmux binary to invoke",
        global = true
    )]
    pub tmux_bin: String,

    #[arg(
        long,
        help = "tmux socket name, passed through to tmux -L",
        global = true
    )]
    pub socket: Option<String>,

    #[arg(
        long,
        help = "limit the board to one tmux session by name",
        global = true
    )]
    pub session: Option<String>,

    #[arg(long, help = "print the current tmux probe as JSON and exit")]
    pub dump_probe_json: bool,

    #[arg(long, help = "print a ready-to-copy default config file and exit")]
    pub print_config_example: bool,

    #[arg(long, help = "print the default keybindings block and exit")]
    pub print_default_keybindings: bool,

    #[arg(long, help = "save a theme preset to the muxboard config and exit")]
    pub theme: Option<String>,

    #[arg(long, help = "open the theme picker on startup")]
    pub theme_picker: bool,

    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum CliCommand {
    #[command(about = "push an explicit agent state event into tmux")]
    AgentEvent(AgentEventCommand),
    #[command(about = "print a compact tmux status segment")]
    StatusLine(StatusLineCommand),
    #[command(about = "print compact session attention dots")]
    SessionDots(SessionDotsCommand),
}

#[derive(Debug, Clone, Parser)]
pub struct AgentEventCommand {
    #[arg(long, help = "agent name, for example codex, claude, or opencode")]
    pub agent: String,

    #[arg(
        long,
        help = "state: running, waiting, done, error, stuck, idle, or off"
    )]
    pub state: String,

    #[arg(long, default_value = "", help = "short user-facing summary")]
    pub summary: String,

    #[arg(long, help = "stable provider thread/session id, when known")]
    pub thread_id: Option<String>,

    #[arg(long, help = "human-readable task or thread name, when known")]
    pub thread_name: Option<String>,

    #[arg(long, help = "compact progress text, for example 3/10 tests or 75%")]
    pub progress: Option<String>,

    #[arg(long, help = "latest structured log line")]
    pub log: Option<String>,

    #[arg(long, help = "mark a terminal event as not yet reviewed")]
    pub unseen: bool,

    #[arg(long, help = "mark a terminal event as already reviewed")]
    pub seen: bool,

    #[arg(long, help = "tmux pane id; defaults to TMUX_PANE or the active pane")]
    pub pane: Option<String>,
}

#[derive(Debug, Clone, Parser)]
pub struct StatusLineCommand {
    #[arg(long, help = "session to summarize; defaults to all visible sessions")]
    pub current_session: Option<String>,
}

#[derive(Debug, Clone, Parser)]
pub struct SessionDotsCommand {
    #[arg(long, help = "session name to mark as current")]
    pub current_session: Option<String>,
}
