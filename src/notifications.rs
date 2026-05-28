use std::{collections::HashMap, process::Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationMode {
    LocalDesktop,
    SshFallback,
    TerminalOnly,
}

impl NotificationMode {
    pub fn display_label(self) -> &'static str {
        match self {
            Self::LocalDesktop => "desktop",
            Self::SshFallback => "ssh-safe",
            Self::TerminalOnly => "terminal",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Notifier {
    mode: NotificationMode,
    backend: DesktopBackend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopBackend {
    None,
    MacOsascript,
    LinuxNotifySend,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DesktopCommand {
    binary: &'static str,
    args: Vec<String>,
}

impl Notifier {
    pub fn from_env() -> Self {
        let env = std::env::vars().collect::<HashMap<_, _>>();
        Self::from_env_map(&env)
    }

    pub(crate) fn from_env_map(env: &HashMap<String, String>) -> Self {
        Self::from_env_map_with(env, command_exists)
    }

    fn from_env_map_with(
        env: &HashMap<String, String>,
        command_exists: impl Fn(&str) -> bool,
    ) -> Self {
        if is_ssh_session(env) {
            return Self {
                mode: NotificationMode::SshFallback,
                backend: DesktopBackend::None,
            };
        }

        let backend = detect_desktop_backend_with(env, command_exists);
        let mode = match backend {
            DesktopBackend::None => NotificationMode::TerminalOnly,
            DesktopBackend::MacOsascript | DesktopBackend::LinuxNotifySend => {
                NotificationMode::LocalDesktop
            }
        };

        Self { mode, backend }
    }

    pub fn mode(&self) -> NotificationMode {
        self.mode
    }

    #[cfg(test)]
    pub(crate) fn with_mode_for_test(mode: NotificationMode) -> Self {
        Self {
            mode,
            backend: DesktopBackend::None,
        }
    }

    pub fn notify_alert(&self, title: &str, body: &str) {
        if let Some(command) = desktop_command(self.backend, title, body) {
            let _ = Command::new(command.binary).args(command.args).spawn();
        }
    }
}

fn is_ssh_session(env: &HashMap<String, String>) -> bool {
    env.contains_key("SSH_CONNECTION")
        || env.contains_key("SSH_CLIENT")
        || env.contains_key("SSH_TTY")
}

fn detect_desktop_backend_with(
    env: &HashMap<String, String>,
    command_exists: impl Fn(&str) -> bool,
) -> DesktopBackend {
    if cfg!(target_os = "macos") && env.contains_key("TERM_PROGRAM") {
        DesktopBackend::MacOsascript
    } else if (env.contains_key("DISPLAY") || env.contains_key("WAYLAND_DISPLAY"))
        && command_exists("notify-send")
    {
        DesktopBackend::LinuxNotifySend
    } else {
        DesktopBackend::None
    }
}

fn desktop_command(backend: DesktopBackend, title: &str, body: &str) -> Option<DesktopCommand> {
    match backend {
        DesktopBackend::None => None,
        DesktopBackend::MacOsascript => {
            let script = format!(
                "display notification \"{}\" with title \"{}\"",
                escape_applescript(body),
                escape_applescript(title)
            );
            Some(DesktopCommand {
                binary: "osascript",
                args: vec![String::from("-e"), script],
            })
        }
        DesktopBackend::LinuxNotifySend => Some(DesktopCommand {
            binary: "notify-send",
            args: vec![title.to_owned(), body.to_owned()],
        }),
    }
}

fn command_exists(binary: &str) -> bool {
    Command::new(binary).arg("--help").output().is_ok()
}

fn escape_applescript(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::{
        DesktopBackend, NotificationMode, Notifier, desktop_command, detect_desktop_backend_with,
        escape_applescript, is_ssh_session,
    };
    use std::collections::HashMap;

    #[test]
    fn ssh_environment_forces_ssh_fallback_mode() {
        let env = HashMap::from([(String::from("SSH_CONNECTION"), String::from("1 2 3 4"))]);
        let notifier = Notifier::from_env_map(&env);

        assert_eq!(notifier.mode(), NotificationMode::SshFallback);
    }

    #[test]
    fn empty_environment_uses_terminal_mode_when_no_desktop_hint_exists() {
        let env = HashMap::new();
        let notifier = Notifier::from_env_map(&env);

        assert_eq!(notifier.mode(), NotificationMode::TerminalOnly);
        assert_eq!(notifier.backend, DesktopBackend::None);
    }

    #[test]
    fn notification_mode_labels_are_stable() {
        assert_eq!(NotificationMode::LocalDesktop.display_label(), "desktop");
        assert_eq!(NotificationMode::SshFallback.display_label(), "ssh-safe");
        assert_eq!(NotificationMode::TerminalOnly.display_label(), "terminal");
    }

    #[test]
    fn any_ssh_marker_counts_as_an_ssh_session() {
        for key in ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"] {
            let env = HashMap::from([(String::from(key), String::from("present"))]);
            assert!(is_ssh_session(&env), "{key}");
        }

        assert!(!is_ssh_session(&HashMap::new()));
    }

    #[test]
    fn desktop_backend_none_is_a_noop_notifier() {
        let notifier = Notifier {
            mode: NotificationMode::TerminalOnly,
            backend: DesktopBackend::None,
        };

        notifier.notify_alert("title", "body");
        assert_eq!(notifier.mode(), NotificationMode::TerminalOnly);
    }

    #[test]
    fn ssh_fallback_wins_over_desktop_environment_hints() {
        let env = HashMap::from([
            (String::from("SSH_TTY"), String::from("/dev/ttys001")),
            (String::from("TERM_PROGRAM"), String::from("Apple_Terminal")),
            (String::from("DISPLAY"), String::from(":0")),
        ]);
        let notifier = Notifier::from_env_map_with(&env, |_| true);

        assert_eq!(notifier.mode(), NotificationMode::SshFallback);
        assert_eq!(notifier.backend, DesktopBackend::None);
    }

    #[test]
    fn local_desktop_mode_requires_a_real_desktop_backend() {
        let x11 = HashMap::from([(String::from("DISPLAY"), String::from(":0"))]);
        let wayland_without_backend =
            HashMap::from([(String::from("WAYLAND_DISPLAY"), String::from("wayland-0"))]);

        let local = Notifier::from_env_map_with(&x11, |binary| binary == "notify-send");
        assert_eq!(local.mode(), NotificationMode::LocalDesktop);
        assert_eq!(local.backend, DesktopBackend::LinuxNotifySend);

        let terminal = Notifier::from_env_map_with(&wayland_without_backend, |_| false);
        assert_eq!(terminal.mode(), NotificationMode::TerminalOnly);
        assert_eq!(terminal.backend, DesktopBackend::None);
    }

    #[test]
    fn desktop_backend_detection_is_deterministic_for_display_hints() {
        let x11 = HashMap::from([(String::from("DISPLAY"), String::from(":0"))]);
        let wayland = HashMap::from([(String::from("WAYLAND_DISPLAY"), String::from("wayland-0"))]);
        let headless = HashMap::new();

        assert_eq!(
            detect_desktop_backend_with(&x11, |binary| binary == "notify-send"),
            DesktopBackend::LinuxNotifySend
        );
        assert_eq!(
            detect_desktop_backend_with(&wayland, |_| false),
            DesktopBackend::None
        );
        assert_eq!(
            detect_desktop_backend_with(&headless, |_| true),
            DesktopBackend::None
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_terminal_program_prefers_osascript_without_requiring_display() {
        let env = HashMap::from([(String::from("TERM_PROGRAM"), String::from("Apple_Terminal"))]);

        assert_eq!(
            detect_desktop_backend_with(&env, |_| false),
            DesktopBackend::MacOsascript
        );
    }

    #[test]
    fn desktop_commands_are_built_without_spawning_processes() {
        assert_eq!(desktop_command(DesktopBackend::None, "title", "body"), None);

        let mac = desktop_command(
            DesktopBackend::MacOsascript,
            r#"mux "board""#,
            r#"path \ ok"#,
        )
        .expect("mac command should exist");
        assert_eq!(mac.binary, "osascript");
        assert_eq!(mac.args[0], "-e");
        assert_eq!(
            mac.args[1],
            r#"display notification "path \\ ok" with title "mux \"board\"""#
        );

        let linux = desktop_command(
            DesktopBackend::LinuxNotifySend,
            "muxboard",
            "needs approval",
        )
        .expect("linux command should exist");
        assert_eq!(linux.binary, "notify-send");
        assert_eq!(linux.args, vec!["muxboard", "needs approval"]);
    }

    #[test]
    fn applescript_escaping_handles_quotes_and_backslashes() {
        assert_eq!(
            escape_applescript(r#"path \ "quoted""#),
            r#"path \\ \"quoted\""#
        );
    }
}
