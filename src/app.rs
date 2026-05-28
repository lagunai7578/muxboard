use std::{
    cell::Cell,
    collections::{HashMap, HashSet, VecDeque, hash_map::DefaultHasher},
    env, fmt,
    hash::{Hash, Hasher},
    path::Path,
    str::FromStr,
    time::{Duration, Instant},
};

use anyhow::{Error, Result};
use serde::{Deserialize, Serialize};

use crate::{
    cli::Cli,
    config,
    core::{
        AgentReport, AgentSourceEvent, AgentSourceProvider, AgentSourceScanner, ObservedPane,
        PaneInsight, PaneRuntime, PaneStatus, WorkloadKind, activity_summary,
        agent_source_matches_path, build_runtime_corpus, collect_runtime_live_lines,
        collect_runtime_recent_lines, effective_agent_report, infer_pane_insight,
        infer_status_from_report, is_agent_report_protocol_line, matches_choice_hint,
        matches_enter_hint, pane_corpus, pane_heat_score, pane_text_has_provider_hint,
        parse_agent_report_line,
    },
    metrics, notifications, state, tmux,
};

#[cfg(test)]
use crate::core::visible_partial_line;

impl<'a> From<&'a tmux::Pane> for ObservedPane<'a> {
    fn from(pane: &'a tmux::Pane) -> Self {
        Self {
            current_command: &pane.current_command,
            title: &pane.title,
            window_name: &pane.window_name,
            current_path: &pane.current_path,
            active: pane.active,
        }
    }
}

#[cfg(test)]
use self::attention::attention_label;
use self::attention::{attention_rank, is_attention_status};

