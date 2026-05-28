pub mod app;
pub mod cli;
pub mod config;
pub mod core;
pub mod metrics;
pub mod notifications;
pub mod paths;
pub mod state;
pub mod tmux;
pub mod tui;

use std::{
    env,
    io::{self, Write},
};

use anyhow::Result;

pub async fn run(cli: cli::Cli) -> Result<()> {
    if let Some(command) = cli.command.clone() {
        return run_command(cli, command).await;
    }

    let dump_probe_json = cli.dump_probe_json;
    let print_config_example = cli.print_config_example;
    let print_default_keybindings = cli.print_default_keybindings;
    let theme = cli.theme.clone();
    let target = tmux::Target::from(&cli);

    if print_config_example {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        writeln!(handle, "{}", config::default_config_json()?)?;
        return Ok(());
    }

    if print_default_keybindings {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        writeln!(handle, "{}", config::default_keybindings_json()?)?;
        return Ok(());
    }

    if let Some(theme) = theme {
        let preset = app::ThemePreset::from_config_name(&theme).map_err(anyhow::Error::msg)?;
        let store = config::Store::new()?;
        store.save_theme_preset(preset)?;
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        writeln!(
            handle,
            "muxboard theme set to {} in {}",
            preset.display_label(),
            store.path().display()
        )?;
        return Ok(());
    }

    if dump_probe_json {
        let probe = tmux::probe(target).await?;
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        serde_json::to_writer_pretty(&mut handle, &probe)?;
        writeln!(handle)?;
        return Ok(());
    }

    let mut app = app::App::bootstrap(cli).await?;
    tui::run(&mut app).await
}

async fn run_command(cli: cli::Cli, command: cli::CliCommand) -> Result<()> {
    let target = tmux::Target::from(&cli);
    match command {
        cli::CliCommand::AgentEvent(command) => run_agent_event_command(&target, command).await,
        cli::CliCommand::StatusLine(command) => {
            let snapshot = tmux::snapshot(target).await?;
            let segment = tmux::agent_status_segment(&snapshot, command.current_session.as_deref());
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            writeln!(handle, "{segment}")?;
            Ok(())
        }
        cli::CliCommand::SessionDots(command) => {
            let snapshot = tmux::snapshot(target).await?;
            let dots = tmux::agent_session_dots(&snapshot, command.current_session.as_deref());
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            writeln!(handle, "{dots}")?;
            Ok(())
        }
    }
}

async fn run_agent_event_command(
    target: &tmux::Target,
    command: cli::AgentEventCommand,
) -> Result<()> {
    let pane_id = if let Some(pane_id) = command.pane.or_else(|| env::var("TMUX_PANE").ok()) {
        pane_id
    } else {
        tmux::current_pane(target).await?
    };

    let state = match tmux::normalize_agent_bridge_state(&command.state) {
        Some(state) => state.to_owned(),
        None if command.state.eq_ignore_ascii_case("off")
            || command.state.eq_ignore_ascii_case("clear") =>
        {
            tmux::clear_agent_bridge_event(target, &pane_id).await?;
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            writeln!(handle, "muxboard agent event cleared for {pane_id}")?;
            return Ok(());
        }
        None => anyhow::bail!(
            "unknown agent state `{}`; use running, waiting, done, error, stuck, idle, or off",
            command.state
        ),
    };

    let agent = command.agent.trim();
    if agent.is_empty() {
        anyhow::bail!("agent name cannot be empty");
    }

    if command.unseen && command.seen {
        anyhow::bail!("use either --unseen or --seen, not both");
    }

    let unseen = if command.unseen {
        Some(true)
    } else if command.seen {
        Some(false)
    } else if matches!(state.as_str(), "done" | "error" | "stuck") {
        Some(true)
    } else {
        None
    };

    tmux::set_agent_bridge_event(
        target,
        &pane_id,
        tmux::AgentBridgeEvent {
            agent: agent.to_owned(),
            state: state.clone(),
            summary: command.summary.trim().to_owned(),
            thread_id: trim_optional(command.thread_id),
            thread_name: trim_optional(command.thread_name),
            progress: trim_optional(command.progress),
            log: trim_optional(command.log),
            unseen,
            updated_at_unix_ms: None,
        },
    )
    .await?;

    let stdout = io::stdout();
    let mut handle = stdout.lock();
    writeln!(
        handle,
        "muxboard agent event: {agent} {state} for {pane_id}"
    )?;
    Ok(())
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}