#[cfg(test)]
const MAX_OUTPUT_LINES: usize = 24;
const MAX_RECENT_EVENTS: usize = 10;
const MAX_INSPECTOR_OUTPUT_SOURCE_LINES: usize = 32;
const MAX_LIVE_TAIL_SOURCE_LINES: usize = 64;
const DETAILS_OUTPUT_VIEWPORT_LINES: usize = 8;
const TAIL_OUTPUT_VIEWPORT_LINES: usize = 16;
const MAX_RECENT_COMMANDS: usize = 8;
const MACRO_SLOT_COUNT: usize = 5;
const MAX_RECENT_ALERTS: usize = 8;
const LAUNCH_PRESETS: [&str; 4] = ["codex", "claude", "opencode", "bash"];
const DIRTY_CAPTURE_LIMIT_PER_TICK: usize = 2;
const NATIVE_AGENT_SOURCE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const SYSTEM_THEME: ThemePreset = ThemePreset::TerminalNative;
const LIGHT_THEME: ThemePreset = ThemePreset::CatppuccinLatte;
const DARK_THEME: ThemePreset = ThemePreset::CatppuccinMocha;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemePickerPage {
    Top,
    More,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ThemePickerOption {
    label: &'static str,
    detail: &'static str,
    preset: Option<ThemePreset>,
    next_page: Option<ThemePickerPage>,
}

const THEME_PICKER_TOP_OPTIONS: [ThemePickerOption; 4] = [
    ThemePickerOption {
        label: "System Colors",
        detail: "match your terminal palette",
        preset: Some(SYSTEM_THEME),
        next_page: None,
    },
    ThemePickerOption {
        label: "Light",
        detail: "clean palette for bright terminals",
        preset: Some(LIGHT_THEME),
        next_page: None,
    },
    ThemePickerOption {
        label: "Dark",
        detail: "soft dark palette",
        preset: Some(DARK_THEME),
        next_page: None,
    },
    ThemePickerOption {
        label: "More themes",
        detail: "Catppuccin, Tokyo Night, Gruvbox...",
        preset: None,
        next_page: Some(ThemePickerPage::More),
    },
];

const THEME_PICKER_MORE_OPTIONS: [ThemePickerOption; 10] = [
    ThemePickerOption {
        label: "Catppuccin Latte",
        detail: "warm light palette",
        preset: Some(ThemePreset::CatppuccinLatte),
        next_page: None,
    },
    ThemePickerOption {
        label: "Catppuccin Mocha",
        detail: "warm dark palette",
        preset: Some(ThemePreset::CatppuccinMocha),
        next_page: None,
    },
    ThemePickerOption {
        label: "Tokyo Night",
        detail: "blue-toned dark palette",
        preset: Some(ThemePreset::TokyoNight),
        next_page: None,
    },
    ThemePickerOption {
        label: "Gruvbox Dark",
        detail: "retro dark palette",
        preset: Some(ThemePreset::GruvboxDark),
        next_page: None,
    },
    ThemePickerOption {
        label: "Gruvbox Light",
        detail: "retro light palette",
        preset: Some(ThemePreset::GruvboxLight),
        next_page: None,
    },
    ThemePickerOption {
        label: "Nord",
        detail: "cool low-contrast palette",
        preset: Some(ThemePreset::Nord),
        next_page: None,
    },
    ThemePickerOption {
        label: "Rose Pine",
        detail: "muted rose-tinted palette",
        preset: Some(ThemePreset::RosePine),
        next_page: None,
    },
    ThemePickerOption {
        label: "Mono",
        detail: "shape cues, minimal color",
        preset: Some(ThemePreset::Mono),
        next_page: None,
    },
    ThemePickerOption {
        label: "Contrast",
        detail: "brighter ANSI palette",
        preset: Some(ThemePreset::Contrast),
        next_page: None,
    },
    ThemePickerOption {
        label: "Back",
        detail: "return to simple choices",
        preset: None,
        next_page: Some(ThemePickerPage::Top),
    },
];

#[derive(Debug)]
pub struct App {
    cli: Cli,
    probe: tmux::Probe,
    runtime_context: tmux::RuntimeContext,
    snapshot: tmux::Snapshot,
    control: Option<tmux::control::Monitor>,
    selected_pane_id: Option<String>,
    selected_window_id: Option<String>,
    initial_attention_autofocus: bool,
    pane_runtime: HashMap<String, PaneRuntime>,
    dirty_pane_ids: HashSet<String>,
    native_agent_scanner: AgentSourceScanner,
    native_agent_assignments: HashMap<String, tmux::AgentBridgeEvent>,
    native_agent_versions: HashMap<String, (PaneStatus, u64)>,
    last_native_agent_scan: Option<Instant>,
    pane_metrics: HashMap<String, metrics::PaneMetrics>,
    pane_last_status: HashMap<String, PaneStatus>,
    last_alerted_at: HashMap<String, Instant>,
    acknowledged_attention: HashMap<AttentionKey, PaneStatus>,
    pending_attention_actions: HashMap<AttentionKey, PendingAttentionAction>,
    state_store: state::Store,
    config_store: config::Store,
    notifier: notifications::Notifier,
    notification_settings: NotificationSettings,
    search_query: String,
    search_query_before_input: Option<String>,
    search_input_active: bool,
    command_buffer: String,
    command_input_active: bool,
    launch_buffer: String,
    launch_input_active: bool,
    recent_commands: VecDeque<String>,
    macro_slots: Vec<Option<String>>,
    macro_assign_active: bool,
    action_menu_active: bool,
    help_overlay_active: bool,
    group_name_buffer: String,
    group_input_active: bool,
    fleet_picker_active: bool,
    fleet_picker_index: usize,
    theme_picker_active: bool,
    theme_picker_page: ThemePickerPage,
    theme_picker_index: usize,
    theme_picker_first_run: bool,
    target_groups: Vec<TargetGroup>,
    selected_group_index: Option<usize>,
    active_group_name: Option<String>,
    marked_pane_ids: HashSet<String>,
    ui_settings: UiSettings,
    context_pane: ContextPane,
    panel_focus: PanelFocus,
    details_scroll: usize,
    rendered_scroll_context: Cell<ContextPane>,
    rendered_scroll_viewport_lines: Cell<usize>,
    rendered_scroll_content_lines: Cell<usize>,
    view_scope: ViewScope,
    fanout_mode: FanoutMode,
    metrics_mode: MetricsMode,
    sort_mode: SortMode,
    filter_mode: FilterMode,
    refresh_count: u64,
    notification_count: u64,
    alert_count: u64,
    pending_bell: bool,
    last_metrics_refresh: Option<Instant>,
    pending_dispatch: Option<StagedDispatch>,
    pane_reports: HashMap<String, AgentReport>,
    control_state: String,
    close_after_jump: bool,
    should_quit: bool,
    status_message: String,
    recent_alerts: VecDeque<String>,
    recent_events: VecDeque<String>,
}

#[derive(Debug, Clone, Copy)]
struct AgentLane {
    workload: WorkloadKind,
    total: usize,
    waiting: usize,
    error: usize,
    stuck: usize,
    running: usize,
    done: usize,
    idle: usize,
    unknown: usize,
    selected: bool,
}

#[derive(Debug, Clone, Copy)]
struct VisiblePaneEntry {
    index: usize,
    insight: PaneInsight,
    acknowledged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandCenterPrimaryActionKind {
    Continue,
    Output,
    Reply,
    Answer,
    ShowWaiting,
}

#[derive(Debug, Clone)]
struct CommandCenterPrimaryAction {
    pane: tmux::Pane,
    kind: CommandCenterPrimaryActionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingAttentionAction {
    status: PaneStatus,
    output_fingerprint: u64,
    kind: PendingAttentionActionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingAttentionActionKind {
    Continue,
    Reply,
    AnswerYes,
    AnswerNo,
    BulkContinue,
}

impl PendingAttentionActionKind {
    fn watching_label(self) -> &'static str {
        match self {
            Self::Continue | Self::BulkContinue => "sent Enter",
            Self::Reply => "sent reply",
            Self::AnswerYes => "answered yes",
            Self::AnswerNo => "answered no",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandCenterPrimaryTrigger {
    Smart,
    Focus,
    Command,
    Actions,
    Jump,
}

#[derive(Debug, Clone, Copy)]
struct WindowNavigationEntry {
    index: usize,
    heat: u16,
    pane_count: usize,
}

#[derive(Debug, Clone)]
struct StagedDispatch {
    text: String,
    expanded: Vec<(String, String)>,
    remember: bool,
    target_description: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DispatchOutcome {
    sent_count: usize,
    disappeared_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandDispatchStatus {
    NoTargets,
    Staged,
    Dispatched(DispatchOutcome),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PaneLocator {
    pub session_name: String,
    pub window_name: String,
    pub pane_index: u32,
}

impl PaneLocator {
    fn from_pane(pane: &tmux::Pane) -> Self {
        Self {
            session_name: pane.session_name.clone(),
            window_name: pane.window_name.clone(),
            pane_index: pane.pane_index,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TargetGroup {
    pub name: String,
    pub members: Vec<PaneLocator>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContextPane {
    Inspect,
    Tail,
    Targets,
    Navigator,
    Control,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelFocus {
    Fleet,
    Details,
}

impl PanelFocus {
    fn toggled(self) -> Self {
        match self {
            Self::Fleet => Self::Details,
            Self::Details => Self::Fleet,
        }
    }
}

impl ContextPane {
    fn next(self) -> Self {
        match self {
            Self::Inspect => Self::Tail,
            Self::Tail => Self::Targets,
            Self::Targets => Self::Navigator,
            Self::Navigator => Self::Control,
            Self::Control => Self::Inspect,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ViewScope {
    All,
    Window { id: String, name: String },
}

impl ViewScope {
    fn display_label(&self) -> String {
        match self {
            Self::All => String::from("all panes"),
            Self::Window { name, .. } => format!("window {name}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetricsMode {
    Off,
    Local,
}

impl MetricsMode {
    fn toggle(self) -> Self {
        match self {
            Self::Off => Self::Local,
            Self::Local => Self::Off,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardRow {
    pub selected: bool,
    pub active: bool,
    pub marked: bool,
    pub targeted: bool,
    pub staged: bool,
    pub show_command_in_latest: bool,
    pub attention: String,
    pub status: String,
    pub lifecycle: String,
    pub mission: String,
    pub heat: String,
    pub age: String,
    pub cpu: String,
    pub mem: String,
    pub lane: String,
    pub pane: String,
    pub location: String,
    pub command: String,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoardRowTone {
    Default,
    Subdued,
    Selected,
    Staged,
    Targeted,
    Attention,
    Watching,
    Alert,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiSettings {
    pub layout_preset: LayoutPreset,
    #[serde(skip_serializing_if = "is_default_theme_preset")]
    pub theme_preset: ThemePreset,
    pub theme: ThemeConfig,
    pub keybindings: KeyBindingsConfig,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            layout_preset: LayoutPreset::Auto,
            theme_preset: ThemePreset::default(),
            theme: ThemeConfig::default(),
            keybindings: KeyBindingsConfig::default(),
        }
    }
}

impl UiSettings {
    pub(crate) fn validate(&self) -> Result<()> {
        self.keybindings.validate()
    }

    pub(crate) fn active_theme_preset(&self) -> ThemePreset {
        self.theme.preset.unwrap_or(self.theme_preset)
    }
}

fn is_default_theme_preset(preset: &ThemePreset) -> bool {
    *preset == ThemePreset::default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) enum LayoutPreset {
    #[default]
    #[serde(alias = "Compact", alias = "Standard", alias = "Dense")]
    Auto,
    Horizontal,
    Vertical,
}

impl LayoutPreset {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Auto => Self::Horizontal,
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Auto,
        }
    }

    pub(crate) fn display_label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Horizontal => "side by side",
            Self::Vertical => "stacked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub(crate) enum ThemePreset {
    Calm,
    Contrast,
    Mono,
    #[default]
    TerminalNative,
    CatppuccinLatte,
    CatppuccinMocha,
    TokyoNight,
    GruvboxDark,
    GruvboxLight,
    Nord,
    RosePine,
}

impl ThemePreset {
    const DISPLAY_NAMES: &'static str = "Calm, Contrast, Mono, TerminalNative, CatppuccinLatte, CatppuccinMocha, TokyoNight, GruvboxDark, GruvboxLight, Nord, or RosePine";
    const ALIAS_EXAMPLES: &'static str = "light, dark, system, terminal, ansi, no-color, catppuccin, tokyo-night, gruvbox, or rose-pine";

    pub(crate) fn from_config_name(input: &str) -> std::result::Result<Self, String> {
        let trimmed = input.trim();
        let normalized = trimmed
            .to_ascii_lowercase()
            .replace([' ', '-', '_'], "")
            .replace(['é', 'É'], "e")
            .replace("colour", "color");

        match normalized.as_str() {
            "calm" => Ok(Self::Calm),
            "default" => Ok(Self::TerminalNative),
            "contrast" | "highcontrast" => Ok(Self::Contrast),
            "mono" | "monochrome" | "nocolor" => Ok(Self::Mono),
            "terminalnative" | "terminal" | "native" | "ansi" | "ansiterminal" | "system"
            | "systemcolors" | "systemtheme" => Ok(Self::TerminalNative),
            "catppuccinlatte" | "latte" | "light" => Ok(Self::CatppuccinLatte),
            "catppuccinmocha" | "catppuccin" | "mocha" | "dark" => Ok(Self::CatppuccinMocha),
            "tokyonight" | "tokyo" => Ok(Self::TokyoNight),
            "gruvboxdark" | "gruvbox" => Ok(Self::GruvboxDark),
            "gruvboxlight" => Ok(Self::GruvboxLight),
            "nord" => Ok(Self::Nord),
            "rosepine" | "rosepinedark" => Ok(Self::RosePine),
            _ => Err(format!(
                "invalid muxboard theme preset `{trimmed}`; use one of: {}; aliases include {}",
                Self::DISPLAY_NAMES,
                Self::ALIAS_EXAMPLES
            )),
        }
    }

    pub(crate) fn display_label(self) -> &'static str {
        match self {
            Self::Calm => "Calm",
            Self::Contrast => "Contrast",
            Self::Mono => "Mono",
            Self::TerminalNative => "System Colors",
            Self::CatppuccinLatte => "Light",
            Self::CatppuccinMocha => "Dark",
            Self::TokyoNight => "Tokyo Night",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::GruvboxLight => "Gruvbox Light",
            Self::Nord => "Nord",
            Self::RosePine => "Rose Pine",
        }
    }
}

impl<'de> Deserialize<'de> for ThemePreset {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::from_config_name(&raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct ThemeConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<ThemePreset>,
    pub overrides: ThemeOverrides,
}

impl ThemeConfig {
    pub(crate) fn example() -> Self {
        Self {
            preset: Some(ThemePreset::TerminalNative),
            overrides: ThemeOverrides::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct ThemeOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<ThemeColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<ThemeColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<ThemeColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<ThemeColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<ThemeColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub danger: Option<ThemeColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<ThemeColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border: Option<ThemeColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_fg: Option<ThemeColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_bg: Option<ThemeColor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemeColor {
    Reset,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    DarkGray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    White,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl FromStr for ThemeColor {
    type Err = String;

    fn from_str(input: &str) -> std::result::Result<Self, Self::Err> {
        let trimmed = input.trim();
        let normalized = trimmed
            .to_ascii_lowercase()
            .replace([' ', '-', '_'], "")
            .replace("bright", "light")
            .replace("grey", "gray")
            .replace("silver", "gray")
            .replace("purple", "magenta")
            .replace("default", "reset")
            .replace("lightblack", "darkgray")
            .replace("lightwhite", "white")
            .replace("lightgray", "white");

        match normalized.as_str() {
            "reset" => Ok(Self::Reset),
            "black" => Ok(Self::Black),
            "red" => Ok(Self::Red),
            "green" => Ok(Self::Green),
            "yellow" => Ok(Self::Yellow),
            "blue" => Ok(Self::Blue),
            "magenta" => Ok(Self::Magenta),
            "cyan" => Ok(Self::Cyan),
            "gray" => Ok(Self::Gray),
            "darkgray" => Ok(Self::DarkGray),
            "lightred" => Ok(Self::LightRed),
            "lightgreen" => Ok(Self::LightGreen),
            "lightyellow" => Ok(Self::LightYellow),
            "lightblue" => Ok(Self::LightBlue),
            "lightmagenta" => Ok(Self::LightMagenta),
            "lightcyan" => Ok(Self::LightCyan),
            "white" => Ok(Self::White),
            _ => {
                if let Ok(index) = trimmed.parse::<u8>() {
                    return Ok(Self::Indexed(index));
                }
                parse_hex_theme_color(trimmed)
                    .map(|(red, green, blue)| Self::Rgb(red, green, blue))
                    .ok_or_else(|| {
                        format!(
                            "invalid muxboard theme color `{trimmed}`; use named ANSI colors, 0-255, or #RGB/#RRGGBB"
                        )
                    })
            }
        }
    }
}

fn parse_hex_theme_color(input: &str) -> Option<(u8, u8, u8)> {
    let hex = input.strip_prefix('#')?;
    if hex.len() == 3 {
        let red = u8::from_str_radix(&hex.get(0..1)?.repeat(2), 16).ok()?;
        let green = u8::from_str_radix(&hex.get(1..2)?.repeat(2), 16).ok()?;
        let blue = u8::from_str_radix(&hex.get(2..3)?.repeat(2), 16).ok()?;
        return Some((red, green, blue));
    }
    if hex.len() != 6 {
        return None;
    }

    let red = u8::from_str_radix(hex.get(0..2)?, 16).ok()?;
    let green = u8::from_str_radix(hex.get(2..4)?, 16).ok()?;
    let blue = u8::from_str_radix(hex.get(4..6)?, 16).ok()?;
    Some((red, green, blue))
}

impl fmt::Display for ThemeColor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reset => write!(formatter, "Reset"),
            Self::Black => write!(formatter, "Black"),
            Self::Red => write!(formatter, "Red"),
            Self::Green => write!(formatter, "Green"),
            Self::Yellow => write!(formatter, "Yellow"),
            Self::Blue => write!(formatter, "Blue"),
            Self::Magenta => write!(formatter, "Magenta"),
            Self::Cyan => write!(formatter, "Cyan"),
            Self::Gray => write!(formatter, "Gray"),
            Self::DarkGray => write!(formatter, "DarkGray"),
            Self::LightRed => write!(formatter, "LightRed"),
            Self::LightGreen => write!(formatter, "LightGreen"),
            Self::LightYellow => write!(formatter, "LightYellow"),
            Self::LightBlue => write!(formatter, "LightBlue"),
            Self::LightMagenta => write!(formatter, "LightMagenta"),
            Self::LightCyan => write!(formatter, "LightCyan"),
            Self::White => write!(formatter, "White"),
            Self::Indexed(index) => write!(formatter, "{index}"),
            Self::Rgb(red, green, blue) => write!(formatter, "#{red:02X}{green:02X}{blue:02X}"),
        }
    }
}

impl Serialize for ThemeColor {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ThemeColor {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct KeyBindingsConfig {
    pub quit: Vec<String>,
    pub move_down: Vec<String>,
    pub move_up: Vec<String>,
    pub panel_focus: Vec<String>,
    pub search: Vec<String>,
    pub command: Vec<String>,
    pub mark: Vec<String>,
    pub clear_marks: Vec<String>,
    pub summaries: Vec<String>,
    pub actions: Vec<String>,
    pub smart_action: Vec<String>,
    pub focus: Vec<String>,
    pub jump: Vec<String>,
    pub refresh: Vec<String>,
    pub repeat_last: Vec<String>,
    pub macro_assign: Vec<String>,
    pub macro_slot_1: Vec<String>,
    pub macro_slot_2: Vec<String>,
    pub macro_slot_3: Vec<String>,
    pub macro_slot_4: Vec<String>,
    pub macro_slot_5: Vec<String>,
    pub action_sort: Vec<String>,
    pub action_filter: Vec<String>,
    pub action_view_browse: Vec<String>,
    pub action_view_command_center: Vec<String>,
    pub action_group_save: Vec<String>,
    pub action_group_load: Vec<String>,
    pub action_group_delete: Vec<String>,
    pub action_launch_agent: Vec<String>,
    pub action_lane_target: Vec<String>,
    pub action_ack_selected: Vec<String>,
    pub action_ack_clear_selected: Vec<String>,
    pub action_ack_all: Vec<String>,
    pub action_ack_clear_all: Vec<String>,
    pub action_enter_queue: Vec<String>,
    pub action_zoom: Vec<String>,
    pub action_send_enter: Vec<String>,
    pub action_send_yes: Vec<String>,
    pub action_send_no: Vec<String>,
    pub action_layout: Vec<String>,
    pub action_metrics: Vec<String>,
    pub action_desktop_notifications: Vec<String>,
    pub action_bell: Vec<String>,
    pub action_alert_debounce: Vec<String>,
    pub action_alert_policy: Vec<String>,
}

impl Default for KeyBindingsConfig {
    fn default() -> Self {
        Self {
            quit: vec![String::from("q")],
            move_down: vec![String::from("j"), String::from("down")],
            move_up: vec![String::from("k"), String::from("up")],
            panel_focus: vec![String::from("tab")],
            search: vec![String::from("/")],
            command: vec![String::from(":")],
            mark: vec![String::from("space")],
            clear_marks: vec![String::from("x")],
            summaries: vec![String::from("s")],
            actions: vec![String::from(".")],
            smart_action: vec![String::from("a")],
            focus: vec![String::from("enter")],
            jump: vec![String::from("g")],
            refresh: vec![String::from("r")],
            repeat_last: vec![String::from("]")],
            macro_assign: vec![String::from("p")],
            macro_slot_1: vec![String::from("1")],
            macro_slot_2: vec![String::from("2")],
            macro_slot_3: vec![String::from("3")],
            macro_slot_4: vec![String::from("4")],
            macro_slot_5: vec![String::from("5")],
            action_sort: vec![String::from("t")],
            action_filter: vec![String::from("f")],
            action_view_browse: vec![String::from("[")],
            action_view_command_center: vec![String::from("]")],
            action_group_save: vec![String::from("g")],
            action_group_load: vec![String::from("l")],
            action_group_delete: vec![String::from("d")],
            action_launch_agent: vec![String::from("+")],
            action_lane_target: vec![String::from("b")],
            action_ack_selected: vec![String::from("c")],
            action_ack_clear_selected: vec![String::from("w")],
            action_ack_all: vec![String::from("a")],
            action_ack_clear_all: vec![String::from("u")],
            action_enter_queue: vec![String::from("i")],
            action_zoom: vec![String::from("z")],
            action_send_enter: vec![String::from("e")],
            action_send_yes: vec![String::from("y")],
            action_send_no: vec![String::from("n")],
            action_layout: vec![String::from("L")],
            action_metrics: vec![String::from("m")],
            action_desktop_notifications: vec![String::from("o")],
            action_bell: vec![String::from("v")],
            action_alert_debounce: vec![String::from("h")],
            action_alert_policy: vec![String::from("p")],
        }
    }
}

impl KeyBindingsConfig {
    fn primary_label(binding: &[String]) -> String {
        binding
            .first()
            .map(|key| format_key_label(key))
            .unwrap_or_else(|| String::from("?"))
    }

    fn move_labels(&self) -> String {
        format!(
            "{}/{}",
            Self::primary_label(&self.move_down),
            Self::primary_label(&self.move_up)
        )
    }

    fn action_label(&self, binding: &[String], description: &str) -> String {
        format!("  {} {}", Self::primary_label(binding), description)
    }

    fn macro_slot_binding(&self, slot: usize) -> &[String] {
        match slot {
            0 => &self.macro_slot_1,
            1 => &self.macro_slot_2,
            2 => &self.macro_slot_3,
            3 => &self.macro_slot_4,
            4 => &self.macro_slot_5,
            _ => &[],
        }
    }

    fn macro_slot_label(&self, slot: usize) -> String {
        Self::primary_label(self.macro_slot_binding(slot))
    }

    pub(crate) fn validate(&self) -> Result<()> {
        validate_binding_scope(
            "top-level",
            vec![
                ("quit", self.quit.as_slice()),
                ("move_down", self.move_down.as_slice()),
                ("move_up", self.move_up.as_slice()),
                ("panel_focus", self.panel_focus.as_slice()),
                ("search", self.search.as_slice()),
                ("command", self.command.as_slice()),
                ("mark", self.mark.as_slice()),
                ("clear_marks", self.clear_marks.as_slice()),
                ("summaries", self.summaries.as_slice()),
                ("actions", self.actions.as_slice()),
                ("smart_action", self.smart_action.as_slice()),
                ("focus", self.focus.as_slice()),
                ("jump", self.jump.as_slice()),
                ("refresh", self.refresh.as_slice()),
                ("repeat_last", self.repeat_last.as_slice()),
                ("macro_assign", self.macro_assign.as_slice()),
                ("macro_slot_1", self.macro_slot_1.as_slice()),
                ("macro_slot_2", self.macro_slot_2.as_slice()),
                ("macro_slot_3", self.macro_slot_3.as_slice()),
                ("macro_slot_4", self.macro_slot_4.as_slice()),
                ("macro_slot_5", self.macro_slot_5.as_slice()),
            ],
        )?;
        validate_binding_scope(
            "actions menu",
            vec![
                ("actions", self.actions.as_slice()),
                ("summaries", self.summaries.as_slice()),
                ("refresh", self.refresh.as_slice()),
                ("action_sort", self.action_sort.as_slice()),
                ("action_filter", self.action_filter.as_slice()),
                ("focus", self.focus.as_slice()),
                ("action_view_browse", self.action_view_browse.as_slice()),
                (
                    "action_view_command_center",
                    self.action_view_command_center.as_slice(),
                ),
                ("clear_marks", self.clear_marks.as_slice()),
                ("action_group_save", self.action_group_save.as_slice()),
                ("action_group_load", self.action_group_load.as_slice()),
                ("action_group_delete", self.action_group_delete.as_slice()),
                ("action_launch_agent", self.action_launch_agent.as_slice()),
                ("action_lane_target", self.action_lane_target.as_slice()),
                ("action_ack_selected", self.action_ack_selected.as_slice()),
                (
                    "action_ack_clear_selected",
                    self.action_ack_clear_selected.as_slice(),
                ),
                ("action_ack_all", self.action_ack_all.as_slice()),
                ("action_ack_clear_all", self.action_ack_clear_all.as_slice()),
                ("action_enter_queue", self.action_enter_queue.as_slice()),
                ("action_zoom", self.action_zoom.as_slice()),
                ("action_send_enter", self.action_send_enter.as_slice()),
                ("action_send_yes", self.action_send_yes.as_slice()),
                ("action_send_no", self.action_send_no.as_slice()),
                ("action_layout", self.action_layout.as_slice()),
                ("action_metrics", self.action_metrics.as_slice()),
                (
                    "action_desktop_notifications",
                    self.action_desktop_notifications.as_slice(),
                ),
                ("action_bell", self.action_bell.as_slice()),
                (
                    "action_alert_debounce",
                    self.action_alert_debounce.as_slice(),
                ),
                ("action_alert_policy", self.action_alert_policy.as_slice()),
            ],
        )
    }
}

fn validate_binding_scope(scope: &str, bindings: Vec<(&str, &[String])>) -> Result<()> {
    let mut seen = HashMap::new();

    for (action, binding) in bindings {
        if binding.is_empty() {
            anyhow::bail!("ui_settings.keybindings.{action} must not be empty");
        }

        for token in binding {
            validate_binding_token(action, token)?;

            if let Some(previous) = seen.insert(token.clone(), action) {
                anyhow::bail!(
                    "ui_settings.keybindings.{action} conflicts with {previous} in the {scope} scope on `{token}`"
                );
            }
        }
    }

    Ok(())
}

fn validate_binding_token(action: &str, token: &str) -> Result<()> {
    const NAMED_KEYS: &[&str] = &["space", "enter", "tab", "backspace", "esc", "up", "down"];

    if token.trim() != token || token.is_empty() {
        anyhow::bail!("ui_settings.keybindings.{action} contains an empty or padded token");
    }

    if NAMED_KEYS.contains(&token) {
        return Ok(());
    }

    let mut chars = token.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) if !ch.is_control() && !ch.is_whitespace() => Ok(()),
        _ => anyhow::bail!("ui_settings.keybindings.{action} contains unsupported token `{token}`"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum AlertPolicy {
    AllAttention,
    ErrorAndWaiting,
    ErrorsOnly,
}

impl AlertPolicy {
    fn next(self) -> Self {
        match self {
            Self::AllAttention => Self::ErrorAndWaiting,
            Self::ErrorAndWaiting => Self::ErrorsOnly,
            Self::ErrorsOnly => Self::AllAttention,
        }
    }

    fn display_label(self) -> &'static str {
        match self {
            Self::AllAttention => "all alerts",
            Self::ErrorAndWaiting => "waiting + errors",
            Self::ErrorsOnly => "errors only",
        }
    }

    fn matches(self, status: PaneStatus) -> bool {
        match self {
            Self::AllAttention => is_attention_status(status),
            Self::ErrorAndWaiting => matches!(status, PaneStatus::Waiting | PaneStatus::Error),
            Self::ErrorsOnly => status == PaneStatus::Error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct NotificationSettings {
    pub bell_enabled: bool,
    pub desktop_enabled: bool,
    pub alert_policy: AlertPolicy,
    pub debounce_seconds: u64,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            bell_enabled: true,
            desktop_enabled: true,
            alert_policy: AlertPolicy::AllAttention,
            debounce_seconds: 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct AttentionKey {
    pub session_name: String,
    pub window_name: String,
    #[serde(default)]
    pub pane_index: u32,
    pub current_path: String,
    pub current_command: String,
    pub title: String,
}

impl AttentionKey {
    fn from_pane(pane: &tmux::Pane) -> Self {
        Self {
            session_name: pane.session_name.clone(),
            window_name: pane.window_name.clone(),
            pane_index: pane.pane_index,
            current_path: pane.current_path.clone(),
            current_command: pane.current_command.clone(),
            title: pane.title.clone(),
        }
    }

    pub(crate) fn sort_key(&self) -> (&str, &str, u32, &str, &str, &str) {
        (
            &self.session_name,
            &self.window_name,
            self.pane_index,
            &self.current_path,
            &self.current_command,
            &self.title,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FanoutMode {
    Off,
    Lane,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SmartAction {
    Focus,
    SendEnter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Attention,
    Heat,
    Natural,
}

impl SortMode {
    fn next(self) -> Self {
        match self {
            Self::Attention => Self::Heat,
            Self::Heat => Self::Natural,
            Self::Natural => Self::Attention,
        }
    }

    fn display_label(self) -> &'static str {
        match self {
            Self::Attention => "priority",
            Self::Heat => "activity",
            Self::Natural => "tmux order",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    All,
    Agents,
    Attention,
}

impl FilterMode {
    fn next(self) -> Self {
        match self {
            Self::All => Self::Agents,
            Self::Agents => Self::Attention,
            Self::Attention => Self::All,
        }
    }

    fn display_label(self) -> &'static str {
        match self {
            Self::All => "all panes",
            Self::Agents => "agents",
            Self::Attention => "needs you",
        }
    }
}

impl App {
    pub async fn bootstrap(cli: Cli) -> Result<Self> {
        let target = tmux::Target::from(&cli);
        let probe = tmux::probe(target).await?;
        let runtime_context = tmux::RuntimeContext::from_env();
        let state_store = state::Store::new()?;
        let config_store = config::Store::new()?;
        let notifier = notifications::Notifier::from_env();

        Self::bootstrap_from_probe(
            cli,
            probe,
            runtime_context,
            state_store,
            config_store,
            notifier,
        )
        .await
    }

    async fn bootstrap_from_probe(
        cli: Cli,
        probe: tmux::Probe,
        runtime_context: tmux::RuntimeContext,
        state_store: state::Store,
        config_store: config::Store,
        notifier: notifications::Notifier,
    ) -> Result<Self> {
        let target = probe.target.clone();
        let force_theme_picker = cli.theme_picker;

        let (snapshot, status_message) = match tmux::snapshot(target.clone()).await {
            Ok(snapshot) => (snapshot, String::new()),
            Err(error) => (
                tmux::Snapshot::default(),
                startup_snapshot_status_message(&target, &error),
            ),
        };

        let (control, control_state) = match tmux::control::start(&target).await {
            Ok(monitor) => (Some(monitor), String::from("connected")),
            Err(error) => (None, format!("not connected: {error}")),
        };

        let (acknowledged_attention, status_message) =
            match state_store.load_acknowledged_attention() {
                Ok(acknowledged_attention) => (acknowledged_attention, status_message),
                Err(error) => (
                    HashMap::new(),
                    append_startup_status(status_message, format!("State load failed: {error}")),
                ),
            };
        let (recent_commands, macro_slots, status_message) = match state_store.load_command_state()
        {
            Ok((recent_commands, macro_slots)) => (recent_commands, macro_slots, status_message),
            Err(error) => (
                Vec::new(),
                default_macro_slots(),
                append_startup_status(
                    status_message,
                    format!("Command state load failed: {error}"),
                ),
            ),
        };
        let (target_groups, status_message) = match state_store.load_target_groups() {
            Ok(target_groups) => (target_groups, status_message),
            Err(error) => (
                Vec::new(),
                append_startup_status(status_message, format!("Fleet load failed: {error}")),
            ),
        };
        let (ui_settings, status_message) = match config_store.load_ui_settings() {
            Ok(ui_settings) => (ui_settings, status_message),
            Err(error) => (
                UiSettings::default(),
                append_startup_status(
                    status_message,
                    format!("UI settings load failed: {error}. Using defaults."),
                ),
            ),
        };
        let (notification_settings, status_message) =
            match config_store.load_notification_settings() {
                Ok(notification_settings) => (notification_settings, status_message),
                Err(error) => (
                    NotificationSettings::default(),
                    append_startup_status(
                        status_message,
                        format!("Notification settings load failed: {error}. Using defaults."),
                    ),
                ),
            };
        let should_show_theme_onboarding =
            config_store.should_show_theme_onboarding().unwrap_or(false);
        let should_open_theme_picker =
            force_theme_picker || (should_show_theme_onboarding && snapshot.pane_count() > 0);

        let mut app = Self {
            cli,
            probe,
            runtime_context,
            snapshot,
            control,
            selected_pane_id: None,
            selected_window_id: None,
            initial_attention_autofocus: true,
            pane_runtime: HashMap::new(),
            dirty_pane_ids: HashSet::new(),
            native_agent_scanner: AgentSourceScanner::from_env(),
            native_agent_assignments: HashMap::new(),
            native_agent_versions: HashMap::new(),
            last_native_agent_scan: None,
            pane_metrics: HashMap::new(),
            pane_last_status: HashMap::new(),
            last_alerted_at: HashMap::new(),
            acknowledged_attention,
            pending_attention_actions: HashMap::new(),
            state_store,
            config_store,
            notifier,
            notification_settings,
            search_query: String::new(),
            search_query_before_input: None,
            search_input_active: false,
            command_buffer: String::new(),
            command_input_active: false,
            launch_buffer: String::new(),
            launch_input_active: false,
            recent_commands: recent_commands.into(),
            macro_slots: normalize_macro_slots(macro_slots),
            macro_assign_active: false,
            action_menu_active: false,
            help_overlay_active: false,
            group_name_buffer: String::new(),
            group_input_active: false,
            fleet_picker_active: false,
            fleet_picker_index: 0,
            theme_picker_active: false,
            theme_picker_page: ThemePickerPage::Top,
            theme_picker_index: 0,
            theme_picker_first_run: false,
            target_groups,
            selected_group_index: None,
            active_group_name: None,
            marked_pane_ids: HashSet::new(),
            ui_settings,
            context_pane: ContextPane::Inspect,
            panel_focus: PanelFocus::Fleet,
            details_scroll: 0,
            rendered_scroll_context: Cell::new(ContextPane::Inspect),
            rendered_scroll_viewport_lines: Cell::new(0),
            rendered_scroll_content_lines: Cell::new(0),
            view_scope: ViewScope::All,
            fanout_mode: FanoutMode::Off,
            metrics_mode: MetricsMode::Off,
            sort_mode: SortMode::Attention,
            filter_mode: FilterMode::All,
            refresh_count: 1,
            notification_count: 0,
            alert_count: 0,
            pending_bell: false,
            last_metrics_refresh: None,
            pending_dispatch: None,
            pane_reports: HashMap::new(),
            control_state,
            close_after_jump: close_after_jump_enabled_from_env(),
            should_quit: false,
            status_message,
            recent_alerts: VecDeque::new(),
            recent_events: VecDeque::new(),
        };
        app.refresh_native_agent_sources(true);
        app.ensure_selection();
        app.sync_selected_window_from_selection();
        app.capture_runtime_from_snapshot(false).await?;
        app.initialize_pane_status_cache();
        app.reset_selection_to_top_visible();
        if should_open_theme_picker {
            app.open_theme_picker(should_show_theme_onboarding);
        }
        Ok(app)
    }

    pub async fn refresh(&mut self) -> Result<()> {
        let refreshed = match tmux::probe(self.target().clone()).await {
            Ok(refreshed) => refreshed,
            Err(error) => {
                self.status_message = format!("Refresh failed: {error}");
                return Ok(());
            }
        };
        self.probe = refreshed;
        let refreshed = self.refresh_snapshot(true).await?;
        self.refresh_count += 1;
        if refreshed {
            self.refresh_control_connection().await;
            self.status_message = String::from("Refreshed.");
        }
        Ok(())
    }

    async fn refresh_control_connection(&mut self) {
        let needs_control = self
            .control
            .as_ref()
            .is_none_or(|control| control.is_finished())
            || self.control_state.starts_with("disconnected")
            || self.control_state.starts_with("not connected");

        if !needs_control {
            return;
        }

        match tmux::control::start(self.target()).await {
            Ok(monitor) => {
                self.control = Some(monitor);
                self.control_state = String::from("connected");
            }
            Err(error) => {
                self.control = None;
                self.control_state = format!("not connected: {error}");
            }
        }
    }

    pub async fn tick(&mut self) -> Result<()> {
        let mut needs_snapshot_refresh = false;
        let mut drained_events = Vec::new();

        if let Some(control) = &mut self.control {
            while let Some(event) = control.try_recv() {
                drained_events.push(event);
            }

            if control.is_finished() && !self.control_state.starts_with("disconnected") {
                self.control_state = String::from("disconnected");
            }
        }

        for event in drained_events {
            self.notification_count += 1;
            needs_snapshot_refresh |= self.handle_event(&event);
            if event.is_loggable() {
                self.push_event(event.summary());
            }
        }

        if !self.dirty_pane_ids.is_empty() {
            self.capture_runtime_for_dirty_panes().await?;
        }

        self.refresh_native_agent_sources_if_due();
        self.reconcile_acknowledgements();
        self.reconcile_pending_attention_actions();
        self.capture_attention_transitions();

        if needs_snapshot_refresh {
            self.refresh_snapshot(false).await?;
            self.reconcile_pending_attention_actions();
            self.capture_attention_transitions();
        }

        if self.metrics_mode == MetricsMode::Local && self.should_refresh_metrics() {
            self.refresh_metrics().await;
        }

        Ok(())
    }

    pub fn request_quit(&mut self) {
        self.should_quit = true;
    }

    pub fn toggle_help_overlay(&mut self) {
        self.help_overlay_active = !self.help_overlay_active;
        self.status_message.clear();
    }

    pub fn close_help_overlay(&mut self) -> bool {
        if !self.help_overlay_active {
            return false;
        }

        self.help_overlay_active = false;
        self.status_message.clear();
        true
    }

    pub fn is_help_overlay_active(&self) -> bool {
        self.help_overlay_active
    }

    pub fn select_next_pane(&mut self) {
        self.initial_attention_autofocus = false;
        if self.panel_focus == PanelFocus::Details {
            if matches!(self.context_pane, ContextPane::Inspect | ContextPane::Tail)
                && self.scroll_details_down()
            {
                return;
            }
            if self.context_pane == ContextPane::Navigator {
                self.select_next_window();
                return;
            }
        }

        self.select_next_visible_pane();
    }

    fn select_next_visible_pane(&mut self) {
        let visible = self.visible_pane_indices();
        if visible.is_empty() {
            self.selected_pane_id = None;
            return;
        }

        let current_index = self
            .selected_visible_pane_position_in(&visible)
            .unwrap_or(0);
        let next_index = (current_index + 1) % visible.len();
        self.selected_pane_id = Some(self.snapshot.panes[visible[next_index]].id.clone());
        self.details_scroll = 0;
        self.sync_selected_window_from_selection();
    }

    pub fn select_previous_pane(&mut self) {
        self.initial_attention_autofocus = false;
        if self.panel_focus == PanelFocus::Details {
            if matches!(self.context_pane, ContextPane::Inspect | ContextPane::Tail)
                && self.scroll_details_up()
            {
                return;
            }
            if self.context_pane == ContextPane::Navigator {
                self.select_previous_window();
                return;
            }
        }

        self.select_previous_visible_pane();
    }

    fn select_previous_visible_pane(&mut self) {
        let visible = self.visible_pane_indices();
        if visible.is_empty() {
            self.selected_pane_id = None;
            return;
        }

        let current_index = self
            .selected_visible_pane_position_in(&visible)
            .unwrap_or(0);
        let previous_index = if current_index == 0 {
            visible.len() - 1
        } else {
            current_index - 1
        };
        self.selected_pane_id = Some(self.snapshot.panes[visible[previous_index]].id.clone());
        self.details_scroll = 0;
        self.sync_selected_window_from_selection();
    }

    pub fn cycle_context_pane(&mut self) {
        self.context_pane = self.context_pane.next();
        self.details_scroll = 0;
        self.status_message.clear();
    }

    pub fn show_browse_view(&mut self) {
        self.context_pane = ContextPane::Navigator;
        self.panel_focus = PanelFocus::Details;
        self.details_scroll = 0;
        self.sync_selected_window_from_selection();
        self.status_message.clear();
    }

    pub fn show_send_view(&mut self) {
        self.context_pane = ContextPane::Targets;
        self.panel_focus = PanelFocus::Details;
        self.details_scroll = 0;
        self.status_message.clear();
    }

    pub fn show_command_center(&mut self) {
        self.context_pane = ContextPane::Control;
        self.panel_focus = PanelFocus::Details;
        self.details_scroll = 0;
        self.status_message.clear();
    }

    pub fn is_command_center_active(&self) -> bool {
        matches!(self.context_pane, ContextPane::Control)
    }

    pub(super) fn command_center_can_reply_to_pane(
        &self,
        _pane: &tmux::Pane,
        insight: PaneInsight,
    ) -> bool {
        insight.status == PaneStatus::Waiting
            && !self.using_explicit_targets()
            && self.fanout_mode == FanoutMode::Off
    }

    pub(crate) fn pane_has_choice_prompt(&self, pane: &tmux::Pane, insight: PaneInsight) -> bool {
        let recent_lines = self.recent_output_lines(&pane.id, 6);
        insight.status == PaneStatus::Waiting
            && recent_lines.iter().any(|line| matches_choice_hint(line))
    }

    pub fn is_browse_view_active(&self) -> bool {
        matches!(self.context_pane, ContextPane::Navigator)
    }

    pub fn is_output_view_active(&self) -> bool {
        matches!(self.context_pane, ContextPane::Tail)
    }

    pub fn go_back(&mut self) -> bool {
        match self.context_pane {
            ContextPane::Tail => {
                self.context_pane = ContextPane::Inspect;
                self.panel_focus = PanelFocus::Fleet;
                self.details_scroll = 0;
                self.status_message.clear();
                true
            }
            ContextPane::Targets | ContextPane::Navigator | ContextPane::Control => {
                self.context_pane = ContextPane::Inspect;
                self.panel_focus = PanelFocus::Details;
                self.details_scroll = 0;
                self.status_message.clear();
                true
            }
            ContextPane::Inspect if self.is_details_panel_focused() => {
                self.panel_focus = PanelFocus::Fleet;
                self.details_scroll = 0;
                self.status_message.clear();
                true
            }
            ContextPane::Inspect => false,
        }
    }

    pub fn cycle_panel_focus(&mut self) {
        if matches!(
            self.context_pane,
            ContextPane::Navigator | ContextPane::Control
        ) {
            self.panel_focus = PanelFocus::Details;
            self.status_message.clear();
            return;
        }

        self.panel_focus = self.panel_focus.toggled();
        self.status_message.clear();
    }

    pub(crate) fn is_details_panel_focused(&self) -> bool {
        self.panel_focus == PanelFocus::Details
    }

    pub(crate) fn is_fleet_panel_focused(&self) -> bool {
        !self.is_details_panel_focused()
    }

    fn scroll_details_down(&mut self) -> bool {
        self.scroll_details_newer_by(1)
    }

    fn scroll_details_up(&mut self) -> bool {
        self.scroll_details_older_by(1)
    }

    fn scroll_details_newer_by(&mut self, rows: usize) -> bool {
        let max_offset = self.details_scroll_max_offset();
        if max_offset == 0 {
            self.details_scroll = 0;
            return false;
        }

        let current = self.details_scroll.min(max_offset);
        self.details_scroll = current.saturating_sub(rows.max(1));
        self.status_message.clear();
        true
    }

    fn scroll_details_older_by(&mut self, rows: usize) -> bool {
        let max_offset = self.details_scroll_max_offset();
        if max_offset == 0 {
            self.details_scroll = 0;
            return false;
        }

        let current = self.details_scroll.min(max_offset);
        self.details_scroll = current.saturating_add(rows.max(1)).min(max_offset);
        self.status_message.clear();
        true
    }

    pub fn scroll_details_page_newer(&mut self) -> bool {
        if self.panel_focus != PanelFocus::Details
            || !matches!(self.context_pane, ContextPane::Inspect | ContextPane::Tail)
        {
            return false;
        }
        self.scroll_details_newer_by(self.details_scroll_page_size())
    }

    pub fn scroll_details_page_older(&mut self) -> bool {
        if self.panel_focus != PanelFocus::Details
            || !matches!(self.context_pane, ContextPane::Inspect | ContextPane::Tail)
        {
            return false;
        }
        self.scroll_details_older_by(self.details_scroll_page_size())
    }

    pub fn scroll_details_to_newest(&mut self) -> bool {
        if self.panel_focus != PanelFocus::Details
            || !matches!(self.context_pane, ContextPane::Inspect | ContextPane::Tail)
        {
            return false;
        }
        let max_offset = self.details_scroll_max_offset();
        if max_offset == 0 {
            self.details_scroll = 0;
            return false;
        }
        let current = self.details_scroll.min(max_offset);
        self.details_scroll = 0;
        self.status_message.clear();
        current != 0
    }

    pub fn scroll_details_to_oldest(&mut self) -> bool {
        if self.panel_focus != PanelFocus::Details
            || !matches!(self.context_pane, ContextPane::Inspect | ContextPane::Tail)
        {
            return false;
        }
        let max_offset = self.details_scroll_max_offset();
        if max_offset == 0 {
            self.details_scroll = 0;
            return false;
        }
        let current = self.details_scroll.min(max_offset);
        self.details_scroll = max_offset;
        self.status_message.clear();
        self.details_scroll != current
    }

    fn clamp_details_scroll_to_content(&mut self) {
        self.details_scroll = self.details_scroll.min(self.details_scroll_max_offset());
    }

    pub fn clear_view_scope(&mut self) {
        if self.view_scope == ViewScope::All
            && self.search_query.is_empty()
            && self.filter_mode == FilterMode::All
        {
            self.status_message = String::from("Already showing all panes.");
            return;
        }

        self.view_scope = ViewScope::All;
        self.search_query.clear();
        self.search_query_before_input = None;
        self.filter_mode = FilterMode::All;
        self.ensure_selection();
        self.sync_selected_window_from_selection();
        self.status_message = String::from("Showing all panes.");
    }

    pub fn cycle_sort_mode(&mut self) {
        self.sort_mode = self.sort_mode.next();
        self.ensure_selection();
        self.status_message = format!("Sorted by {}.", self.sort_mode.display_label());
    }

    pub fn cycle_filter_mode(&mut self) {
        self.filter_mode = self.filter_mode.next();
        self.ensure_selection();
        self.status_message = format!("Showing {}.", self.filter_mode.display_label());
    }

    pub fn begin_search(&mut self) {
        self.action_menu_active = false;
        self.macro_assign_active = false;
        self.command_input_active = false;
        self.group_input_active = false;
        self.launch_input_active = false;
        self.fleet_picker_active = false;
        self.search_query_before_input = Some(self.search_query.clone());
        self.search_input_active = true;
        self.status_message.clear();
    }

    pub fn push_search_char(&mut self, ch: char) {
        if !self.search_input_active || ch.is_control() {
            return;
        }

        self.search_query.push(ch);
        self.ensure_selection();
    }

    pub fn pop_search_char(&mut self) {
        if !self.search_input_active {
            return;
        }

        self.search_query.pop();
        self.ensure_selection();
    }

    pub fn finish_search(&mut self) {
        self.search_input_active = false;
        self.search_query_before_input = None;
        self.ensure_selection();
        self.status_message = if self.search_query.is_empty() {
            String::new()
        } else {
            format!("Filtering panes by `{}`.", self.search_query)
        };
    }

    pub fn cancel_search(&mut self) -> bool {
        if !self.search_input_active {
            return false;
        }

        if let Some(previous_query) = self.search_query_before_input.take() {
            self.search_query = previous_query;
        }
        self.search_input_active = false;
        self.ensure_selection();
        self.status_message.clear();
        true
    }

    pub fn is_search_input_active(&self) -> bool {
        self.search_input_active
    }

    pub fn begin_command_input(&mut self) {
        if !self.using_explicit_targets() {
            self.ensure_selection();
            if self.visible_pane_indices().is_empty()
                && !self.snapshot.panes.is_empty()
                && (self.view_scope != ViewScope::All
                    || !self.search_query.is_empty()
                    || self.filter_mode != FilterMode::All)
            {
                self.status_message = String::from("Show all panes before sending.");
                return;
            }
        }
        if self.active_target_panes().is_empty() {
            self.status_message = self.no_active_targets_message();
            return;
        }

        self.macro_assign_active = false;
        self.action_menu_active = false;
        self.search_input_active = false;
        self.group_input_active = false;
        self.launch_input_active = false;
        self.fleet_picker_active = false;
        self.command_input_active = true;
        self.status_message.clear();
    }

    pub fn push_command_char(&mut self, ch: char) {
        if !self.command_input_active || ch.is_control() {
            return;
        }

        self.command_buffer.push(ch);
    }

    pub fn pop_command_char(&mut self) {
        if !self.command_input_active {
            return;
        }

        self.command_buffer.pop();
    }

    pub async fn submit_command_input(&mut self) -> Result<()> {
        if !self.command_input_active {
            return Ok(());
        }

        let was_reply = self.command_input_is_reply_context();
        let text = self.command_buffer.clone();
        if text.trim().is_empty() {
            self.status_message = String::from("Send text is empty.");
            return Ok(());
        }
        self.command_input_active = false;
        self.command_buffer.clear();
        if was_reply {
            self.send_reply_text(&text).await
        } else {
            self.send_command_text(&text).await
        }
    }

    pub fn cancel_command_input(&mut self) -> bool {
        if !self.command_input_active {
            return false;
        }

        let was_reply = self.command_input_is_reply_context();
        self.command_input_active = false;
        self.command_buffer.clear();
        self.status_message = if was_reply {
            String::from("Closed Reply.")
        } else {
            String::from("Closed Send.")
        };
        true
    }

    pub fn is_command_input_active(&self) -> bool {
        self.command_input_active
    }

    pub fn command_input_can_repeat_recent(&self) -> bool {
        self.command_input_active
            && !self.command_input_is_reply_context()
            && self.command_buffer.trim().is_empty()
            && !self.recent_commands.is_empty()
    }

    pub fn begin_launch_input(&mut self) {
        if self.reject_hidden_selected_pane_action("starting an agent") {
            return;
        }

        let Some(pane) = self.selected_pane().cloned() else {
            self.status_message = String::from("Select a pane first.");
            return;
        };

        self.search_input_active = false;
        self.command_input_active = false;
        self.macro_assign_active = false;
        self.action_menu_active = false;
        self.group_input_active = false;
        self.fleet_picker_active = false;
        self.launch_input_active = true;
        self.launch_buffer = self.default_launch_command_for_pane(&pane);
        self.status_message.clear();
    }

    pub fn is_launch_input_active(&self) -> bool {
        self.launch_input_active
    }

    pub fn push_launch_char(&mut self, ch: char) {
        if !self.launch_input_active || ch.is_control() {
            return;
        }

        self.launch_buffer.push(ch);
    }

    pub fn pop_launch_char(&mut self) {
        if !self.launch_input_active {
            return;
        }

        self.launch_buffer.pop();
    }

    pub fn cycle_launch_preset(&mut self, forward: bool) {
        if !self.launch_input_active {
            return;
        }

        let current = self.launch_buffer.trim();
        let current_index = LAUNCH_PRESETS
            .iter()
            .position(|preset| preset.eq_ignore_ascii_case(current));
        let next_index = match (forward, current_index) {
            (true, Some(index)) => (index + 1) % LAUNCH_PRESETS.len(),
            (false, Some(0)) => LAUNCH_PRESETS.len() - 1,
            (false, Some(index)) => index - 1,
            (true, None) => 0,
            (false, None) => LAUNCH_PRESETS.len() - 1,
        };
        self.launch_buffer = String::from(LAUNCH_PRESETS[next_index]);
        self.status_message.clear();
    }

    pub fn cancel_launch_input(&mut self) -> bool {
        if !self.launch_input_active {
            return false;
        }

        self.launch_input_active = false;
        self.launch_buffer.clear();
        self.status_message = String::from("Closed Start.");
        true
    }

    pub async fn submit_launch_input(&mut self) -> Result<()> {
        if !self.launch_input_active {
            return Ok(());
        }

        let command = self.launch_buffer.trim().to_owned();
        if command.is_empty() {
            self.status_message = String::from("Type a command or press Tab for a preset.");
            return Ok(());
        }

        let Some(pane) = self.selected_pane().cloned() else {
            self.status_message = String::from("Select a pane first.");
            return Ok(());
        };

        let window_name = launch_window_name(&command);
        if let Err(error) = tmux::new_window(
            self.target(),
            &pane.session_name,
            &window_name,
            &pane.current_path,
            &command,
        )
        .await
        {
            if tmux_error_indicates_target_unavailable(&error) {
                self.launch_input_active = false;
                self.launch_buffer.clear();
                let refreshed = self.refresh_snapshot(false).await?;
                if refreshed {
                    self.status_message =
                        String::from("Start canceled; pane disappeared. Refreshed panes.");
                }
                return Ok(());
            }
            self.status_message = format!("Start failed: {error}");
            return Ok(());
        }
        self.launch_input_active = false;
        self.launch_buffer.clear();
        self.status_message = format!(
            "Started `{}` in {}/{}.",
            command, pane.session_name, window_name
        );
        self.refresh_snapshot(true).await?;
        Ok(())
    }

    fn default_launch_command_for_pane(&self, pane: &tmux::Pane) -> String {
        match self.pane_insight(pane).workload {
            WorkloadKind::Codex => String::from("codex"),
            WorkloadKind::ClaudeCode => String::from("claude"),
            WorkloadKind::Opencode => String::from("opencode"),
            WorkloadKind::Aider => String::from("aider"),
            WorkloadKind::Gemini => String::from("gemini"),
            WorkloadKind::Agent | WorkloadKind::Job | WorkloadKind::Shell => String::new(),
        }
    }

    pub fn has_pending_dispatch(&self) -> bool {
        self.pending_dispatch.is_some()
    }

    pub fn cancel_pending_dispatch(&mut self) -> bool {
        let Some(staged) = self.pending_dispatch.take() else {
            return false;
        };

        self.status_message = format!(
            "Canceled {}.",
            send_target_phrase(&staged.target_description)
        );
        true
    }

    pub async fn confirm_pending_dispatch(&mut self) -> Result<()> {
        let Some(staged) = self.pending_dispatch.take() else {
            return Ok(());
        };

        let outcome = self.send_expanded_text(&staged.expanded).await?;
        if outcome.sent_count == 0 {
            self.status_message = no_target_panes_remain_message(
                &format!("`{}`", staged.text),
                outcome.disappeared_count,
            );
            return Ok(());
        }

        let command_save_failure = if staged.remember {
            self.remember_command(&staged.text);
            (!self.save_command_state()).then(|| self.status_message.clone())
        } else {
            None
        };
        let sent_message = if outcome.disappeared_count > 0 {
            format!(
                "Sent `{}` to {}; {} disappeared.",
                staged.text,
                pane_count_label(outcome.sent_count),
                pane_count_label(outcome.disappeared_count)
            )
        } else {
            format!(
                "Sent `{}` to {}.",
                staged.text,
                send_target_object_phrase(&staged.target_description)
            )
        };
        self.status_message = if let Some(failure) = command_save_failure {
            format!("{sent_message} {failure}")
        } else {
            sent_message
        };
        Ok(())
    }

    async fn send_expanded_text(
        &mut self,
        expanded: &[(String, String)],
    ) -> Result<DispatchOutcome> {
        let live_pane_ids = self
            .snapshot
            .panes
            .iter()
            .map(|pane| pane.id.as_str())
            .collect::<HashSet<_>>();
        let mut sent_count = 0;
        let mut disappeared_count = 0;
        let mut disappeared_after_snapshot = false;

        for (pane_id, expanded_text) in expanded {
            if !live_pane_ids.contains(pane_id.as_str()) {
                disappeared_count += 1;
                continue;
            }

            if let Err(error) = tmux::send_text(self.target(), pane_id, expanded_text, true).await {
                if tmux_error_indicates_target_unavailable(&error) {
                    disappeared_count += 1;
                    disappeared_after_snapshot = true;
                    continue;
                }
                return Err(error);
            }
            sent_count += 1;
        }

        if disappeared_after_snapshot {
            self.refresh_snapshot(false).await?;
        }

        Ok(DispatchOutcome {
            sent_count,
            disappeared_count,
        })
    }

    async fn send_keys_to_pane_ids(
        &mut self,
        pane_ids: &[String],
        keys: &[&str],
    ) -> Result<DispatchOutcome> {
        let live_pane_ids = self
            .snapshot
            .panes
            .iter()
            .map(|pane| pane.id.as_str())
            .collect::<HashSet<_>>();
        let mut sent_count = 0;
        let mut disappeared_count = 0;
        let mut disappeared_after_snapshot = false;

        for pane_id in pane_ids {
            if !live_pane_ids.contains(pane_id.as_str()) {
                disappeared_count += 1;
                continue;
            }

            if let Err(error) = tmux::send_keys(self.target(), pane_id, keys).await {
                if tmux_error_indicates_target_unavailable(&error) {
                    disappeared_count += 1;
                    disappeared_after_snapshot = true;
                    continue;
                }
                return Err(error);
            }
            sent_count += 1;
        }

        if disappeared_after_snapshot {
            self.refresh_snapshot(false).await?;
        }

        Ok(DispatchOutcome {
            sent_count,
            disappeared_count,
        })
    }

    async fn recover_missing_pane_action(
        &mut self,
        pane: &tmux::Pane,
        error: Error,
    ) -> Result<bool> {
        if !tmux_error_indicates_target_unavailable(&error) {
            return Err(error);
        }

        self.refresh_snapshot(false).await?;
        self.status_message = format!(
            "{} disappeared. Refreshed panes.",
            self.pane_target_label(pane)
        );
        Ok(true)
    }

    async fn recover_missing_pane_result(
        &mut self,
        pane: &tmux::Pane,
        result: Result<()>,
    ) -> Result<bool> {
        match result {
            Ok(()) => Ok(false),
            Err(error) => self.recover_missing_pane_action(pane, error).await,
        }
    }

    pub fn begin_macro_assign(&mut self) {
        let Some(command) = self.recent_commands.front() else {
            self.status_message = String::from("No recent command to pin.");
            return;
        };

        self.search_input_active = false;
        self.command_input_active = false;
        self.action_menu_active = false;
        self.group_input_active = false;
        self.launch_input_active = false;
        self.fleet_picker_active = false;
        self.macro_assign_active = true;
        let slot_labels = (0..MACRO_SLOT_COUNT)
            .map(|slot| self.ui_settings.keybindings.macro_slot_label(slot))
            .collect::<Vec<_>>()
            .join(", ");
        self.status_message =
            format!("Pin `{command}` to a macro slot ({slot_labels}). Esc cancels.");
    }

    pub fn open_action_menu(&mut self) {
        if self.pending_dispatch.is_some() {
            return;
        }

        self.search_input_active = false;
        self.command_input_active = false;
        self.macro_assign_active = false;
        self.group_input_active = false;
        self.launch_input_active = false;
        self.fleet_picker_active = false;
        self.help_overlay_active = false;
        self.action_menu_active = true;
        self.status_message.clear();
    }

    pub fn is_action_menu_active(&self) -> bool {
        self.action_menu_active
    }

    pub fn close_action_menu(&mut self) -> bool {
        if !self.action_menu_active {
            return false;
        }

        self.dismiss_action_menu();
        self.status_message = String::from("More closed.");
        true
    }

    pub fn dismiss_action_menu(&mut self) -> bool {
        if !self.action_menu_active {
            return false;
        }

        self.action_menu_active = false;
        true
    }

    pub fn begin_group_save_input(&mut self) {
        if self.marked_pane_ids.is_empty() {
            self.status_message = String::from("Build a send list first before saving a fleet.");
            return;
        }

        self.search_input_active = false;
        self.command_input_active = false;
        self.macro_assign_active = false;
        self.action_menu_active = false;
        self.launch_input_active = false;
        self.fleet_picker_active = false;
        self.group_input_active = true;
        self.group_name_buffer.clear();
        self.status_message = String::from("Name this fleet. Enter saves, Esc cancels.");
    }

    pub fn is_group_input_active(&self) -> bool {
        self.group_input_active
    }

    pub fn push_group_name_char(&mut self, ch: char) {
        if !self.group_input_active || ch.is_control() {
            return;
        }

        self.group_name_buffer.push(ch);
    }

    pub fn pop_group_name_char(&mut self) {
        if !self.group_input_active {
            return;
        }

        self.group_name_buffer.pop();
    }

    pub fn cancel_group_input(&mut self) -> bool {
        if !self.group_input_active {
            return false;
        }

        self.group_input_active = false;
        self.group_name_buffer.clear();
        self.status_message = String::from("Closed fleet naming.");
        true
    }

    pub fn submit_group_input(&mut self) {
        if !self.group_input_active {
            return;
        }

        let name = self.group_name_buffer.trim().to_owned();
        if name.is_empty() {
            self.status_message = String::from("Fleet name is empty.");
            return;
        }

        let mut members = self
            .marked_target_panes()
            .into_iter()
            .map(PaneLocator::from_pane)
            .collect::<Vec<_>>();
        members.sort_by(|left, right| {
            left.session_name
                .cmp(&right.session_name)
                .then_with(|| left.window_name.cmp(&right.window_name))
                .then_with(|| left.pane_index.cmp(&right.pane_index))
        });

        let target_group = TargetGroup {
            name: name.clone(),
            members,
        };
        let saved = self.upsert_target_group(target_group);
        let save_failure = (!saved).then(|| self.status_message.clone());
        self.group_input_active = false;
        self.group_name_buffer.clear();
        let saved_message = format!(
            "Saved fleet `{}` with {}.",
            name,
            pane_count_label(self.marked_pane_ids.len())
        );
        self.status_message = if let Some(failure) = save_failure {
            format!("{saved_message} {failure}")
        } else {
            saved_message
        };
    }

    pub fn load_next_target_group(&mut self) {
        if self.target_groups.is_empty() {
            self.status_message = String::from("No saved fleets.");
            return;
        }

        let next_index = self
            .selected_group_index
            .map(|index| (index + 1) % self.target_groups.len())
            .unwrap_or(0);
        self.apply_target_group(next_index);
    }

    pub fn open_fleet_picker(&mut self) {
        if self.target_groups.is_empty() {
            self.status_message = String::from("No saved fleets.");
            return;
        }

        self.search_input_active = false;
        self.command_input_active = false;
        self.macro_assign_active = false;
        self.action_menu_active = false;
        self.group_input_active = false;
        self.launch_input_active = false;
        self.fleet_picker_index = self
            .active_group_name
            .as_ref()
            .and_then(|name| {
                self.target_groups
                    .iter()
                    .position(|group| group.name == *name)
            })
            .or(self.selected_group_index)
            .unwrap_or(0)
            .min(self.target_groups.len().saturating_sub(1));
        self.fleet_picker_active = true;
        self.status_message.clear();
    }

    pub fn is_fleet_picker_active(&self) -> bool {
        self.fleet_picker_active
    }

    pub fn select_next_fleet(&mut self) {
        if !self.fleet_picker_active {
            return;
        }
        if self.target_groups.is_empty() {
            self.fleet_picker_active = false;
            self.fleet_picker_index = 0;
            self.status_message = String::from("No saved fleets.");
            return;
        }

        self.fleet_picker_index = (self.fleet_picker_index + 1) % self.target_groups.len();
        self.status_message.clear();
    }

    pub fn select_previous_fleet(&mut self) {
        if !self.fleet_picker_active {
            return;
        }
        if self.target_groups.is_empty() {
            self.fleet_picker_active = false;
            self.fleet_picker_index = 0;
            self.status_message = String::from("No saved fleets.");
            return;
        }

        self.fleet_picker_index = if self.fleet_picker_index == 0 {
            self.target_groups.len() - 1
        } else {
            self.fleet_picker_index - 1
        };
        self.status_message.clear();
    }

    pub fn submit_fleet_picker(&mut self) {
        if !self.fleet_picker_active {
            return;
        }
        if self.target_groups.is_empty() {
            self.fleet_picker_active = false;
            self.fleet_picker_index = 0;
            self.status_message = String::from("No saved fleets.");
            return;
        }

        let index = self.fleet_picker_index.min(self.target_groups.len() - 1);
        let group = &self.target_groups[index];
        let live_count = group
            .members
            .iter()
            .filter(|locator| {
                self.snapshot.panes.iter().any(|pane| {
                    pane.session_name == locator.session_name
                        && pane.window_name == locator.window_name
                        && pane.pane_index == locator.pane_index
                })
            })
            .count();
        if live_count == 0 {
            self.status_message = format!("Fleet `{}` has no live panes.", group.name);
            return;
        }

        self.fleet_picker_active = false;
        self.apply_target_group(index);
    }

    pub fn delete_fleet_picker_selection(&mut self) {
        if !self.fleet_picker_active {
            return;
        }
        if self.target_groups.is_empty() {
            self.fleet_picker_active = false;
            self.fleet_picker_index = 0;
            self.status_message = String::from("No saved fleets.");
            return;
        }

        let index = self.fleet_picker_index.min(self.target_groups.len() - 1);
        let removed = self.target_groups.remove(index);
        let saved = self.save_target_groups();
        let save_failure = (!saved).then(|| self.status_message.clone());
        if self.active_group_name.as_deref() == Some(removed.name.as_str()) {
            self.active_group_name = None;
            self.selected_group_index = None;
        } else if let Some(active_name) = &self.active_group_name {
            self.selected_group_index = self
                .target_groups
                .iter()
                .position(|group| group.name == *active_name);
        }

        if self.target_groups.is_empty() {
            self.fleet_picker_active = false;
            self.fleet_picker_index = 0;
        } else {
            self.fleet_picker_index = index.min(self.target_groups.len() - 1);
        }
        let deleted_message = format!("Deleted fleet `{}`.", removed.name);
        self.status_message = if let Some(failure) = save_failure {
            format!("{deleted_message} {failure}")
        } else {
            deleted_message
        };
    }

    pub fn close_fleet_picker(&mut self) -> bool {
        if !self.fleet_picker_active {
            return false;
        }

        self.fleet_picker_active = false;
        self.status_message = String::from("Closed Fleets.");
        true
    }

    pub fn open_theme_picker(&mut self, first_run: bool) {
        self.search_input_active = false;
        self.command_input_active = false;
        self.macro_assign_active = false;
        self.action_menu_active = false;
        self.help_overlay_active = false;
        self.group_input_active = false;
        self.launch_input_active = false;
        self.fleet_picker_active = false;
        self.pending_dispatch = None;
        self.theme_picker_active = true;
        self.theme_picker_page = ThemePickerPage::Top;
        self.theme_picker_index = self.theme_picker_default_index(first_run);
        self.theme_picker_first_run = first_run;
        if first_run {
            self.ui_settings.theme.preset = Some(SYSTEM_THEME);
        }
    }

    pub fn is_theme_picker_active(&self) -> bool {
        self.theme_picker_active
    }

    fn theme_picker_default_index(&self, first_run: bool) -> usize {
        if first_run {
            return 0;
        }
        match self.ui_settings.active_theme_preset() {
            ThemePreset::TerminalNative => 0,
            ThemePreset::CatppuccinLatte => 1,
            ThemePreset::CatppuccinMocha => 2,
            _ => 3,
        }
    }

    fn theme_picker_options(&self) -> &'static [ThemePickerOption] {
        match self.theme_picker_page {
            ThemePickerPage::Top => &THEME_PICKER_TOP_OPTIONS,
            ThemePickerPage::More => &THEME_PICKER_MORE_OPTIONS,
        }
    }

    pub fn select_next_theme_option(&mut self) {
        if !self.theme_picker_active {
            return;
        }
        let len = self.theme_picker_options().len();
        if len > 0 {
            self.theme_picker_index = (self.theme_picker_index + 1) % len;
        }
        self.status_message.clear();
    }

    pub fn select_previous_theme_option(&mut self) {
        if !self.theme_picker_active {
            return;
        }
        let len = self.theme_picker_options().len();
        if len > 0 {
            self.theme_picker_index = if self.theme_picker_index == 0 {
                len - 1
            } else {
                self.theme_picker_index - 1
            };
        }
        self.status_message.clear();
    }

    pub fn submit_theme_picker(&mut self) {
        if !self.theme_picker_active {
            return;
        }

        let option = self.theme_picker_options()[self
            .theme_picker_index
            .min(self.theme_picker_options().len().saturating_sub(1))];
        if let Some(page) = option.next_page {
            self.theme_picker_page = page;
            self.theme_picker_index = 0;
            self.status_message.clear();
            return;
        }
        if let Some(preset) = option.preset {
            self.apply_theme_choice(preset, option.label);
        }
    }

    pub fn cancel_theme_picker(&mut self) -> bool {
        if !self.theme_picker_active {
            return false;
        }
        if self.theme_picker_page == ThemePickerPage::More {
            self.theme_picker_page = ThemePickerPage::Top;
            self.theme_picker_index = self.theme_picker_default_index(self.theme_picker_first_run);
            self.status_message.clear();
            return true;
        }
        if self.theme_picker_first_run {
            self.apply_theme_choice(SYSTEM_THEME, "System Colors");
        } else {
            self.theme_picker_active = false;
            self.status_message = String::from("Kept current theme.");
        }
        true
    }

    pub fn quit_from_theme_picker(&mut self) {
        if self.theme_picker_active {
            self.request_quit();
        }
    }

    fn apply_theme_choice(&mut self, preset: ThemePreset, label: &str) {
        self.ui_settings.theme_preset = ThemePreset::default();
        self.ui_settings.theme.preset = Some(preset);
        self.theme_picker_active = false;
        self.theme_picker_page = ThemePickerPage::Top;
        self.theme_picker_index = self.theme_picker_default_index(false);
        self.theme_picker_first_run = false;
        if self.save_ui_settings() {
            self.status_message = format!("Theme: {label}.");
        }
    }

    pub fn delete_selected_target_group(&mut self) {
        let Some(index) = self.selected_group_index else {
            self.status_message = if self.target_groups.is_empty() {
                String::from("No saved fleets.")
            } else {
                String::from("Load a fleet before deleting it.")
            };
            return;
        };

        let removed = self.target_groups.remove(index);
        let saved = self.save_target_groups();
        let save_failure = (!saved).then(|| self.status_message.clone());
        self.selected_group_index = if self.target_groups.is_empty() {
            None
        } else {
            Some(index % self.target_groups.len())
        };
        if self.active_group_name.as_deref() == Some(removed.name.as_str()) {
            self.active_group_name = None;
        }
        let deleted_message = format!("Deleted fleet `{}`.", removed.name);
        self.status_message = if let Some(failure) = save_failure {
            format!("{deleted_message} {failure}")
        } else {
            deleted_message
        };
    }

    pub fn is_macro_assign_active(&self) -> bool {
        self.macro_assign_active
    }

    pub fn cancel_macro_assign(&mut self) -> bool {
        if !self.macro_assign_active {
            return false;
        }

        self.macro_assign_active = false;
        self.status_message = String::from("Closed macro pin mode.");
        true
    }

    pub fn assign_recent_command_to_slot(&mut self, slot: usize) {
        let Some(command) = self.recent_commands.front().cloned() else {
            self.status_message = String::from("No recent command to pin.");
            self.macro_assign_active = false;
            return;
        };
        let Some(slot_ref) = self.macro_slots.get_mut(slot) else {
            self.status_message = String::from("Invalid macro slot.");
            self.macro_assign_active = false;
            return;
        };

        *slot_ref = Some(command.clone());
        self.macro_assign_active = false;
        self.context_pane = ContextPane::Targets;
        self.panel_focus = PanelFocus::Details;
        self.details_scroll = 0;
        let saved = self.save_command_state();
        let pinned_message = format!("Pinned `{command}` to slot {}.", slot + 1);
        self.status_message = if saved {
            pinned_message
        } else {
            format!("{pinned_message} {}", self.status_message)
        };
    }

    pub fn macro_slot_for_key_token(&self, token: &str) -> Option<usize> {
        (0..MACRO_SLOT_COUNT).find(|slot| {
            self.ui_settings
                .keybindings
                .macro_slot_binding(*slot)
                .iter()
                .any(|binding| binding == token)
        })
    }

    pub async fn run_macro_slot(&mut self, slot: usize) -> Result<()> {
        let Some(command) = self
            .macro_slots
            .get(slot)
            .and_then(|command| command.clone())
        else {
            self.status_message = format!("Macro slot {} is empty.", slot + 1);
            return Ok(());
        };

        self.send_command_text(&command).await
    }

    pub async fn repeat_last_command(&mut self) -> Result<()> {
        let Some(command) = self.recent_commands.front().cloned() else {
            self.status_message = String::from("No recent command to replay.");
            return Ok(());
        };

        self.send_command_text(&command).await
    }

    pub fn toggle_selected_mark(&mut self) {
        if self.reject_hidden_selected_pane_action("changing the send list") {
            return;
        }

        let Some(pane) = self.selected_pane().cloned() else {
            self.status_message = String::from("Select a pane first.");
            return;
        };

        if self.marked_pane_ids.insert(pane.id.clone()) {
            self.active_group_name = None;
            self.fanout_mode = FanoutMode::Off;
            self.status_message =
                format!("Added {} to the send list.", self.pane_target_label(&pane));
        } else {
            self.marked_pane_ids.remove(&pane.id);
            self.active_group_name = None;
            self.status_message = format!(
                "Removed {} from the send list.",
                self.pane_target_label(&pane)
            );
        }
    }

    pub fn clear_marked_panes(&mut self) {
        let cleared = self.marked_pane_ids.len();
        if cleared == 0 {
            self.status_message = String::from("The send list is already clear.");
            return;
        }

        self.marked_pane_ids.clear();
        self.active_group_name = None;
        self.status_message = format!("Cleared {} from the send list.", pane_count_label(cleared));
    }

    pub fn toggle_fanout_mode(&mut self) {
        match self.fanout_mode {
            FanoutMode::Off => {
                if self.reject_hidden_selected_pane_action("sending a lane") {
                    return;
                }

                let Some(workload) = self.selected_lane_workload() else {
                    self.status_message =
                        String::from("Select an agent pane before sending a lane.");
                    return;
                };

                self.fanout_mode = FanoutMode::Lane;
                self.status_message =
                    format!("Lane send enabled for {}.", workload.display_label());
            }
            FanoutMode::Lane => {
                self.fanout_mode = FanoutMode::Off;
                self.status_message = String::from("Lane send disabled.");
            }
        }
    }

    pub async fn request_target_summaries(&mut self) -> Result<()> {
        let prompt =
            "Reply in exactly one line as: STATUS=<status> | BLOCKER=<blocker> | NEXT=<next>.";
        let target_count = self.active_target_panes().len();
        if target_count == 0 {
            self.status_message = self.no_summary_targets_message();
            return Ok(());
        }

        let target_scope = self.summary_target_scope();
        match self.dispatch_command_text(prompt, false, false).await? {
            CommandDispatchStatus::Dispatched(outcome) if outcome.sent_count == 0 => {
                self.status_message = self.no_summary_targets_remain_message(outcome);
            }
            CommandDispatchStatus::Dispatched(outcome) if outcome.disappeared_count > 0 => {
                self.status_message = format!(
                    "Asked {} for {}: {}; {} disappeared.",
                    pane_count_label(outcome.sent_count),
                    one_line_summary_label(outcome.sent_count),
                    target_scope,
                    pane_count_label(outcome.disappeared_count)
                );
            }
            CommandDispatchStatus::Dispatched(_) => {
                self.status_message = format!(
                    "Asked {} for {}: {}.",
                    pane_count_label(target_count),
                    one_line_summary_label(target_count),
                    target_scope
                );
            }
            CommandDispatchStatus::NoTargets => {
                self.status_message = self.no_summary_targets_message();
            }
            CommandDispatchStatus::Staged => {}
        }
        Ok(())
    }

    fn no_summary_targets_message(&self) -> String {
        if let Some(name) = &self.active_group_name {
            format!("Fleet `{name}` has no live panes to summarize.")
        } else {
            String::from("No panes available to summarize.")
        }
    }

    fn no_summary_targets_remain_message(&self, outcome: DispatchOutcome) -> String {
        if let Some(name) = &self.active_group_name {
            let mut parts = vec![format!("Fleet `{name}`: no panes remain for summaries")];
            if outcome.disappeared_count > 0 {
                parts.push(format!(
                    "{} disappeared",
                    pane_count_label(outcome.disappeared_count)
                ));
            }
            format!("{}.", parts.join("; "))
        } else {
            no_target_panes_remain_message("summaries", outcome.disappeared_count)
        }
    }

    pub fn toggle_metrics_mode(&mut self) {
        self.metrics_mode = self.metrics_mode.toggle();

        match self.metrics_mode {
            MetricsMode::Off => {
                self.pane_metrics.clear();
                self.last_metrics_refresh = None;
                self.status_message = String::from("Pane CPU/memory hidden.");
            }
            MetricsMode::Local => {
                self.status_message =
                    String::from("Pane CPU/memory shown for local tmux pane PIDs.");
            }
        }
    }

    pub fn cycle_layout_preset(&mut self) {
        self.ui_settings.layout_preset = self.ui_settings.layout_preset.next();
        if self.save_ui_settings() {
            self.status_message = format!(
                "Layout: {}.",
                self.ui_settings.layout_preset.display_label()
            );
        }
    }

    pub fn toggle_bell_notifications(&mut self) {
        self.notification_settings.bell_enabled = !self.notification_settings.bell_enabled;
        if self.save_notification_settings() {
            self.status_message = format!(
                "Terminal bell {}.",
                if self.notification_settings.bell_enabled {
                    "on"
                } else {
                    "off"
                }
            );
        }
    }

    pub fn toggle_desktop_notifications(&mut self) {
        self.notification_settings.desktop_enabled = !self.notification_settings.desktop_enabled;
        if self.save_notification_settings() {
            self.status_message = self.desktop_notification_status_message();
        }
    }

    pub fn cycle_alert_policy(&mut self) {
        self.notification_settings.alert_policy = self.notification_settings.alert_policy.next();
        if self.save_notification_settings() {
            self.status_message = format!(
                "Alerts: {}.",
                self.notification_settings.alert_policy.display_label()
            );
        }
    }

    pub fn cycle_alert_debounce(&mut self) {
        self.notification_settings.debounce_seconds =
            next_debounce_seconds(self.notification_settings.debounce_seconds);
        if self.save_notification_settings() {
            self.status_message = format!(
                "Alert repeat delay: {}.",
                format_debounce(self.notification_settings.debounce_seconds)
            );
        }
    }

    pub async fn focus_selected_pane(&mut self) -> Result<()> {
        if self.context_pane == ContextPane::Navigator {
            self.drill_into_selected_window();
            return Ok(());
        }

        if self.reject_hidden_selected_pane_action("opening output") {
            return Ok(());
        }

        let Some(pane) = self.selected_pane().cloned() else {
            self.status_message = String::from("Select a pane first.");
            return Ok(());
        };

        if self.context_pane == ContextPane::Tail {
            self.status_message.clear();
        } else {
            self.context_pane = ContextPane::Tail;
            self.details_scroll = 0;
            self.panel_focus = if self.details_scrollable_output_line_count() > 0 {
                PanelFocus::Details
            } else {
                PanelFocus::Fleet
            };
            self.status_message = format!("Showing output for {}.", self.pane_target_label(&pane));
        }
        self.mark_agent_bridge_review_seen(&pane).await;
        Ok(())
    }

    pub(crate) async fn perform_command_center_primary_action(
        &mut self,
        trigger: CommandCenterPrimaryTrigger,
    ) -> Result<bool> {
        if self.context_pane != ContextPane::Control {
            return Ok(false);
        }

        let Some(action) = self.command_center_primary_attention_action() else {
            return Ok(false);
        };

        match (trigger, action.kind) {
            (CommandCenterPrimaryTrigger::Smart, CommandCenterPrimaryActionKind::Continue) => {
                self.select_command_center_action_pane(&action.pane);
                let outcome = self
                    .send_keys_to_pane_ids(std::slice::from_ref(&action.pane.id), &["Enter"])
                    .await?;
                if outcome.sent_count == 0 {
                    self.status_message =
                        no_target_panes_remain_message("Enter", outcome.disappeared_count);
                } else {
                    self.mark_attention_action_pending(
                        &action.pane,
                        PendingAttentionActionKind::Continue,
                    );
                    self.select_next_attention_after_action(&action.pane);
                    self.status_message =
                        self.attention_action_status_after_send("Sent Enter to", &action.pane);
                }
                Ok(true)
            }
            (CommandCenterPrimaryTrigger::Focus, CommandCenterPrimaryActionKind::Output) => {
                self.select_command_center_action_pane(&action.pane);
                self.focus_selected_pane().await?;
                Ok(true)
            }
            (CommandCenterPrimaryTrigger::Command, CommandCenterPrimaryActionKind::Reply) => {
                self.select_command_center_action_pane(&action.pane);
                self.begin_command_input();
                Ok(true)
            }
            (CommandCenterPrimaryTrigger::Actions, CommandCenterPrimaryActionKind::Answer) => {
                self.select_command_center_action_pane(&action.pane);
                self.open_action_menu();
                Ok(true)
            }
            (CommandCenterPrimaryTrigger::Jump, CommandCenterPrimaryActionKind::ShowWaiting) => {
                self.select_command_center_action_pane(&action.pane);
                self.jump_to_pane(&action.pane).await?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub async fn jump_to_selected_pane(&mut self) -> Result<()> {
        if self.context_pane == ContextPane::Navigator {
            return self.jump_to_selected_window().await;
        }

        if self.reject_hidden_selected_pane_action("showing a pane in tmux") {
            return Ok(());
        }

        let Some(pane) = self.selected_pane().cloned() else {
            self.status_message = String::from("Select a pane first.");
            return Ok(());
        };

        self.jump_to_pane(&pane).await
    }

    fn command_center_primary_attention_action(&self) -> Option<CommandCenterPrimaryAction> {
        let (pane, insight) = self.attention_queue().into_iter().next()?;
        let kind = match self.recommended_action_summary(pane, insight).as_str() {
            "continue" => CommandCenterPrimaryActionKind::Continue,
            "show output" => CommandCenterPrimaryActionKind::Output,
            "answer" if self.pane_has_choice_prompt(pane, insight) => {
                CommandCenterPrimaryActionKind::Answer
            }
            "show prompt" if self.command_center_can_reply_to_pane(pane, insight) => {
                CommandCenterPrimaryActionKind::Reply
            }
            "answer" | "show prompt" => CommandCenterPrimaryActionKind::ShowWaiting,
            _ => CommandCenterPrimaryActionKind::Output,
        };

        Some(CommandCenterPrimaryAction {
            pane: pane.clone(),
            kind,
        })
    }

    fn select_command_center_action_pane(&mut self, pane: &tmux::Pane) {
        self.selected_pane_id = Some(pane.id.clone());
        self.sync_selected_window_from_selection();
        self.details_scroll = 0;
    }

    fn drill_into_selected_window(&mut self) {
        self.ensure_selected_window_visible();

        let Some(window) = self.selected_window().cloned() else {
            self.status_message = String::from("No window selected in Browse.");
            return;
        };

        self.view_scope = ViewScope::Window {
            id: window.id.clone(),
            name: format!("{}/{}", window.session_name, window.name),
        };
        self.ensure_selection();
        self.sync_selected_window_from_selection();
        self.status_message = format!("Showing {} only. Backspace show all.", window.name);
    }

    pub async fn toggle_selected_zoom(&mut self) -> Result<()> {
        if self.reject_hidden_selected_pane_action("zooming a pane") {
            return Ok(());
        }

        let Some(pane) = self.selected_pane().cloned() else {
            self.status_message = String::from("Select a pane first.");
            return Ok(());
        };

        let result = tmux::toggle_zoom(self.target(), &pane.id).await;
        if self.recover_missing_pane_result(&pane, result).await? {
            return Ok(());
        }
        self.status_message = format!("Toggled zoom for {}.", self.pane_target_label(&pane));
        Ok(())
    }

    pub async fn send_enter_to_selected(&mut self) -> Result<()> {
        self.send_keys_to_selected(&["Enter"], "Sent Enter").await
    }

    pub async fn send_yes_to_selected(&mut self) -> Result<()> {
        let command_center_choice_pane = self.command_center_choice_pane_for_pending_action();
        self.send_keys_to_selected(&["y", "Enter"], "Sent y + Enter")
            .await?;
        if let Some(pane) = command_center_choice_pane
            && self.status_message.starts_with("Sent y + Enter")
        {
            self.mark_attention_action_pending(&pane, PendingAttentionActionKind::AnswerYes);
            self.select_next_attention_after_action(&pane);
            self.status_message = self.attention_action_status_after_send("Answered yes in", &pane);
        }
        Ok(())
    }

    pub async fn send_no_to_selected(&mut self) -> Result<()> {
        let command_center_choice_pane = self.command_center_choice_pane_for_pending_action();
        self.send_keys_to_selected(&["n", "Enter"], "Sent n + Enter")
            .await?;
        if let Some(pane) = command_center_choice_pane
            && self.status_message.starts_with("Sent n + Enter")
        {
            self.mark_attention_action_pending(&pane, PendingAttentionActionKind::AnswerNo);
            self.select_next_attention_after_action(&pane);
            self.status_message = self.attention_action_status_after_send("Answered no in", &pane);
        }
        Ok(())
    }

    fn command_center_choice_pane_for_pending_action(&self) -> Option<tmux::Pane> {
        if self.context_pane != ContextPane::Control
            || !self.action_menu_active
            || self.using_explicit_targets()
            || self.fanout_mode != FanoutMode::Off
        {
            return None;
        }

        self.selected_pane().cloned().filter(|pane| {
            let insight = self.pane_insight(pane);
            self.pane_has_choice_prompt(pane, insight)
        })
    }

    pub async fn perform_smart_action(&mut self) -> Result<()> {
        if self.selected_pane_hidden_by_current_view()
            && (!self.using_explicit_targets() || self.visible_pane_indices().is_empty())
        {
            self.status_message = String::from("Show all panes before using Smart Action.");
            return Ok(());
        }

        let using_explicit_targets = self.using_explicit_targets();
        let using_marked_targets = self.using_marked_targets();
        let using_lane_fanout = self.fanout_mode == FanoutMode::Lane;

        if using_explicit_targets || using_lane_fanout {
            let targets = self.active_target_panes();
            let mut skipped = 0;
            let mut ready_target_ids = Vec::new();

            for target in targets {
                let insight = self.pane_insight(target);
                if self.recommended_smart_action(target, insight) == SmartAction::SendEnter {
                    ready_target_ids.push(target.id.clone());
                } else {
                    skipped += 1;
                }
            }

            if ready_target_ids.is_empty() {
                self.status_message = if let Some(name) = &self.active_group_name {
                    format!("Fleet `{name}` has no panes ready for Enter.")
                } else if using_marked_targets {
                    String::from("No send-list panes are ready for Enter.")
                } else {
                    String::from("No lane panes are ready for Enter.")
                };
                return Ok(());
            }

            let outcome = self
                .send_keys_to_pane_ids(&ready_target_ids, &["Enter"])
                .await?;

            self.status_message = if outcome.sent_count > 0 {
                if let Some(name) = &self.active_group_name {
                    let prefix = format!("Fleet `{name}`");
                    smart_action_status(&prefix, outcome, skipped)
                } else if using_marked_targets {
                    smart_action_send_list_status(outcome, skipped)
                } else {
                    smart_action_lane_status(outcome, skipped)
                }
            } else {
                if let Some(name) = &self.active_group_name {
                    let prefix = format!("Fleet `{name}`");
                    smart_action_empty_after_send_status(&prefix, outcome, skipped)
                } else if using_marked_targets {
                    smart_action_empty_after_send_status("Send list", outcome, skipped)
                } else {
                    smart_action_empty_after_send_status("Lane", outcome, skipped)
                }
            };
            return Ok(());
        }

        let Some(pane) = self.selected_pane().cloned() else {
            self.status_message = String::from("Select a pane first.");
            return Ok(());
        };

        let insight = self.pane_insight(&pane);
        let action = self.recommended_smart_action(&pane, insight);

        match action {
            SmartAction::SendEnter => {
                let outcome = self
                    .send_keys_to_pane_ids(std::slice::from_ref(&pane.id), &["Enter"])
                    .await?;
                if outcome.sent_count == 0 {
                    self.status_message =
                        no_target_panes_remain_message("Enter", outcome.disappeared_count);
                } else {
                    self.status_message =
                        format!("Sent Enter to {}.", self.pane_target_label(&pane));
                }
            }
            SmartAction::Focus => {
                let result = tmux::focus_pane(self.target(), &pane).await;
                if self.recover_missing_pane_result(&pane, result).await? {
                    return Ok(());
                }
                self.status_message = format!("Showing {} in tmux.", self.pane_target_label(&pane));
            }
        }

        Ok(())
    }

    pub fn acknowledge_selected_attention(&mut self) {
        let Some(pane) = self.selected_pane().cloned() else {
            self.status_message = String::from("Select a pane first.");
            return;
        };

        let insight = self.pane_insight(&pane);
        if !self.pane_requires_attention(&pane, insight.status) {
            self.status_message =
                format!("{} has no alert to mute.", self.pane_target_label(&pane));
            return;
        }

        self.acknowledged_attention
            .insert(AttentionKey::from_pane(&pane), insight.status);
        let saved = self.save_persistent_state();
        let muted_message = format!("Muted alert for {}.", self.pane_target_label(&pane));
        self.status_message = if saved {
            muted_message
        } else {
            format!("{muted_message} {}", self.status_message)
        };
        self.ensure_selection();
    }

    pub fn clear_selected_acknowledgement(&mut self) {
        let Some(pane) = self.selected_pane().cloned() else {
            self.status_message = String::from("Select a pane first.");
            return;
        };

        if self
            .acknowledged_attention
            .remove(&AttentionKey::from_pane(&pane))
            .is_some()
        {
            let saved = self.save_persistent_state();
            let unmuted_message = format!("Unmuted alert for {}.", self.pane_target_label(&pane));
            self.status_message = if saved {
                unmuted_message
            } else {
                format!("{unmuted_message} {}", self.status_message)
            };
        } else {
            self.status_message = format!("{} was not muted.", self.pane_target_label(&pane));
        }
    }

    pub fn acknowledge_all_attention(&mut self) {
        let mut added = 0;

        for pane in &self.snapshot.panes {
            let insight = self.pane_insight(pane);
            if self.pane_requires_attention(pane, insight.status)
                && !self.is_acknowledged(pane, insight.status)
            {
                self.acknowledged_attention
                    .insert(AttentionKey::from_pane(pane), insight.status);
                added += 1;
            }
        }

        if added == 0 {
            self.status_message = String::from("No new alerts to mute.");
            return;
        }

        let saved = self.save_persistent_state();
        self.ensure_selection();
        let muted_message = format!("Muted {}.", alert_count_label(added));
        self.status_message = if saved {
            muted_message
        } else {
            format!("{muted_message} {}", self.status_message)
        };
    }

    pub fn clear_all_acknowledgements(&mut self) {
        let cleared = self.acknowledged_attention.len();
        if cleared == 0 {
            self.status_message = String::from("No muted alerts to clear.");
            return;
        }

        self.acknowledged_attention.clear();
        let saved = self.save_persistent_state();
        self.ensure_selection();
        let unmuted_message = format!("Unmuted {}.", alert_count_label(cleared));
        self.status_message = if saved {
            unmuted_message
        } else {
            format!("{unmuted_message} {}", self.status_message)
        };
    }

    pub async fn send_enter_to_attention_queue(&mut self) -> Result<()> {
        let pending_panes = self
            .attention_queue()
            .into_iter()
            .filter(|(pane, insight)| {
                self.recommended_smart_action(pane, *insight) == SmartAction::SendEnter
            })
            .map(|(pane, _)| pane.clone())
            .collect::<Vec<_>>();
        if pending_panes.is_empty() {
            self.status_message = String::from("No waiting panes are ready for Enter.");
            return Ok(());
        }

        let pane_ids = pending_panes
            .iter()
            .map(|pane| pane.id.clone())
            .collect::<Vec<_>>();
        let outcome = self.send_keys_to_pane_ids(&pane_ids, &["Enter"]).await?;

        self.status_message = if outcome.sent_count == 0 {
            no_waiting_panes_remain_message(outcome.disappeared_count)
        } else if outcome.disappeared_count > 0 {
            format!(
                "Sent Enter to {}; {} disappeared.",
                waiting_pane_count_label(outcome.sent_count),
                pane_count_label(outcome.disappeared_count)
            )
        } else {
            format!(
                "Sent Enter to {}.",
                waiting_pane_count_label(pane_ids.len())
            )
        };
        if outcome.sent_count > 0 {
            for pane in &pending_panes {
                self.mark_attention_action_pending(pane, PendingAttentionActionKind::BulkContinue);
            }
            if let Some(pane) = pending_panes.first() {
                self.select_next_attention_after_action(pane);
            }
            if outcome.disappeared_count == 0 {
                self.status_message = format!(
                    "{} Watching for updates.",
                    self.status_message.trim_end_matches('.')
                );
            }
        }
        Ok(())
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn status_message(&self) -> &str {
        &self.status_message
    }

    pub(crate) fn report_action_error(&mut self, error: &Error) {
        self.status_message = action_error_status_message(error);
    }

    #[cfg(test)]
    pub(crate) fn set_status_message_for_test(&mut self, message: impl Into<String>) {
        self.status_message = message.into();
    }

    #[cfg(test)]
    pub(crate) fn set_search_query_for_test(&mut self, query: impl Into<String>) {
        self.search_query = query.into();
    }

    #[cfg(test)]
    pub(crate) fn set_selected_window_id_for_test(&mut self, window_id: Option<String>) {
        self.selected_window_id = window_id;
    }

    #[cfg(test)]
    pub(crate) fn set_selected_pane_id_for_test(&mut self, pane_id: Option<String>) {
        self.selected_pane_id = pane_id;
    }

    #[cfg(test)]
    pub(crate) fn set_target_groups_for_test(&mut self, target_groups: Vec<TargetGroup>) {
        self.target_groups = target_groups;
    }

    #[cfg(test)]
    pub(crate) fn set_layout_preset_for_test(&mut self, layout_preset: LayoutPreset) {
        self.ui_settings.layout_preset = layout_preset;
    }

    #[cfg(test)]
    pub(crate) fn set_theme_for_test(&mut self, theme: ThemeConfig) {
        self.ui_settings.theme = theme;
    }

    #[cfg(test)]
    pub(crate) fn set_close_after_jump_for_test(&mut self, enabled: bool) {
        self.close_after_jump = enabled;
    }

    pub fn take_pending_bell(&mut self) -> bool {
        let pending = self.pending_bell;
        self.pending_bell = false;
        pending
    }

    async fn refresh_snapshot(&mut self, force_runtime_capture: bool) -> Result<bool> {
        match tmux::snapshot(self.target().clone()).await {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                self.refresh_native_agent_sources(false);
                self.ensure_selection();
                self.prune_runtime();
                self.capture_runtime_from_snapshot(force_runtime_capture)
                    .await?;
                self.reconcile_pending_attention_actions();
                Ok(true)
            }
            Err(error) => {
                self.snapshot = tmux::Snapshot::default();
                self.prune_runtime();
                self.reconcile_pending_attention_actions();
                self.ensure_selection();
                self.sync_selected_window_from_selection();
                self.status_message = startup_snapshot_status_message(self.target(), &error);
                Ok(false)
            }
        }
    }

    fn refresh_native_agent_sources_if_due(&mut self) {
        if !self.native_agent_scanner.is_enabled() {
            return;
        }

        if self.last_native_agent_scan.is_some_and(|last| {
            Instant::now().duration_since(last) < NATIVE_AGENT_SOURCE_REFRESH_INTERVAL
        }) {
            return;
        }

        self.refresh_native_agent_sources(false);
    }

    fn refresh_native_agent_sources(&mut self, seed: bool) -> bool {
        let events = self.native_agent_scanner.scan();
        self.last_native_agent_scan = Some(Instant::now());
        self.apply_native_agent_source_events(events, seed)
    }

    fn apply_native_agent_source_events(
        &mut self,
        events: Vec<AgentSourceEvent>,
        seed: bool,
    ) -> bool {
        self.clear_native_agent_assignments();
        let previous_versions = self.native_agent_versions.clone();
        let assignments = native_agent_assignments_for_panes(&self.snapshot.panes, &events);

        for event in &events {
            self.native_agent_versions.insert(
                event.identity_key(),
                (event.status, event.updated_at_unix_ms),
            );
        }

        let mut changed = false;

        for (pane_index, event) in assignments {
            let pane_id = self.snapshot.panes[pane_index].id.clone();
            if native_agent_event_should_yield_to_runtime(
                &event,
                &self.snapshot.panes[pane_index],
                self.pane_runtime.get(&pane_id),
            ) {
                continue;
            }

            let unseen = native_agent_event_unseen(
                &previous_versions,
                &event,
                seed,
                event.status,
                event.updated_at_unix_ms,
            );
            let agent_event = native_agent_bridge_event(&event, unseen);
            self.snapshot.panes[pane_index].agent_event = Some(agent_event.clone());
            self.native_agent_assignments.insert(pane_id, agent_event);
            changed = true;
        }

        changed
    }

    fn clear_native_agent_assignments(&mut self) {
        if self.native_agent_assignments.is_empty() {
            return;
        }

        for pane in &mut self.snapshot.panes {
            if let Some(previous) = self.native_agent_assignments.get(&pane.id)
                && pane.agent_event.as_ref() == Some(previous)
            {
                pane.agent_event = None;
            }
        }

        self.native_agent_assignments.clear();
    }

    fn handle_event(&mut self, event: &tmux::control::Event) -> bool {
        if let Some((pane_id, _output)) = event.output_chunk() {
            self.dirty_pane_ids.insert(pane_id.to_owned());
        }

        match event {
            tmux::control::Event::Exit { reason } => {
                self.control_state = match reason {
                    Some(reason) => format!("disconnected: {reason}"),
                    None => String::from("disconnected"),
                };
                false
            }
            tmux::control::Event::WindowPaneChanged { window_id, pane_id } => {
                for pane in &mut self.snapshot.panes {
                    if pane.window_id == *window_id {
                        pane.active = pane.id == *pane_id;
                    }
                }
                false
            }
            tmux::control::Event::WindowRenamed { window_id, name } => {
                for window in &mut self.snapshot.windows {
                    if window.id == *window_id {
                        window.name = name.clone();
                    }
                }
                for pane in &mut self.snapshot.panes {
                    if pane.window_id == *window_id {
                        pane.window_name = name.clone();
                    }
                }
                false
            }
            tmux::control::Event::SessionRenamed { session_id, name } => {
                for session in &mut self.snapshot.sessions {
                    if session.id == *session_id {
                        session.name = name.clone();
                    }
                }
                for window in &mut self.snapshot.windows {
                    if window.session_id == *session_id {
                        window.session_name = name.clone();
                    }
                }
                for pane in &mut self.snapshot.panes {
                    if pane.session_id == *session_id {
                        pane.session_name = name.clone();
                    }
                }
                false
            }
            tmux::control::Event::SessionChanged { session_id, name } => {
                self.control_state = format!("connected to {session_id} ({name})");
                false
            }
            tmux::control::Event::ClientSessionChanged {
                client,
                session_id,
                name,
            } => {
                self.status_message = format!("Client {client} switched to {session_id} ({name}).");
                false
            }
            _ => event.is_structural(),
        }
    }

    fn pane_insight(&self, pane: &tmux::Pane) -> PaneInsight {
        let runtime = self.pane_runtime.get(&pane.id);
        let mut insight = infer_pane_insight(&ObservedPane::from(pane), runtime);
        if let Some(event) = &pane.agent_event {
            insight.workload = agent_bridge_workload(event);
            if let Some(status) = agent_bridge_status(event) {
                insight.status = status;
            }
        }
        insight
    }

    #[cfg(test)]
    fn append_output(&mut self, pane_id: &str, output: String, age_millis: Option<u64>) {
        if output.is_empty() {
            return;
        }

        let pane = self
            .snapshot
            .panes
            .iter()
            .find(|pane| pane.id == pane_id)
            .cloned();
        let runtime = self.pane_runtime.entry(pane_id.to_owned()).or_default();
        let latest_line = append_output_chunk(runtime, &output);

        if output.chars().any(|ch| !ch.is_whitespace()) {
            let now = Instant::now();
            runtime.last_output_at = Some(match age_millis {
                Some(age_millis) => now
                    .checked_sub(Duration::from_millis(age_millis))
                    .unwrap_or(now),
                None => now,
            });
        }

        while runtime.output.len() > MAX_OUTPUT_LINES {
            runtime.output.pop_front();
        }

        if let Some(pane) = &pane {
            runtime.corpus = build_runtime_corpus(&ObservedPane::from(pane), runtime);
        }

        if let Some(line) = latest_line.or_else(|| visible_partial_line(runtime).map(str::to_owned))
            && let Some(report) = parse_agent_report_line(&line)
        {
            self.pane_reports.insert(pane_id.to_owned(), report);
        }
        self.reconcile_pending_attention_actions();
        self.clamp_details_scroll_to_content();
    }

    fn rebuild_runtime_corpora(&mut self) {
        for pane in &self.snapshot.panes {
            let runtime = self.pane_runtime.entry(pane.id.clone()).or_default();
            runtime.corpus = build_runtime_corpus(&ObservedPane::from(pane), runtime);
        }
    }

    fn ensure_selection(&mut self) {
        let previous_selection = self.selected_pane_id.clone();
        let visible = self.visible_pane_indices();
        if let Some(selected_pane_id) = &self.selected_pane_id
            && visible
                .iter()
                .any(|index| self.snapshot.panes[*index].id == *selected_pane_id)
        {
            if self.context_pane == ContextPane::Navigator {
                self.ensure_selected_window_visible();
            }
            return;
        }

        self.selected_pane_id = visible
            .into_iter()
            .next()
            .map(|index| &self.snapshot.panes[index])
            .map(|pane| pane.id.clone());

        if self.selected_pane_id != previous_selection {
            self.details_scroll = 0;
        }
        self.sync_selected_window_from_selection();
    }

    fn ensure_selected_window_visible(&mut self) {
        let next_window_id = {
            let windows = self.window_navigation_targets();
            if windows.is_empty() {
                None
            } else if self
                .selected_window_id
                .as_deref()
                .is_some_and(|window_id| windows.iter().any(|window| window.id == window_id))
            {
                return;
            } else {
                windows.first().map(|window| window.id.clone())
            }
        };

        self.selected_window_id = next_window_id;
    }

    fn reset_selection_to_top_visible(&mut self) {
        self.selected_pane_id = None;
        self.selected_window_id = None;
        self.ensure_selection();
    }

    fn sync_selected_window_from_selection(&mut self) {
        if let Some(pane) = self.selected_pane() {
            self.selected_window_id = Some(pane.window_id.clone());
        } else {
            self.selected_window_id = self
                .window_navigation_targets()
                .first()
                .map(|window| window.id.clone());
        }
    }

    fn prune_runtime(&mut self) {
        self.dirty_pane_ids.retain(|pane_id| {
            self.snapshot
                .panes
                .iter()
                .any(|pane| pane.id.as_str() == pane_id.as_str())
        });
        self.pane_runtime.retain(|pane_id, _| {
            self.snapshot
                .panes
                .iter()
                .any(|pane| pane.id.as_str() == pane_id.as_str())
        });
        self.pane_last_status.retain(|pane_id, _| {
            self.snapshot
                .panes
                .iter()
                .any(|pane| pane.id.as_str() == pane_id.as_str())
        });
        self.last_alerted_at.retain(|pane_id, _| {
            self.snapshot
                .panes
                .iter()
                .any(|pane| pane.id.as_str() == pane_id.as_str())
        });
        self.pane_metrics.retain(|pane_id, _| {
            self.snapshot
                .panes
                .iter()
                .any(|pane| pane.id.as_str() == pane_id.as_str())
        });
        self.pane_reports.retain(|pane_id, _| {
            self.snapshot
                .panes
                .iter()
                .any(|pane| pane.id.as_str() == pane_id.as_str())
        });
        self.marked_pane_ids.retain(|pane_id| {
            self.snapshot
                .panes
                .iter()
                .any(|pane| pane.id.as_str() == pane_id.as_str())
        });
        self.acknowledged_attention.retain(|key, _| {
            self.snapshot
                .panes
                .iter()
                .any(|pane| AttentionKey::from_pane(pane) == *key)
        });
        self.reconcile_acknowledgements();
    }

    fn remove_disappeared_panes(&mut self, panes: &[tmux::Pane]) {
        if panes.is_empty() {
            return;
        }

        let pane_ids = panes
            .iter()
            .map(|pane| pane.id.as_str())
            .collect::<HashSet<_>>();
        self.snapshot
            .panes
            .retain(|pane| !pane_ids.contains(pane.id.as_str()));
        self.prune_runtime();
        self.ensure_selection();
        self.sync_selected_window_from_selection();

        self.status_message = if panes.len() == 1 {
            format!(
                "{} disappeared. Refreshed panes.",
                self.pane_target_label(&panes[0])
            )
        } else {
            format!(
                "{} disappeared. Refreshed panes.",
                pane_count_label(panes.len())
            )
        };
    }

    async fn capture_runtime_from_snapshot(&mut self, force: bool) -> Result<()> {
        let panes = self.snapshot.panes.clone();
        let mut disappeared_panes = Vec::new();

        for pane in panes {
            let needs_seed = self
                .pane_runtime
                .get(&pane.id)
                .is_none_or(|runtime| force || runtime.output.is_empty());

            if !needs_seed {
                continue;
            }

            match tmux::capture_pane_tail(self.target(), &pane, 24).await {
                Ok(lines) => {
                    let runtime = self.pane_runtime.entry(pane.id.clone()).or_default();
                    runtime.output = lines.into();
                    runtime.last_output_at = None;
                    runtime.partial_line.clear();
                    runtime.corpus = build_runtime_corpus(&ObservedPane::from(&pane), runtime);
                }
                Err(error) if tmux_error_indicates_server_unavailable(&error) => {
                    self.snapshot = tmux::Snapshot::default();
                    self.prune_runtime();
                    self.ensure_selection();
                    self.sync_selected_window_from_selection();
                    self.status_message = startup_snapshot_status_message(self.target(), &error);
                    return Ok(());
                }
                Err(error) if tmux_error_indicates_target_unavailable(&error) => {
                    disappeared_panes.push(pane);
                }
                Err(error) => {
                    self.status_message =
                        pane_capture_failed_message(&self.pane_target_label(&pane), &error);
                }
            }
        }

        self.remove_disappeared_panes(&disappeared_panes);
        self.rebuild_runtime_corpora();
        self.clamp_details_scroll_to_content();

        Ok(())
    }

    async fn capture_runtime_for_dirty_panes(&mut self) -> Result<()> {
        let dirty_pane_ids = self.take_dirty_pane_batch(DIRTY_CAPTURE_LIMIT_PER_TICK);
        let mut disappeared_panes = Vec::new();

        for pane_id in dirty_pane_ids {
            let pane = self
                .snapshot
                .panes
                .iter()
                .find(|pane| pane.id == pane_id)
                .cloned();

            let Some(pane) = pane else {
                continue;
            };

            match tmux::capture_pane_tail(self.target(), &pane, 24).await {
                Ok(lines) => {
                    let runtime = self.pane_runtime.entry(pane.id.clone()).or_default();
                    runtime.output = lines.into();
                    runtime.partial_line.clear();
                    runtime.last_output_at = Some(Instant::now());
                    runtime.corpus = build_runtime_corpus(&ObservedPane::from(&pane), runtime);

                    if let Some(line) = runtime
                        .output
                        .iter()
                        .rev()
                        .find(|line| !line.trim().is_empty())
                        && let Some(report) = parse_agent_report_line(line)
                    {
                        self.pane_reports.insert(pane.id.clone(), report);
                    }
                }
                Err(error) if tmux_error_indicates_server_unavailable(&error) => {
                    self.snapshot = tmux::Snapshot::default();
                    self.prune_runtime();
                    self.ensure_selection();
                    self.sync_selected_window_from_selection();
                    self.status_message = startup_snapshot_status_message(self.target(), &error);
                    return Ok(());
                }
                Err(error) if tmux_error_indicates_target_unavailable(&error) => {
                    disappeared_panes.push(pane);
                }
                Err(error) => {
                    self.status_message =
                        pane_capture_failed_message(&self.pane_target_label(&pane), &error);
                }
            }
        }

        self.remove_disappeared_panes(&disappeared_panes);
        self.clamp_details_scroll_to_content();
        Ok(())
    }

    fn take_dirty_pane_batch(&mut self, limit: usize) -> Vec<String> {
        if limit == 0 || self.dirty_pane_ids.is_empty() {
            return Vec::new();
        }

        let pane_ids = self
            .dirty_pane_ids
            .iter()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        for pane_id in &pane_ids {
            self.dirty_pane_ids.remove(pane_id);
        }

        pane_ids
    }

    fn selected_pane(&self) -> Option<&tmux::Pane> {
        let pane_id = self.selected_pane_id.as_deref()?;
        self.snapshot.panes.iter().find(|pane| pane.id == pane_id)
    }

    pub(crate) fn selected_pane_hidden_by_current_view(&self) -> bool {
        self.selected_pane()
            .is_some_and(|pane| !self.matches_pane_visibility(pane))
    }

    fn reject_hidden_selected_pane_action(&mut self, action: &str) -> bool {
        if self.selected_pane_hidden_by_current_view() {
            self.status_message = format!("Show all panes before {action}.");
            true
        } else {
            false
        }
    }

    fn selected_window(&self) -> Option<&tmux::Window> {
        let window_id = self.selected_window_id.as_deref()?;
        self.snapshot
            .windows
            .iter()
            .find(|window| window.id == window_id)
    }

    fn select_next_window(&mut self) {
        self.initial_attention_autofocus = false;
        let windows = self.window_navigation_targets();
        if windows.is_empty() {
            self.selected_window_id = None;
            return;
        }

        let next_window_id = if let Some(current_index) = self
            .selected_window_id
            .as_deref()
            .and_then(|window_id| windows.iter().position(|window| window.id == window_id))
        {
            let next_index = (current_index + 1) % windows.len();
            windows[next_index].id.clone()
        } else {
            windows[0].id.clone()
        };

        self.selected_window_id = Some(next_window_id);
        self.status_message.clear();
    }

    fn select_previous_window(&mut self) {
        self.initial_attention_autofocus = false;
        let windows = self.window_navigation_targets();
        if windows.is_empty() {
            self.selected_window_id = None;
            return;
        }

        let previous_window_id = if let Some(current_index) = self
            .selected_window_id
            .as_deref()
            .and_then(|window_id| windows.iter().position(|window| window.id == window_id))
        {
            let previous_index = if current_index == 0 {
                windows.len() - 1
            } else {
                current_index - 1
            };
            windows[previous_index].id.clone()
        } else {
            windows[0].id.clone()
        };

        self.selected_window_id = Some(previous_window_id);
        self.status_message.clear();
    }

    async fn jump_to_selected_window(&mut self) -> Result<()> {
        self.ensure_selected_window_visible();

        let Some(pane) = self.selected_window_focus_pane().cloned() else {
            self.status_message = String::from("No window selected in Browse.");
            return Ok(());
        };

        self.selected_pane_id = Some(pane.id.clone());
        self.sync_selected_window_from_selection();
        self.jump_to_pane(&pane).await
    }

    async fn jump_to_pane(&mut self, pane: &tmux::Pane) -> Result<()> {
        let current_pane_id = self.runtime_context.pane_id.clone();
        let current_pane_id = current_pane_id.as_deref();
        let same_server = self.runtime_context.is_same_server(self.target());

        if same_server {
            if let Some(current_pane_id) = current_pane_id {
                match tmux::current_client_tty(self.target(), current_pane_id).await {
                    Ok(client_tty) if !client_tty.is_empty() => {
                        let result =
                            tmux::jump_client_to_pane(self.target(), &client_tty, pane).await;
                        if self.recover_missing_pane_result(pane, result).await? {
                            return Ok(());
                        }
                    }
                    _ => {
                        let result = tmux::focus_pane(self.target(), pane).await;
                        if self.recover_missing_pane_result(pane, result).await? {
                            return Ok(());
                        }
                    }
                }
            } else {
                let result = tmux::focus_pane(self.target(), pane).await;
                if self.recover_missing_pane_result(pane, result).await? {
                    return Ok(());
                }
            }
        } else {
            let result = tmux::focus_pane(self.target(), pane).await;
            if self.recover_missing_pane_result(pane, result).await? {
                return Ok(());
            }
        }
        if self.close_after_jump {
            self.status_message = format!("Showing {} in tmux.", self.pane_target_label(pane));
            self.request_quit();
        } else {
            self.status_message = format!(
                "Showing {} in tmux. Muxboard is still running.",
                self.pane_target_label(pane)
            );
        }
        self.mark_agent_bridge_review_seen(pane).await;
        Ok(())
    }

    async fn mark_agent_bridge_review_seen(&mut self, pane: &tmux::Pane) {
        let Some(event) = pane.agent_event.as_ref() else {
            return;
        };
        if !agent_bridge_review_event_should_mark_seen(event) {
            return;
        }

        self.mark_snapshot_agent_event_seen(&pane.id);
        let _ = tmux::mark_agent_bridge_event_seen(self.target(), &pane.id).await;
    }

    fn mark_snapshot_agent_event_seen(&mut self, pane_id: &str) {
        if let Some(pane) = self
            .snapshot
            .panes
            .iter_mut()
            .find(|pane| pane.id == pane_id)
            && let Some(event) = pane.agent_event.as_mut()
        {
            event.unseen = Some(false);
        }
    }

    fn selected_window_focus_pane(&self) -> Option<&tmux::Pane> {
        let window = self.selected_window()?;
        self.snapshot
            .panes
            .iter()
            .filter(|pane| pane.window_id == window.id)
            .filter(|pane| self.matches_pane_visibility(pane))
            .find(|pane| pane.active)
            .or_else(|| {
                self.snapshot
                    .panes
                    .iter()
                    .find(|pane| pane.window_id == window.id && self.matches_pane_visibility(pane))
            })
    }

    fn selected_visible_pane_position(&self) -> Option<usize> {
        let visible = self.visible_pane_indices();
        self.selected_visible_pane_position_in(&visible)
    }

    fn selected_visible_pane_position_in(&self, visible: &[usize]) -> Option<usize> {
        let pane_id = self.selected_pane_id.as_deref()?;
        visible
            .iter()
            .position(|index| self.snapshot.panes[*index].id == pane_id)
    }

    fn board_row_entries(&self, limit: usize) -> Vec<VisiblePaneEntry> {
        let visible = self.visible_pane_entries();
        if visible.len() <= limit {
            return visible;
        }

        let selected_position = self
            .selected_visible_pane_position_in_entries(&visible)
            .unwrap_or(0);
        let half_window = limit / 2;
        let max_start = visible.len().saturating_sub(limit);
        let start = selected_position.saturating_sub(half_window).min(max_start);
        let end = (start + limit).min(visible.len());

        visible[start..end].to_vec()
    }

    fn board_window_summary(&self, limit: usize) -> Option<String> {
        self.board_window_summary_for_entries(&self.visible_pane_entries(), limit)
    }

    fn board_window_summary_for_entries(
        &self,
        visible: &[VisiblePaneEntry],
        limit: usize,
    ) -> Option<String> {
        if limit == 0 {
            return None;
        }

        if visible.is_empty() {
            return Some(String::from("0 panes"));
        }

        let selected_position = self
            .selected_visible_pane_position_in_entries(visible)
            .unwrap_or(0);
        let window_size = visible.len().min(limit);
        let half_window = limit / 2;
        let max_start = visible.len().saturating_sub(window_size);
        let start = selected_position.saturating_sub(half_window).min(max_start);
        let end = start + window_size;

        if visible.len() <= limit {
            Some(format!("1-{} / {}", visible.len(), visible.len()))
        } else {
            Some(format!("{}-{} / {}", start + 1, end, visible.len()))
        }
    }

    fn fleet_health_summary(&self) -> String {
        self.fleet_health_summary_for_entries(&self.visible_pane_entries())
    }

    fn fleet_health_summary_for_entries(&self, visible: &[VisiblePaneEntry]) -> String {
        let mut needs_user = 0;
        let mut working = 0;

        for entry in visible {
            let pane = &self.snapshot.panes[entry.index];
            if self.pane_requires_attention(pane, entry.insight.status) && !entry.acknowledged {
                needs_user += 1;
            } else if entry.insight.status == PaneStatus::Running {
                working += 1;
            }
        }

        let mut parts = Vec::new();
        if needs_user > 0 {
            parts.push(count_label(needs_user, "needs you", "need you"));
        }
        if working > 0 {
            parts.push(count_label(working, "working", "working"));
        }

        if parts.is_empty() {
            String::from("all quiet")
        } else {
            parts.join(", ")
        }
    }

    fn window_navigation_targets(&self) -> Vec<&tmux::Window> {
        self.window_navigation_entries()
            .into_iter()
            .map(|entry| &self.snapshot.windows[entry.index])
            .collect()
    }

    fn window_navigation_entries(&self) -> Vec<WindowNavigationEntry> {
        let mut stats_by_window = HashMap::<&str, (usize, u16)>::new();
        for entry in self.visible_pane_entries() {
            let pane = &self.snapshot.panes[entry.index];
            let heat =
                pane_heat_score(&ObservedPane::from(pane), entry.insight, entry.acknowledged);
            stats_by_window
                .entry(pane.window_id.as_str())
                .and_modify(|(pane_count, hottest)| {
                    *pane_count += 1;
                    *hottest = (*hottest).max(heat);
                })
                .or_insert((1, heat));
        }

        let mut entries = self
            .snapshot
            .windows
            .iter()
            .enumerate()
            .filter_map(|(index, window)| {
                stats_by_window
                    .get(window.id.as_str())
                    .map(|(pane_count, heat)| WindowNavigationEntry {
                        index,
                        heat: *heat,
                        pane_count: *pane_count,
                    })
            })
            .collect::<Vec<_>>();

        entries.sort_by(|left, right| {
            let left_window = &self.snapshot.windows[left.index];
            let right_window = &self.snapshot.windows[right.index];

            right
                .heat
                .cmp(&left.heat)
                .then_with(|| left_window.session_name.cmp(&right_window.session_name))
                .then_with(|| left_window.name.cmp(&right_window.name))
                .then_with(|| left_window.id.cmp(&right_window.id))
        });

        entries
    }

    fn visible_pane_indices(&self) -> Vec<usize> {
        self.visible_pane_entries()
            .into_iter()
            .map(|entry| entry.index)
            .collect()
    }

    fn visible_pane_entries(&self) -> Vec<VisiblePaneEntry> {
        let mut visible = self
            .snapshot
            .panes
            .iter()
            .enumerate()
            .filter_map(|(index, pane)| {
                let insight = self.pane_insight(pane);
                if !self.matches_pane_visibility_with_insight(pane, insight) {
                    return None;
                }

                Some(VisiblePaneEntry {
                    index,
                    insight,
                    acknowledged: self.is_acknowledged(pane, insight.status),
                })
            })
            .collect::<Vec<_>>();

        match self.sort_mode {
            SortMode::Natural => {}
            SortMode::Heat => visible.sort_by(|left, right| {
                let left_pane = &self.snapshot.panes[left.index];
                let right_pane = &self.snapshot.panes[right.index];

                pane_heat_score(
                    &ObservedPane::from(right_pane),
                    right.insight,
                    right.acknowledged,
                )
                .cmp(&pane_heat_score(
                    &ObservedPane::from(left_pane),
                    left.insight,
                    left.acknowledged,
                ))
                .then_with(|| {
                    attention_rank(left.insight.status).cmp(&attention_rank(right.insight.status))
                })
                .then_with(|| left_pane.session_name.cmp(&right_pane.session_name))
                .then_with(|| left_pane.window_name.cmp(&right_pane.window_name))
                .then_with(|| left_pane.pane_index.cmp(&right_pane.pane_index))
                .then_with(|| left_pane.id.cmp(&right_pane.id))
            }),
            SortMode::Attention => visible.sort_by(|left, right| {
                let left_pane = &self.snapshot.panes[left.index];
                let right_pane = &self.snapshot.panes[right.index];

                self.visible_entry_attention_rank(left)
                    .cmp(&self.visible_entry_attention_rank(right))
                    .then_with(|| left.acknowledged.cmp(&right.acknowledged))
                    .then_with(|| {
                        workload_rank(left.insight.workload)
                            .cmp(&workload_rank(right.insight.workload))
                    })
                    .then_with(|| left_pane.session_name.cmp(&right_pane.session_name))
                    .then_with(|| left_pane.window_name.cmp(&right_pane.window_name))
                    .then_with(|| left_pane.pane_index.cmp(&right_pane.pane_index))
                    .then_with(|| left_pane.id.cmp(&right_pane.id))
            }),
        }

        visible
    }

    fn visible_entry_attention_rank(&self, entry: &VisiblePaneEntry) -> u8 {
        let pane = &self.snapshot.panes[entry.index];
        if !entry.acknowledged && self.pane_requires_attention(pane, entry.insight.status) {
            return match entry.insight.status {
                PaneStatus::Error => 0,
                PaneStatus::Waiting => 1,
                PaneStatus::Stuck => 2,
                PaneStatus::Done => 3,
                _ => attention_rank(entry.insight.status),
            };
        }

        match entry.insight.status {
            PaneStatus::Running => 4,
            PaneStatus::Idle => 5,
            PaneStatus::Done => 6,
            PaneStatus::Unknown => 7,
            PaneStatus::Waiting => 8,
            PaneStatus::Error => 9,
            PaneStatus::Stuck => 10,
        }
    }

    fn selected_visible_pane_position_in_entries(
        &self,
        visible: &[VisiblePaneEntry],
    ) -> Option<usize> {
        let pane_id = self.selected_pane_id.as_deref()?;
        visible
            .iter()
            .position(|entry| self.snapshot.panes[entry.index].id == pane_id)
    }

    fn matches_pane_visibility(&self, pane: &tmux::Pane) -> bool {
        let insight = self.pane_insight(pane);
        self.matches_pane_visibility_with_insight(pane, insight)
    }

    fn matches_pane_visibility_with_insight(
        &self,
        pane: &tmux::Pane,
        insight: PaneInsight,
    ) -> bool {
        self.matches_base_filter_with_insight(pane, insight)
    }

    fn matches_base_filter(&self, pane: &tmux::Pane) -> bool {
        let insight = self.pane_insight(pane);
        self.matches_base_filter_with_insight(pane, insight)
    }

    fn matches_base_filter_with_insight(&self, pane: &tmux::Pane, insight: PaneInsight) -> bool {
        if self.runtime_context.is_same_server(self.target())
            && self.runtime_context.pane_id.as_deref() == Some(pane.id.as_str())
        {
            return false;
        }

        let matches_filter = match self.filter_mode {
            FilterMode::All => true,
            FilterMode::Agents => insight.workload.is_agent(),
            FilterMode::Attention => {
                self.pane_requires_attention(pane, insight.status)
                    && !self.is_acknowledged(pane, insight.status)
            }
        };

        matches_filter && self.matches_view_scope(pane) && self.matches_search(pane)
    }

    fn matches_view_scope(&self, pane: &tmux::Pane) -> bool {
        match &self.view_scope {
            ViewScope::All => true,
            ViewScope::Window { id, .. } => pane.window_id == *id,
        }
    }

    fn matches_search(&self, pane: &tmux::Pane) -> bool {
        let query = self.search_query.trim();
        if query.is_empty() {
            return true;
        }

        pane_corpus(&ObservedPane::from(pane), self.pane_runtime.get(&pane.id))
            .contains(&query.to_ascii_lowercase())
    }

    #[must_use]
    fn save_persistent_state(&mut self) -> bool {
        if let Err(error) = self
            .state_store
            .save_acknowledged_attention(&self.acknowledged_attention)
        {
            self.status_message = format!(
                "State save failed at {}: {error}",
                self.state_store.path().display()
            );
            return false;
        }
        true
    }

    #[must_use]
    fn save_command_state(&mut self) -> bool {
        let recent_commands = self.recent_commands.iter().cloned().collect::<Vec<_>>();
        if let Err(error) = self
            .state_store
            .save_command_state(&recent_commands, &self.macro_slots)
        {
            self.status_message = format!(
                "Command state save failed at {}: {error}",
                self.state_store.path().display()
            );
            return false;
        }
        true
    }

    fn push_event(&mut self, line: String) {
        self.recent_events.push_front(line);
        while self.recent_events.len() > MAX_RECENT_EVENTS {
            self.recent_events.pop_back();
        }
    }

    #[must_use]
    fn save_notification_settings(&mut self) -> bool {
        if let Err(error) = self
            .config_store
            .save_notification_settings(&self.notification_settings)
        {
            self.status_message = format!(
                "Notification settings save failed at {}: {error}",
                self.config_store.path().display()
            );
            return false;
        }
        true
    }

    #[must_use]
    fn save_ui_settings(&mut self) -> bool {
        if let Err(error) = self.config_store.save_ui_settings(&self.ui_settings) {
            self.status_message = format!(
                "UI settings save failed at {}: {error}",
                self.config_store.path().display()
            );
            return false;
        }
        true
    }

    fn desktop_notification_status_message(&self) -> String {
        if !self.notification_settings.desktop_enabled {
            return String::from("Desktop alerts off.");
        }

        match self.notifier.mode() {
            notifications::NotificationMode::LocalDesktop => String::from("Desktop alerts on."),
            notifications::NotificationMode::SshFallback => {
                String::from("Desktop alerts unavailable on SSH; terminal bell still works.")
            }
            notifications::NotificationMode::TerminalOnly => {
                String::from("Desktop alerts unavailable here; terminal bell still works.")
            }
        }
    }

    fn should_refresh_metrics(&self) -> bool {
        self.last_metrics_refresh
            .is_none_or(|instant| instant.elapsed() >= Duration::from_secs(2))
    }

    async fn refresh_metrics(&mut self) {
        let pane_pid_pairs = self
            .snapshot
            .panes
            .iter()
            .map(|pane| (pane.id.clone(), pane.pane_pid))
            .filter(|(_, pid)| *pid > 0)
            .collect::<Vec<_>>();
        let pids = pane_pid_pairs
            .iter()
            .map(|(_, pid)| *pid)
            .collect::<Vec<_>>();

        match metrics::collect(&pids).await {
            Ok(collected) => {
                self.pane_metrics = pane_pid_pairs
                    .into_iter()
                    .filter_map(|(pane_id, pid)| {
                        collected.get(&pid).cloned().map(|metric| (pane_id, metric))
                    })
                    .collect();
                self.last_metrics_refresh = Some(Instant::now());
            }
            Err(error) => {
                self.status_message = format!("Pane CPU/memory refresh failed: {error}");
                self.last_metrics_refresh = Some(Instant::now());
            }
        }
    }

    fn agent_lanes(&self) -> Vec<AgentLane> {
        let selected_pane_id = self.selected_pane_id.as_deref();
        let mut lanes = lane_workloads()
            .into_iter()
            .filter_map(|workload| {
                let mut lane = AgentLane {
                    workload,
                    total: 0,
                    waiting: 0,
                    error: 0,
                    stuck: 0,
                    running: 0,
                    done: 0,
                    idle: 0,
                    unknown: 0,
                    selected: false,
                };

                for pane in self
                    .snapshot
                    .panes
                    .iter()
                    .filter(|pane| self.matches_base_filter(pane))
                {
                    let insight = self.pane_insight(pane);
                    if insight.workload != workload {
                        continue;
                    }

                    lane.total += 1;
                    lane.selected |= selected_pane_id == Some(pane.id.as_str());

                    match insight.status {
                        PaneStatus::Waiting => lane.waiting += 1,
                        PaneStatus::Error => lane.error += 1,
                        PaneStatus::Stuck => lane.stuck += 1,
                        PaneStatus::Running => lane.running += 1,
                        PaneStatus::Done => lane.done += 1,
                        PaneStatus::Idle => lane.idle += 1,
                        PaneStatus::Unknown => lane.unknown += 1,
                    }
                }

                (lane.total > 0).then_some(lane)
            })
            .collect::<Vec<_>>();

        lanes.sort_by(|left, right| {
            lane_attention_rank(*left)
                .cmp(&lane_attention_rank(*right))
                .then_with(|| right.total.cmp(&left.total))
                .then_with(|| workload_rank(left.workload).cmp(&workload_rank(right.workload)))
        });

        lanes
    }
}

fn format_age(age: Duration) -> String {
    let seconds = age.as_secs();
    if seconds < 60 {
        return format!("{}s ago", seconds);
    }

    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{}m ago", minutes);
    }

    let hours = minutes / 60;
    format!("{}h ago", hours)
}

fn format_age_short(age: Duration) -> String {
    let seconds = age.as_secs();
    if seconds < 60 {
        return format!("{seconds}s");
    }

    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }

    let hours = minutes / 60;
    format!("{hours}h")
}

fn format_key_label(key: &str) -> String {
    match key {
        "enter" => String::from("Enter"),
        "space" => String::from("Space"),
        "tab" => String::from("Tab"),
        "esc" => String::from("Esc"),
        "down" => String::from("Down"),
        "up" => String::from("Up"),
        _ if key.chars().count() == 1 => key.to_ascii_uppercase(),
        _ => key.to_owned(),
    }
}

fn launch_window_name(command: &str) -> String {
    let executable = command
        .split_whitespace()
        .next()
        .and_then(|token| Path::new(token).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("agent");
    let sanitized = executable
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        .take(24)
        .collect::<String>();

    if sanitized.is_empty() {
        String::from("agent")
    } else {
        sanitized
    }
}

fn workload_rank(workload: WorkloadKind) -> u8 {
    match workload {
        WorkloadKind::Codex => 0,
        WorkloadKind::ClaudeCode => 1,
        WorkloadKind::Opencode => 2,
        WorkloadKind::Aider => 3,
        WorkloadKind::Gemini => 4,
        WorkloadKind::Agent => 5,
        WorkloadKind::Job => 6,
        WorkloadKind::Shell => 7,
    }
}

fn agent_bridge_workload(event: &tmux::AgentBridgeEvent) -> WorkloadKind {
    let agent = event.agent.trim().to_ascii_lowercase();
    if agent.contains("codex") {
        WorkloadKind::Codex
    } else if agent.contains("claude") {
        WorkloadKind::ClaudeCode
    } else if agent.contains("opencode") || agent.contains("open-code") {
        WorkloadKind::Opencode
    } else if agent.contains("aider") {
        WorkloadKind::Aider
    } else if agent.contains("gemini") {
        WorkloadKind::Gemini
    } else {
        WorkloadKind::Agent
    }
}

fn agent_bridge_status(event: &tmux::AgentBridgeEvent) -> Option<PaneStatus> {
    match tmux::normalize_agent_bridge_state(&event.state)? {
        "running" => Some(PaneStatus::Running),
        "waiting" => Some(PaneStatus::Waiting),
        "done" => Some(PaneStatus::Done),
        "error" => Some(PaneStatus::Error),
        "stuck" => Some(PaneStatus::Stuck),
        "idle" => Some(PaneStatus::Idle),
        _ => None,
    }
}

fn agent_bridge_review_event_should_mark_seen(event: &tmux::AgentBridgeEvent) -> bool {
    matches!(
        tmux::normalize_agent_bridge_state(&event.state),
        Some("done" | "error" | "stuck")
    ) && event.unseen != Some(false)
}

fn agent_bridge_report(event: &tmux::AgentBridgeEvent, status: PaneStatus) -> Option<AgentReport> {
    let summary = event.summary.trim();
    let progress = event
        .progress
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let log = event
        .log
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let thread_name = event
        .thread_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let meaningful_summary =
        agent_bridge_summary(summary, progress, log, thread_name).or_else(|| {
            thread_name
                .map(ToOwned::to_owned)
                .or_else(|| progress.map(ToOwned::to_owned))
                .or_else(|| log.map(ToOwned::to_owned))
        });

    let (blocker, next) = match status {
        PaneStatus::Waiting => (
            meaningful_summary
                .clone()
                .unwrap_or_else(|| String::from("input needed")),
            String::from("respond"),
        ),
        PaneStatus::Error => (
            meaningful_summary
                .clone()
                .unwrap_or_else(|| String::from("check output")),
            String::from("show output"),
        ),
        PaneStatus::Stuck => (
            meaningful_summary
                .clone()
                .unwrap_or_else(|| String::from("stale")),
            String::from("show output"),
        ),
        PaneStatus::Running | PaneStatus::Done | PaneStatus::Idle | PaneStatus::Unknown => {
            let summary = meaningful_summary?;
            (String::from("none"), summary)
        }
    };

    Some(AgentReport {
        status: status.display_label().to_ascii_lowercase(),
        blocker,
        next,
        updated_at: Instant::now(),
    })
}

fn agent_bridge_summary(
    summary: &str,
    progress: Option<&str>,
    log: Option<&str>,
    thread_name: Option<&str>,
) -> Option<String> {
    let summary = summary.trim();
    let primary = if !summary.is_empty() {
        Some(summary)
    } else {
        log.or(thread_name)
    }?;

    let mut parts = vec![primary.to_owned()];
    if let Some(progress) = progress
        && !primary.eq_ignore_ascii_case(progress)
    {
        parts.push(progress.to_owned());
    }
    if let Some(thread_name) = thread_name
        && !parts
            .iter()
            .any(|part| part.eq_ignore_ascii_case(thread_name))
    {
        parts.push(thread_name.to_owned());
    }
    Some(parts.join(" · "))
}

fn native_agent_assignments_for_panes(
    panes: &[tmux::Pane],
    events: &[AgentSourceEvent],
) -> Vec<(usize, AgentSourceEvent)> {
    let mut candidates_by_pane: HashMap<usize, Vec<&AgentSourceEvent>> = HashMap::new();

    for event in events {
        let candidates = panes
            .iter()
            .enumerate()
            .filter(|(_, pane)| native_agent_event_matches_pane(event, pane))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();

        if candidates.len() == 1 {
            candidates_by_pane
                .entry(candidates[0])
                .or_default()
                .push(event);
        }
    }

    let mut assignments = candidates_by_pane
        .into_iter()
        .filter_map(|(pane_index, mut candidates)| {
            candidates.sort_by(|left, right| {
                right
                    .updated_at_unix_ms
                    .cmp(&left.updated_at_unix_ms)
                    .then_with(|| right.identity_key().cmp(&left.identity_key()))
            });
            candidates
                .into_iter()
                .next()
                .map(|event| (pane_index, event.clone()))
        })
        .collect::<Vec<_>>();
    assignments.sort_by_key(|(pane_index, _)| *pane_index);
    assignments
}

fn native_agent_event_matches_pane(event: &AgentSourceEvent, pane: &tmux::Pane) -> bool {
    pane.agent_event.is_none()
        && agent_source_matches_path(event, &pane.current_path)
        && pane_text_has_provider_hint(
            event.provider,
            &pane.current_command,
            &pane.title,
            &pane.window_name,
        )
}

fn native_agent_event_should_yield_to_runtime(
    event: &AgentSourceEvent,
    pane: &tmux::Pane,
    runtime: Option<&PaneRuntime>,
) -> bool {
    let Some(runtime) = runtime else {
        return false;
    };
    let runtime_insight = infer_pane_insight(&ObservedPane::from(pane), Some(runtime));
    if runtime_insight.workload == event.provider.workload()
        && runtime_insight.status != PaneStatus::Unknown
        && runtime_insight.status != event.status
        && runtime_contains_provider_activity(runtime, event.provider)
    {
        return true;
    }

    matches!(
        runtime_insight.status,
        PaneStatus::Waiting | PaneStatus::Done | PaneStatus::Error | PaneStatus::Stuck
    )
}

fn runtime_contains_provider_activity(
    runtime: &PaneRuntime,
    provider: AgentSourceProvider,
) -> bool {
    runtime.output.iter().any(|line| {
        let lower = line.to_ascii_lowercase();
        match provider {
            AgentSourceProvider::Codex => lower.contains("codex") || lower.contains("openai"),
            AgentSourceProvider::ClaudeCode => {
                lower.contains("claude") || lower.contains("anthropic")
            }
        }
    })
}

fn native_agent_bridge_event(
    event: &AgentSourceEvent,
    unseen: Option<bool>,
) -> tmux::AgentBridgeEvent {
    tmux::AgentBridgeEvent {
        agent: event.provider.agent_key().to_owned(),
        state: native_agent_state(event.status).to_owned(),
        summary: event.summary.clone(),
        thread_id: event.thread_id.clone(),
        thread_name: event.thread_name.clone(),
        progress: event.progress.clone(),
        log: event.log.clone(),
        unseen,
        updated_at_unix_ms: Some(event.updated_at_unix_ms),
    }
}

fn native_agent_event_unseen(
    previous_versions: &HashMap<String, (PaneStatus, u64)>,
    event: &AgentSourceEvent,
    seed: bool,
    status: PaneStatus,
    updated_at_unix_ms: u64,
) -> Option<bool> {
    if !matches!(
        status,
        PaneStatus::Done | PaneStatus::Error | PaneStatus::Stuck
    ) {
        return None;
    }

    let changed = previous_versions
        .get(&event.identity_key())
        .is_none_or(|previous| *previous != (status, updated_at_unix_ms));

    Some(!seed && changed)
}

fn native_agent_state(status: PaneStatus) -> &'static str {
    match status {
        PaneStatus::Running => "running",
        PaneStatus::Waiting => "waiting",
        PaneStatus::Done => "done",
        PaneStatus::Error => "error",
        PaneStatus::Stuck => "stuck",
        PaneStatus::Idle => "idle",
        PaneStatus::Unknown => "idle",
    }
}

fn lane_workloads() -> [WorkloadKind; 6] {
    [
        WorkloadKind::Codex,
        WorkloadKind::ClaudeCode,
        WorkloadKind::Opencode,
        WorkloadKind::Aider,
        WorkloadKind::Gemini,
        WorkloadKind::Agent,
    ]
}

fn lane_attention_rank(lane: AgentLane) -> (u8, u8) {
    if lane.error > 0 {
        (0, 0)
    } else if lane.waiting > 0 {
        (1, 0)
    } else if lane.stuck > 0 {
        (2, 0)
    } else if lane.running > 0 {
        (3, 0)
    } else if lane.idle > 0 {
        (4, 0)
    } else if lane.done > 0 {
        (5, 0)
    } else {
        (6, 0)
    }
}

fn default_macro_slots() -> Vec<Option<String>> {
    vec![None; MACRO_SLOT_COUNT]
}

fn normalize_macro_slots(slots: Vec<Option<String>>) -> Vec<Option<String>> {
    let mut slots = slots.into_iter().take(MACRO_SLOT_COUNT).collect::<Vec<_>>();
    slots.resize(MACRO_SLOT_COUNT, None);
    slots
}

fn truncate_for_panel(text: &str) -> String {
    const MAX_CHARS: usize = 44;
    let mut chars = text.chars();
    let truncated = chars.by_ref().take(MAX_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn truncate_for_width(text: &str, max_chars: usize) -> String {
    let max_chars = max_chars.max(6);
    let mut chars = text.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() && max_chars > 3 {
        let mut shortened = truncated.chars().take(max_chars - 3).collect::<String>();
        shortened.push_str("...");
        shortened
    } else {
        truncated
    }
}

fn truncate_for_board(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn count_label(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
    }
}

fn pane_count_label(count: usize) -> String {
    count_label(count, "pane", "panes")
}

fn one_line_summary_label(count: usize) -> &'static str {
    if count == 1 {
        "a one-line summary"
    } else {
        "one-line summaries"
    }
}

fn more_pane_count_label(count: usize) -> String {
    count_label(count, "more pane", "more panes")
}

fn waiting_pane_count_label(count: usize) -> String {
    count_label(count, "waiting pane", "waiting panes")
}

fn alert_count_label(count: usize) -> String {
    count_label(count, "alert", "alerts")
}

fn send_target_object_phrase(target_description: &str) -> String {
    if target_description.starts_with("send list") {
        format!("the {target_description}")
    } else {
        target_description.to_owned()
    }
}

fn send_target_phrase(target_description: &str) -> String {
    format!("send to {}", send_target_object_phrase(target_description))
}

fn smart_action_send_list_status(outcome: DispatchOutcome, skipped: usize) -> String {
    smart_action_status("Send list", outcome, skipped)
}

fn smart_action_lane_status(outcome: DispatchOutcome, skipped: usize) -> String {
    smart_action_status("Lane", outcome, skipped)
}

fn smart_action_empty_after_send_status(
    prefix: &str,
    outcome: DispatchOutcome,
    skipped: usize,
) -> String {
    let mut parts = vec![format!("{prefix}: no panes remain for Enter")];
    if skipped > 0 {
        parts.push(format!("skipped {}", pane_count_label(skipped)));
    }
    if outcome.disappeared_count > 0 {
        parts.push(format!(
            "{} disappeared",
            pane_count_label(outcome.disappeared_count)
        ));
    }
    format!("{}.", parts.join(", "))
}

fn smart_action_status(prefix: &str, outcome: DispatchOutcome, skipped: usize) -> String {
    let mut parts = vec![format!(
        "{prefix}: sent Enter to {}",
        pane_count_label(outcome.sent_count)
    )];
    if skipped > 0 {
        parts.push(format!("skipped {}", pane_count_label(skipped)));
    }
    if outcome.disappeared_count > 0 {
        parts.push(format!(
            "{} disappeared",
            pane_count_label(outcome.disappeared_count)
        ));
    }
    format!("{}.", parts.join(", "))
}

fn no_target_panes_remain_message(action: &str, disappeared_count: usize) -> String {
    if disappeared_count > 0 {
        format!(
            "No panes remain for {action}; {} disappeared.",
            pane_count_label(disappeared_count)
        )
    } else {
        format!("No panes remain for {action}.")
    }
}

fn no_waiting_panes_remain_message(disappeared_count: usize) -> String {
    if disappeared_count > 0 {
        format!(
            "No waiting panes remain for Enter; {} disappeared.",
            pane_count_label(disappeared_count)
        )
    } else {
        String::from("No waiting panes remain ready for Enter.")
    }
}

fn action_error_status_message(error: &Error) -> String {
    let detail = action_error_detail(error);
    let detail = detail.trim_end_matches('.');
    format!("Action failed: {detail}.")
}

fn pane_capture_failed_message(target: &str, error: &Error) -> String {
    let detail = action_error_detail(error);
    let detail = detail.trim_end_matches('.');
    format!("Could not read output for {target}: {detail}.")
}

fn action_error_detail(error: &Error) -> String {
    let message = error.to_string();
    let detail = message
        .strip_prefix("tmux command failed for ")
        .and_then(|rest| rest.split_once(": ").map(|(_, detail)| detail))
        .unwrap_or(message.as_str())
        .trim();

    if detail.is_empty() {
        String::from("unknown error")
    } else {
        truncate_for_width(detail, 120)
    }
}

fn tmux_error_indicates_target_unavailable(error: &Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("can't find pane")
        || message.contains("can't find window")
        || message.contains("can't find target")
        || message.contains("can't find session")
        || message.contains("session not found")
        || tmux_error_indicates_server_unavailable(error)
}

fn tmux_error_indicates_server_unavailable(error: &Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("no server running")
        || message.contains("failed to connect to server")
        || message.contains("error connecting to")
}

fn group_count_label(count: usize) -> String {
    count_label(count, "fleet", "fleets")
}

fn format_debounce(seconds: u64) -> String {
    if seconds == 0 {
        String::from("off")
    } else if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m", seconds / 60)
    }
}

fn next_debounce_seconds(current: u64) -> u64 {
    match current {
        0 => 15,
        15 => 30,
        30 => 60,
        60 => 120,
        120 => 300,
        _ => 0,
    }
}

fn startup_snapshot_status_message(target: &tmux::Target, error: &Error) -> String {
    let issue = classify_startup_snapshot_error(error);
    let target_label = target.display_target();
    match issue {
        StartupSnapshotIssue::NoServer => {
            format!("No tmux server found for {target_label}. Start tmux, then refresh.")
        }
        StartupSnapshotIssue::MissingSession => {
            format!("Session not found for {target_label}. Choose another session or refresh.")
        }
        StartupSnapshotIssue::Unavailable => {
            format!(
                "Could not read tmux panes for {target_label}: {}",
                concise_error(error)
            )
        }
    }
}

fn append_startup_status(status_message: String, warning: String) -> String {
    let status = status_message.trim();
    let warning = warning.trim();

    if status.is_empty() {
        warning.to_owned()
    } else if warning.is_empty() {
        status.to_owned()
    } else {
        format!("{status} {warning}")
    }
}

fn close_after_jump_enabled_from_env() -> bool {
    env_flag_enabled(env::var("MUXBOARD_CLOSE_AFTER_JUMP").ok().as_deref())
}

fn env_flag_enabled(value: Option<&str>) -> bool {
    value.map(str::trim).is_some_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupSnapshotIssue {
    NoServer,
    MissingSession,
    Unavailable,
}

fn classify_startup_snapshot_error(error: &Error) -> StartupSnapshotIssue {
    let text = format!("{error:#}").to_ascii_lowercase();
    if text.contains("can't find session")
        || text.contains("can't find window")
        || text.contains("session not found")
    {
        return StartupSnapshotIssue::MissingSession;
    }

    if text.contains("no server running")
        || text.contains("failed to connect to server")
        || text.contains("error connecting to")
    {
        return StartupSnapshotIssue::NoServer;
    }

    StartupSnapshotIssue::Unavailable
}

fn concise_error(error: &Error) -> String {
    error
        .to_string()
        .trim()
        .trim_end_matches('.')
        .chars()
        .take(160)
        .collect::<String>()
}

fn expand_command_template(text: &str, pane: &tmux::Pane, workload: WorkloadKind) -> String {
    let replacements = [
        ("{id}", pane.id.as_str()),
        ("{session}", pane.session_name.as_str()),
        ("{window}", pane.window_name.as_str()),
        ("{path}", pane.current_path.as_str()),
        ("{cmd}", pane.current_command.as_str()),
        ("{title}", pane.title.as_str()),
        ("{lane}", workload.short_label()),
    ];

    let mut expanded = text.to_owned();
    for (needle, value) in replacements {
        expanded = expanded.replace(needle, value);
    }
    expanded
}

#[cfg(test)]
fn append_output_chunk(runtime: &mut PaneRuntime, chunk: &str) -> Option<String> {
    crate::core::append_output_chunk(runtime, chunk)
}

mod attention;
mod presentation;
mod targets;
#[cfg(test)]
pub(crate) mod tests;
